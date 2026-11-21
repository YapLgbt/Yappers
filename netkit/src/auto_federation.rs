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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covenant: Option<CovenantFederation>,
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
}
impl CatalogOp<'_> {
    pub(super) fn public_join(&self, group: &str) -> Result<JoinConfig> {
        ensure!(
            self.state.auto.enabled && self.store.hosts_group(group)?,
            "group not hosted here"
        );
        Ok(JoinConfig {
            protocol_version: PROTOCOL_VERSION,
            server_addr: self.state.logical_addr(),
            community: group.into(),
            access_token: self.group_token(group),
            network_mode: self.network_mode,
        })
    }
    pub(super) fn group_summary(&self, id: &str) -> Result<GroupSummary> {
        self.store
            .group_summaries()?
            .into_iter()
            .find(|g| g.id == id)
            .context("group not found")
    }
    pub(super) fn local_directory(&mut self) -> Result<DirectorySnapshot> {
        ensure!(self.state.auto.enabled, "automatic federation is disabled");
        let own = self.state.logical_addr().id.to_string();
        let store = &mut *self.store;
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
            let address = self.state.logical_addr();
            let needs_refresh = previous.is_none_or(|h| {
                h.payload.expires_at <= now() + ANNOUNCEMENT_LIFETIME / 2
                    || serde_json::to_vec(&h.payload.address).ok()
                        != serde_json::to_vec(&address).ok()
            });
            if needs_refresh {
                let previous_time = previous.map_or(0, |h| h.payload.issued_at);
                announcements.push(HostAnnouncement::create_after(
                    &self.state.key,
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
            .retain(|p| !self.state.auto.blocked_hosts.contains(&p.host));
        snapshot.hosts.retain(|h| {
            !self
                .auto
                .blocked_hosts
                .contains(&h.payload.address.id.to_string())
        });
        Ok(snapshot)
    }
    pub(super) fn group_page(
        &mut self,
        community: &str,
        token: &str,
        after: &str,
    ) -> Result<Response> {
        self.member(community, token)?;
        self.local_directory()?;
        let mut bytes = 0;
        let mut groups = Vec::new();
        for group in self
            .store
            .group_summaries()?
            .into_iter()
            .filter(|g| g.id.as_str() > after)
            .filter(|g| self.covenant_visible(&g.owner, &g.id))
        {
            bytes += serde_json::to_vec(&group)?.len() + 1;
            if bytes > MAX_FRAME - 1024 {
                return Ok(Response::GroupPage { groups });
            }
            groups.push(group);
        }
        Ok(Response::Groups { groups })
    }

    pub(super) fn advertised_directory(&mut self) -> Result<DirectorySnapshot> {
        let mut snapshot = self.local_directory()?;
        self.filter_covenant_directory(&mut snapshot);
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
    pub(super) fn import_public_directory(&mut self, snapshot: &DirectorySnapshot) -> Result<()> {
        ensure!(self.state.auto.enabled, "automatic federation is disabled");
        let mut filtered = snapshot.clone();
        self.filter_covenant_directory(&mut filtered);
        filtered
            .permits
            .retain(|p| !self.state.auto.blocked_hosts.contains(&p.host));
        filtered.hosts.retain(|h| {
            !self
                .auto
                .blocked_hosts
                .contains(&h.payload.address.id.to_string())
        });
        self.store
            .merge_directory(&filtered, Some(&self.state.logical_addr().id.to_string()))
    }

    fn covenant_visible(&self, home: &str, group: &str) -> bool {
        self.auto.covenant.is_none()
            || (!self.auto.blocked_hosts.contains(home)
                && self.access(home, group) == Some(Access::Active))
    }

    fn filter_covenant_directory(&self, snapshot: &mut DirectorySnapshot) {
        if self.auto.covenant.is_none() {
            return;
        }
        snapshot
            .groups
            .retain(|group| self.covenant_visible(&group.payload.owner, &group.id));
        let groups: BTreeSet<_> = snapshot
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        snapshot.permits.retain(|permit| {
            groups.contains(permit.group.as_str())
                && self.covenant_visible(&permit.host, &permit.group)
        });
        let permits: BTreeSet<_> = snapshot
            .permits
            .iter()
            .map(|permit| (permit.group.as_str(), permit.host.as_str()))
            .collect();
        snapshot.hosts.retain(|host| {
            permits.contains(&(
                host.payload.group.as_str(),
                host.payload.address.id.to_string().as_str(),
            ))
        });
    }
    pub(super) fn automatic_scopes(&self, id: &str) -> Result<BTreeSet<String>> {
        let mut scopes = BTreeSet::new();
        if !self.state.auto.enabled || self.state.auto.blocked_hosts.contains(id) {
            return Ok(scopes);
        }
        if let Some(covenant) = &self.auto.covenant
            && self.covenants.confirmed(id)
        {
            scopes.insert(covenant.scope());
        }
        let store = &*self.store;
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
        let address = self
            .covenants
            .address(id)?
            .unwrap_or(EndpointAddr::new(id.parse()?));
        Ok(Some(PeerConfig {
            address,
            communities,
        }))
    }
    pub(super) fn enroll(&mut self, id: &str, group: &str) -> Result<HostPermit> {
        ensure!(
            self.state.auto.enabled
                && !self.state.auto.blocked_hosts.contains(id)
                && (self.auto.covenant.is_none() || self.active_covenant_peer(id))
                && self.access(id, group) == Some(Access::Active),
            "host enrollment disabled or blocked"
        );
        let store = &mut *self.store;
        let group = store.group(group)?.context("unknown group")?;
        ensure!(
            group.payload.owner == self.state.logical_addr().id.to_string(),
            "enroll at the group authority"
        );
        if let Some(permit) = store.permit(&group.id, id)? {
            return Ok(permit);
        }
        let permit = HostPermit::create(&self.state.key, &group, id.into())?;
        store.merge_directory(
            &DirectorySnapshot {
                permits: vec![permit.clone()],
                ..Default::default()
            },
            Some(&self.state.logical_addr().id.to_string()),
        )?;
        Ok(permit)
    }
    pub(super) fn create_group(&mut self, details: NewGroup) -> Result<SignedGroup> {
        ensure!(
            self.state.auto.enabled,
            "enable automatic federation to create public groups"
        );
        details.validate()?;
        let store = &mut *self.store;
        let existing = store.directory()?.groups.into_iter().find(|group| {
            group.payload.owner == self.state.logical_addr().id.to_string()
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
        let group = SignedGroup::create(&self.state.key, details)?;
        let permit = HostPermit::create(
            &self.state.key,
            &group,
            self.state.logical_addr().id.to_string(),
        )?;
        store.merge_directory(
            &DirectorySnapshot {
                groups: vec![group.clone()],
                permits: vec![permit],
                hosts: vec![],
            },
            Some(&self.state.logical_addr().id.to_string()),
        )?;
        store.host_group(&group.id)?;
        Ok(group)
    }
}
impl State {
    pub(super) async fn join_group(self: &Arc<Self>, group: &str) -> Result<()> {
        ensure!(self.auto.enabled, "automatic federation is disabled");
        let own = self.logical_addr().id.to_string();
        let scope = group.to_owned();
        let host = own.clone();
        let (descriptor, snapshot, permitted) = self
            .blocking(move |state| {
                let store = &*state.store;
                Ok((
                    store
                        .group(&scope)?
                        .context("refresh groups before joining")?,
                    store.directory()?,
                    store.permit(&scope, &host)?.is_some(),
                ))
            })
            .await?;
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
            let address = if self.auto.covenant.is_some() {
                self.covenants
                    .address(&descriptor.payload.owner)?
                    .context("group authority has not registered a covenant")?
            } else {
                authority.payload.address.clone()
            };
            let response = transport::rpc(
                &self.endpoint,
                address,
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
            self.blocking(move |state| {
                permit.validate(&descriptor)?;
                state.store.merge_directory(
                    &DirectorySnapshot {
                        permits: vec![permit.clone()],
                        ..Default::default()
                    },
                    Some(&own),
                )
            })
            .await?;
        }
        let scope = group.to_owned();
        self.blocking(move |state| state.store.host_group(&scope))
            .await
    }
}
impl Server {
    /// Public identity/metadata only: suitable for operator bootstrap distribution.
    pub fn public_address(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    pub fn create_group(&self, details: NewGroup) -> Result<GroupSummary> {
        ensure!(self.state.ha.is_none(), "use the fenced HA group RPC");
        self.state.operation(|op| {
            let group = op.create_group(details.clone())?;
            op.local_directory()?;
            op.group_summary(&group.id)
        })
    }
    pub async fn join_group(&self, group: &str) -> Result<JoinConfig> {
        ensure!(self.state.ha.is_none(), "use the fenced HA group RPC");
        self.state.join_group(group).await?;
        let group = group.to_owned();
        self.state
            .blocking(move |state| {
                state.local_directory()?;
                state.public_join(&group)
            })
            .await
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
        self.state.operation(|op| {
            Ok(op
                .store
                .group_summaries()?
                .into_iter()
                .filter(|group| op.covenant_visible(&group.owner, &group.id))
                .collect())
        })
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
        if self.state.auto.covenant.is_some() {
            for address in self.state.covenants.addresses()? {
                destinations.entry(address.id).or_insert(address);
            }
        } else {
            for host in self.state.blocking(|op| op.local_directory()).await?.hosts {
                destinations
                    .entry(host.payload.address.id)
                    .or_insert(host.payload.address);
            }
        }
        let destinations: Vec<_> = destinations
            .into_values()
            .filter(|a| {
                let id = a.id.to_string();
                a.id != own
                    && !self.state.auto.blocked_hosts.contains(&id)
                    && !self.state.covenants.stopped(&id)
                    && (self.state.auto.covenant.is_none() || self.state.active_covenant_peer(&id))
            })
            .collect();
        let mut errors = Vec::new();
        let round = self
            .state
            .discovery_round
            .fetch_add(DISCOVERY_FANOUT, Ordering::Relaxed);
        let selected = (0..destinations.len().min(DISCOVERY_FANOUT))
            .map(|i| destinations[round.wrapping_add(i) % destinations.len()].clone());
        let mut results = stream::iter(selected.map(|destination| async move {
            let snapshot = self.state.blocking(|op| op.advertised_directory()).await?;
            let response = transport::rpc(
                &self.endpoint,
                destination.clone(),
                &Request::Directory { snapshot },
            )
            .await
            .with_context(|| destination.id.to_string())?;
            let Response::Directory { snapshot } = response else {
                bail!("invalid directory response");
            };
            self.state
                .blocking(move |state| state.import_public_directory(&snapshot))
                .await
        }))
        .buffered_unordered(DISCOVERY_FANOUT);
        while let Some(result) = results.next().await {
            if let Err(error) = result {
                errors.push(format!("{error:#}"));
            }
        }
        ensure!(
            errors.is_empty(),
            "some discovery destinations failed: {}",
            errors.join("; ")
        );
        Ok(())
    }
}

impl CatalogOp<'_> {
    fn covenant_addresses(&self) -> Result<Vec<EndpointAddr>> {
        let mut addresses: BTreeMap<_, _> = self
            .covenants
            .addresses()?
            .into_iter()
            .filter(|address| {
                let id = address.id.to_string();
                self.covenants.confirmed(&id) && !self.auto.blocked_hosts.contains(&id)
            })
            .map(|address| (address.id, address))
            .collect();
        for seed in self
            .seeds
            .read()
            .map_err(|_| anyhow::anyhow!("seed policy unavailable"))?
            .iter()
        {
            if let Some(address) = addresses.get_mut(&seed.id) {
                *address = seed.clone();
            }
        }
        Ok(addresses.into_values().collect())
    }

    fn automatic_peer_at(&self, address: EndpointAddr) -> Result<Option<PeerConfig>> {
        let id = address.id.to_string();
        if address.id == self.endpoint.id() || self.auto.blocked_hosts.contains(&id) {
            return Ok(None);
        }
        let mut communities = self.automatic_scopes(&id)?;
        communities.retain(|scope| self.access(&id, scope).is_some());
        Ok((!communities.is_empty()).then_some(PeerConfig {
            address,
            communities,
        }))
    }

    pub(super) fn auto_peers(&mut self) -> Result<Vec<PeerConfig>> {
        if !self.auto.enabled {
            return Ok(vec![]);
        }
        let addresses = if self.auto.covenant.is_some() {
            self.covenant_addresses()?
        } else {
            let snapshot = self.local_directory()?;
            let mut addresses = BTreeMap::new();
            for host in snapshot.hosts {
                addresses
                    .entry(host.payload.address.id)
                    .or_insert(host.payload.address);
            }
            addresses.into_values().collect()
        };
        addresses
            .into_iter()
            .filter_map(|address| self.automatic_peer_at(address).transpose())
            .collect()
    }

    pub(super) fn select_auto_peers(&mut self) -> Result<Vec<PeerConfig>> {
        let round = self
            .replication_round
            .fetch_add(AUTO_SYNC_FANOUT, Ordering::Relaxed);
        if self.auto.covenant.is_some() {
            let addresses = self.covenant_addresses()?;
            return (0..addresses.len().min(AUTO_SYNC_FANOUT))
                .map(|i| addresses[round.wrapping_add(i) % addresses.len()].clone())
                .filter_map(|address| self.automatic_peer_at(address).transpose())
                .collect();
        }
        let peers = self.auto_peers()?;
        Ok((0..peers.len().min(AUTO_SYNC_FANOUT))
            .map(|i| peers[round.wrapping_add(i) % peers.len()].clone())
            .collect())
    }
}
