// SPDX-License-Identifier: AGPL-3.0-only
//! Home admission and explicitly scoped pull replication.
//!
//! A membership capability authorizes feed/submit. Federation instead authenticates
//! the connecting endpoint and independently trusts each event's original home.
//! Do not infer origin trust from the peer that forwarded an envelope.

use crate::{MAX_FRAME, MAX_PAGE_ENTRIES, RPC_TIMEOUT_SECS, model::*, store::Store, transport};
use anyhow::{Context, Result, bail, ensure};
use ed25519_dalek::SigningKey;
use iroh::{Endpoint, EndpointAddr};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, RwLock},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{sync::Semaphore, task::JoinHandle};

#[path = "auto_federation.rs"]
mod auto_federation;
pub use auto_federation::AutoFederationConfig;
use std::sync::atomic::AtomicUsize;

const MAX_COMMUNITIES: usize = 32;
const MAX_PEERS: usize = 16;
const MAX_CONNECTIONS: usize = 64;
const MAX_SYNC_PAGES: usize = 20;
const AUTHOR_EVENTS_PER_MINUTE: usize = 120;
const RATE_WINDOW_SECS: i64 = 60;

/// A transport identity's explicitly allowed community scopes; never a membership token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeerConfig {
    pub address: EndpointAddr,
    pub communities: BTreeSet<String>,
}
/// Private operator configuration. Do not log or derive Debug: it contains credentials.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerConfig {
    /// Public projections to cache and expose anonymously, keyed by founding home ID/public.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub public_replica_feeds: BTreeSet<String>,
    pub private_key: String,
    pub database_path: PathBuf,
    pub communities: BTreeMap<String, String>,
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
    #[serde(default)]
    pub network_mode: NetworkMode,
    #[serde(default)]
    pub auto_federation: AutoFederationConfig,
}
/// Running home instance. Call [`Server::close`] to stop its accept task and endpoint.
pub struct Server {
    pub endpoint: Endpoint,
    state: Arc<State>,
    accept_task: JoinHandle<()>,
}
#[path = "ha.rs"]
mod ha;
#[path = "replica.rs"]
mod replica;
use crate::peering::{Access, Policy};
pub use ha::{HaOptions, HaRole, export_ha, moderate_ha};

struct State {
    reports: Option<Mutex<crate::report_inbox::Inbox>>,
    peering: Option<Policy>,
    _process_lock: std::fs::File,
    _guard_lock: Option<std::fs::File>,
    ha: Option<ha::Ha>,
    public_replica_feeds: BTreeSet<String>,
    endpoint: Endpoint,
    network_mode: NetworkMode,
    auto: AutoFederationConfig,
    seeds: RwLock<Vec<EndpointAddr>>,
    discovery_round: AtomicUsize,
    replication_round: AtomicUsize,
    generation: String,
    key: SigningKey,
    store: Mutex<Store>,
    communities: BTreeMap<String, String>,
    peers: RwLock<Vec<PeerConfig>>,
}
impl State {
    fn store(&self) -> Result<MutexGuard<'_, Store>> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("store unavailable"))
    }

    fn access(&self, id: &str, community: &str) -> Option<Access> {
        match &self.peering {
            Some(policy) if id != self.logical_addr().id.to_string() => {
                policy.access(id, community)
            }
            _ => Some(Access::Active),
        }
    }
    fn peer(&self, id: &str) -> Result<PeerConfig> {
        let mut peer = self.peer_for_pull(id)?;
        peer.communities
            .retain(|scope| self.access(id, scope) == Some(Access::Active));
        ensure!(!peer.communities.is_empty(), "peer suspended");
        Ok(peer)
    }
    fn peer_for_pull(&self, id: &str) -> Result<PeerConfig> {
        ensure!(
            !self.auto.blocked_hosts.contains(id),
            "federation host blocked"
        );
        let explicit = self
            .peers
            .read()
            .map_err(|_| anyhow::anyhow!("peer policy unavailable"))?
            .iter()
            .find(|p| p.address.id.to_string() == id)
            .cloned();
        let automatic = self.automatic_peer(id)?;
        let mut peer = match (explicit, automatic) {
            (Some(mut peer), Some(auto)) => {
                peer.communities.extend(auto.communities);
                peer
            }
            (Some(peer), None) | (None, Some(peer)) => peer,
            _ => bail!("untrusted federation endpoint"),
        };
        peer.communities
            .retain(|scope| self.access(id, scope).is_some());
        ensure!(
            !peer.communities.is_empty(),
            "peer covenant approval required"
        );
        Ok(peer)
    }

    fn member(&self, community: &str, token: &str) -> Result<()> {
        let expected = match self.communities.get(community) {
            Some(token) => token.clone(),
            None => {
                ensure!(
                    self.auto.enabled && self.store()?.hosts_group(community)?,
                    "unknown community"
                );
                self.group_token(community)
            }
        };
        ensure!(
            bool::from(expected.as_bytes().ct_eq(token.as_bytes())),
            "invalid community capability"
        );
        Ok(())
    }
    fn validate_import(&self, envelope: &Envelope) -> Result<()> {
        envelope.validate()?;
        ensure!(
            !matches!(
                envelope.event.payload.action,
                Action::PostV2 {
                    profile: Some(_),
                    ..
                }
            ),
            "federated posts must omit profile details"
        );
        if let Some(proof) = &envelope.deletion {
            self.validate_import(proof)?;
        }
        let community = &envelope.event.payload.community;
        let origin = &envelope.event.payload.home;
        ensure!(!self.auto.blocked_hosts.contains(origin), "origin blocked");
        match self.access(origin, community) {
            Some(Access::Active) => {}
            Some(Access::Suspended)
                if matches!(
                    envelope.event.payload.action,
                    Action::Delete { .. } | Action::Erased { .. }
                ) => {}
            _ => bail!("origin lacks covenant approval for this content"),
        }
        if self.auto.enabled && self.store()?.hosts_group(community)? {
            let store = self.store()?;
            let descriptor = store
                .group(community)?
                .context("missing group descriptor")?;
            let permit = store
                .permit(community, origin)?
                .context("origin lacks group authority permission")?;
            return permit.validate(&descriptor);
        }
        ensure!(
            self.communities.contains_key(community),
            "community not hosted here"
        );
        if *origin != hex::encode(self.key.verifying_key().as_bytes()) {
            ensure!(
                self.peer_for_pull(origin)?
                    .communities
                    .contains(&envelope.event.payload.community),
                "origin not authorized for community"
            );
        }
        Ok(())
    }
    fn logical_addr(&self) -> EndpointAddr {
        let logical = iroh::SecretKey::from_bytes(&self.key.to_bytes()).public();
        if logical == self.endpoint.id() {
            self.endpoint.addr()
        } else {
            EndpointAddr::new(logical)
        }
    }
    async fn dispatch(&self, remote: &str, call: Request) -> Result<Response> {
        match call {
            Request::JoinGroup {
                token,
                community,
                group,
            } => {
                self.member(&community, &token)?;
                self.join_group(&group).await?;
                self.local_directory()?;
                Ok(Response::GroupJoined {
                    join: self.public_join(&group)?,
                    group: Box::new(self.group_summary(&group)?),
                })
            }
            call => self.handle(remote, call),
        }
    }
    fn report_inbox(&self) -> Result<MutexGuard<'_, crate::report_inbox::Inbox>> {
        self.reports
            .as_ref()
            .context("this home has no staffed report inbox")?
            .lock()
            .map_err(|_| anyhow::anyhow!("report inbox unavailable"))
    }
    fn handle(&self, remote: &str, request: Request) -> Result<Response> {
        match request {
            Request::ReportAvailability => {
                ensure!(
                    self.reports.is_some(),
                    "reporting is unavailable on this home"
                );
                Ok(Response::ReportAvailable)
            }
            Request::SubmitReport {
                token,
                community,
                receipt,
                evidence,
            } => {
                self.member(&community, &token)?;
                let result = self.report_inbox()?.submit(
                    &self.endpoint.id().to_string(),
                    &receipt,
                    &evidence,
                    now(),
                )?;
                Ok(Response::ReportResult { result })
            }
            Request::ReportStatus { receipt } => {
                let result = self.report_inbox()?.lookup(
                    &self.endpoint.id().to_string(),
                    &receipt,
                    now(),
                )?;
                Ok(Response::ReportResult { result })
            }
            Request::ServerSource => {
                let revision = option_env!("YAP_BUILD_SOURCE_SHA256")
                    .context("this build has no published source revision")?;
                let base = std::env::var("YAP_SOURCE_BASE_URL")
                    .context("public source URL not configured")?;
                Ok(Response::ServerSource {
                    source_url: format!("{}/{revision}.tar.gz", base.trim_end_matches('/')),
                    revision: revision.into(),
                })
            }
            Request::HomeOperation { .. } => bail!("unexpected HA operation wrapper"),
            Request::Social {
                token,
                community,
                account,
                proof,
                op,
            } => {
                self.member(&community, &token)?;
                let home = self.logical_addr().id.to_string();
                let mut store = self.store()?;
                let chain = store.account_chain(&account)?;
                let record = chain
                    .last()
                    .context("register your account before messaging")?;
                ensure!(record.payload.home == home, "account moved to another home");
                proof.validate(
                    record,
                    "social-v1",
                    &home,
                    &crate::portability::digest(&(remote, &community, &op))?,
                )?;
                store.social(&home, &community, record, &proof.device, *op)
            }
            Request::AnnounceAccount { chain } => {
                let record = crate::portability::validate_chain(&chain)?;
                let mut store = self.store()?;
                // Relaying a root-signed update needs no membership token, but may only
                // advance an already-known account. It cannot allocate arbitrary accounts.
                ensure!(
                    !store.account_chain(&record.payload.account)?.is_empty(),
                    "account not known here"
                );
                store.register_account(&chain)?;
                Ok(Response::AccountState {
                    social: true,
                    privacy: true,
                    chain: store.account_chain(&record.payload.account)?,
                })
            }
            Request::AccountLookup {
                token,
                community,
                account,
            } => {
                self.member(&community, &token)?;
                crate::model::public_key(&account)?;
                Ok(Response::AccountState {
                    social: true,
                    privacy: true,
                    chain: self.store()?.account_chain(&account)?,
                })
            }
            Request::PrivateGroup {
                token,
                community,
                account,
                proof,
                op,
            } => {
                self.member(&community, &token)?;
                let home = hex::encode(self.key.verifying_key().as_bytes());
                let mut store = self.store()?;
                let chain = store.account_chain(&account)?;
                let record = chain
                    .last()
                    .context("register your account with this home first")?;
                ensure!(
                    record.payload.home == home,
                    "private group account belongs to another home"
                );
                let digest = crate::portability::digest(&(remote, &community, &op))?;
                proof.validate(record, "private-group-v1", &home, &digest)?;
                let result = store.private_group(&home, &account, &proof.device, *op)?;
                Ok(Response::PrivateGroup {
                    result: Box::new(result),
                })
            }
            Request::AccountRegister {
                token,
                community,
                chain,
                proof,
            } => {
                self.member(&community, &token)?;
                let record = crate::portability::validate_chain(&chain)?;
                let own = hex::encode(self.key.verifying_key().as_bytes());
                proof.validate(
                    record,
                    "register",
                    &own,
                    &crate::portability::digest(&chain)?,
                )?;
                ensure!(record.payload.home == own, "account names another home");
                let mut store = self.store()?;
                let current = store.register_account(&chain)?;
                ensure!(
                    current.payload.home == own && current.payload.devices.contains(&proof.device),
                    "account moved or device revoked"
                );
                Ok(Response::AccountState {
                    social: true,
                    privacy: true,
                    chain: store.account_chain(&record.payload.account)?,
                })
            }
            Request::AccountExport {
                token,
                community,
                chain,
                proof,
                after,
            } => {
                self.member(&community, &token)?;
                let record = crate::portability::validate_chain(&chain)?;
                let own = hex::encode(self.key.verifying_key().as_bytes());
                proof.validate(
                    record,
                    "export",
                    &own,
                    &crate::portability::digest(&(&community, &after))?,
                )?;
                let mut store = self.store()?;
                let current = store.register_account(&chain)?;
                // Reads of an old home's retained history remain possible after a move, using
                // an active key in the latest known account record. Membership still applies.
                ensure!(
                    current.payload.devices.contains(&proof.device),
                    "export device revoked"
                );
                let all = store.owned_events(&record.payload.account)?;
                let mut events = Vec::new();
                let mut more = false;
                for event in all
                    .into_iter()
                    .filter(|e| after.as_ref().is_none_or(|id| e.event.id > *id))
                {
                    events.push(event);
                    if events.len() > 50 || serde_json::to_vec(&events)?.len() > MAX_FRAME / 2 {
                        events.pop();
                        more = true;
                        break;
                    }
                }
                ensure!(
                    !more || !events.is_empty(),
                    "single owned event exceeds export frame"
                );
                let next = events.last().map(|e| e.event.id.clone());
                Ok(Response::OwnedEvents { events, next, more })
            }
            Request::AccountImport {
                token,
                community,
                chain,
                proof,
                events,
            } => {
                self.member(&community, &token)?;
                let record = crate::portability::validate_chain(&chain)?;
                let own = hex::encode(self.key.verifying_key().as_bytes());
                ensure!(
                    record.payload.home == own,
                    "import requires the account's chosen home"
                );
                proof.validate(
                    record,
                    "import",
                    &own,
                    &crate::portability::digest(&events)?,
                )?;
                // Recovery can deliver a home-accepted deletion before federation does.
                // Only a receipt that passes this home's normal federation policy may
                // affect the shared feed. All other imported history stays private.
                let admitted_deletes: Vec<_> = events
                    .iter()
                    .filter_map(|entry| {
                        if matches!(entry.event.payload.action, Action::Delete { .. }) {
                            Some(entry)
                        } else {
                            entry.deletion.as_deref()
                        }
                    })
                    .filter(|deletion| self.validate_import(deletion).is_ok())
                    .collect();
                let mut store = self.store()?;
                let current = store.register_account(&chain)?;
                ensure!(
                    current == *record && current.payload.devices.contains(&proof.device),
                    "stale or revoked import device"
                );
                store.import_history(&record.payload.account, &events, &admitted_deletes)?;
                Ok(Response::PersonalImported {
                    count: events.len(),
                })
            }
            Request::Directory { snapshot } => {
                self.import_public_directory(&snapshot)?;
                Ok(Response::Directory {
                    snapshot: self.local_directory()?,
                })
            }
            Request::EnrollHost { group } => Ok(Response::HostPermit {
                permit: self.enroll(remote, &group)?,
            }),
            Request::Groups { token, community } => {
                self.member(&community, &token)?;
                ensure!(self.auto.enabled, "automatic federation is disabled");
                self.local_directory()?;
                Ok(Response::Groups {
                    groups: self.store()?.group_summaries()?,
                })
            }
            Request::CreateGroup {
                token,
                community,
                details,
            } => {
                self.member(&community, &token)?;
                let group = self.create_group(details)?;
                self.local_directory()?;
                Ok(Response::GroupJoined {
                    join: self.public_join(&group.id)?,
                    group: Box::new(self.group_summary(&group.id)?),
                })
            }
            Request::JoinGroup { .. } => bail!("async group admission required"),
            Request::PublicReplicaFeed { community } => self.replica_feed(&community),
            Request::PeerProfiles { community, known } => {
                ensure!(
                    known.len() <= 100 && self.peer(remote)?.communities.contains(&community),
                    "profile scope denied"
                );
                let store = self.store()?;
                let home = self.logical_addr().id.to_string();
                let owners = store.named_authors(&home, &community)?;
                let mut records = std::collections::BTreeMap::new();
                for (owner, revision) in known {
                    if owners.contains(&owner)
                        && let Some(record) = store.current_profile(&home, &owner)?
                        && record.revision > revision
                    {
                        records.insert(owner, record);
                    }
                }
                Ok(Response::Social {
                    result: serde_json::to_value(records)?,
                })
            }
            Request::PullPublicProjection { community, digest } => {
                self.public_projection(remote, &community, digest.as_deref())
            }
            Request::Pull {
                after,
                generation,
                limit,
            } => {
                let peer = self.peer_for_pull(remote)?;
                let deletions_only = peer
                    .communities
                    .iter()
                    .all(|scope| self.access(remote, scope) == Some(Access::Suspended));
                let store = self.store()?;
                let page = store.peer_page(
                    if generation.as_deref() == Some(&self.generation) {
                        after
                    } else {
                        0
                    },
                    limit,
                    &peer.communities,
                    &self.generation,
                    deletions_only.then_some(remote),
                )?;
                if !deletions_only
                    && self.peering.as_ref().is_some_and(|p| p.external(remote))
                    && let Response::Page { entries, .. } = &page
                {
                    store.remember_deliveries(
                        remote,
                        entries
                            .iter()
                            .map(|e| &e.envelope.event)
                            .filter(|e| {
                                matches!(
                                    e.payload.action,
                                    Action::Post { .. }
                                        | Action::PostV2 { .. }
                                        | Action::Reply { .. }
                                )
                            })
                            .map(|e| (e.payload.community.as_str(), e.id.as_str())),
                    )?;
                }
                Ok(page)
            }
            Request::Feed {
                token,
                community,
                author: _,
                nearby,
            } => {
                self.member(&community, &token)?;
                let store = self.store()?;
                let events = store.events(&community)?;
                if let Some(filter) = &nearby {
                    filter.validate()?;
                }
                let mut posts =
                    project_nearby(&events, &community, "", &store.hidden()?, nearby.as_ref());
                store.apply_current_bylines(&mut posts, &events)?;
                let account_chain = Vec::new();
                let deleted = store.erased_ids(&community)?;
                let account_bytes =
                    serde_json::to_vec(&account_chain)?.len() + serde_json::to_vec(&deleted)?.len();
                // Thumbnail-bearing feeds must still fit the bounded transport frame.
                // Retain the newest complete conversations rather than failing the whole RPC.
                while serde_json::to_vec(&posts)?.len() + account_bytes > MAX_FRAME - 1024 {
                    posts.pop();
                }
                Ok(Response::Feed {
                    deleted,
                    direct: false,
                    privacy: false,
                    posts,
                    event_count: events.len(),
                    account_chain,
                })
            }
            Request::Submit {
                token,
                event,
                account_chain,
            } => {
                self.member(&event.payload.community, &token)?;
                ensure!(
                    !matches!(
                        event.payload.action,
                        Action::PostV2 {
                            profile: Some(_),
                            ..
                        }
                    ),
                    "save public profiles separately from posts"
                );
                event.validate()?;
                ensure!(
                    event.payload.version == 4
                        && !matches!(event.payload.action, Action::Erased { .. }),
                    "submit a complete v4 event"
                );
                ensure!(
                    event.payload.home == hex::encode(self.key.verifying_key().as_bytes()),
                    "submit to the event's designated home"
                );
                let mut store = self.store()?;
                if let Some(existing) = store.existing(&event.id)? {
                    ensure!(
                        existing.event.header()? == event.header()?
                            && existing.event.signature == event.signature,
                        "conflicting event"
                    );
                    return store.acceptance(existing);
                }
                if let Some(record) = &event.payload.account {
                    let supplied = crate::portability::validate_chain(&account_chain)?;
                    ensure!(
                        supplied.payload.account == record.payload.account,
                        "account chain mismatch"
                    );
                    ensure!(
                        supplied == record.as_ref(),
                        "event account state not in supplied chain"
                    );
                    let current = store.register_account(&account_chain)?;
                    ensure!(
                        current == **record
                            && current.payload.home == event.payload.home
                            && current.payload.devices.contains(&event.payload.author),
                        "account moved or device revoked"
                    );
                } else {
                    ensure!(account_chain.is_empty(), "legacy event has account chain");
                    ensure!(
                        store.account_chain(&event.payload.author)?.is_empty(),
                        "legacy key is now a recovery authority; use an authorized device"
                    );
                }
                let events = store.events(&event.payload.community)?;
                let target = match &event.payload.action {
                    Action::Reply { parent, .. } => Some(parent),
                    Action::Vote { target, .. } | Action::Delete { target } => Some(target),
                    _ => None,
                };
                if let Some(target) = target {
                    let parent = events
                        .iter()
                        .find(|e| &e.event.id == target)
                        .context("target not available in this community; refresh first")?;
                    if matches!(
                        event.payload.action,
                        Action::Reply { .. } | Action::Vote { .. }
                    ) {
                        ensure!(
                            matches!(
                                parent.event.payload.action,
                                Action::Post { .. } | Action::PostV2 { .. }
                            ),
                            "target must be a post"
                        );
                    }
                    if matches!(event.payload.action, Action::Delete { .. }) {
                        ensure!(
                            matches!(
                                parent.event.payload.action,
                                Action::Post { .. }
                                    | Action::PostV2 { .. }
                                    | Action::Reply { .. }
                                    | Action::Erased { .. }
                            ) && parent.event.owner_id() == event.owner_id(),
                            "only the author may delete"
                        );
                    }
                }
                let recent = events
                    .iter()
                    .filter(|e| {
                        e.event.owner_id() == event.owner_id()
                            && e.accepted_at >= now() - RATE_WINDOW_SECS
                    })
                    .count();
                ensure!(
                    recent < AUTHOR_EVENTS_PER_MINUTE,
                    "author rate limit; retry later"
                );
                let username = store.sign_username(&event, &self.key)?;
                let envelope = Envelope::accept(event, &self.key)?;
                envelope.validate()?;
                store.accept_with_username(&envelope, username.as_ref())?;
                let envelope = store
                    .existing(&envelope.event.id)?
                    .context("missing accepted event")?;
                store.acceptance(envelope)
            }
        }
    }
}
fn validate_peers(
    peers: &[PeerConfig],
    own_id: &str,
    communities: &BTreeMap<String, String>,
) -> Result<()> {
    ensure!(peers.len() <= MAX_PEERS, "too many federation peers");
    let mut ids = BTreeSet::new();
    for peer in peers {
        ensure!(
            peer.address.id.to_string() != own_id && ids.insert(peer.address.id),
            "duplicate or self peer"
        );
        ensure!(
            !peer.communities.is_empty()
                && peer.communities.iter().all(|c| communities.contains_key(c)),
            "invalid peer community scope"
        );
    }
    Ok(())
}

fn lock_server(path: &std::path::Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_added_extension("serve-lock"))?;
    file.try_lock()
        .context("a home process is already using this database")?;
    Ok(file)
}
fn mark_ha(path: &std::path::Path) -> Result<()> {
    let marker = path.with_added_extension("ha-managed");
    if !marker.try_exists()? {
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&marker)?;
        file.sync_all()?;
        std::fs::File::open(marker.parent().context("missing staging directory")?)?.sync_all()?;
    }
    Ok(())
}
fn retire_seed(path: &std::path::Path, store: Store) -> Result<()> {
    let tx = store.db.unchecked_transaction()?;
    tx.execute_batch(
        "DELETE FROM guard.opening_keys; UPDATE guard.state SET key=randomblob(32), epoch=epoch+1;
        DELETE FROM public_projections; DELETE FROM personal_events; DELETE FROM events;",
    )?;
    tx.commit()?;
    drop(store);
    std::fs::remove_file(path)?;
    Ok(())
}
impl Server {
    /// Validate configuration and open storage before creating long-lived accept tasks.
    pub async fn start(config: ServerConfig) -> Result<Self> {
        Self::start_inner(config, None).await
    }
    /// Seed an empty authority once using a quiesced dedicated database and a client-only endpoint.
    pub async fn bootstrap_ha(
        config: ServerConfig,
        grant: crate::takeover::FullHomeStandby,
    ) -> Result<()> {
        let key = signing_key(&config.private_key)?;
        let home = hex::encode(key.verifying_key().as_bytes());
        grant.validate(&home)?;
        ensure!(
            config.database_path.is_file(),
            "bootstrap requires the existing dedicated database"
        );
        let _exclusive = lock_server(&config.database_path)?;
        let store = Store::open_staging(&config.database_path)?;
        let _guard_lock = crate::backup::lock_authority(&config.database_path)?;
        let image = store.snapshot()?;
        let endpoint = transport::bind(
            config.network_mode,
            Some(iroh::SecretKey::from_bytes(&key.to_bytes())),
            false,
        )
        .await?;
        let ha = ha::Ha::new(
            &HaOptions {
                grant,
                role: HaRole::Primary,
            },
            &config,
            Policy::load()?.as_ref(),
        )?;
        let result = ha.seed_image(&endpoint, image).await;
        endpoint.close().await;
        result?;
        mark_ha(&config.database_path)?;
        // The authority is now durable. Retire the bootstrap copy and invalidate
        // its separate backup keys before future primary/standby service.
        retire_seed(&config.database_path, store)
    }
    pub async fn start_ha(config: ServerConfig, options: HaOptions) -> Result<Self> {
        Self::start_inner(config, Some(options)).await
    }
    async fn start_inner(config: ServerConfig, options: Option<HaOptions>) -> Result<Self> {
        ensure!(
            !config.communities.is_empty() && config.communities.len() <= MAX_COMMUNITIES,
            "configure 1..32 communities"
        );
        for (community, token) in &config.communities {
            ensure!(
                !community.is_empty()
                    && community.len() <= 160
                    && community.is_ascii()
                    && (32..=256).contains(&token.len()),
                "invalid community configuration"
            );
        }
        ensure!(
            config.auto_federation.seeds.len() <= 16
                && config.auto_federation.blocked_hosts.len() <= 128,
            "automatic federation policy exceeds capacity"
        );
        let key = signing_key(&config.private_key)?;
        let own_id = hex::encode(key.verifying_key().as_bytes());
        validate_peers(&config.peers, &own_id, &config.communities)?;
        for community in &config.public_replica_feeds {
            ensure!(
                config
                    .peers
                    .iter()
                    .any(|p| community == &format!("{}/public", p.address.id)
                        && p.communities.contains(community)),
                "replica requires its founding public community peer"
            );
        }
        let process_lock = lock_server(&config.database_path)?;
        let store = if options.is_some() {
            ensure!(
                !config.database_path.exists(),
                "HA staging path contains a previous database; complete the verified bootstrap retirement before serving"
            );
            // HA workers materialize the authority's fenced image in RAM on each
            // revision change. An offline worker leaves no stale content on disk.
            Store {
                db: rusqlite::Connection::open_in_memory()?,
            }
        } else {
            Store::open(&config.database_path)?
        };
        let guard_lock = if options.is_none() {
            let lock = crate::backup::lock_authority(&config.database_path)?;
            crate::backup::verify_epoch(&store.db)?;
            store.recover_local_first_seen(&own_id)?;
            Some(lock)
        } else {
            None
        };
        let mut transport_key = key.clone();

        if let Some(options) = &options {
            options.grant.validate(&own_id)?;
            match &options.role {
                HaRole::Primary => {}
                HaRole::Standby {
                    transport_key: secret,
                    ledger,
                } => {
                    ensure!(
                        *ledger != config.database_path,
                        "ledger and staging store must differ"
                    );
                    transport_key = signing_key(secret)?;
                    ensure!(
                        hex::encode(transport_key.verifying_key().as_bytes())
                            == options.grant.standby.id.to_string(),
                        "wrong standby transport identity"
                    );
                }
            }
        }
        let peering = Policy::load()?;
        let ha = options
            .as_ref()
            .map(|o| ha::Ha::new(o, &config, peering.as_ref()))
            .transpose()?;
        if ha.is_some() {
            mark_ha(&config.database_path)?;
        }
        let secret = iroh::SecretKey::from_bytes(&transport_key.to_bytes());
        let endpoint = transport::bind(config.network_mode, Some(secret), true).await?;
        let reports = if endpoint.id().to_string() == own_id {
            std::env::var_os("YAP_REPORT_INBOX")
                .map(|path| {
                    crate::report_inbox::Inbox::open(std::path::Path::new(&path), Some(&own_id))
                        .map(Mutex::new)
                })
                .transpose()?
        } else {
            None
        };
        let state = Arc::new(State {
            reports,
            peering,
            _process_lock: process_lock,
            _guard_lock: guard_lock,
            ha,
            public_replica_feeds: config.public_replica_feeds,
            endpoint: endpoint.clone(),
            network_mode: config.network_mode,
            seeds: RwLock::new(config.auto_federation.seeds.clone()),
            auto: config.auto_federation,
            discovery_round: AtomicUsize::new(0),
            replication_round: AtomicUsize::new(0),
            generation: new_secret(),
            key,
            store: Mutex::new(store),
            communities: config.communities,
            peers: RwLock::new(config.peers),
        });
        let accept_endpoint = endpoint.clone();
        let accept_state = state.clone();
        let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let accept_task = tokio::spawn(async move {
            let mut sweep = tokio::time::interval(Duration::from_secs(60));
            loop {
                let incoming = tokio::select! {
                    incoming=accept_endpoint.accept()=>match incoming{Some(incoming)=>incoming,None=>break},
                    _=sweep.tick(), if accept_state.reports.is_some()=>{
                        if accept_state.report_inbox().and_then(|mut inbox|inbox.purge(now())).is_err(){eprintln!("Report inbox expiry failed");}
                        continue;
                    }
                };
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    incoming.refuse();
                    continue;
                };
                let state = accept_state.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = tokio::time::timeout(Duration::from_secs(RPC_TIMEOUT_SECS), async {
                        let connection = incoming.await?;
                        let (mut send, mut recv) = connection.accept_bi().await?;
                        let remote=connection.remote_id().to_string();
                        let mut first=[0u8;1];
                        recv.read_exact(&mut first).await?;
                        if first[0]==ha::CONTROL {
                            let ha=state.ha.as_ref().context("HA control disabled")?;
                            let bytes=ha.control(&remote,&mut recv).await?;
                            send.write_all(&bytes).await?;
                            send.finish()?;
                            connection.closed().await;
                            return anyhow::Ok(());
                        }
                        let mut bytes=first.to_vec();
                        bytes.extend_from_slice(&recv.read_to_end(MAX_FRAME-1).await?);
                        let response = match serde_json::from_slice(&bytes).context("invalid request") {
                            Ok(request) if state.ha.is_some() => {
                                let (id,call)=match request {
                                    Request::HomeOperation{request_id,call}=>(request_id,*call),
                                    other=>(new_secret(),other)
                                };
                                state.ha.as_ref().unwrap().request(&state,&remote,id,call).await.unwrap_or_else(|_|Response::Error{code:"ha_unavailable".into(),message:"Home commit authority unavailable; retry this operation".into()})
                            }
                            Ok(call)=>state.dispatch(&remote,call).await.unwrap_or_else(|e|Response::Error{code:"rejected".into(),message:e.to_string()}),
                            Err(e)=>Response::Error{code:"rejected".into(),message:e.to_string()},
                        };
                        let mut response = crate::transport::encode_response(&response)?;
                        if response.len() > MAX_FRAME {
                            response = serde_json::to_vec(&Response::Error {
                                code: "capacity".into(),
                                message: "feed exceeds v1 response capacity".into(),
                            })?;
                        }
                        send.write_all(&response).await?;
                        send.finish()?;
                        // Keep streams alive until the client acknowledges receipt by closing.
                        connection.closed().await;
                        anyhow::Ok(())
                    })
                    .await;
                });
            }
        });
        Ok(Self {
            endpoint,
            state,
            accept_task,
        })
    }
    /// Atomically replace peer policy. Removal prevents future exchange, not history recall.
    pub fn set_peers(&self, peers: Vec<PeerConfig>) -> Result<()> {
        validate_peers(
            &peers,
            &self.endpoint.id().to_string(),
            &self.state.communities,
        )?;
        *self
            .state
            .peers
            .write()
            .map_err(|_| anyhow::anyhow!("peer policy unavailable"))? = peers;
        Ok(())
    }
    pub fn join(&self, community: &str, mode: NetworkMode) -> Result<JoinConfig> {
        Ok(JoinConfig {
            protocol_version: crate::PROTOCOL_VERSION,
            server_addr: self.endpoint.addr(),
            community: community.to_owned(),
            access_token: self
                .state
                .communities
                .get(community)
                .context("unknown community")?
                .clone(),
            network_mode: mode,
        })
    }
    /// Attempt every peer; return examined entries (including duplicates), or aggregated errors.
    pub async fn sync_once(&self) -> Result<usize> {
        if let Some(ha) = &self.state.ha {
            return ha.synchronize(self).await;
        }
        self.sync_inner().await
    }
    async fn sync_inner(&self) -> Result<usize> {
        let mut peers = self
            .state
            .peers
            .read()
            .map_err(|_| anyhow::anyhow!("peer policy unavailable"))?
            .clone();
        let mut imported = 0;
        let mut errors = Vec::new();
        if let Err(error) = self.discover_once().await {
            errors.push(format!("discovery: {error:#}"));
        }
        for automatic in self.select_auto_peers()? {
            if let Some(existing) = peers
                .iter_mut()
                .find(|p| p.address.id == automatic.address.id)
            {
                existing.communities.extend(automatic.communities);
            } else {
                peers.push(automatic);
            }
        }
        for peer in peers {
            if !peer.communities.iter().any(|scope| {
                self.state
                    .access(&peer.address.id.to_string(), scope)
                    .is_some()
            }) {
                continue;
            }
            match self.sync_peer_inner(&peer).await {
                Ok(count) => imported += count,
                Err(error) => errors.push(format!("{}: {error:#}", peer.address.id)),
            }
        }
        ensure!(
            errors.is_empty(),
            "some peers failed after attempting all: {}",
            errors.join("; ")
        );
        Ok(imported)
    }
    /// Validate complete bounded pages before committing imports and cursor together.
    pub async fn sync_peer(&self, peer: &PeerConfig) -> Result<usize> {
        ensure!(self.state.ha.is_none(), "use fenced sync_once for HA peers");
        self.sync_peer_inner(peer).await
    }
    async fn sync_peer_inner(&self, peer: &PeerConfig) -> Result<usize> {
        let id = peer.address.id.to_string();
        // Authorization is rechecked rather than accepting an arbitrary caller-provided address.
        let current = self.state.peer_for_pull(&id)?;
        let scope = blake3::hash(&serde_json::to_vec(&(
            &current.communities,
            &self.state.auto.blocked_hosts,
            &self.state.peering,
        ))?)
        .to_hex()
        .to_string();
        let mut count = 0;
        for _ in 0..MAX_SYNC_PAGES {
            let (known_generation, after) = self.state.store()?.scoped_cursor(&id, &scope)?;
            let response = transport::rpc(
                &self.endpoint,
                peer.address.clone(),
                &Request::Pull {
                    after,
                    generation: known_generation.clone(),
                    limit: MAX_PAGE_ENTRIES,
                },
            )
            .await?;
            let Response::Page {
                generation,
                entries,
                next,
                more,
            } = response
            else {
                bail!("invalid federation response");
            };
            ensure!(
                generation.len() == 64 && hex::decode(&generation).is_ok(),
                "invalid log generation"
            );
            let after = if known_generation.as_deref() == Some(&generation) {
                after
            } else {
                0
            };
            ensure!(
                next >= after && next <= i64::MAX as u64 && (!more || next > after),
                "non-progressing federation cursor"
            );
            let mut previous = after;
            let mut accepted = Vec::new();
            for entry in &entries {
                ensure!(
                    entry.cursor > previous && entry.cursor <= next,
                    "invalid federation page ordering"
                );
                ensure!(
                    current
                        .communities
                        .contains(&entry.envelope.event.payload.community),
                    "peer exceeded configured community scope"
                );
                // A deliberate local deny is different from an invalid/untrusted page:
                // verify its bytes, skip that origin, and keep other hosts progressing.
                entry.envelope.validate()?;
                previous = entry.cursor;
                if self
                    .state
                    .auto
                    .blocked_hosts
                    .contains(&entry.envelope.event.payload.home)
                {
                    continue;
                }
                let payload = &entry.envelope.event.payload;
                let is_deletion = matches!(
                    payload.action,
                    Action::Delete { .. } | Action::Erased { .. }
                );
                let permitted = |party: &str| match self.state.access(party, &payload.community) {
                    Some(Access::Active) => true,
                    Some(Access::Suspended) => is_deletion,
                    None => false,
                };
                if !permitted(&id) || !permitted(&payload.home) {
                    continue;
                }
                self.state.validate_import(&entry.envelope)?;
                accepted.push(entry.clone());
            }
            self.state
                .store()?
                .import_scoped_page(&id, &generation, next, &accepted, &scope)?;
            count += entries.len();
            if !more {
                for community in &current.communities {
                    if self.state.access(&id, community) != Some(Access::Active) {
                        continue;
                    }
                    let owners: Vec<_> = self
                        .state
                        .store()?
                        .named_authors(&id, community)?
                        .into_iter()
                        .collect();
                    for batch in owners.chunks(100) {
                        let known = {
                            let store = self.state.store()?;
                            batch
                                .iter()
                                .map(|owner| {
                                    Ok((
                                        owner.clone(),
                                        store
                                            .current_profile(&id, owner)?
                                            .map_or(0, |p| p.revision),
                                    ))
                                })
                                .collect::<Result<std::collections::BTreeMap<_, _>>>()?
                        };
                        let Response::Social { result } = transport::rpc(
                            &self.endpoint,
                            peer.address.clone(),
                            &Request::PeerProfiles {
                                community: community.clone(),
                                known,
                            },
                        )
                        .await?
                        else {
                            bail!("invalid profile response")
                        };
                        let records: std::collections::BTreeMap<
                            String,
                            crate::profiles::ProfileRecord,
                        > = serde_json::from_value(result)?;
                        let store = self.state.store()?;
                        for (owner, record) in records {
                            ensure!(batch.contains(&owner), "unrequested profile");
                            store.import_profile(&id, &owner, record)?;
                        }
                    }
                }
                if self.state.access(&id, &format!("{id}/public")) == Some(Access::Active) {
                    self.refresh_public_projection(&current).await?;
                }
                break;
            }
        }
        Ok(count)
    }
    /// Change only this instance's materialized view; signed history is retained.
    pub fn moderate(&self, event: &str, hide: bool) -> Result<()> {
        ensure!(
            self.state.ha.is_none(),
            "HA staging cannot be edited directly"
        );
        self.state.store()?.moderate(event, hide)
    }
    /// Stop accepting and close active transport connections; consumes this running server.
    pub async fn close(self) {
        self.accept_task.abort();
        self.endpoint.close().await;
    }
}
