// SPDX-License-Identifier: AGPL-3.0-only
//! Home admission and explicitly scoped pull replication.
//!
//! A membership capability authorizes feed/submit. Federation instead authenticates
//! the connecting endpoint and independently trusts each event's original home.
//! Do not infer origin trust from the peer that forwarded an envelope.

use crate::{MAX_FRAME, MAX_PAGE_ENTRIES, model::*, store::Store, transport};
use anyhow::{Context, Result, bail, ensure};
use ed25519_dalek::SigningKey;
use iroh::{Endpoint, EndpointAddr};
use n0_future::{BufferedStreamExt, StreamExt, stream};
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
const SYNC_CONCURRENCY: usize = 4;
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
    sync_slots: Semaphore,
    peer_syncs: Mutex<BTreeMap<iroh::EndpointId, std::sync::Weak<tokio::sync::Mutex<()>>>>,
    sync_round: tokio::sync::Mutex<()>,
}
#[path = "ha.rs"]
mod ha;
#[path = "replica.rs"]
mod replica;
#[path = "server_account_delete.rs"]
mod server_account_delete;
#[path = "server_streams.rs"]
mod server_streams;
#[path = "store_read.rs"]
mod store_read;
use crate::peering::{Access, Policy};
pub use ha::{HaOptions, HaRole, export_ha, moderate_ha};
pub(crate) use store_read::DisclosureFence;
use store_read::{CatalogOp, Disclosure, NeedsExclusive, StoreWorkers};

/// Keep roots visible before spending the remaining frame budget on recent replies.
pub(crate) fn fit_feed_page(
    posts: &mut Vec<RemotePost>,
    deleted: &mut Vec<String>,
    chain_bytes: usize,
) -> Result<bool> {
    let budget = MAX_FRAME
        .checked_sub(chain_bytes + 1024)
        .context("account history exceeds feed frame")?;
    // Deletion IDs are fixed-width lowercase hex plus JSON quotes and a comma.
    // Leave room for at least the first root when the account chain and one
    // deletion cursor fit together with it.
    let root_bytes = if let Some(post) = posts.first_mut() {
        let replies = std::mem::take(&mut post.replies);
        let bytes = serde_json::to_vec(post)?.len() + 32;
        post.replies = replies;
        bytes
    } else {
        0
    };
    let deletion_bytes = if root_bytes <= budget.saturating_sub(67) {
        budget - root_bytes
    } else {
        budget
    };
    let count = (deletion_bytes / 67).min(crate::DELETION_PAGE);
    ensure!(
        count > 0 || deleted.is_empty(),
        "account history leaves no room for deletion cursor"
    );
    let more = deleted.len() > count;
    deleted.truncate(count);
    // The deletion wrapper has always carried inline profiles. Keep that wire
    // shape for older clients while sizing ordinary feeds with the dictionary.
    let mut budget = transport::FeedBudget::new(budget - 67 * deleted.len(), !more);
    let mut pending = Vec::new();
    for post in posts.iter_mut() {
        let replies = std::mem::take(&mut post.replies);
        let had_more = post.more_replies;
        post.more_replies |= !replies.is_empty();
        if !budget.add_post(post)? {
            break;
        }
        pending.push((replies, had_more));
    }
    posts.truncate(pending.len());
    for (post, (mut replies, had_more)) in posts.iter_mut().zip(pending) {
        post.more_replies = had_more;
        while let Some(reply) = replies.pop() {
            if !budget.add_reply(&reply)? {
                post.more_replies = true;
                break;
            }
            post.replies.push(reply);
        }
        post.replies.reverse();
    }
    Ok(more)
}

/// Return a bounded preceding window in the same display order as the feed.
pub(crate) fn preceding_replies(
    mut replies: Vec<RemoteReply>,
    before: Option<&str>,
) -> Result<serde_json::Value> {
    if let Some(id) = before {
        let end = replies
            .iter()
            .position(|reply| reply.id == id)
            .context("reply cursor unavailable; refresh the thread")?;
        replies.truncate(end);
    }
    let mut budget = transport::FeedBudget::new(MAX_FRAME - 1024, false);
    let mut visible = Vec::new();
    let count = replies.len();
    while let Some(reply) = replies.pop() {
        if !budget.add_reply(&reply)? {
            break;
        }
        visible.push(reply);
    }
    visible.reverse();
    Ok(serde_json::json!({ "more": visible.len() < count, "replies": visible }))
}

impl Response {
    pub(crate) fn with_deletion_pages(self, more: bool) -> Self {
        if more {
            Self::FeedWithMoreDeletions {
                feed: Box::new(self),
            }
        } else {
            self
        }
    }
}

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
    advertisement_round: AtomicUsize,
    replication_round: AtomicUsize,
    generation: String,
    key: SigningKey,
    workers: StoreWorkers,
    disclosure: Arc<DisclosureFence>,
    communities: BTreeMap<String, String>,
    peers: RwLock<Vec<PeerConfig>>,
}
impl State {
    fn stamped_operation<T>(
        &self,
        mut work: impl FnMut(&mut CatalogOp<'_>) -> Result<T>,
    ) -> Result<(T, u64)> {
        self.workers.checkout(|store| {
            let mut perform = |exclusive: bool| {
                let peers = self
                    .peers
                    .read()
                    .map_err(|_| anyhow::anyhow!("peer policy unavailable"))?;
                store.operation(|store| {
                    work(&mut CatalogOp {
                        state: self,
                        store,
                        peers: &peers,
                        disclosure: if exclusive {
                            Disclosure::Exclusive
                        } else {
                            Disclosure::Shared
                        },
                    })
                })
            };
            let result = self.disclosure.shared(|| perform(self.ha.is_some()));
            if !matches!(&result, Err(e) if e.is::<NeedsExclusive>()) {
                return result;
            }
            // The shared attempt rolled back. Recheck authorization from a fresh
            // snapshot after outstanding authorized operations finish committing.
            self.disclosure.exclusive(|| perform(true))
        })
    }

    fn operation<T>(&self, work: impl FnMut(&mut CatalogOp<'_>) -> Result<T>) -> Result<T> {
        self.stamped_operation(work).map(|(value, _)| value)
    }

    async fn spawn_job<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnOnce(&State) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let permit = self.workers.slots.clone().acquire_owned().await?;
        let state = self.clone();
        let staging = ha::STAGING.try_with(Arc::clone).ok();
        tokio::task::spawn_blocking(move || {
            let (_permit, state, _staging) = (permit, state, staging);
            work(&state)
        })
        .await?
    }

    async fn blocking<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnMut(&mut CatalogOp<'_>) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.spawn_job(move |state| state.operation(work)).await
    }

    async fn ha_blocking<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnOnce(&mut Store) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.spawn_job(move |state| {
            ensure!(state.ha.is_some(), "HA staging required");
            state.workers.checkout(work)
        })
        .await
    }

    fn access(&self, id: &str, community: &str) -> Option<Access> {
        match &self.peering {
            Some(policy) if id != self.logical_addr().id.to_string() => {
                policy.access(id, community)
            }
            _ => Some(Access::Active),
        }
    }
    fn logical_addr(&self) -> EndpointAddr {
        let logical = iroh::SecretKey::from_bytes(&self.key.to_bytes()).public();
        if logical == self.endpoint.id() {
            self.endpoint.addr()
        } else {
            EndpointAddr::new(logical)
        }
    }
    async fn dispatch(self: &Arc<Self>, remote: &str, call: Request) -> Result<Response> {
        self.dispatch_stamped(remote, call)
            .await
            .map(|(response, _)| response)
    }

    async fn dispatch_stamped(
        self: &Arc<Self>,
        remote: &str,
        call: Request,
    ) -> Result<(Response, u64)> {
        match call {
            Request::JoinGroup {
                token,
                community,
                group,
            } => {
                self.blocking(move |op| op.member(&community, &token))
                    .await?;
                self.join_group(&group).await?;
                self.spawn_job(move |state| {
                    state.stamped_operation(|op| {
                        op.local_directory()?;
                        Ok(Response::GroupJoined {
                            join: op.public_join(&group)?,
                            group: Box::new(op.group_summary(&group)?),
                        })
                    })
                })
                .await
            }
            Request::SubmitReport {
                token,
                community,
                receipt,
                evidence,
            } => {
                self.blocking(move |op| op.member(&community, &token))
                    .await?;
                self.spawn_job(move |state| {
                    let stamp = *state.disclosure.epoch.read().unwrap();
                    let result = state.report_inbox()?.submit(
                        &state.endpoint.id().to_string(),
                        &receipt,
                        &evidence,
                        now(),
                    )?;
                    Ok((Response::ReportResult { result }, stamp))
                })
                .await
            }
            call => {
                let remote = remote.to_owned();
                self.spawn_job(move |state| {
                    state.stamped_operation(|op| op.handle(&remote, call.clone()))
                })
                .await
            }
        }
    }
    async fn send_response(
        &self,
        send: &mut iroh::endpoint::SendStream,
        bytes: &[u8],
        stamp: Option<u64>,
    ) -> Result<()> {
        use std::{
            future::{Future, poll_fn},
            task::Poll,
        };
        let mut started = false;
        let expired = {
            let write = async {
                send.write_all(bytes).await?;
                send.finish()?;
                anyhow::Ok(false)
            };
            tokio::pin!(write);
            poll_fn(|cx| {
                // Linearize each nonblocking write (including FIN) with revocation.
                // A Pending poll releases the guard before waiting on the transport.
                let current = self.disclosure.epoch.read().unwrap();
                if stamp.is_some_and(|epoch| epoch != *current) {
                    return Poll::Ready(Ok(true));
                }
                started = true;
                write.as_mut().poll(cx)
            })
            .await?
        };
        if expired {
            if started {
                send.reset(0u32.into())?;
            } else {
                let error = Response::Error {
                    code: "expired".into(),
                    message: "Content changed; refresh".into(),
                };
                send.write_all(&serde_json::to_vec(&error)?).await?;
                send.finish()?;
            }
        }
        Ok(())
    }

    fn report_inbox(&self) -> Result<MutexGuard<'_, crate::report_inbox::Inbox>> {
        self.reports
            .as_ref()
            .context("this home has no staffed report inbox")?
            .lock()
            .map_err(|_| anyhow::anyhow!("report inbox unavailable"))
    }
}

impl CatalogOp<'_> {
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
        self.check_membership(community, token, || self.store.hosts_group(community))
    }

    fn check_membership(
        &self,
        community: &str,
        token: &str,
        hosts_group: impl FnOnce() -> Result<bool>,
    ) -> Result<()> {
        let expected = match self.communities.get(community) {
            Some(token) => token.clone(),
            None => {
                ensure!(self.auto.enabled && hosts_group()?, "unknown community");
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
        if self.auto.enabled && self.store.hosts_group(community)? {
            let store = &*self.store;
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
    fn sync_peers(&mut self) -> Result<Vec<PeerConfig>> {
        let mut peers = self.peers.to_vec();
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
        peers.retain(|peer| {
            peer.communities
                .iter()
                .any(|scope| self.access(&peer.address.id.to_string(), scope).is_some())
        });
        Ok(peers)
    }
    fn register_account(
        store: &mut Store,
        chain: &[crate::portability::AccountRecord],
        disclosure: &Disclosure,
    ) -> Result<crate::portability::AccountRecord> {
        let next = chain.last().context("missing account record")?;
        let previous = store.account_chain(&next.payload.account)?;
        disclosure.restrict_if(
            chain.len() > previous.len()
                && previous.last().is_none_or(|old| {
                    old.payload.home != next.payload.home
                        || !old.payload.devices.is_subset(&next.payload.devices)
                }),
        )?;
        store.register_account(chain)
    }

    fn feed_or_social(&mut self, remote: &str, call: Request) -> Result<Response> {
        match call {
            Request::Feed {
                token,
                community,
                author: _,
                nearby,
            } => {
                self.member(&community, &token)?;
                let store = &self.store;
                if let Some(filter) = &nearby {
                    filter.validate()?;
                }
                let crate::feed_page::FeedPage {
                    mut posts,
                    event_count,
                    ..
                } = store.feed_page(&community, "", nearby.as_ref())?;
                let mut deleted = store.erased_ids(&community)?;
                let more = fit_feed_page(&mut posts, &mut deleted, 0)?;
                Ok(Response::Feed {
                    deleted,
                    direct: false,
                    privacy: false,
                    posts,
                    event_count,
                    account_chain: Vec::new(),
                }
                .with_deletion_pages(more))
            }
            Request::Social {
                token,
                community,
                account,
                proof,
                op,
            } => {
                self.member(&community, &token)?;
                let store = &self.store;
                let home = self.logical_addr().id.to_string();
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
                use crate::messaging::SocialOp;
                self.disclosure.restrict_if(matches!(
                    op.as_ref(),
                    SocialOp::Block { .. }
                        | SocialOp::SetPrivacy { .. }
                        | SocialOp::DismissRequest { .. }
                        | SocialOp::SetProfile { .. }
                        | SocialOp::SetProfilePortrait { .. }
                ))?;
                self.store
                    .social(&home, &community, record, &proof.device, *op)
            }
            _ => bail!("expected a feed or social operation"),
        }
    }

    fn handle(&mut self, remote: &str, request: Request) -> Result<Response> {
        match request {
            Request::ReportAvailability => {
                ensure!(
                    self.reports.is_some(),
                    "reporting is unavailable on this home"
                );
                Ok(Response::ReportAvailable)
            }
            Request::SubmitReport { .. } => {
                bail!("report submission requires separate inbox dispatch")
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
            call @ (Request::Feed { .. } | Request::Social { .. }) => {
                self.feed_or_social(remote, call)
            }
            Request::AccountDeleteAvailability {
                token,
                community,
                chain,
                intent,
                proof,
            } => {
                self.member(&community, &token)?;
                self.check_account_deletion(
                    &chain,
                    &intent,
                    &proof,
                    "check-account-delete-v1",
                    &crate::portability::digest(&intent)?,
                )?;
                Ok(Response::AccountDeleteAvailable {
                    account: intent.account,
                })
            }
            Request::AccountDelete {
                token,
                community,
                chain,
                intent,
                proof,
                deletes,
            } => {
                self.member(&community, &token)?;
                self.delete_account(&chain, &intent, &proof, &deletes)
            }
            Request::AnnounceAccount { chain } => {
                let record = crate::portability::validate_chain(&chain)?;
                let store = &mut *self.store;
                // Relaying a root-signed update needs no membership token, but may only
                // advance an already-known account. It cannot allocate arbitrary accounts.
                ensure!(
                    !store.account_chain(&record.payload.account)?.is_empty(),
                    "account not known here"
                );
                Self::register_account(store, &chain, &self.disclosure)?;
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
                    chain: self.store.account_chain(&account)?,
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
                let store = &mut *self.store;
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
                self.disclosure.restrict_if(matches!(
                    op.as_ref(),
                    crate::private_groups::Operation::Change { .. }
                ))?;
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
                let store = &mut *self.store;
                let current = Self::register_account(store, &chain, &self.disclosure)?;
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
                let store = &mut *self.store;
                let current = Self::register_account(store, &chain, &self.disclosure)?;
                // Reads of an old home's retained history remain possible after a move, using
                // an active key in the latest known account record. Membership still applies.
                ensure!(
                    current.payload.devices.contains(&proof.device),
                    "export device revoked"
                );
                let all = store.owned_after(
                    &record.payload.account,
                    after.as_deref().unwrap_or(""),
                    51,
                )?;
                let mut events = Vec::new();
                let mut more = false;
                for event in all {
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
                let store = &mut *self.store;
                let current = Self::register_account(store, &chain, &self.disclosure)?;
                ensure!(
                    current == *record && current.payload.devices.contains(&proof.device),
                    "stale or revoked import device"
                );
                self.disclosure.restrict_if(events.iter().any(|e| {
                    e.deletion.is_some()
                        || matches!(
                            e.event.payload.action,
                            Action::Delete { .. } | Action::Erased { .. }
                        )
                }))?;
                store.import_history(&record.payload.account, &events, &admitted_deletes)?;
                Ok(Response::PersonalImported {
                    count: events.len(),
                })
            }
            Request::Directory { snapshot } => {
                self.import_public_directory(&snapshot)?;
                Ok(Response::Directory {
                    snapshot: self.advertised_directory()?,
                })
            }
            Request::EnrollHost { group } => Ok(Response::HostPermit {
                permit: self.enroll(remote, &group)?,
            }),
            Request::Groups { token, community } => self.group_page(&community, &token, ""),
            Request::GroupsAfter {
                token,
                community,
                after,
            } => self.group_page(&community, &token, &after),
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
            Request::DeletionsAfter {
                community,
                token,
                after,
            } => {
                ensure!(
                    after.len() == 64 && after.bytes().all(|c| c.is_ascii_hexdigit()),
                    "invalid deletion cursor"
                );
                if let Some(token) = token {
                    self.member(&community, &token)?;
                } else if self.public_replica_feeds.contains(&community) {
                    self.authorize_replica(&community)?;
                } else {
                    ensure!(
                        community == format!("{}/public", self.logical_addr().id)
                            && self.peer(remote)?.communities.contains(&community),
                        "projection unavailable"
                    );
                }
                let mut ids = self.store.erased_after(&community, &after)?;
                let more = ids.len() > crate::DELETION_PAGE;
                ids.truncate(crate::DELETION_PAGE);
                Ok(Response::DeletionPage { ids, more })
            }
            Request::PeerProfiles { community, known } => {
                ensure!(
                    known.len() <= 100 && self.peer(remote)?.communities.contains(&community),
                    "profile scope denied"
                );
                let store = &*self.store;
                let home = self.logical_addr().id.to_string();
                let owners = store.named_authors(&home, &community)?;
                let mut records = std::collections::BTreeMap::new();
                let mut remaining = MAX_FRAME - 1024;
                for (owner, revision) in known {
                    if owners.contains(&owner)
                        && let Some(record) = store.current_profile(&home, &owner)?
                        && record.revision > revision
                    {
                        let bytes = serde_json::to_vec(&record)?.len() + owner.len() + 4;
                        if bytes > remaining {
                            continue;
                        }
                        remaining -= bytes;
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
                let store = &*self.store;
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
                let store = &mut *self.store;
                crate::account_store::require_live(&store.db, event.owner_id())?;
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
                    let current = Self::register_account(store, &account_chain, &self.disclosure)?;
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
                let target = match &event.payload.action {
                    Action::Reply { parent, .. } => Some(parent),
                    Action::Vote { target, .. } | Action::Delete { target } => Some(target),
                    _ => None,
                };
                if let Some(target) = target {
                    let parent = store
                        .existing(target)?
                        .filter(|e| e.event.payload.community == event.payload.community)
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
                let recent: usize = store.db.query_row("SELECT COUNT(*) FROM events WHERE community=?
                    AND COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author)=?
                    AND json_extract(envelope,'$.acceptedAt')>=?", rusqlite::params![event.payload.community,event.owner_id(),now()-RATE_WINDOW_SECS], |r|r.get(0))?;
                ensure!(
                    recent < AUTHOR_EVENTS_PER_MINUTE,
                    "author rate limit; retry later"
                );
                self.disclosure
                    .restrict_if(matches!(event.payload.action, Action::Delete { .. }))?;
                let username = store.sign_username(&event, &self.state.key)?;
                let envelope = Envelope::accept(event, &self.state.key)?;
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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerHealth {
    pub ready: bool,
    pub image_bytes: Option<usize>,
    pub budget_bytes: Option<usize>,
    pub oldest_projection_age_seconds: Option<i64>,
}
impl WorkerHealth {
    pub fn has_headroom(&self) -> bool {
        self.image_bytes
            .zip(self.budget_bytes)
            .is_none_or(|(used, budget)| used > 0 && (used as u64) * 5 < (budget as u64) * 4)
    }
}

impl Server {
    /// Read committed authority metadata or all configured projection verification times.
    pub fn health_probe(
        &self,
    ) -> impl Fn() -> Result<WorkerHealth> + Clone + Send + Sync + 'static {
        let state = Arc::downgrade(&self.state);
        move || {
            let state = state.upgrade().context("worker stopped")?;
            ensure!(!state.endpoint.is_closed(), "transport closed");
            if let Some(ha) = &state.ha {
                let (used, budget) = ha.committed_capacity()?;
                return Ok(WorkerHealth {
                    ready: used > 0 && used <= budget,
                    image_bytes: Some(used),
                    budget_bytes: Some(budget),
                    oldest_projection_age_seconds: None,
                });
            }
            ensure!(
                !state.public_replica_feeds.is_empty(),
                "no configured replica scope"
            );
            state.operation(|op| {
                let store = &*op.store;
                let mut age = 0;
                for scope in &state.public_replica_feeds {
                    let verified: i64 = store.db.query_row(
                        "SELECT verified FROM public_projections WHERE community=?",
                        [scope],
                        |row| row.get(0),
                    )?;
                    let elapsed = now()
                        .checked_sub(verified)
                        .context("invalid projection timestamp")?;
                    ensure!(elapsed >= 0, "invalid projection timestamp");
                    age = age.max(elapsed);
                }
                Ok(WorkerHealth {
                    ready: age <= 120,
                    image_bytes: None,
                    budget_bytes: None,
                    oldest_projection_age_seconds: Some(age),
                })
            })
        }
    }

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
        let path = config.database_path.clone();
        let (store, locks, image) = tokio::task::spawn_blocking(move || {
            ensure!(
                path.is_file(),
                "bootstrap requires the existing dedicated database"
            );
            let exclusive = lock_server(&path)?;
            let store = Store::open_staging(&path)?;
            let guard_lock = crate::backup::lock_authority(&path)?;
            let image = store.snapshot()?;
            anyhow::Ok((store, (exclusive, guard_lock), image))
        })
        .await??;
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
        tokio::task::spawn_blocking(move || {
            let _locks = locks;
            mark_ha(&config.database_path)?;
            // Invalidate the bootstrap keys after the authority has made the seed durable.
            store.retire_seed(&config.database_path)
        })
        .await?
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
        let home = own_id.clone();
        let (config, workers, disclosure, process_lock, guard_lock, peering, ha) = tokio::task::spawn_blocking(move || {
            let process_lock = lock_server(&config.database_path)?;
            let mut store = if options.is_some() {
                ensure!(
                    !config.database_path.exists(),
                    "HA staging path contains a previous database; complete the verified bootstrap retirement before serving"
                );
                // HA workers materialize the authority's fenced image in RAM on each
                // revision change. An offline worker leaves no stale content on disk.
                Store {
                    db: rusqlite::Connection::open_in_memory()?.into(),
                }
            } else {
                Store::open(&config.database_path)?
            };
            let guard_lock = if options.is_none() {
                let lock = crate::backup::lock_authority(&config.database_path)?;
                store.verify_authority()?;
                store.operation(|store| store.recover_local_first_seen(&home))?;
                Some(lock)
            } else {
                None
            };
            let peering = Policy::load()?;
            let ha = options
                .as_ref()
                .map(|o| ha::Ha::new(o, &config, peering.as_ref()))
                .transpose()?;
            if ha.is_some() {
                mark_ha(&config.database_path)?;
            }
            let disclosure = store.db.native_home().map(|home| home.disclosure.clone()).unwrap_or_default();
            let workers = StoreWorkers::new(store, ha.is_none())?;
            anyhow::Ok((config, workers, disclosure, process_lock, guard_lock, peering, ha))
        }).await??;
        let secret = iroh::SecretKey::from_bytes(&transport_key.to_bytes());
        let endpoint = transport::bind(config.network_mode, Some(secret), true).await?;
        let reports = if endpoint.id().to_string() == own_id
            && let Some(path) = std::env::var_os("YAP_REPORT_INBOX")
        {
            Some(Mutex::new(
                tokio::task::spawn_blocking(move || {
                    crate::report_inbox::Inbox::open(std::path::Path::new(&path), Some(&own_id))
                })
                .await??,
            ))
        } else {
            None
        };
        let state = Arc::new(State {
            workers,
            disclosure,
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
            advertisement_round: AtomicUsize::new(0),
            replication_round: AtomicUsize::new(0),
            generation: new_secret(),
            key,
            communities: config.communities,
            peers: RwLock::new(config.peers),
        });
        let accept_endpoint = endpoint.clone();
        let accept_state = state.clone();
        let work = Arc::new(Semaphore::new(server_streams::MAX_WORK));
        let sessions = Arc::new(Semaphore::new(server_streams::MAX_MUX_SESSIONS));
        let accept_task = tokio::spawn(async move {
            let expire_reports = async {
                let mut sweep = tokio::time::interval(Duration::from_secs(60));
                loop {
                    sweep.tick().await;
                    if accept_state
                        .spawn_job(|state| state.report_inbox()?.purge(now()))
                        .await
                        .is_err()
                    {
                        eprintln!("Report inbox expiry failed");
                    }
                }
            };
            tokio::pin!(expire_reports);
            let expire_mail = async {
                // An unseeded standby cannot lease work until the initial image exists.
                if accept_state.ha.is_some() {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                loop {
                    let result = match &accept_state.ha {
                        Some(ha) => ha.expire_mail(&accept_state).await,
                        None => {
                            accept_state
                                .blocking(|op| op.store.sweep_expired_mail())
                                .await
                        }
                    };
                    let delay = match result {
                        Ok(true) => 1,
                        Ok(false) => 60,
                        Err(error) => {
                            eprintln!("Mailbox expiry failed: {error}");
                            60
                        }
                    };
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
            };
            tokio::pin!(expire_mail);
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                let incoming = tokio::select! {
                    incoming = accept_endpoint.accept() => match incoming {
                        Some(incoming) => incoming,
                        None => break,
                    },
                    () = &mut expire_reports, if accept_state.reports.is_some() => break,
                    () = &mut expire_mail => break,
                    Some(_) = handlers.join_next(), if !handlers.is_empty() => continue,
                };
                let Ok(handshake) = work.clone().try_acquire_owned() else {
                    incoming.refuse();
                    continue;
                };
                handlers.spawn(server_streams::serve(
                    accept_state.clone(),
                    incoming,
                    handshake,
                    sessions.clone(),
                ));
            }
            handlers.shutdown().await;
            // A cancelled stream's blocking encoder retains its admission permit.
            let _drained = work.acquire_many(server_streams::MAX_WORK as u32).await;
        });
        Ok(Self {
            endpoint,
            state,
            accept_task,
            sync_slots: Semaphore::new(SYNC_CONCURRENCY),
            peer_syncs: Mutex::default(),
            sync_round: tokio::sync::Mutex::default(),
        })
    }
    /// Atomically replace peer policy. Removal prevents future exchange, not history recall.
    pub fn set_peers(&self, peers: Vec<PeerConfig>) -> Result<()> {
        validate_peers(
            &peers,
            &self.endpoint.id().to_string(),
            &self.state.communities,
        )?;
        self.state.disclosure.restrict(|| {
            *self
                .state
                .peers
                .write()
                .map_err(|_| anyhow::anyhow!("peer policy unavailable"))? = peers;
            Ok(())
        })
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
    pub async fn dispatch_notifications(&self) -> Result<()> {
        use std::process::Stdio;
        use tokio::io::AsyncWriteExt;
        let Ok(worker) = std::env::var("YAP_PUSH_WORKER") else {
            return Ok(());
        };
        // This is a scheduling hint only. Leases and policy checks use the fenced,
        // committed home image. Both nodes recover it on the next client/federation call.
        if !self
            .state
            .blocking(|state| {
                Ok(state.store.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM push_devices)",
                    [],
                    |r| r.get::<_, bool>(0),
                )?)
            })
            .await?
        {
            return Ok(());
        }
        let jobs = match &self.state.ha {
            Some(ha) => ha.push_operation(&self.state, None).await?,
            None => {
                self.state
                    .blocking(|state| state.store.claim_push())
                    .await?
            }
        };
        if jobs.as_array().is_none_or(Vec::is_empty) {
            return Ok(());
        }
        let mut child = tokio::process::Command::new(worker)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().context("missing push worker input")?;
        input.write_all(&serde_json::to_vec(&jobs)?).await?;
        drop(input);
        let output =
            tokio::time::timeout(Duration::from_secs(45), child.wait_with_output()).await??;
        ensure!(
            output.status.success() && output.stdout.len() < 4096,
            "push worker unavailable"
        );
        let outcomes = serde_json::from_slice(&output.stdout)?;
        match &self.state.ha {
            Some(ha) => {
                ha.push_operation(&self.state, Some((jobs, outcomes)))
                    .await?;
            }
            None => {
                self.state
                    .blocking(move |state| state.store.finish_push(&jobs, &outcomes))
                    .await?;
            }
        }
        Ok(())
    }
    pub async fn sync_once(&self) -> Result<usize> {
        if let Some(ha) = &self.state.ha {
            return ha.synchronize(self).await;
        }
        let _round = self.sync_round.lock().await;
        self.sync_inner().await
    }
    async fn sync_inner(&self) -> Result<usize> {
        let mut imported = 0;
        let mut errors = Vec::new();
        if let Err(error) = self.discover_once().await {
            errors.push(format!("discovery: {error:#}"));
        }
        let peers = self.state.blocking(|op| op.sync_peers()).await?;
        let mut results = stream::iter(
            peers
                .into_iter()
                .map(|peer| async move { (peer.address.id, self.sync_peer_inner(&peer).await) }),
        )
        .buffered_unordered(SYNC_CONCURRENCY);
        while let Some((id, result)) = results.next().await {
            match result {
                Ok(count) => imported += count,
                Err(error) => errors.push(format!("{id}: {error:#}")),
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
        // A direct sync and a scheduled round may target the same peer.
        let lock = {
            let mut locks = self
                .peer_syncs
                .lock()
                .map_err(|_| anyhow::anyhow!("peer sync locks unavailable"))?;
            locks.retain(|_, lock| lock.strong_count() != 0);
            if let Some(lock) = locks
                .get(&peer.address.id)
                .and_then(std::sync::Weak::upgrade)
            {
                lock
            } else {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                locks.insert(peer.address.id, Arc::downgrade(&lock));
                lock
            }
        };
        let _peer = lock.lock().await;
        let _slot = self.sync_slots.acquire().await?;
        let mut count = 0;
        for _ in 0..MAX_SYNC_PAGES {
            let (entries, more) = self.sync_page(peer).await?;
            count += entries;
            if !more {
                self.sync_peer_metadata(peer).await?;
                break;
            }
        }
        Ok(count)
    }

    async fn sync_page(&self, peer: &PeerConfig) -> Result<(usize, bool)> {
        let id = peer.address.id.to_string();
        // Recheck authorization before trusting a caller-provided address.
        let remote = id.clone();
        let (current, scope, known_generation, after) = self
            .state
            .blocking(move |state| {
                let current = state.peer_for_pull(&remote)?;
                let scope = blake3::hash(&serde_json::to_vec(&(
                    &current.communities,
                    &state.auto.blocked_hosts,
                    &state.peering,
                ))?)
                .to_hex()
                .to_string();
                let (known_generation, after) = state.store.scoped_cursor(&remote, &scope)?;
                Ok((current, scope, known_generation, after))
            })
            .await?;
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
        self.state
            .blocking(move |state| {
                let Response::Page {
                    generation,
                    entries,
                    next,
                    more,
                } = &response
                else {
                    bail!("invalid federation response");
                };
                let fresh = state.peer_for_pull(&id)?;
                ensure!(
                    fresh.communities == current.communities
                        && state.store.scoped_cursor(&id, &scope)?
                            == (known_generation.clone(), after),
                    "peer scope or cursor changed during pull"
                );
                let (next, more) = (*next, *more);
                ensure!(
                    generation.len() == 64 && hex::decode(generation).is_ok(),
                    "invalid log generation"
                );
                let after = if known_generation.as_deref() == Some(generation) {
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
                let examined = entries.len();
                for entry in entries {
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
                    if state
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
                    let permitted = |party: &str| match state.access(party, &payload.community) {
                        Some(Access::Active) => true,
                        Some(Access::Suspended) => is_deletion,
                        None => false,
                    };
                    if !permitted(&id) || !permitted(&payload.home) {
                        continue;
                    }
                    state.validate_import(&entry.envelope)?;
                    accepted.push(entry.clone());
                }
                state.disclosure.restrict_if(accepted.iter().any(|entry| {
                    entry.envelope.deletion.is_some()
                        || matches!(
                            entry.envelope.event.payload.action,
                            Action::Delete { .. } | Action::Erased { .. }
                        )
                }))?;
                state
                    .store
                    .import_scoped_page(&id, generation, next, &accepted, &scope)?;
                Ok((examined, more))
            })
            .await
    }

    async fn sync_peer_metadata(&self, peer: &PeerConfig) -> Result<()> {
        let id = peer.address.id.to_string();
        let remote = id.clone();
        let current = self
            .state
            .blocking(move |state| state.peer_for_pull(&remote))
            .await?;
        for community in &current.communities {
            if self.state.access(&id, community) != Some(Access::Active) {
                continue;
            }
            let remote = id.clone();
            let scope = community.clone();
            let owners: Vec<_> = self
                .state
                .blocking(move |state| {
                    Ok(state
                        .store
                        .named_authors(&remote, &scope)?
                        .into_iter()
                        .collect())
                })
                .await?;
            for batch in owners.chunks(PROFILE_LOOKUP_BATCH) {
                let requested = batch.to_vec();
                let remote = id.clone();
                let known = self
                    .state
                    .blocking(move |state| {
                        let store = &*state.store;
                        requested
                            .iter()
                            .map(|owner| {
                                let revision = store
                                    .current_profile(&remote, owner)?
                                    .map_or(0, |p| p.revision);
                                Ok((owner.clone(), revision))
                            })
                            .collect::<Result<BTreeMap<_, _>>>()
                    })
                    .await?;
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
                let requested = batch.to_vec();
                let remote = id.clone();
                let scope = community.clone();
                self.state
                    .blocking(move |state| {
                        ensure!(
                            state.peer(&remote)?.communities.contains(&scope),
                            "profile scope changed"
                        );
                        let records: BTreeMap<String, crate::profiles::ProfileRecord> =
                            serde_json::from_value(result.clone())?;
                        let store = &*state.store;
                        for (owner, record) in records {
                            ensure!(requested.contains(&owner), "unrequested profile");
                            state.disclosure.restrict_if(
                                store
                                    .current_profile(&remote, &owner)?
                                    .is_some_and(|old| old.revision < record.revision),
                            )?;
                            store.import_profile(&remote, &owner, record)?;
                        }
                        Ok(())
                    })
                    .await?;
            }
        }
        if self.state.access(&id, &format!("{id}/public")) == Some(Access::Active) {
            self.refresh_public_projection(&current).await?;
        }
        Ok(())
    }
    /// Change only this instance's materialized view; signed history is retained.
    pub fn moderate(&self, event: &str, hide: bool) -> Result<()> {
        ensure!(
            self.state.ha.is_none(),
            "HA staging cannot be edited directly"
        );
        self.state.operation(|op| {
            op.disclosure.restrict_if(hide)?;
            op.store.moderate(event, hide)
        })
    }
    /// Stop accepting, cancel network work, and drain blocking jobs before releasing storage.
    pub async fn close(self) {
        self.endpoint.close().await;
        let _ = self.accept_task.await;
        self.state.workers.drain().await;
        if let Some(ha) = &self.state.ha {
            ha.drain().await;
        }
        let _ = tokio::task::spawn_blocking(move || drop(self.state)).await;
    }
}

#[cfg(test)]
mod federation_tests {
    use super::*;

    pub(super) fn config(database_path: PathBuf) -> ServerConfig {
        ServerConfig {
            database_path,
            private_key: new_secret(),
            communities: BTreeMap::from([("test".into(), new_secret())]),
            peers: Vec::new(),
            network_mode: NetworkMode::Local,
            public_replica_feeds: Default::default(),
            auto_federation: Default::default(),
        }
    }

    #[tokio::test]
    async fn idle_home_expires_mail_and_exports_only_unexpired_rows() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("home");
        let store = Store::open(&path)?;
        for (id, created) in [
            ("expired", crate::messaging::mail_cutoff(now()) - 10),
            ("current", now()),
        ] {
            store.db.execute(
                "INSERT INTO dm_mail(id,sender,recipient,packet,created) VALUES(?,'a','b','sealed',?)",
                rusqlite::params![id, created],
            )?;
        }
        let archive = dir.path().join("archive");
        store.backup(&archive)?;
        drop(store);
        let restored = dir.path().join("restored");
        crate::backup::restore(
            &path.with_added_extension("deletion-guard"),
            &archive,
            &restored,
        )?;
        let copy = Store::open(&restored)?;
        assert_eq!(
            copy.db
                .query_row("SELECT id FROM dm_mail", [], |r| r.get::<_, String>(0))?,
            "current"
        );
        drop(copy);

        let server = Server::start(config(path)).await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let ids = server
                    .state
                    .blocking(|op| {
                        let mut query =
                            op.store.db.prepare("SELECT id FROM dm_mail ORDER BY id")?;
                        Ok(query
                            .query_map([], |r| r.get::<_, String>(0))?
                            .collect::<rusqlite::Result<Vec<_>>>()?)
                    })
                    .await?;
                if ids == ["current"] {
                    return anyhow::Ok(());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await??;
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn overlapping_syncs_advance_while_another_peer_stalls() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let stalled = transport::bind(NetworkMode::Local, None, true).await?;
        let healthy = transport::bind(NetworkMode::Local, None, true).await?;
        let mut config = config(dir.path().join("home"));
        let peer = |endpoint: &Endpoint| PeerConfig {
            address: endpoint.addr(),
            communities: BTreeSet::from(["test".into()]),
        };
        let source = peer(&healthy);
        config.peers = vec![peer(&stalled), source.clone()];
        let server = Server::start(config).await?;
        let reply = |expected_after| {
            let endpoint = &healthy;
            async move {
                let connection = endpoint.accept().await.context("endpoint closed")?.await?;
                let (mut send, mut recv) = connection.accept_bi().await?;
                let request = serde_json::from_slice(&recv.read_to_end(MAX_FRAME).await?)?;
                let Request::Pull { after, .. } = request else {
                    bail!("expected pull")
                };
                ensure!(after == expected_after, "stale cursor {after}");
                send.write_all(&serde_json::to_vec(&Response::Page {
                    generation: "aa".repeat(32),
                    entries: vec![],
                    next: after + 1,
                    more: false,
                })?)
                .await?;
                send.finish()?;
                connection.closed().await;
                anyhow::Ok(())
            }
        };
        let (round, responses) = tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(server.sync_once(), async {
                let stalled_connection = stalled
                    .accept()
                    .await
                    .context("stalled peer closed")?
                    .await?;
                let (mut send, mut recv) = stalled_connection.accept_bi().await?;
                recv.read_to_end(MAX_FRAME).await?;
                // Keep the first peer waiting until both pulls from the second peer finish.
                let (direct, replies) = tokio::join!(server.sync_peer(&source), async {
                    reply(0).await?;
                    reply(1).await
                });
                replies?;
                send.write_all(&serde_json::to_vec(&Response::Error {
                    code: "rejected".into(),
                    message: "test peer unavailable".into(),
                })?)
                .await?;
                send.finish()?;
                stalled_connection.closed().await;
                direct
            })
        })
        .await?;
        responses?;
        assert!(round.is_err());
        stalled.close().await;
        healthy.close().await;
        server.close().await;
        Ok(())
    }
}

#[cfg(test)]
mod feed_page_tests {
    use super::*;

    #[test]
    fn long_account_history_leaves_a_valid_deletion_cursor() -> Result<()> {
        let mut ids = (0..20).map(|n| format!("{n:064x}")).collect::<Vec<_>>();
        assert!(fit_feed_page(
            &mut vec![],
            &mut ids,
            MAX_FRAME - 1024 - 3 * 67
        )?);
        assert_eq!(ids.len(), 3);
        assert_eq!(ids.last().unwrap(), &format!("{:064x}", 2));
        Ok(())
    }

    #[test]
    fn feed_budget_keeps_roots_and_allows_older_reply_windows() -> Result<()> {
        let make_post = |id, replies| RemotePost {
            id,
            replies,
            headline: "Neighborhood event".into(),
            area: None,
            username: None,
            visibility: None,
            profile: None,
            author: "Neighbor".into(),
            body: "See you there".into(),
            topic: "Plans".into(),
            score: 0,
            vote: 0,
            mine: false,
            more_replies: false,
            accepted_at: 1726681534,
        };
        let replies: Vec<_> = (0..200)
            .map(|index| RemoteReply {
                id: format!("{index:064x}"),
                author: "Neighbor".into(),
                body: "界".repeat(500),
                profile: Some(CommunityProfile {
                    photo: "A".repeat(4096),
                    ..Default::default()
                }),
                visibility: Some(AuthorVisibility {
                    peer: Some(format!("{index:064x}")),
                    anonymous: false,
                    age: 7,
                }),
                username: None,
                mine: false,
            })
            .collect();
        let encode = |posts| {
            transport::encode_response(&Response::Feed {
                posts,
                direct: true,
                privacy: true,
                deleted: vec![],
                event_count: 202,
                account_chain: vec![],
            })
        };
        let mut posts = vec![
            make_post("new".into(), replies.clone()),
            make_post("older".into(), vec![]),
        ];
        assert!(!fit_feed_page(&mut posts, &mut vec![], 0)?);
        assert_eq!(posts.len(), 2);
        assert!(posts[0].more_replies);
        let preview = posts[0].replies.clone();
        assert_eq!(preview.last(), replies.last());
        let encoded = encode(posts)?;
        assert!(encoded.len() <= MAX_FRAME);
        transport::decode_response(&encoded)?;
        let page = preceding_replies(replies.clone(), Some(&preview[0].id))?;
        let mut previous: Vec<RemoteReply> = serde_json::from_value(page["replies"].clone())?;
        previous.extend(preview);
        assert_eq!(previous, replies);

        let shared: Vec<_> = replies
            .into_iter()
            .map(|mut reply| {
                reply.visibility.as_mut().unwrap().peer = Some("0".repeat(64));
                reply
            })
            .collect();
        let mut posts = vec![make_post("shared".into(), shared.clone())];
        assert!(!fit_feed_page(&mut posts, &mut vec![], 0)?);
        assert_eq!(posts[0].replies, shared);
        assert!(!posts[0].more_replies);
        transport::decode_response(&encode(posts)?)?;
        Ok(())
    }
}
