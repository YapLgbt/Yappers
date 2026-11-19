// SPDX-License-Identifier: AGPL-3.0-only
//! Single-home storage. Only signed public membership state and encrypted data are stored.
use crate::{portability::digest, private_groups::*, store::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS private_groups(id TEXT PRIMARY KEY, owner TEXT NOT NULL, room TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS private_group_rosters(group_id TEXT NOT NULL, revision INTEGER NOT NULL,
    roster TEXT NOT NULL, PRIMARY KEY(group_id, revision));
CREATE TABLE IF NOT EXISTS private_group_sessions(group_id TEXT NOT NULL, id TEXT NOT NULL,
    packet TEXT NOT NULL, PRIMARY KEY(group_id, id));
CREATE TABLE IF NOT EXISTS private_group_events(cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id TEXT NOT NULL, id TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL,
    session TEXT NOT NULL, ratchet_index INTEGER NOT NULL, parent TEXT, event TEXT NOT NULL,
    UNIQUE(group_id, session, ratchet_index));
CREATE INDEX IF NOT EXISTS private_group_event_page ON private_group_events(group_id, cursor);
";

const GROUPS_PER_DEVICE: u32 = 16;
const GROUPS_PER_HOME: u32 = 128;
const EVENTS_PER_GROUP: u32 = 2000;
const PAGE_SIZE: usize = 24;

impl Store {
    fn private_roster(&self, group: &str) -> Result<Signed<Roster>> {
        hash_id(group)?;
        let value: String = self.db.query_row(
            "SELECT roster FROM private_group_rosters WHERE group_id=? ORDER BY revision DESC LIMIT 1",
            [group], |row| row.get(0),
        ).optional()?.context("private group unavailable")?;
        Ok(serde_json::from_str(&value)?)
    }

    fn private_group_owner(&self, group: &str) -> Result<String> {
        Ok(self.db.query_row(
            "SELECT owner FROM private_groups WHERE id=?",
            [group],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn private_group(
        &mut self,
        home: &str,
        account: &str,
        device: &str,
        op: Operation,
    ) -> Result<Outcome> {
        match op {
            Operation::Create { room, roster } => {
                let id = room.check(home)?;
                ensure!(
                    room.value.owner == device,
                    "group creator must own the group"
                );
                roster.check(&id, device, None)?;
                ensure!(
                    roster.value.members[device].device.account == account,
                    "group creator account mismatch"
                );
                if self
                    .db
                    .query_row("SELECT 1 FROM private_groups WHERE id=?", [&id], |row| {
                        row.get::<_, u8>(0)
                    })
                    .optional()?
                    .is_some()
                {
                    return Ok(Outcome::Saved);
                }
                let total: u32 =
                    self.db
                        .query_row("SELECT count(*) FROM private_groups", [], |row| row.get(0))?;
                let owned: u32 = self.db.query_row(
                    "SELECT count(*) FROM private_groups WHERE owner=?",
                    [device],
                    |row| row.get(0),
                )?;
                ensure!(
                    owned < GROUPS_PER_DEVICE && total < GROUPS_PER_HOME,
                    "private group capacity reached"
                );
                let tx = self.db.transaction()?;
                tx.execute(
                    "INSERT INTO private_groups VALUES(?,?,?)",
                    params![id, device, serde_json::to_string(&room)?],
                )?;
                tx.execute(
                    "INSERT INTO private_group_rosters VALUES(?,1,?)",
                    params![id, serde_json::to_string(&roster)?],
                )?;
                tx.commit()?;
                Ok(Outcome::Saved)
            }
            Operation::Roster { group } => {
                let current = self.private_roster(&group)?;
                if !current
                    .value
                    .members
                    .get(device)
                    .is_some_and(|m| m.device.account == account)
                {
                    return Ok(Outcome::Roster {
                        history: Vec::new(),
                    });
                }
                let mut query = self.db.prepare(
                    "SELECT roster FROM private_group_rosters WHERE group_id=? ORDER BY revision",
                )?;
                let rows = query.query_map([group], |row| row.get::<_, String>(0))?;
                let mut history = Vec::new();
                for row in rows {
                    history.push(serde_json::from_str(&row?)?);
                }
                Ok(Outcome::Roster { history })
            }
            Operation::Change { roster, request } => {
                let group = &roster.value.group;
                let prior = self.private_roster(group)?;
                ensure!(
                    self.private_group_owner(group)? == device,
                    "only the group owner can change membership"
                );
                if prior == roster {
                    return Ok(Outcome::Saved);
                }
                roster.check(group, device, Some(&prior))?;
                let added = roster
                    .value
                    .members
                    .iter()
                    .find(|(id, _)| !prior.value.members.contains_key(*id));
                match (added, request) {
                    (Some((_, member)), Some(request)) => {
                        request.check(group)?;
                        let new_device = &request.value.device;
                        ensure!(
                            new_device == &member.device,
                            "join request does not match the approved device"
                        );
                        let chain = self.account_chain(&new_device.account)?;
                        let record = chain
                            .last()
                            .context("the invited device must connect to this home first")?;
                        ensure!(
                            record.payload.home == home
                                && record.payload.devices.contains(&new_device.signing),
                            "the invited device is no longer authorized on this home"
                        );
                    }
                    (None, None) => {}
                    _ => anyhow::bail!("membership change requires a matching join request"),
                }
                self.db.execute(
                    "INSERT INTO private_group_rosters VALUES(?,?,?)",
                    params![
                        group,
                        roster.value.revision,
                        serde_json::to_string(&roster)?
                    ],
                )?;
                Ok(Outcome::Saved)
            }
            Operation::Publish { event, session } => {
                self.private_publish(account, device, event, session)
            }
            Operation::Read {
                group,
                roster,
                before,
            } => self.private_read(account, device, &group, &roster, before),
        }
    }

    fn private_publish(
        &mut self,
        account: &str,
        device: &str,
        event: Signed<Event>,
        session: Option<Signed<Session>>,
    ) -> Result<Outcome> {
        let header = &event.value.header;
        let current = self.private_roster(&header.group)?;
        let member = current
            .value
            .members
            .get(device)
            .filter(|m| m.device.account == account)
            .context("this device is not an active private group member")?;
        ensure!(header.sender == device, "private post sender mismatch");
        let id = header.id()?;
        // An acknowledged retry can cross an epoch boundary. Its stable ID binds
        // group, authorized sender and draft nonce. The original write wins.
        let existing: Option<String> = self
            .db
            .query_row(
                "SELECT event FROM private_group_events WHERE id=?",
                [&id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            event.verify(device)?;
            let original: Signed<Event> = serde_json::from_str(&existing)?;
            return Ok(Outcome::Accepted {
                id,
                roster: original.value.header.roster,
            });
        }
        ensure!(
            current.value.revision < MAX_REVISIONS,
            "private group membership capacity reached; this group is read-only"
        );
        let message = event.check(&current)?;
        if let Some(parent) = &header.parent {
            let visible: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM private_group_events
                  WHERE id=? AND group_id=? AND revision>=? AND parent IS NULL)",
                params![parent, header.group, member.since],
                |r| r.get(0),
            )?;
            ensure!(visible, "this post is outside your group history");
        }
        let count: u32 = self.db.query_row(
            "SELECT count(*) FROM private_group_events WHERE group_id=?",
            [&header.group],
            |row| row.get(0),
        )?;
        ensure!(
            count < EVENTS_PER_GROUP,
            "private group post capacity reached"
        );
        let tx = self.db.transaction()?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT packet FROM private_group_sessions WHERE group_id=? AND id=?",
                params![header.group, header.session],
                |row| row.get(0),
            )
            .optional()?;
        let packet: Signed<Session> = match (stored, session) {
            (Some(packet), None) => serde_json::from_str(&packet)?,
            (None, Some(packet)) => {
                packet.check(&current)?;
                tx.execute(
                    "INSERT INTO private_group_sessions VALUES(?,?,?)",
                    params![
                        header.group,
                        header.session,
                        serde_json::to_string(&packet)?
                    ],
                )?;
                packet
            }
            _ => anyhow::bail!("unexpected private group session"),
        };
        ensure!(
            packet.value.id == header.session
                && packet.value.group == header.group
                && packet.value.sender == device
                && packet.value.roster == header.roster,
            "private post session mismatch"
        );
        tx.execute(
            "INSERT INTO private_group_events(group_id,id,revision,session,ratchet_index,parent,event)
             VALUES(?,?,?,?,?,?,?)",
            params![header.group, id, header.revision, header.session, message.message_index(),
                header.parent, serde_json::to_string(&event)?],
        )?;
        tx.commit()?;
        Ok(Outcome::Accepted {
            id,
            roster: header.roster.clone(),
        })
    }

    fn private_read(
        &self,
        account: &str,
        device: &str,
        group: &str,
        roster: &str,
        before: Option<u64>,
    ) -> Result<Outcome> {
        let current = self.private_roster(group)?;
        ensure!(
            digest(&current)? == roster,
            "private group membership changed; refresh and retry"
        );
        let member = current
            .value
            .members
            .get(device)
            .filter(|m| m.device.account == account)
            .context("this device is not an active private group member")?;
        let mut query = self.db.prepare(
            "SELECT e.cursor,e.event,s.packet FROM private_group_events e
             JOIN private_group_sessions s ON s.group_id=e.group_id AND s.id=e.session
             WHERE e.group_id=? AND e.revision>=? AND e.cursor<?
             ORDER BY e.cursor DESC LIMIT ?",
        )?;
        let rows = query.query_map(
            params![
                group,
                member.since,
                before.unwrap_or(i64::MAX as u64),
                PAGE_SIZE + 1
            ],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut entries = Vec::new();
        let mut next = None;
        for row in rows {
            let (cursor, event, packet) = row?;
            if entries.len() == PAGE_SIZE {
                next = entries.last().map(|entry: &Delivery| entry.cursor);
                break;
            }
            let mut packet: Signed<Session> = serde_json::from_str(&packet)?;
            entries.push(Delivery {
                cursor,
                event: serde_json::from_str(&event)?,
                key: packet
                    .value
                    .keys
                    .remove(device)
                    .context("private group key unavailable")?,
            });
        }
        Ok(Outcome::Page {
            entries,
            before: next,
        })
    }
}
