// SPDX-License-Identifier: AGPL-3.0-only
//! Bounded durable public catalog, host admission certificates, and expiring addresses.
//! Directory gossip is not peer trust: only a verified group-authority permit grants scope.
use crate::{groups::*, model::now, store::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

pub(crate) const SCHEMA: &str = "
 CREATE TABLE IF NOT EXISTS public_groups(id TEXT PRIMARY KEY, record TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS group_permits(group_id TEXT NOT NULL, host TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(group_id,host));
 CREATE TABLE IF NOT EXISTS group_hosts(group_id TEXT NOT NULL, host TEXT NOT NULL, issued_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, record TEXT NOT NULL, PRIMARY KEY(group_id,host));
 CREATE TABLE IF NOT EXISTS hosted_groups(group_id TEXT PRIMARY KEY);
";
fn decode<T: DeserializeOwned>(text: String) -> Result<T> {
    Ok(serde_json::from_str(&text)?)
}
impl Store {
    pub fn group(&self, id: &str) -> Result<Option<SignedGroup>> {
        self.db
            .query_row("SELECT record FROM public_groups WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(decode)
            .transpose()
    }
    pub fn permit(&self, group: &str, host: &str) -> Result<Option<HostPermit>> {
        self.db
            .query_row(
                "SELECT record FROM group_permits WHERE group_id=?1 AND host=?2",
                params![group, host],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()
    }
    pub fn hosts_group(&self, group: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM hosted_groups WHERE group_id=?1)",
            [group],
            |r| r.get(0),
        )?)
    }
    pub fn hosted_groups(&self) -> Result<Vec<String>> {
        self.db
            .prepare("SELECT group_id FROM hosted_groups ORDER BY group_id")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }
    pub fn host_group(&self, group: &str) -> Result<()> {
        ensure!(self.group(group)?.is_some(), "unknown group");
        self.db
            .execute("INSERT OR IGNORE INTO hosted_groups VALUES(?1)", [group])?;
        Ok(())
    }
    /// Records are private-table data, but only these public signed fields are exported.
    pub fn directory(&self) -> Result<DirectorySnapshot> {
        fn records<T: DeserializeOwned>(db: &rusqlite::Connection, sql: &str) -> Result<Vec<T>> {
            let mut query = db.prepare(sql)?;
            query
                .query_map([], |r| r.get::<_, String>(0))?
                .map(|row| decode(row?))
                .collect()
        }
        let groups = records(&self.db, "SELECT record FROM public_groups ORDER BY id")?;
        let permits = records(
            &self.db,
            "SELECT record FROM group_permits ORDER BY group_id,host",
        )?;
        let mut query = self
            .db
            .prepare("SELECT record FROM group_hosts WHERE expires_at>?1 ORDER BY group_id,host")?;
        let hosts = query
            .query_map([now()], |r| r.get::<_, String>(0))?
            .map(|row| decode(row?))
            .collect::<Result<Vec<_>>>()?;
        Ok(DirectorySnapshot {
            groups,
            permits,
            hosts,
        })
    }
    /// Validate the whole snapshot and its capacity before any catalog row is written.
    pub fn import_directory(&mut self, incoming: &DirectorySnapshot) -> Result<()> {
        self.merge_directory(incoming, None)
    }

    pub(crate) fn merge_directory(
        &mut self,
        incoming: &DirectorySnapshot,
        local_home: Option<&str>,
    ) -> Result<()> {
        ensure!(
            incoming.groups.len() <= MAX_GROUPS
                && incoming.permits.len() <= MAX_PROVIDERS
                && incoming.hosts.len() <= MAX_PROVIDERS,
            "directory page exceeds capacity"
        );
        let existing = self.directory()?;
        let mut groups: BTreeMap<_, _> = existing
            .groups
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();
        for group in &incoming.groups {
            group.validate()?;
            groups.insert(group.id.clone(), group.clone());
        }
        // Bound what unsolicited gossip may allocate. A group signed by this home
        // has already passed its own creation policy.
        let foreign: std::collections::BTreeSet<_> = groups
            .values()
            .filter(|g| Some(g.payload.owner.as_str()) != local_home)
            .map(|g| g.id.as_str())
            .collect();
        ensure!(
            foreign.len() <= MAX_GROUPS,
            "foreign group catalog capacity reached"
        );
        let mut permits: BTreeMap<_, _> = existing
            .permits
            .into_iter()
            .map(|p| ((p.group.clone(), p.host.clone()), p))
            .collect();
        for permit in &incoming.permits {
            permit.validate(
                groups
                    .get(&permit.group)
                    .context("permit for unknown group")?,
            )?;
            permits.insert((permit.group.clone(), permit.host.clone()), permit.clone());
        }
        ensure!(
            permits
                .keys()
                .filter(|(g, _)| foreign.contains(g.as_str()))
                .count()
                <= MAX_PROVIDERS,
            "provider catalog capacity reached"
        );
        let mut provider_counts = BTreeMap::new();
        for (group, _) in permits.keys() {
            let count = provider_counts.entry(group).or_insert(0usize);
            *count += 1;
            ensure!(
                *count <= MAX_GROUP_PROVIDERS,
                "group provider capacity reached"
            );
        }
        let mut hosts: BTreeMap<_, _> = existing
            .hosts
            .into_iter()
            .map(|h| {
                (
                    (h.payload.group.clone(), h.payload.address.id.to_string()),
                    h,
                )
            })
            .collect();
        for host in &incoming.hosts {
            let key = (
                host.payload.group.clone(),
                host.payload.address.id.to_string(),
            );
            host.validate(
                permits
                    .get(&key)
                    .context("address has no authority permit")?,
            )?;
            if hosts
                .get(&key)
                .is_none_or(|old| old.payload.issued_at < host.payload.issued_at)
            {
                hosts.insert(key, host.clone());
            }
        }
        ensure!(
            hosts
                .keys()
                .filter(|(g, _)| foreign.contains(g.as_str()))
                .count()
                <= MAX_PROVIDERS,
            "foreign host catalog capacity reached"
        );
        let tx = self.db.transaction()?;
        for group in incoming.groups.iter() {
            tx.execute(
                "INSERT OR IGNORE INTO public_groups VALUES(?1,?2)",
                params![group.id, serde_json::to_string(group)?],
            )?;
        }
        for permit in incoming.permits.iter() {
            tx.execute(
                "INSERT OR IGNORE INTO group_permits VALUES(?1,?2,?3)",
                params![permit.group, permit.host, serde_json::to_string(permit)?],
            )?;
        }
        tx.execute("DELETE FROM group_hosts WHERE expires_at<=?1", [now()])?;
        for host in hosts.values() {
            tx.execute("INSERT INTO group_hosts VALUES(?1,?2,?3,?4,?5) ON CONFLICT(group_id,host) DO UPDATE SET issued_at=excluded.issued_at,expires_at=excluded.expires_at,record=excluded.record",
                params![host.payload.group,host.payload.address.id.to_string(),host.payload.issued_at,host.payload.expires_at,serde_json::to_string(host)?])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn group_summaries(&self) -> Result<Vec<GroupSummary>> {
        let snapshot = self.directory()?;
        snapshot
            .groups
            .into_iter()
            .map(|group| {
                let hosted = self.hosts_group(&group.id)?;
                let homes = snapshot
                    .hosts
                    .iter()
                    .filter(|h| h.payload.group == group.id)
                    .count();
                let details = group.payload.details;
                Ok(GroupSummary {
                    id: group.id,
                    owner: group.payload.owner,
                    name: details.name,
                    description: details.description,
                    category: details.category,
                    locality: details.locality,
                    homes,
                    hosted,
                })
            })
            .collect()
    }
}
