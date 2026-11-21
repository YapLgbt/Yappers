// SPDX-License-Identifier: AGPL-3.0-only
//! Operator admission survives restarts independently of the replicated content log.
use super::*;
use crate::peering::Acceptance;
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    net::SocketAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

const PAGE: usize = 16;
const MAX_PEERS: usize = 1024;
const CONTACTS_PER_ROUND: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CovenantFederation {
    pub main_home: String,
    pub acceptance: Acceptance,
    #[serde(default = "automatic_admission")]
    pub admit_new: bool,
}

fn automatic_admission() -> bool {
    true
}

impl CovenantFederation {
    pub fn scope(&self) -> String {
        format!("{}/public", self.main_home)
    }
}

impl ServerConfig {
    pub fn enable_covenant(
        &mut self,
        main_home: String,
        acceptance: Acceptance,
        bootstrap: Vec<EndpointAddr>,
    ) -> Result<()> {
        acceptance.validate()?;
        let own = hex::encode(signing_key(&self.private_key)?.verifying_key().as_bytes());
        ensure!(acceptance.home() == own, "receipt belongs to another Home");
        ensure!(canonical_id(&main_home), "invalid main Home ID");
        let auto = &mut self.auto_federation;
        ensure!(
            auto.covenant
                .as_ref()
                .is_none_or(|previous| previous.main_home == main_home),
            "this Home already belongs to a different covenant network"
        );
        let covenant = CovenantFederation {
            main_home,
            acceptance,
            admit_new: auto
                .covenant
                .as_ref()
                .is_none_or(|previous| previous.admit_new),
        };
        let scope = covenant.scope();
        ensure!(
            self.communities.contains_key(&scope) || self.communities.len() < MAX_COMMUNITIES,
            "Home community limit reached"
        );
        let mut seeds = auto.seeds.clone();
        for seed in bootstrap {
            if let Some(existing) = seeds.iter_mut().find(|existing| existing.id == seed.id) {
                *existing = seed;
            } else {
                seeds.push(seed);
            }
        }
        ensure!(
            seeds.len() <= 16
                && (covenant.main_home == own
                    || seeds.iter().any(|seed| seed.id.to_string() != own)),
            "configure 1–16 bootstrap addresses for an outside Home"
        );
        self.communities.entry(scope).or_insert_with(new_secret);
        auto.enabled = true;
        auto.seeds = seeds;
        auto.covenant = Some(covenant);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CovenantPeer {
    pub acceptance: Acceptance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_addresses: Vec<SocketAddr>,
}

impl CovenantPeer {
    fn id(&self) -> &str {
        self.acceptance.home()
    }

    fn validate(&self, mode: NetworkMode) -> Result<()> {
        self.acceptance.validate()?;
        ensure!(
            self.local_addresses.len() <= 4
                && (self.local_addresses.is_empty() || matches!(mode, NetworkMode::Local))
                && self
                    .local_addresses
                    .iter()
                    .all(|addr| addr.ip().is_loopback() && addr.port() != 0),
            "invalid covenant peer address"
        );
        Ok(())
    }

    fn address(&self) -> Result<EndpointAddr> {
        let id = self.id().parse()?;
        Ok(EndpointAddr::from_parts(
            id,
            self.local_addresses
                .iter()
                .copied()
                .map(iroh::TransportAddr::Ip),
        ))
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Saved {
    network: Option<String>,
    peers: BTreeMap<String, CovenantPeer>,
    #[serde(default)]
    confirmed: BTreeSet<String>,
    stopped: BTreeSet<String>,
    revision: u64,
}

pub(super) struct CovenantRegistry {
    path: PathBuf,
    saved: RwLock<Saved>,
    rounds: AtomicUsize,
    after: Mutex<BTreeMap<String, String>>,
}

pub(super) fn canonical_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn public_group_scope(scope: &str) -> bool {
    scope
        .split_once("/groups/")
        .is_some_and(|(owner, id)| canonical_id(owner) && canonical_id(id))
}

impl CovenantRegistry {
    pub(super) fn open(database: &Path, network: Option<&str>, mode: NetworkMode) -> Result<Self> {
        let path = database.with_added_extension("federation.json");
        let mut saved: Saved = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_file() && metadata.permissions().mode() & 0o077 == 0,
                    "federation state must be a private regular file"
                );
                serde_json::from_slice(&fs::read(&path)?)?
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Saved::default(),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            saved.peers.len() <= MAX_PEERS && saved.stopped.len() <= MAX_PEERS,
            "federation state exceeds capacity"
        );
        for (id, peer) in &saved.peers {
            ensure!(peer.id() == id, "federation record belongs to another peer");
            peer.validate(mode)?;
        }
        ensure!(
            saved
                .confirmed
                .iter()
                .all(|id| saved.peers.contains_key(id)),
            "invalid confirmed peer state"
        );
        if let Some(network) = network {
            ensure!(
                saved
                    .network
                    .as_deref()
                    .is_none_or(|previous| previous == network),
                "federation state belongs to another network"
            );
            saved.network = Some(network.to_owned());
        }
        Ok(Self {
            path,
            saved: RwLock::new(saved),
            rounds: AtomicUsize::new(0),
            after: Mutex::default(),
        })
    }

    fn update(&self, edit: impl FnOnce(&mut Saved) -> Result<bool>) -> Result<()> {
        let mut current = self.saved.write().expect("covenant registry");
        let mut next = current.clone();
        if !edit(&mut next)? {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let temporary = parent.join(format!(".federation-{}.tmp", new_secret()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            serde_json::to_writer(&mut file, &next)?;
            file.flush()?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            *current = next;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    fn merge(
        &self,
        our_id: &str,
        remote: &str,
        direct: CovenantPeer,
        peers: Vec<CovenantPeer>,
        mode: NetworkMode,
        admit_new: bool,
    ) -> Result<()> {
        ensure!(
            direct.id() == remote && peers.len() <= PAGE,
            "covenant receipt does not match the connecting Home"
        );
        direct.validate(mode)?;
        for peer in &peers {
            peer.validate(mode)?;
        }
        self.update(|saved| {
            ensure!(
                admit_new || saved.peers.contains_key(remote),
                "new covenant admission is paused"
            );
            let mut changed = false;
            let mut new_admission = false;
            // An authenticated sender takes the first slot. A full catalogue may
            // continue exchanging with its current members.
            for peer in std::iter::once(direct).chain(peers) {
                let id = peer.id().to_owned();
                if id == our_id {
                    continue;
                }
                if let Some(prior) = saved.peers.get(&id) {
                    let issued = peer.acceptance.issued_at();
                    let previous = prior.acceptance.issued_at();
                    if peer == *prior || issued < previous || (issued == previous && id != remote) {
                        continue;
                    }
                } else if !admit_new {
                    continue;
                } else if saved.peers.len() >= MAX_PEERS {
                    ensure!(id != remote, "federation directory is full");
                    continue;
                } else {
                    new_admission = true;
                }
                saved.peers.insert(id, peer);
                changed = true;
            }
            if new_admission {
                saved.revision += 1;
            }
            if saved.peers.contains_key(remote) {
                changed |= saved.confirmed.insert(remote.to_owned());
            }
            Ok(changed)
        })
    }

    pub(super) fn stop(&self, id: &str) -> Result<()> {
        self.update(|saved| {
            if saved.stopped.contains(id) {
                return Ok(false);
            }
            ensure!(
                saved.stopped.len() < MAX_PEERS,
                "peer suspension list is full"
            );
            saved.stopped.insert(id.to_owned());
            saved.revision += 1;
            Ok(true)
        })
    }

    pub(super) fn stopped(&self, id: &str) -> bool {
        self.saved
            .read()
            .expect("covenant registry")
            .stopped
            .contains(id)
    }

    pub(super) fn revision(&self) -> u64 {
        self.saved.read().expect("covenant registry").revision
    }

    pub(super) fn admits(&self, id: &str) -> bool {
        self.saved
            .read()
            .expect("covenant registry")
            .peers
            .contains_key(id)
    }

    pub(super) fn confirmed(&self, id: &str) -> bool {
        self.saved
            .read()
            .expect("covenant registry")
            .confirmed
            .contains(id)
    }

    pub(super) fn address(&self, id: &str) -> Result<Option<EndpointAddr>> {
        self.saved
            .read()
            .expect("covenant registry")
            .peers
            .get(id)
            .map(CovenantPeer::address)
            .transpose()
    }

    pub(super) fn addresses(&self) -> Result<Vec<EndpointAddr>> {
        self.saved
            .read()
            .expect("covenant registry")
            .peers
            .values()
            .map(CovenantPeer::address)
            .collect()
    }

    pub(super) fn operator(&self, id: &str) -> Option<String> {
        self.saved
            .read()
            .expect("covenant registry")
            .peers
            .get(id)
            .map(|entry| entry.acceptance.operator().to_owned())
    }

    fn page(&self, after: Option<&str>) -> (Vec<CovenantPeer>, Option<String>) {
        let saved = self.saved.read().expect("covenant registry");
        let mut candidates = saved.peers.iter().filter(|(id, _)| {
            after.is_none_or(|cursor| id.as_str() > cursor) && !saved.stopped.contains(*id)
        });
        let listed: Vec<_> = candidates
            .by_ref()
            .take(PAGE)
            .map(|(_, entry)| entry.clone())
            .collect();
        let next = candidates
            .next()
            .and_then(|_| listed.last().map(|entry| entry.id().to_owned()));
        (listed, next)
    }
}

impl State {
    fn covenant_card(&self) -> Result<CovenantPeer> {
        let covenant = self
            .auto
            .covenant
            .as_ref()
            .context("automatic covenant admission is disabled")?;
        let local_addresses = if matches!(self.network_mode, NetworkMode::Local) {
            self.endpoint
                .addr()
                .ip_addrs()
                .filter(|address| address.ip().is_loopback())
                .take(4)
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        Ok(CovenantPeer {
            acceptance: covenant.acceptance.clone(),
            local_addresses,
        })
    }

    pub(super) fn exchange_covenants(
        &self,
        remote: &str,
        network: &str,
        peer: CovenantPeer,
        after: Option<&str>,
    ) -> Result<Response> {
        let own = self.covenant_card()?;
        let covenant = self.auto.covenant.as_ref().unwrap();
        ensure!(
            covenant.main_home == network,
            "different federation network"
        );
        ensure!(
            after.is_none_or(canonical_id),
            "invalid federation directory cursor"
        );
        ensure!(
            !self.covenants.stopped(remote) && !self.auto.blocked_hosts.contains(remote),
            "peer suspended"
        );
        self.covenants.merge(
            own.id(),
            remote,
            peer,
            Vec::new(),
            self.network_mode,
            covenant.admit_new,
        )?;
        let (peers, next) = self.covenants.page(after);
        Ok(Response::CovenantPeers {
            peer: own,
            peers,
            next,
        })
    }
}

impl Server {
    pub(super) async fn discover_covenants(&self) -> Result<()> {
        let state = &self.state;
        let Some(covenant) = &state.auto.covenant else {
            return Ok(());
        };
        let own = state.covenant_card()?;
        let mut destinations: BTreeMap<_, _> = state
            .covenants
            .addresses()?
            .into_iter()
            .map(|address| (address.id, address))
            .collect();
        for address in state
            .seeds
            .read()
            .map_err(|_| anyhow::anyhow!("seed policy unavailable"))?
            .iter()
        {
            destinations.insert(address.id, address.clone());
        }
        let destinations: Vec<_> = destinations
            .into_values()
            .filter(|address| {
                let id = address.id.to_string();
                id != own.id()
                    && !state.covenants.stopped(&id)
                    && !state.auto.blocked_hosts.contains(&id)
                    && (covenant.admit_new || state.covenants.admits(&id))
            })
            .collect();
        let round = state
            .covenants
            .rounds
            .fetch_add(CONTACTS_PER_ROUND, Ordering::Relaxed);
        let selected = (0..destinations.len().min(CONTACTS_PER_ROUND))
            .map(|index| destinations[round.wrapping_add(index) % destinations.len()].clone());
        let mut failures = Vec::new();
        let mut requests = stream::iter(selected.map(|address| {
            let own = own.clone();
            async move {
                let id = address.id.to_string();
                let after = state
                    .covenants
                    .after
                    .lock()
                    .expect("covenant page cursors")
                    .get(&id)
                    .cloned();
                let Response::CovenantPeers { peer, peers, next } = transport::rpc(
                    &self.endpoint,
                    address,
                    &Request::CovenantPeers {
                        network: covenant.main_home.clone(),
                        peer: own.clone(),
                        after: after.clone(),
                    },
                )
                .await
                .with_context(|| id.clone())?
                else {
                    bail!("unexpected covenant directory response");
                };
                ensure!(
                    next.as_ref().is_none_or(|next| canonical_id(next)
                        && after.as_ref().is_none_or(|cursor| next > cursor)),
                    "invalid federation directory progress"
                );
                let admit_new = covenant.admit_new;
                let state = state.clone();
                tokio::task::spawn_blocking(move || {
                    state.covenants.merge(
                        own.id(),
                        &id,
                        peer,
                        peers,
                        state.network_mode,
                        admit_new,
                    )?;
                    let mut cursors = state.covenants.after.lock().expect("covenant page cursors");
                    if let Some(next) = next {
                        cursors.insert(id, next);
                    } else {
                        cursors.remove(&id);
                    }
                    anyhow::Ok(())
                })
                .await??;
                Ok(())
            }
        }))
        .buffered_unordered(CONTACTS_PER_ROUND);
        while let Some(result) = requests.next().await {
            if let Err(error) = result {
                failures.push(format!("{error:#}"));
            }
        }
        ensure!(
            failures.is_empty(),
            "covenant discovery: {}",
            failures.join(", ")
        );
        Ok(())
    }
}
