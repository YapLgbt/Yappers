// SPDX-License-Identifier: AGPL-3.0-only
//! Route local maintenance to the process that owns the MVCC database.
//! The backup is published by the CLI under the operator's credentials.
use crate::{
    backup,
    model::Action,
    native_transfer,
    store::{NativeHome, Store},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{ErrorKind, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

#[derive(Serialize, Deserialize)]
enum Command {
    Backup,
    Moderate {
        id: String,
        hide: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<ModerationKind>,
    },
    ModerationEvent {
        id: String,
    },
    HiddenEvents {
        after: Option<String>,
    },
    Status,
    Peers,
    StopPeer {
        id: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModerationKind {
    Post,
    Reply,
}

impl ModerationKind {
    fn of(action: &Action) -> Option<Self> {
        match action {
            Action::Post { .. } | Action::PostV2 { .. } => Some(Self::Post),
            Action::Reply { .. } => Some(Self::Reply),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ModerationContent {
    kind: ModerationKind,
    community: String,
    headline: Option<String>,
    body: String,
}

#[derive(Serialize, Deserialize)]
pub struct ModerationEvent {
    hidden: bool,
    content: Option<ModerationContent>,
}

#[derive(Serialize, Deserialize)]
pub struct HiddenEvents {
    ids: Vec<String>,
    next: Option<String>,
}

impl Store {
    fn moderation_event(&self, id: &str) -> Result<ModerationEvent> {
        let hidden = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM hidden WHERE id=?1)",
            [id],
            |row| row.get(0),
        )?;
        let content = self.existing(id)?.and_then(|envelope| {
            let payload = envelope.event.payload;
            let kind = ModerationKind::of(&payload.action)?;
            let (headline, body) = match payload.action {
                Action::PostV2 { headline, body, .. } => (Some(headline), body),
                Action::Post { body, .. } | Action::Reply { body, .. } => (None, body),
                _ => return None,
            };
            Some(ModerationContent {
                kind,
                community: payload.community,
                headline,
                body,
            })
        });
        Ok(ModerationEvent { hidden, content })
    }

    fn hidden_events_page(&self, after: Option<&str>) -> Result<HiddenEvents> {
        const PAGE_SIZE: usize = 20;
        let mut query = self
            .db
            .prepare("SELECT id FROM hidden WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let mut ids = query
            .query_map(
                rusqlite::params![after.unwrap_or_default(), PAGE_SIZE as i64 + 1],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let next = if ids.len() > PAGE_SIZE {
            ids.pop();
            ids.last().cloned()
        } else {
            None
        };
        Ok(HiddenEvents { ids, next })
    }
}

fn socket_path(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    let hash = blake3::hash(path.as_os_str().as_encoded_bytes()).to_hex();
    let name = format!(".yap-{}.sock", &hash[..20]);
    let address = path.with_file_name(&name);
    if address.as_os_str().len() < 100 {
        return Ok(address);
    }
    // Deep development data directories can exceed sockaddr_un.sun_path.
    let directory = std::env::temp_dir().join(format!("yap-{}", unsafe { libc::geteuid() }));
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = directory.symlink_metadata()?;
    ensure!(
        meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "invalid private Home socket directory"
    );
    Ok(directory.join(name))
}

pub(crate) struct Admin {
    path: PathBuf,
    wake: UnixStream,
}

impl Drop for Admin {
    fn drop(&mut self) {
        let _ = self.wake.shutdown(std::net::Shutdown::Both);
        let _ = fs::remove_file(&self.path);
    }
}

impl Admin {
    pub(crate) fn start(home: &Arc<NativeHome>) -> Result<Self> {
        let path = socket_path(&home.path)?;
        match path.symlink_metadata() {
            Ok(meta) if meta.file_type().is_socket() => fs::remove_file(&path)?,
            Ok(_) => anyhow::bail!("an unrelated file occupies the Home admin socket path"),
            Err(e) if e.kind() == ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let (wake, stop) = UnixStream::pair()?;
        let handle = Self { path, wake };
        let owner = Arc::downgrade(home);
        std::thread::Builder::new()
            .name("home-admin".into())
            .spawn(move || serve(listener, stop, owner))?;
        Ok(handle)
    }
}

// The listener can publish moderation decisions and read a protected archive.
// Authenticate the connecting local process in addition to socket permissions.
fn authorize(peer: &UnixStream) -> Result<()> {
    #[cfg(target_os = "linux")]
    let uid = {
        let mut cred = std::mem::MaybeUninit::<libc::ucred>::uninit();
        let mut n = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SO_PEERCRED fills a ucred for this connected Unix socket.
        let status = unsafe {
            libc::getsockopt(
                peer.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                cred.as_mut_ptr().cast(),
                &mut n,
            )
        };
        if status != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        ensure!(
            n as usize == std::mem::size_of::<libc::ucred>(),
            "incomplete local credentials"
        );
        unsafe { cred.assume_init().uid }
    };
    #[cfg(not(target_os = "linux"))]
    let uid = {
        let (mut uid, mut gid) = (0, 0);
        let status = unsafe { libc::getpeereid(peer.as_raw_fd(), &mut uid, &mut gid) };
        if status != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        uid
    };
    ensure!(
        uid == 0 || uid == unsafe { libc::geteuid() },
        "unauthorized local maintenance request"
    );
    Ok(())
}

fn serve(listener: UnixListener, stop: UnixStream, home: Weak<NativeHome>) {
    let mut fds = [
        libc::pollfd {
            fd: stop.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // Both descriptors remain owned and open throughout this loop.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, -1) };
        if n < 0 && std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
            continue;
        }
        if n < 0 || fds[0].revents != 0 {
            return;
        }
        let Ok((mut peer, _)) = listener.accept() else {
            continue;
        };
        let Some(home) = home.upgrade() else { return };
        let result = (|| {
            authorize(&peer)?;
            peer.set_read_timeout(Some(Duration::from_secs(3)))?;
            let request = frame(&mut peer, Some(4096))?;
            match serde_json::from_slice::<Command>(&request)? {
                Command::Backup => native_transfer::archive(&home.connection()?),
                Command::Status => {
                    home.connection()?;
                    Ok(Vec::new())
                }
                Command::ModerationEvent { id } => {
                    let mut store = home.connection()?;
                    let event = store.operation(|store| store.moderation_event(&id))?;
                    Ok(serde_json::to_vec(&event)?)
                }
                Command::HiddenEvents { after } => Ok(serde_json::to_vec(
                    &home.connection()?.hidden_events_page(after.as_deref())?,
                )?),
                Command::Moderate { id, hide, kind } => {
                    let mut store = home.connection()?;
                    if hide {
                        home.disclosure.restrict(|| {
                            store.operation(|store| {
                                if let Some(kind) = kind {
                                    ensure!(
                                        store.existing(&id)?.and_then(|entry| ModerationKind::of(
                                            &entry.event.payload.action
                                        )) == Some(kind),
                                        "the expected post or reply is unavailable"
                                    );
                                }
                                store.moderate(&id, true)
                            })
                        })?;
                    } else {
                        store.operation(|store| store.moderate(&id, false))?;
                    }
                    Ok(Vec::new())
                }
                operation @ (Command::Peers | Command::StopPeer { .. }) => {
                    let server = home
                        .peering_server
                        .lock()
                        .expect("Home peering control")
                        .upgrade()
                        .context("Home service is offline")?;
                    if let Command::StopPeer { id } = operation {
                        server.stop_peer(&id)?;
                        Ok(Vec::new())
                    } else {
                        Ok(serde_json::to_vec(&server.admin_peers()?)?)
                    }
                }
            }
        })();
        let (ok, data) = match result {
            Ok(bytes) => (1, bytes),
            Err(err) => (0, format!("{err:#}").into_bytes()),
        };
        let _ = peer.set_write_timeout(Some(Duration::from_secs(30)));
        let _ = peer
            .write_all(&[ok])
            .and_then(|()| write_frame(&mut peer, &data));
    }
}

fn frame(input: &mut UnixStream, maximum: Option<usize>) -> Result<Vec<u8>> {
    let mut size = [0; 8];
    input.read_exact(&mut size)?;
    let size = usize::try_from(u64::from_be_bytes(size))?;
    ensure!(
        maximum.is_none_or(|n| size <= n),
        "oversized Home admin message"
    );
    let mut data = Vec::new();
    input.take(size as u64).read_to_end(&mut data)?;
    ensure!(data.len() == size, "incomplete Home admin response");
    Ok(data)
}
fn write_frame(output: &mut UnixStream, data: &[u8]) -> std::io::Result<()> {
    output.write_all(&(data.len() as u64).to_be_bytes())?;
    output.write_all(data)
}
fn remote(path: &Path, command: &Command) -> Result<Option<Vec<u8>>> {
    let mut stream = match UnixStream::connect(socket_path(path)?) {
        Ok(stream) => stream,
        Err(e) if matches!(e.kind(), ErrorKind::ConnectionRefused | ErrorKind::NotFound) => {
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    write_frame(&mut stream, &serde_json::to_vec(command)?)?;
    let mut ok = [0];
    stream.read_exact(&mut ok)?;
    let bytes = frame(&mut stream, if ok[0] == 1 { None } else { Some(65536) })?;
    ensure!(ok[0] == 1, "{}", String::from_utf8_lossy(&bytes));
    Ok(Some(bytes))
}

pub(crate) fn backup_existing(path: &Path, output: &Path) -> Result<bool> {
    if !NativeHome::is_locator(path)? {
        return Ok(false);
    }
    if !backup_live(path, output)? {
        let store = Store::open(path)?;
        let _authority = backup::lock_authority(path)?;
        store.backup(output)?;
    }
    Ok(true)
}

fn backup_live(path: &Path, output: &Path) -> Result<bool> {
    let Some(archive) = remote(path, &Command::Backup)? else {
        return Ok(false);
    };
    backup::publish(&archive, output)?;
    Ok(true)
}

fn moderate_live(path: &Path, id: &str, hide: bool, kind: Option<ModerationKind>) -> Result<bool> {
    Ok(remote(
        path,
        &Command::Moderate {
            id: id.to_owned(),
            hide,
            kind,
        },
    )?
    .is_some())
}

impl Store {
    pub fn live_moderation_event(path: &Path, id: &str) -> Result<ModerationEvent> {
        let payload = remote(path, &Command::ModerationEvent { id: id.to_owned() })?
            .context("Home service is offline")?;
        Ok(serde_json::from_slice(&payload)?)
    }

    pub fn live_hidden_events(path: &Path, after: Option<String>) -> Result<HiddenEvents> {
        let payload =
            remote(path, &Command::HiddenEvents { after })?.context("Home service is offline")?;
        Ok(serde_json::from_slice(&payload)?)
    }

    pub fn live_peers(path: &Path) -> Result<Vec<crate::server::PeerSummary>> {
        let payload = remote(path, &Command::Peers)?.context("Home service is offline")?;
        Ok(serde_json::from_slice(&payload)?)
    }

    pub fn stop_live_peering(path: &Path, id: &str) -> Result<()> {
        remote(path, &Command::StopPeer { id: id.to_owned() })?
            .context("Home service is offline")?;
        Ok(())
    }

    pub fn live_admin_available(path: &Path) -> Result<bool> {
        Ok(path.is_file()
            && NativeHome::is_locator(path)?
            && remote(path, &Command::Status)?.is_some())
    }

    pub fn backup_live(path: &Path, output: &Path) -> Result<()> {
        ensure!(
            !path.with_added_extension("ha-managed").try_exists()?,
            "use ha-export for an HA home"
        );
        ensure!(
            path.is_file() && NativeHome::is_locator(path)? && backup_live(path, output)?,
            "Home storage is offline"
        );
        Ok(())
    }

    pub fn moderate_live(path: &Path, id: &str, hide: bool) -> Result<()> {
        Self::moderate_content_live(path, id, hide, None)
    }

    pub fn moderate_content_live(
        path: &Path,
        id: &str,
        hide: bool,
        kind: Option<ModerationKind>,
    ) -> Result<()> {
        ensure!(
            path.is_file() && NativeHome::is_locator(path)? && moderate_live(path, id, hide, kind)?,
            "Home storage is offline"
        );
        Ok(())
    }

    pub fn moderate_existing(path: &Path, id: &str, hide: bool) -> Result<()> {
        if path.is_file() && NativeHome::is_locator(path)? && moderate_live(path, id, hide, None)? {
            return Ok(());
        }
        let mut store = Self::open(path).context("open Home for moderation")?;
        let _authority = backup::lock_authority(path)?;
        store.operation(|s| s.moderate(id, hide))
    }
}
