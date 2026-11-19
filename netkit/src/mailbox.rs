// SPDX-License-Identifier: AGPL-3.0-only
//! Private, non-replicating tables. Caller has verified the membership and channel-bound device proof.
use crate::{
    messaging::*,
    model::*,
    portability::{AccountRecord, digest},
    store::Store,
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use std::collections::HashSet;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS user_blocks(owner TEXT NOT NULL, peer TEXT NOT NULL, handle TEXT UNIQUE NOT NULL, label TEXT NOT NULL, PRIMARY KEY(owner,peer));
CREATE TABLE IF NOT EXISTS dm_devices(owner TEXT PRIMARY KEY, device TEXT NOT NULL, identity TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS dm_prekeys(owner TEXT NOT NULL, once_key TEXT NOT NULL, bundle TEXT, PRIMARY KEY(owner,once_key));
CREATE TABLE IF NOT EXISTS dm_mail(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, sender TEXT NOT NULL, recipient TEXT NOT NULL, packet TEXT, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS dm_claims(owner TEXT NOT NULL, peer TEXT NOT NULL, created INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS dm_claim_owner ON dm_claims(owner,created);
CREATE INDEX IF NOT EXISTS dm_claim_age ON dm_claims(created);
CREATE INDEX IF NOT EXISTS dm_mail_age ON dm_mail(created);
CREATE INDEX IF NOT EXISTS dm_recipient ON dm_mail(recipient,seq);
";

impl Store {
    fn block_peers(&self, owner: &str) -> Result<HashSet<String>> {
        Ok(self
            .db
            .prepare("SELECT peer FROM user_blocks WHERE owner=?1")?
            .query_map([owner], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn contact_event(
        &self,
        community: &str,
        event: &str,
        allow_anonymous: bool,
    ) -> Result<(String, String)> {
        let event = self
            .existing(event)?
            .context("post or reply unavailable")?
            .event;
        ensure!(event.payload.community == community, "wrong community");
        let fallback = match &event.payload.action {
            Action::Post {
                display_name,
                anonymous,
                ..
            }
            | Action::PostV2 {
                display_name,
                anonymous,
                ..
            } => {
                ensure!(
                    allow_anonymous || !anonymous,
                    "anonymous posts cannot receive DMs"
                );
                if *anonymous {
                    "Anonymous neighbor"
                } else {
                    display_name
                }
            }
            Action::Reply { display_name, .. } => {
                ensure!(
                    allow_anonymous || display_name != "Anonymous neighbor",
                    "anonymous replies cannot receive DMs"
                );
                display_name
            }
            _ => bail!("choose a post or reply"),
        };
        let name = if crate::usernames::named(&event) {
            self.current_profile(&event.payload.home, event.owner_id())?
                .map(|record| record.name)
        } else {
            None
        };
        Ok((
            event.owner_id().to_string(),
            name.unwrap_or_else(|| fallback.to_owned()),
        ))
    }
    fn unavailable(&self, owner: &str, peer: &str) -> Result<bool> {
        Ok(self.db.query_row("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (owner=?1 AND peer=?2) OR (owner=?2 AND peer=?1))", [owner,peer], |r| r.get(0))?)
    }
    pub(crate) fn social(
        &mut self,
        home: &str,
        community: &str,
        record: &AccountRecord,
        device: &str,
        op: SocialOp,
    ) -> Result<Response> {
        let owner = record.payload.account.as_str();
        let enhanced = matches!(&op, SocialOp::PrivacyFeed { .. });
        let result = match op {
            SocialOp::PrivacyFeed { nearby } | SocialOp::Feed { nearby } => {
                if let Some(n) = &nearby {
                    n.validate()?;
                }
                let blocked = self.block_peers(owner)?;
                let mut events = self.events(community)?;
                let policy = crate::privacy::settings(&self.db, owner)?;
                let ages = self.ages()?;
                let visibility = |e: &Envelope| crate::privacy::visibility(e, &ages);
                events.retain(|e| {
                    let author = e.event.owner_id();
                    if blocked.contains(author) {
                        return false;
                    }
                    if author == owner {
                        return true;
                    }
                    if matches!(
                        e.event.payload.action,
                        Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
                    ) {
                        let meta = visibility(e);
                        return !(policy.hide_anonymous && meta.anonymous)
                            && meta.age >= policy.feed_days;
                    }
                    true
                });
                let mut posts =
                    project_nearby(&events, community, owner, &self.hidden()?, nearby.as_ref());
                self.apply_current_bylines(&mut posts, &events)?;
                let by_id: std::collections::HashMap<_, _> =
                    events.iter().map(|e| (e.event.id.as_str(), e)).collect();
                for post in &mut posts {
                    if enhanced {
                        post.visibility = by_id.get(post.id.as_str()).map(|e| visibility(e));
                    }
                    for reply in &mut post.replies {
                        if enhanced && let Some(e) = by_id.get(reply.id.as_str()) {
                            reply.mine = e.event.owner_id() == owner;
                            reply.visibility = Some(visibility(e));
                        }
                    }
                }
                let chain = self.account_chain(owner)?;
                let deleted = self.erased_ids(community)?;
                let overhead =
                    serde_json::to_vec(&chain)?.len() + serde_json::to_vec(&deleted)?.len() + 1024;
                ensure!(
                    overhead < crate::MAX_FRAME,
                    "deletion history exceeds frame"
                );
                while serde_json::to_vec(&posts)?.len() + overhead > crate::MAX_FRAME {
                    posts.pop();
                }
                return Ok(Response::Feed {
                    deleted,
                    posts,
                    event_count: events.len(),
                    account_chain: chain,
                    direct: true,
                    privacy: enhanced,
                });
            }
            SocialOp::SetProfile {
                expected,
                name,
                profile,
            } => serde_json::to_value(self.set_profile(home, owner, expected, name, profile)?)?,
            SocialOp::Profiles { events } => {
                ensure!(events.len() <= 100, "too many profile references");
                let hidden = self.hidden()?;
                let mut profiles = serde_json::Map::new();
                for id in events {
                    if hidden.contains(&id) {
                        continue;
                    }
                    let Some(envelope) = self.existing(&id)? else {
                        continue;
                    };
                    let event = envelope.event;
                    let peer = event.owner_id();
                    if event.payload.community == community
                        && crate::usernames::named(&event)
                        && !self.unavailable(owner, peer)?
                        && !profiles.contains_key(peer)
                        && let Some(record) = self.current_profile(&event.payload.home, peer)?
                    {
                        profiles.insert(peer.to_owned(), serde_json::to_value(record)?);
                    }
                }
                serde_json::Value::Object(profiles)
            }
            SocialOp::PrivacyStatus => {
                let mut result = self.privacy_status(owner)?;
                result["currentProfile"] =
                    serde_json::to_value(self.current_profile(home, owner)?)?;
                if let Some(domain) = crate::usernames::domain(home) {
                    result["usernameDomain"] = json!(domain);
                    result["username"] = json!(
                        self.username(home, owner)?
                            .map(|name| format!("{name}@{domain}"))
                    );
                }
                result
            }
            SocialOp::ClaimUsername { name } => {
                let local = self.claim_username(home, owner, &name)?;
                json!({"username": format!("{}@{}",local,crate::usernames::domain(home).context("No username domain")?)})
            }
            SocialOp::SetPrivacy { settings } => {
                self.update_privacy(owner, &settings)?;
                json!({})
            }
            SocialOp::Approve { peer } => {
                valid_peer(&peer, owner)?;
                ensure!(!self.unavailable(owner, &peer)?, "contact unavailable");
                let tx = self.db.transaction()?;
                crate::privacy::approve(&tx, owner, &peer)?;
                tx.commit()?;
                json!({})
            }
            SocialOp::Contact { event } => {
                let (peer, label) = self.contact_event(community, &event, false)?;
                valid_peer(&peer, owner)?;
                ensure!(!self.unavailable(owner, &peer)?, "messaging unavailable");
                json!({"peer":peer,"label":label,"event":self.existing(&event)?.context("missing contact event")?.event})
            }
            SocialOp::Block { target } => {
                let (peer, label) = match target {
                    Target::Event(id) => {
                        let (peer, label) = self.contact_event(community, &id, true)?;
                        // Avoid retaining a copy of a post's byline in the block list.
                        let label = if label == "Anonymous neighbor" {
                            label
                        } else {
                            "Blocked account".into()
                        };
                        (peer, label)
                    }
                    Target::Account(peer) => {
                        valid_peer(&peer, owner)?;
                        let label = format!("Account {}", &peer[..10]);
                        (peer, label)
                    }
                };
                valid_peer(&peer, owner)?;
                let tx = self.db.transaction()?;
                tx.execute(
                    "INSERT OR IGNORE INTO user_blocks(owner,peer,handle,label) VALUES(?,?,?,?)",
                    params![owner, peer, new_secret(), label],
                )?;
                tx.execute("UPDATE dm_mail SET packet=NULL WHERE (sender=?1 AND recipient=?2) OR (sender=?2 AND recipient=?1)",[owner,&peer])?;
                tx.execute("DELETE FROM dm_approved WHERE (owner=?1 AND peer=?2) OR (owner=?2 AND peer=?1)",[owner,&peer])?;
                let id: String = tx.query_row(
                    "SELECT handle FROM user_blocks WHERE owner=? AND peer=?",
                    [owner, &peer],
                    |r| r.get(0),
                )?;
                tx.commit()?;
                json!({"id":id})
            }
            SocialOp::Unblock { id } => {
                self.db.execute(
                    "DELETE FROM user_blocks WHERE owner=? AND handle=?",
                    [owner, &id],
                )?;
                json!({})
            }
            SocialOp::Blocks => {
                let rows = self
                    .db
                    .prepare(
                        "SELECT handle,label FROM user_blocks WHERE owner=? ORDER BY rowid DESC",
                    )?
                    .query_map([owner], |r| {
                        Ok(json!({"id":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?}))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                json!({"blocks":rows})
            }
            SocialOp::Publish { keys } => {
                ensure!(keys.len() <= PREKEY_BATCH, "too many prekeys");
                let tx = self.db.transaction()?;
                for k in keys {
                    k.check(owner, home)?;
                    ensure!(
                        k.record == *record && k.device == device,
                        "unauthorized prekey"
                    );
                    let identity = k.value.identity.to_base64();
                    tx.execute(
                        "INSERT OR IGNORE INTO dm_devices VALUES(?,?,?)",
                        params![owner, device, identity],
                    )?;
                    let matches: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM dm_devices WHERE owner=? AND device=? AND identity=?)", params![owner,device,identity], |r| r.get(0))?;
                    ensure!(matches, "messaging is registered to a different device");
                    tx.execute(
                        "INSERT OR IGNORE INTO dm_prekeys VALUES(?,?,?)",
                        params![owner, k.value.once.to_base64(), serde_json::to_string(&k)?],
                    )?;
                }
                let remaining: u32 = tx.query_row(
                    "SELECT COUNT(*) FROM dm_prekeys WHERE owner=? AND bundle IS NOT NULL",
                    [owner],
                    |r| r.get(0),
                )?;
                ensure!(remaining <= 32, "prekey capacity reached");
                tx.commit()?;
                json!({"remaining":remaining})
            }
            SocialOp::Claim { peer } => {
                valid_peer(&peer, owner)?;
                ensure!(!self.unavailable(owner, &peer)?, "messaging unavailable");
                let chain = self.account_chain(&peer)?;
                let current = chain
                    .last()
                    .context("recipient has not enabled messaging on this home")?;
                let tx = self.db.transaction()?;
                tx.execute("DELETE FROM dm_claims WHERE created < ?", [now() - 3600])?;
                let (total, pair): (u32, u32) = tx.query_row(
                    "SELECT COUNT(*), COUNT(CASE WHEN peer=? THEN 1 END) FROM dm_claims WHERE owner=?",
                    [&peer,owner], |r| Ok((r.get(0)?, r.get(1)?)))?;
                ensure!(
                    total < 32 && pair < 3,
                    "too many new conversations; retry later"
                );
                let row:Option<(String,String)>=tx.query_row("SELECT once_key,bundle FROM dm_prekeys WHERE owner=? AND bundle IS NOT NULL ORDER BY rowid LIMIT 1",[&peer],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                let (key, text) =
                    row.context("recipient has no available message keys; ask them to open Yap")?;
                let bundle: Signed<Prekey> = serde_json::from_str(&text)?;
                bundle.check(&peer, home)?;
                ensure!(
                    current.payload.home == home
                        && current.payload.devices.contains(&bundle.device),
                    "recipient device changed"
                );
                tx.execute(
                    "UPDATE dm_prekeys SET bundle=NULL WHERE owner=? AND once_key=?",
                    [&peer, &key],
                )?;
                tx.execute(
                    "INSERT INTO dm_claims(owner,peer,created) VALUES(?,?,?)",
                    params![owner, peer, now()],
                )?;
                tx.commit()?;
                json!({"bundle":bundle})
            }
            SocialOp::Deliver { letter } => {
                letter.check(owner, home)?;
                ensure!(
                    letter.device == device
                        && letter.record.payload.account == owner
                        && serde_json::to_vec(&letter)?.len() <= 16384,
                    "invalid message"
                );
                let peer = &letter.value.to;
                valid_peer(peer, owner)?;
                if self.unavailable(owner, peer)? {
                    return Ok(Response::Social {
                        result: json!({"accepted":false}),
                    });
                }
                let current = self.account_chain(peer)?;
                let receiver: Option<String> = self
                    .db
                    .query_row(
                        "SELECT device FROM dm_devices WHERE owner=? AND identity=?",
                        params![peer, letter.value.recipient_key.to_base64()],
                        |r| r.get(0),
                    )
                    .optional()?;
                let sender_ok:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM dm_devices WHERE owner=? AND device=? AND identity=?)",params![owner,device,letter.value.identity.to_base64()],|r|r.get(0))?;
                ensure!(sender_ok, "sender messaging key changed");
                if !current.last().is_some_and(|r| {
                    r.payload.home == home
                        && receiver.is_some_and(|d| r.payload.devices.contains(&d))
                }) {
                    return Ok(Response::Social {
                        result: json!({"accepted":false}),
                    });
                }
                let id = digest(&letter)?;
                let tx = self.db.transaction()?;
                tx.execute(
                    "DELETE FROM dm_mail WHERE created < ?",
                    [now() - 30 * 86400],
                )?;
                if !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM dm_mail WHERE id=?)",
                    [&id],
                    |r| r.get::<_, bool>(0),
                )? {
                    let count: u32 = tx.query_row(
                        "SELECT COUNT(*) FROM dm_mail WHERE recipient=? AND packet IS NOT NULL",
                        [peer],
                        |r| r.get(0),
                    )?;
                    ensure!(count < 512, "mailbox full; retry later");
                    let count: u32 = tx.query_row(
                        "SELECT COUNT(*) FROM dm_mail WHERE sender=? AND created>=?",
                        params![owner, now() - 3600],
                        |r| r.get(0),
                    )?;
                    ensure!(count < 240, "message rate limit; retry later");
                    let policy = crate::privacy::settings(&tx, peer)?;
                    let held = crate::privacy::needs_request(&tx, peer, owner, &policy)?;
                    if held {
                        let (total, from_sender): (u32,u32) = tx.query_row(
                            "SELECT COUNT(*), COUNT(CASE WHEN sender=? THEN 1 END) FROM dm_mail WHERE recipient=? AND held=1 AND packet IS NOT NULL",
                            [owner,peer], |r| Ok((r.get(0)?,r.get(1)?)))?;
                        if total >= 128 || from_sender >= 8 {
                            return Ok(Response::Social {
                                result: json!({"accepted":false}),
                            });
                        }
                    }
                    crate::privacy::approve(&tx, owner, peer)?;
                    tx.execute(
                        "INSERT INTO dm_mail(id,sender,recipient,packet,created,held) VALUES(?,?,?,?,?,?)",
                        params![id, owner, peer, serde_json::to_string(&letter)?, now(),held],
                    )?;
                }
                tx.commit()?;
                json!({"accepted":true})
            }
            SocialOp::Inbox { after } => {
                ensure!(after >= 0, "invalid inbox cursor");
                let matches: bool = self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM dm_devices WHERE owner=? AND device=?)",
                    [owner, device],
                    |r| r.get(0),
                )?;
                ensure!(matches, "messaging is registered to a different device");
                self.db.execute(
                    "DELETE FROM dm_mail WHERE created < ?",
                    [now() - 30 * 86400],
                )?;
                self.db.execute("UPDATE dm_mail SET packet=NULL WHERE recipient=? AND seq<=? AND held=0 AND packet IS NOT NULL",params![owner,after])?;
                let mut incoming = Vec::new();
                let mut query=self.db.prepare("SELECT seq,packet,sender FROM dm_mail WHERE recipient=? AND seq>? AND held=0 AND packet IS NOT NULL AND created>=? ORDER BY seq LIMIT ?")?;
                let mut rows = query.query(params![owner, after, now() - 30 * 86400, MAIL_PAGE])?;
                while let Some(r) = rows.next()? {
                    incoming.push(Incoming {
                        cursor: r.get(0)?,
                        age: crate::privacy::peer_age(&self.db, &r.get::<_, String>(2)?)?,
                        letter: serde_json::from_str(&r.get::<_, String>(1)?)?,
                    });
                }
                json!({"incoming":incoming})
            }
        };
        Ok(Response::Social { result })
    }
}
