// SPDX-License-Identifier: AGPL-3.0-only
//! An accepted delete wins over every representation of the same event. Proofs are
//! scoped by owner and community even when a delete arrives ahead of its target.
use crate::home_sql::HomeDb;
use crate::{model::*, store::Store};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};

impl Store {
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
        self.erased_after(community, "")
    }

    pub(crate) fn erased_after(&self, community: &str, after: &str) -> Result<Vec<String>> {
        let mut stmt = self.db.prepare("SELECT id FROM events WHERE community=? AND id>? AND json_extract(envelope,'$.deletion') IS NOT NULL ORDER BY id LIMIT ?")?;
        Ok(stmt
            .query_map(params![community, after, crate::DELETION_PAGE + 1], |row| {
                row.get(0)
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn apply_deletion(db: &HomeDb, proof: &Envelope, private: bool) -> Result<()> {
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

        if db.native().is_some() {
            crate::native_guard::stage_delete(db, proof, private)?;
            db.execute(
                "DELETE FROM home_openings WHERE target=? AND owner=? AND community=? AND private>=?",
                params![target, owner, proof.event.payload.community, private],
            )?;
        }

        // Attach the proof directly to each stored attestation. Neither a
        // pending native retirement nor an already retired key needs reopening.
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
            let old: Option<String> = statement.query_row(args, |r| r.get(0)).optional()?;
            if let Some(old) = old {
                let target_event: Envelope = serde_json::from_str(&old)?;
                if target_event.deleted_by(proof)
                    && target_event.deletion.is_none()
                    && matches!(
                        target_event.event.payload.action,
                        Action::Post { .. }
                            | Action::PostV2 { .. }
                            | Action::Reply { .. }
                            | Action::Erased { .. }
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
    pub(crate) fn reconcile(db: &HomeDb, entry: &Envelope, private: bool) -> Result<Envelope> {
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
                    // Native projections are arrays of [event_id, ciphertext].
                    // A portable projection carries nested post and reply objects.
                    let clear = if db.native().is_some() {
                        "DELETE FROM public_projections WHERE community=? AND EXISTS
                         (SELECT 1 FROM json_each(posts) WHERE json_extract(value,'$[0]')=?)"
                    } else {
                        "DELETE FROM public_projections WHERE community=? AND EXISTS
                         (SELECT 1 FROM json_tree(posts) WHERE key='id' AND value=?)"
                    };
                    db.execute(
                        clear,
                        params![entry.event.payload.community, entry.event.id],
                    )?;
                }
                entry.erased(&serde_json::from_str(&raw)?)
            }
            None => Ok(entry.clone()),
        }
    }
}
