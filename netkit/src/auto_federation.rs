// SPDX-License-Identifier: AGPL-3.0-only
//! Server-side catalog and automatic, authority-scoped provider enrollment.
use super::*;
use crate::{PROTOCOL_VERSION, groups::*};
use std::sync::atomic::Ordering;

const DISCOVERY_FANOUT: usize = 4;
const AUTO_SYNC_FANOUT: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoFederationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub seeds: Vec<EndpointAddr>,
    #[serde(default)]
    pub blocked_hosts: BTreeSet<String>,
}
impl State {
    pub(super) fn group_token(&self, group: &str) -> String {
        blake3::keyed_hash(
            &self.key.to_bytes(),
            &[b"yappers:public-group-token:v1\0", group.as_bytes()].concat(),
        )
        .to_hex()
        .to_string()
    }
    pub(super) fn public_join(&self, group: &str) -> Result<JoinConfig> {
        ensure!(
            self.auto.enabled && self.store()?.hosts_group(group)?,
            "group not hosted here"
        );
        Ok(JoinConfig {
            protocol_version: PROTOCOL_VERSION,
            server_addr: self.logical_addr(),
            community: group.into(),
            access_token: self.group_token(group),
            network_mode: self.network_mode,
        })
    }
    pub(super) fn group_summary(&self, id: &str) -> Result<GroupSummary> {
        self.store()?
            .group_summaries()?
            .into_iter()
            .find(|g| g.id == id)
            .context("group not found")
    }
    pub(super) fn local_directory(&self) -> Result<DirectorySnapshot> {
        ensure!(self.auto.enabled, "automatic federation is disabled");
        let own = self.logical_addr().id.to_string();
        let mut store = self.store()?;
        let current = store.directory()?;
        let mut announcements = Vec::new();
        for group in store.hosted_groups()? {
            if store.permit(&group, &own)?.is_none() {
                continue;
            }
            let previous = current
                .hosts
                .iter()
                .find(|h| h.payload.group == group && h.payload.address.id.to_string() == own);
            let address = self.logical_addr();
            let needs_refresh = previous.is_none_or(|h| {
                h.payload.expires_at <= now() + ANNOUNCEMENT_LIFETIME / 2
                    || serde_json::to_vec(&h.payload.address).ok()
                        != serde_json::to_vec(&address).ok()
            });
            if needs_refresh {
                let previous_time = previous.map_or(0, |h| h.payload.issued_at);
                announcements.push(HostAnnouncement::create_after(
                    &self.key,
                    group,
                    address,
                    previous_time,
                )?);
            }
        }
        for hosts in announcements.chunks(MAX_PROVIDERS) {
            store.merge_directory(
                &DirectorySnapshot {
                    hosts: hosts.to_vec(),
                    ..Default::default()
                },
                Some(&own),
            )?;
        }
        let mut snapshot = store.directory()?;
        snapshot
            .permits
            .retain(|p| !self.auto.blocked_hosts.contains(&p.host));
        snapshot.hosts.retain(|h| {
            !self
                .auto
                .blocked_hosts
                .contains(&h.payload.address.id.to_string())
        });
        Ok(snapshot)
    }
    pub(super) fn group_page(&self, community: &str, token: &str, after: &str) -> Result<Response> {
        self.member(community, token)?;
        self.local_directory()?;
        let mut bytes = 0;
        let mut groups = Vec::new();
        for group in self
            .store()?
            .group_summaries()?
            .into_iter()
            .filter(|g| g.id.as_str() > after)
        {
            bytes += serde_json::to_vec(&group)?.len() + 1;
            if bytes > MAX_FRAME - 1024 {
                return Ok(Response::GroupPage { groups });
            }
            groups.push(group);
        }
        Ok(Response::Groups { groups })
    }

    pub(super) fn advertised_directory(&self) -> Result<DirectorySnapshot> {
        let mut snapshot = self.local_directory()?;
        if snapshot.groups.len() <= MAX_GROUPS
            && snapshot.permits.len() <= MAX_PROVIDERS
            && snapshot.hosts.len() <= MAX_PROVIDERS
        {
            return Ok(snapshot);
        }
        // Every page includes the descriptors and signed permits needed to verify its hosts.
        let width = MAX_PROVIDERS / MAX_GROUP_PROVIDERS;
        let page = self.advertisement_round.fetch_add(1, Ordering::Relaxed)
            % snapshot.groups.len().div_ceil(width);
        snapshot.groups = snapshot
            .groups
            .into_iter()
            .skip(page * width)
            .take(width)
            .collect();
        let selected: BTreeSet<_> = snapshot.groups.iter().map(|g| g.id.as_str()).collect();
        snapshot
            .permits
            .retain(|p| selected.contains(p.group.as_str()));
        snapshot
            .hosts
            .retain(|h| selected.contains(h.payload.group.as_str()));
        Ok(snapshot)
    }
    pub(super) fn import_public_directory(&self, snapshot: &DirectorySnapshot) -> Result<()> {
        ensure!(self.auto.enabled, "automatic federation is disabled");
        let mut filtered = snapshot.clone();
        filtered
            .permits
            .retain(|p| !self.auto.blocked_hosts.contains(&p.host));
        filtered.hosts.retain(|h| {
            !self
                .auto
                .blocked_hosts
                .contains(&h.payload.address.id.to_string())
        });
        self.store()?
            .merge_directory(&filtered, Some(&self.logical_addr().id.to_string()))
    }
    pub(super) fn automatic_scopes(&self, id: &str) -> Result<BTreeSet<String>> {
        let mut scopes = BTreeSet::new();
        if !self.auto.enabled || self.auto.blocked_hosts.contains(id) {
            return Ok(scopes);
        }
        let store = self.store()?;
        for group in store.hosted_groups()? {
            if let Some(permit) = store.permit(&group, id)? {
                permit.validate(&store.group(&group)?.context("missing group descriptor")?)?;
                scopes.insert(group);
            }
        }
        Ok(scopes)
    }
    pub(super) fn automatic_peer(&self, id: &str) -> Result<Option<PeerConfig>> {
        let communities = self.automatic_scopes(id)?;
        if communities.is_empty() {
            return Ok(None);
        }
        let address = EndpointAddr::new(id.parse()?);
        Ok(Some(PeerConfig {
            address,
            communities,
        }))
    }
    pub(super) fn enroll(&self, id: &str, group: &str) -> Result<HostPermit> {
        ensure!(
            self.auto.enabled
                && !self.auto.blocked_hosts.contains(id)
                && self.access(id, group) == Some(Access::Active),
            "host enrollment disabled or blocked"
        );
        let mut store = self.store()?;
        let group = store.group(group)?.context("unknown group")?;
        ensure!(
            group.payload.owner == self.logical_addr().id.to_string(),
            "enroll at the group authority"
        );
        if let Some(permit) = store.permit(&group.id, id)? {
            return Ok(permit);
        }
        let permit = HostPermit::create(&self.key, &group, id.into())?;
        store.merge_directory(
            &DirectorySnapshot {
                permits: vec![permit.clone()],
                ..Default::default()
            },
            Some(&self.logical_addr().id.to_string()),
        )?;
        Ok(permit)
    }
    pub(super) fn create_group(&self, details: NewGroup) -> Result<SignedGroup> {
        ensure!(
            self.auto.enabled,
            "enable automatic federation to create public groups"
        );
        details.validate()?;
        let mut store = self.store()?;
        let existing = store.directory()?.groups.into_iter().find(|group| {
            group.payload.owner == self.logical_addr().id.to_string()
                && group.payload.details.name.trim().to_lowercase()
                    == details.name.trim().to_lowercase()
                && group.payload.details.locality.as_ref().map(Locality::key)
                    == details.locality.as_ref().map(Locality::key)
        });
        if let Some(group) = existing {
            ensure!(
                group.payload.details.description == details.description
                    && group.payload.details.category == details.category,
                "a group with this name and locality already exists; refresh groups"
            );
            store.host_group(&group.id)?;
            return Ok(group);
        }
        let group = SignedGroup::create(&self.key, details)?;
        let permit = HostPermit::create(&self.key, &group, self.logical_addr().id.to_string())?;
        store.merge_directory(
            &DirectorySnapshot {
                groups: vec![group.clone()],
                permits: vec![permit],
                hosts: vec![],
            },
            Some(&self.logical_addr().id.to_string()),
        )?;
        store.host_group(&group.id)?;
        Ok(group)
    }
    pub(super) async fn join_group(&self, group: &str) -> Result<()> {
        ensure!(self.auto.enabled, "automatic federation is disabled");
        let own = self.logical_addr().id.to_string();
        let (descriptor, snapshot, permitted) = {
            let store = self.store()?;
            (
                store
                    .group(group)?
                    .context("refresh groups before joining")?,
                store.directory()?,
                store.permit(group, &own)?.is_some(),
            )
        };
        ensure!(
            !self.auto.blocked_hosts.contains(&descriptor.payload.owner)
                && self.access(&descriptor.payload.owner, group) == Some(Access::Active),
            "group authority blocked"
        );
        if !permitted {
            let authority = snapshot
                .hosts
                .iter()
                .find(|h| {
                    h.payload.group == group
                        && h.payload.address.id.to_string() == descriptor.payload.owner
                })
                .context("group authority is offline; try again later")?;
            let response = transport::rpc(
                &self.endpoint,
                authority.payload.address.clone(),
                &Request::EnrollHost {
                    group: group.into(),
                },
            )
            .await?;
            let Response::HostPermit { permit } = response else {
                bail!("invalid host enrollment response");
            };
            ensure!(
                permit.group == group && permit.host == own,
                "enrollment identity mismatch"
            );
            permit.validate(&descriptor)?;
            self.store()?.merge_directory(
                &DirectorySnapshot {
                    permits: vec![permit],
                    ..Default::default()
                },
                Some(&self.logical_addr().id.to_string()),
            )?;
        }
        self.store()?.host_group(group)?;
        Ok(())
    }
}
impl Server {
    /// Public identity/metadata only: suitable for operator bootstrap distribution.
    pub fn public_address(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    pub fn create_group(&self, details: NewGroup) -> Result<GroupSummary> {
        ensure!(self.state.ha.is_none(), "use the fenced HA group RPC");
        let group = self.state.create_group(details)?;
        self.state.local_directory()?;
        self.state.group_summary(&group.id)
    }
    pub async fn join_group(&self, group: &str) -> Result<JoinConfig> {
        ensure!(self.state.ha.is_none(), "use the fenced HA group RPC");
        self.state.join_group(group).await?;
        self.state.local_directory()?;
        self.state.public_join(group)
    }
    /// Useful for local fixture wiring. This changes discovery seeds, never grants peer trust.
    pub fn set_discovery_seeds(&self, seeds: Vec<EndpointAddr>) -> Result<()> {
        ensure!(seeds.len() <= 16, "too many discovery seeds");
        *self
            .state
            .seeds
            .write()
            .map_err(|_| anyhow::anyhow!("seed policy unavailable"))? = seeds;
        Ok(())
    }
    pub fn groups(&self) -> Result<Vec<GroupSummary>> {
        ensure!(self.state.ha.is_none(), "use the fenced HA group RPC");
        self.state.store()?.group_summaries()
    }

    pub(super) fn auto_peers(&self) -> Result<Vec<PeerConfig>> {
        if !self.state.auto.enabled {
            return Ok(vec![]);
        }
        let snapshot = self.state.local_directory()?;
        let mut by_host: BTreeMap<String, PeerConfig> = BTreeMap::new();
        for host in snapshot.hosts {
            let id = host.payload.address.id.to_string();
            if id == self.endpoint.id().to_string() || self.state.auto.blocked_hosts.contains(&id) {
                continue;
            }
            let mut scopes = self.state.automatic_scopes(&id)?;
            scopes.retain(|c| self.state.access(&id, c) == Some(Access::Active));
            if scopes.is_empty() {
                continue;
            }
            by_host.entry(id).or_insert(PeerConfig {
                address: host.payload.address,
                communities: scopes,
            });
        }
        Ok(by_host.into_values().collect())
    }
    pub(super) async fn discover_once(&self) -> Result<()> {
        if !self.state.auto.enabled {
            return Ok(());
        }
        let own = self.endpoint.id();
        let mut destinations: BTreeMap<_, _> = self
            .state
            .seeds
            .read()
            .map_err(|_| anyhow::anyhow!("seed policy unavailable"))?
            .iter()
            .map(|a| (a.id, a.clone()))
            .collect();
        for host in self.state.local_directory()?.hosts {
            destinations
                .entry(host.payload.address.id)
                .or_insert(host.payload.address);
        }
        let destinations: Vec<_> = destinations
            .into_values()
            .filter(|a| a.id != own && !self.state.auto.blocked_hosts.contains(&a.id.to_string()))
            .collect();
        let mut errors = Vec::new();
        let round = self
            .state
            .discovery_round
            .fetch_add(DISCOVERY_FANOUT, Ordering::Relaxed);
        for index in 0..destinations.len().min(DISCOVERY_FANOUT) {
            let destination = &destinations[(round.wrapping_add(index)) % destinations.len()];
            let response = transport::rpc(
                &self.endpoint,
                destination.clone(),
                &Request::Directory {
                    snapshot: self.state.advertised_directory()?,
                },
            )
            .await;
            match response {
                Ok(Response::Directory { snapshot }) => {
                    if let Err(error) = self.state.import_public_directory(&snapshot) {
                        errors.push(error.to_string());
                    }
                }
                Ok(_) => errors.push("invalid directory response".into()),
                Err(error) => errors.push(format!("{}: {error:#}", destination.id)),
            }
        }
        ensure!(
            errors.is_empty(),
            "some discovery destinations failed: {}",
            errors.join("; ")
        );
        Ok(())
    }
    pub(super) fn select_auto_peers(&self) -> Result<Vec<PeerConfig>> {
        let peers = self.auto_peers()?;
        let round = self
            .state
            .replication_round
            .fetch_add(AUTO_SYNC_FANOUT, Ordering::Relaxed);
        Ok((0..peers.len().min(AUTO_SYNC_FANOUT))
            .map(|i| peers[round.wrapping_add(i) % peers.len()].clone())
            .collect())
    }
}
