// SPDX-License-Identifier: AGPL-3.0-only
//! An accepted delete wins over every representation of the same event. Proofs are
//! scoped by owner and community even when a delete arrives ahead of its target.
use crate::{model::*, store::Store};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS peer_deliveries(peer TEXT NOT NULL, community TEXT NOT NULL, target TEXT NOT NULL, PRIMARY KEY(peer,community,target));
    CREATE TABLE IF NOT EXISTS deletions(
    target TEXT NOT NULL, owner TEXT NOT NULL, community TEXT NOT NULL, private INTEGER NOT NULL,
    proof TEXT NOT NULL, PRIMARY KEY(target, owner, community, private));
    CREATE INDEX IF NOT EXISTS live_events ON events(id)
      WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply');
    CREATE INDEX IF NOT EXISTS live_personal_events ON personal_events(event_id)
      WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply');";

impl Store {
    /// Each open post or reply reserves one slot for the delete that will close it.
    /// Reconciliation has already released that slot when inserting an authorized delete.
    pub(crate) fn check_event_capacity(
        db: &Connection,
        entry: &Envelope,
        private: bool,
    ) -> Result<()> {
        let (table, limit) = if private {
            (
                "personal_events",
                crate::portability::MAX_ARCHIVE_EVENTS as i64,
            )
        } else {
            ("events", crate::store::MAX_EVENTS)
        };
        let used: i64 = db.query_row(&format!("SELECT (SELECT count(*) FROM {table}) +
            (SELECT count(*) FROM {table} WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply'))"), [], |r| r.get(0))?;
        let live = matches!(
            entry.event.payload.action,
            Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
        );
        ensure!(
            used + 1 + i64::from(live) <= limit,
            "event capacity reached"
        );
        Ok(())
    }
    pub(crate) fn acceptance(&self, envelope: Envelope) -> Result<Response> {
        let erased = if let Action::Delete { target } = &envelope.event.payload.action {
            let target = self
                .existing(target)?
                .filter(|e| e.deletion.is_some() && e.deleted_by(&envelope));
            target.map(Box::new)
        } else {
            None
        };
        Ok(Response::Accepted {
            event_id: envelope.event.id.clone(),
            envelope: Some(Box::new(envelope)),
            erased,
        })
    }

    pub(crate) fn erased_ids(&self, community: &str) -> Result<Vec<String>> {
        let mut stmt=self.db.prepare("SELECT id FROM events WHERE community=? AND json_extract(envelope,'$.deletion') IS NOT NULL ORDER BY id")?;
        Ok(stmt
            .query_map([community], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn apply_deletion(db: &Connection, proof: &Envelope, private: bool) -> Result<()> {
        let Action::Delete { target } = &proof.event.payload.action else {
            anyhow::bail!("expected deletion")
        };
        let owner = proof.event.owner_id();
        let changed = db.execute(
            "INSERT OR IGNORE INTO deletions VALUES(?,?,?,?,?)",
            params![
                target,
                owner,
                proof.event.payload.community,
                private,
                serde_json::to_string(proof)?
            ],
        )?;
        if changed == 0 {
            return Ok(());
        }

        for table in ["events", "personal_events"] {
            if private && table == "events" {
                continue;
            }
            let (select, update) = if table == "events" {
                (
                    "SELECT envelope FROM events WHERE id=?",
                    "UPDATE events SET envelope=? WHERE id=?",
                )
            } else {
                (
                    "SELECT envelope FROM personal_events WHERE event_id=? AND account=?2",
                    "UPDATE personal_events SET envelope=? WHERE event_id=? AND account=?3",
                )
            };
            let mut statement = db.prepare(select)?;
            let mut args = vec![target.as_str()];
            if table == "personal_events" {
                args.push(owner);
            }
            let old: Option<String> = statement
                .query_row(rusqlite::params_from_iter(args), |r| r.get(0))
                .optional()?;
            if let Some(old) = old {
                let target_event: Envelope = serde_json::from_str(&old)?;
                if target_event.deleted_by(proof)
                    && target_event.deletion.is_none()
                    && matches!(
                        target_event.event.payload.action,
                        Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
                    )
                {
                    let raw = serde_json::to_string(&target_event.erased(proof)?)?;
                    if table == "events" {
                        db.execute(update, params![raw, target])?;
                        db.execute("DELETE FROM username_receipts WHERE event_id=?", [target])?;
                        db.execute(
                            "DELETE FROM public_projections WHERE community=?",
                            [&proof.event.payload.community],
                        )?;
                    } else {
                        db.execute(update, params![raw, target, owner])?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Call inside the same transaction as the insertion and replication checkpoint.
    pub(crate) fn reconcile(db: &Connection, entry: &Envelope, private: bool) -> Result<Envelope> {
        ensure!(
            entry.event.payload.version == 4,
            "legacy events require the pre-cutover home"
        );
        if let Some(proof) = &entry.deletion {
            Self::apply_deletion(db, proof, private)?;
        }
        if matches!(entry.event.payload.action, Action::Delete { .. }) {
            Self::apply_deletion(db, entry, private)?;
        }
        if !matches!(
            entry.event.payload.action,
            Action::Post { .. }
                | Action::PostV2 { .. }
                | Action::Reply { .. }
                | Action::Erased { .. }
        ) {
            return Ok(entry.clone());
        }
        let proof: Option<String> = db
            .query_row(
                "SELECT proof FROM deletions
            WHERE target=? AND owner=? AND community=? AND private<=? ORDER BY private LIMIT 1",
                params![
                    entry.event.id,
                    entry.event.owner_id(),
                    entry.event.payload.community,
                    private
                ],
                |r| r.get(0),
            )
            .optional()?;
        match proof {
            Some(raw) => {
                if !private {
                    db.execute("DELETE FROM public_projections WHERE community=? AND EXISTS (SELECT 1 FROM json_tree(posts) WHERE key='id' AND value=?)",
                        params![entry.event.payload.community,entry.event.id])?;
                }
                entry.erased(&serde_json::from_str(&raw)?)
            }
            None => Ok(entry.clone()),
        }
    }
}
