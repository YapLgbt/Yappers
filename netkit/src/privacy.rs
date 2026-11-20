// SPDX-License-Identifier: AGPL-3.0-only
//! Home-local observations, inbox permissions and private message requests.
use crate::home_sql::HomeDb as Connection;
use crate::model::{Action, AuthorVisibility, Envelope};
use crate::{messaging::PrivacySettings, model::now, store::Store};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) fn observe(db: &Connection, owner: &str) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO first_seen VALUES(?,?)",
        params![owner, now()],
    )?;
    Ok(())
}

// Only disclose the supported age brackets, including for anonymous authors.
pub(crate) fn age(since: i64) -> u8 {
    let days = (now() - since).max(0) / 86400;
    [30, 7, 1, 0]
        .into_iter()
        .find(|d| days >= i64::from(*d))
        .unwrap_or(0)
}

pub(crate) fn settings(db: &Connection, owner: &str) -> Result<PrivacySettings> {
    let text: Option<String> = db
        .query_row(
            "SELECT settings FROM privacy_settings WHERE owner=?",
            [owner],
            |r| r.get(0),
        )
        .optional()?;
    Ok(text
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}

pub(crate) fn needs_request(
    db: &Connection,
    owner: &str,
    peer: &str,
    policy: &PrivacySettings,
) -> Result<bool> {
    let approved: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM dm_approved WHERE owner=? AND peer=?)",
        [owner, peer],
        |r| r.get(0),
    )?;
    if approved || policy.approved_only {
        return Ok(!approved);
    }
    if peer_age(db, peer)? < policy.dm_days {
        return Ok(true);
    }
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM dm_mail WHERE recipient=? AND sender=? AND held=1 AND packet IS NOT NULL AND created>=?)",
        params![owner,peer,now()-30*86400], |r| r.get(0),
    )?)
}

pub(crate) fn peer_age(db: &Connection, peer: &str) -> Result<u8> {
    let since: Option<i64> = db
        .query_row("SELECT since FROM first_seen WHERE owner=?", [peer], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(since.map(age).unwrap_or(0))
}

/// Moving approved packets to fresh sequence numbers keeps every existing inbox cursor valid.
pub(crate) fn approve(db: &Connection, owner: &str, peer: &str) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO dm_approved VALUES(?,?)",
        [owner, peer],
    )?;
    let packets = db.prepare("SELECT id,packet,created FROM dm_mail WHERE recipient=? AND sender=? AND held=1 AND packet IS NOT NULL AND created>=? ORDER BY seq")?
        .query_map(params![owner,peer,now()-30*86400],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, packet, created) in packets {
        db.execute("DELETE FROM dm_mail WHERE id=?", [&id])?;
        db.execute(
            "INSERT INTO dm_mail(id,sender,recipient,packet,created) VALUES(?,?,?,?,?)",
            params![id, peer, owner, packet, created],
        )?;
    }
    Ok(())
}

impl Store {
    // A receipt signed by this home is local evidence. Remote timestamps cannot backdate age.
    pub(crate) fn recover_local_first_seen(&self, home: &str) -> Result<()> {
        self.db.execute("INSERT INTO first_seen(owner,since)
            SELECT COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author),
                   MIN(json_extract(envelope,'$.acceptedAt'))
            FROM events WHERE json_extract(envelope,'$.event.payload.home')=?
            GROUP BY COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author)
            ON CONFLICT(owner) DO UPDATE SET since=MIN(since,excluded.since)",[home])?;
        Ok(())
    }
    pub(crate) fn ages(&self) -> Result<HashMap<String, u8>> {
        Ok(self
            .db
            .prepare("SELECT owner,since FROM first_seen")?
            .query_map([], |r| Ok((r.get(0)?, age(r.get(1)?))))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn page_ages(&self, events: &[Envelope]) -> Result<HashMap<String, u8>> {
        let owners: std::collections::BTreeSet<_> =
            events.iter().map(|e| e.event.owner_id()).collect();
        let mut stmt = self.db.prepare(
            "SELECT owner,since FROM first_seen WHERE owner IN (SELECT CAST(value AS TEXT) FROM json_each(?))",
        )?;
        Ok(stmt
            .query_map([serde_json::to_string(&owners)?], |r| {
                Ok((r.get(0)?, age(r.get(1)?)))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn update_privacy(&mut self, owner: &str, p: &PrivacySettings) -> Result<()> {
        ensure!(
            [0, 1, 7, 30].contains(&p.feed_days) && [0, 1, 7, 30].contains(&p.dm_days),
            "choose a supported account age"
        );
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO privacy_settings VALUES(?,?) ON CONFLICT(owner) DO UPDATE SET settings=excluded.settings",params![owner,serde_json::to_string(p)?])?;
        let peers=tx.prepare("SELECT DISTINCT sender FROM dm_mail WHERE recipient=? AND held=0 AND packet IS NOT NULL")?
            .query_map([owner],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for peer in peers {
            if needs_request(&tx, owner, &peer, p)? {
                tx.execute("UPDATE dm_mail SET held=1 WHERE recipient=? AND sender=? AND packet IS NOT NULL",[owner,&peer])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn privacy_status(&self, owner: &str) -> Result<Value> {
        let requests=self.db.prepare("SELECT sender FROM dm_mail WHERE recipient=? AND held=1 AND packet IS NOT NULL AND created>=? GROUP BY sender ORDER BY MAX(seq) DESC")?
            .query_map(params![owner,now()-30*86400],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let has_blocks: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE owner=?)",
            [owner],
            |r| r.get(0),
        )?;
        Ok(json!({"settings":settings(&self.db,owner)?,"requests":requests,"hasBlocks":has_blocks}))
    }
}

pub(crate) fn visibility(e: &Envelope, ages: &HashMap<String, u8>) -> AuthorVisibility {
    let anonymous = matches!(
        &e.event.payload.action,
        Action::Post {
            anonymous: true,
            ..
        } | Action::PostV2 {
            anonymous: true,
            ..
        }
    );
    let anonymous = anonymous
        || matches!(&e.event.payload.action,Action::Reply {display_name,..} if display_name=="Anonymous neighbor");
    let author = e.event.owner_id();
    AuthorVisibility {
        peer: (!anonymous).then(|| author.to_owned()),
        anonymous,
        age: ages.get(author).copied().unwrap_or(0),
    }
}
