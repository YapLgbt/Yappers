// SPDX-License-Identifier: AGPL-3.0-only
//! Account-wide erasure composes the existing per-event deletion proofs. The
//! caller authenticates the current device and commits the terminal guard intent
//! in the same transaction as the final purge.
use crate::{
    home_sql::HomeDb,
    model::{Action, Envelope},
    store::Store,
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};

const OWNER: &str =
    "coalesce(json_extract(envelope,'$.event.payload.account.payload.account'),author)";
const LIVE: &str = "json_extract(envelope,'$.deletion') IS NULL AND
    json_extract(envelope,'$.event.payload.action.kind') IN ('erased','post','postV2','reply')";

/// Each source contributes at most one page. A post saved in both scopes needs
/// just one signature; applying a public proof also retires its personal opening.
pub(crate) fn targets(db: &HomeDb, account: &str, limit: usize) -> Result<Vec<(String, String)>> {
    let limit = i64::try_from(limit)?;
    let sql = format!(
        "SELECT id,community FROM (
        SELECT id,community FROM (SELECT id,community FROM events
            WHERE {OWNER}=?1 AND {LIVE} ORDER BY id LIMIT ?2)
        UNION SELECT id,community FROM (SELECT event_id AS id,
            json_extract(envelope,'$.event.payload.community') AS community FROM personal_events
            WHERE account=?1 AND {LIVE} ORDER BY event_id LIMIT ?2)
        ) ORDER BY id LIMIT ?2"
    );
    Ok(db
        .prepare(&sql)?
        .query_map(params![account, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Apply a Home-accepted, device-signed Delete only to an owned content header.
/// Private-only targets and their deletion proofs never enter the public log.
pub(crate) fn apply_proof(db: &HomeDb, account: &str, proof: &Envelope) -> Result<()> {
    ensure!(
        !db.is_autocommit()?,
        "account deletion requires a transaction"
    );
    proof.validate()?;
    let Action::Delete { target } = &proof.event.payload.action else {
        anyhow::bail!("expected signed content deletion");
    };
    ensure!(
        proof.event.payload.version == 4 && proof.event.owner_id() == account,
        "wrong deletion owner"
    );
    let public: Option<String> = db
        .query_row("SELECT envelope FROM events WHERE id=?", [target], |r| {
            r.get(0)
        })
        .optional()?;
    let private = public.is_none();
    let raw = match public {
        Some(raw) => raw,
        None => db.query_row(
            "SELECT envelope FROM personal_events WHERE account=? AND event_id=?",
            params![account, target],
            |r| r.get(0),
        )?,
    };
    let header: Envelope = serde_json::from_str(&raw)?;
    ensure!(
        header.deleted_by(proof)
            && matches!(
                header.event.payload.action,
                Action::Post { .. }
                    | Action::PostV2 { .. }
                    | Action::Reply { .. }
                    | Action::Erased { .. }
            ),
        "invalid account deletion target"
    );
    // A retried request can receive a fresh local acceptance timestamp. Keep
    // the first signed Home attestation and the original replication cursor.
    if header.deletion.is_some() {
        return Ok(());
    }
    if private {
        Store::apply_deletion(db, proof, true)?;
        db.execute(
            "INSERT INTO personal_events(account,event_id,envelope) VALUES(?,?,?)",
            params![account, proof.event.id, serde_json::to_string(proof)?],
        )?;
    } else {
        Store::insert(db, proof)?;
    }
    Ok(())
}

/// Purge only after all known signed public and private targets are redacted.
/// Managed restore applies authoritative proofs before reaching this function.
pub(crate) fn purge(db: &HomeDb, account: &str) -> Result<()> {
    ensure!(!db.is_autocommit()?, "account purge requires a transaction");
    ensure!(
        targets(db, account, 1)?.is_empty(),
        "account still has live content requiring a signed deletion"
    );
    let owned = format!("SELECT id FROM events WHERE {OWNER}=?1");
    for sql in [
        format!("DELETE FROM username_receipts WHERE event_id IN ({owned})"),
        format!("DELETE FROM inbox_arrivals WHERE event IN ({owned})"),
        format!("DELETE FROM inbox_mutes WHERE owner=?1 OR target IN ({owned})"),
        format!(
            "DELETE FROM inbox_reads WHERE owner=?1 OR (kind='message' AND target=?1) OR (kind='reply' AND target IN ({owned}))"
        ),
        format!(
            "DELETE FROM push_routes WHERE owner=?1 OR (category IN ('message','request') AND target=?1) OR (category='reply' AND target IN ({owned}))"
        ),
        format!(
            "DELETE FROM public_projections WHERE community IN (SELECT community FROM events WHERE {OWNER}=?1)"
        ),
        format!(
            "DELETE FROM events WHERE {OWNER}=?1 AND json_extract(envelope,'$.event.payload.action.kind')='vote'"
        ),
    ] {
        db.execute(&sql, [account])?;
    }
    purge_private_groups(db, account)?;
    for sql in [
        "DELETE FROM personal_events WHERE account=?",
        "DELETE FROM public_profiles WHERE owner=?",
        "DELETE FROM public_profile_portraits WHERE owner=?",
        "DELETE FROM encrypted_attachment_chunks WHERE id IN (SELECT id FROM encrypted_attachments WHERE owner=?)",
        "DELETE FROM encrypted_attachments WHERE owner=?",
        "DELETE FROM username_claims WHERE account=?",
        "DELETE FROM dm_mail WHERE sender=?1 OR recipient=?1",
        "DELETE FROM dm_claims WHERE owner=?1 OR peer=?1",
        "DELETE FROM dm_prekeys WHERE owner=?",
        "DELETE FROM dm_devices WHERE owner=?",
        "DELETE FROM dm_approved WHERE owner=?1 OR peer=?1",
        "DELETE FROM user_blocks WHERE owner=?1 OR peer=?1",
        "DELETE FROM push_starts WHERE owner=?",
        "DELETE FROM push_devices WHERE owner=?",
        "DELETE FROM privacy_settings WHERE owner=?",
        "DELETE FROM first_seen WHERE owner=?",
        "DELETE FROM account_states WHERE account=?",
    ] {
        db.execute(sql, [account])?;
    }
    if db.native().is_some() {
        db.execute("DELETE FROM home_openings WHERE owner=?", [account])?;
    }
    Ok(())
}

fn purge_private_groups(db: &HomeDb, account: &str) -> Result<()> {
    // Signed roster membership binds a device to the account that used it at
    // that revision. Search membership records once before touching ciphertext.
    let mut stmt = db.prepare(
        "SELECT DISTINCT r.group_id FROM private_group_rosters r,
        json_each(r.roster,'$.value.members') m WHERE json_extract(m.value,'$.device.account')=?",
    )?;
    let rooms = stmt
        .query_map([account], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for group in rooms {
        // The creating device is a member of every signed roster. Delete the
        // whole room only if every historical member belonged to this account.
        let shared = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM private_group_rosters r,
            json_each(r.roster,'$.value.members') m WHERE r.group_id=?1
            AND json_extract(m.value,'$.device.account')<>?2)",
            params![group, account],
            |r| r.get::<_, bool>(0),
        )?;
        if !shared {
            for sql in [
                "DELETE FROM private_group_events WHERE group_id=?",
                "DELETE FROM private_group_sessions WHERE group_id=?",
                "DELETE FROM private_group_rosters WHERE group_id=?",
                "DELETE FROM private_groups WHERE id=?",
            ] {
                db.execute(sql, [&group])?;
            }
            continue;
        }
        let mut next = 0;
        let mut authored = db.prepare("SELECT e.cursor,e.id,e.session FROM private_group_events e
            JOIN private_group_rosters r ON r.group_id=e.group_id AND r.revision=e.revision
            JOIN json_each(r.roster,'$.value.members') m ON m.key=json_extract(e.event,'$.value.header.sender')
            WHERE e.group_id=?1 AND e.cursor>?2 AND json_extract(m.value,'$.device.account')=?3
            ORDER BY e.cursor LIMIT 128")?;
        loop {
            let mut ids = Vec::new();
            let mut sessions = std::collections::BTreeSet::new();
            {
                let mut rows = authored.query(params![group, next, account])?;
                while let Some(row) = rows.next()? {
                    next = row.get::<_, i64>(0)?;
                    ids.push(row.get::<_, String>(1)?);
                    sessions.insert(row.get::<_, String>(2)?);
                }
            }
            if ids.is_empty() {
                break;
            }
            db.execute(
                "DELETE FROM private_group_events WHERE group_id=? AND id IN
                (SELECT CAST(value AS TEXT) FROM json_each(?))",
                params![group, serde_json::to_string(&ids)?],
            )?;
            // Signed session packets remain byte-for-byte intact for any
            // surviving event, including another member's reply to a removed post.
            db.execute("DELETE FROM private_group_sessions WHERE group_id=?1 AND id IN
                (SELECT CAST(value AS TEXT) FROM json_each(?2)) AND NOT EXISTS (
                    SELECT 1 FROM private_group_events e WHERE e.group_id=?1 AND e.session=private_group_sessions.id)",
                params![group,serde_json::to_string(&sessions)?])?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/turso/account_erasure.rs"]
mod tests;
