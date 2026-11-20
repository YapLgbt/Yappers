// SPDX-License-Identifier: AGPL-3.0-only
//! Route local maintenance to the process that owns the MVCC database.
//! The backup is published by the CLI under the operator's credentials.
use crate::{
    backup, native_transfer,
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
    Moderate { id: String, hide: bool },
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
            let mut store = home.connection()?;
            match serde_json::from_slice::<Command>(&request)? {
                Command::Backup => native_transfer::archive(&store),
                Command::Moderate { id, hide } => {
                    if hide {
                        home.disclosure
                            .restrict(|| store.operation(|store| store.moderate(&id, true)))?;
                    } else {
                        store.operation(|store| store.moderate(&id, false))?;
                    }
                    Ok(Vec::new())
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
    if let Some(archive) = remote(path, &Command::Backup)? {
        backup::publish(&archive, output)?;
    } else {
        let store = Store::open(path)?;
        let _authority = backup::lock_authority(path)?;
        store.backup(output)?;
    }
    Ok(true)
}

impl Store {
    pub fn moderate_existing(path: &Path, id: &str, hide: bool) -> Result<()> {
        if path.is_file()
            && NativeHome::is_locator(path)?
            && remote(
                path,
                &Command::Moderate {
                    id: id.to_owned(),
                    hide,
                },
            )?
            .is_some()
        {
            return Ok(());
        }
        let mut store = Self::open(path).context("open Home for moderation")?;
        let _authority = backup::lock_authority(path)?;
        store.operation(|s| s.moderate(id, hide))
    }
}
