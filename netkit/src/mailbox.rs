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

impl Store {
    pub(crate) fn sweep_expired_mail(&self) -> Result<bool> {
        const BATCH: usize = 512;
        let deleted = self.db.execute(
            "DELETE FROM dm_mail WHERE seq IN (
                SELECT seq FROM dm_mail WHERE created<? ORDER BY created LIMIT ?
             )",
            params![mail_cutoff(now()), BATCH],
        )?;
        Ok(deleted == BATCH)
    }

    fn block_peers(&self, owner: &str) -> Result<HashSet<String>> {
        Ok(self
            .db
            .prepare("SELECT peer FROM user_blocks WHERE owner=?1")?
            .query_map([owner], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn visible_contact(&self, community: &str, id: &str) -> Result<SignedEvent> {
        let raw: String = self
            .db
            .query_row(
                "SELECT envelope FROM events e WHERE e.id=?1 AND e.community=?2
               AND NOT EXISTS(SELECT 1 FROM hidden WHERE id=e.id)",
                [id, community],
                |row| row.get(0),
            )
            .optional()?
            .context("post or reply unavailable")?;
        let event = crate::native_content::hydrate(&self.db, &raw, false)?.event;
        if let Action::Reply { parent, .. } = &event.payload.action {
            let parent_visible: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM events e WHERE e.id=?1 AND e.community=?2
                   AND json_extract(e.envelope,'$.deletion') IS NULL
                   AND (json_extract(e.envelope,'$.event.payload.action.kind') IN ('post','postV2')
                     OR (json_extract(e.envelope,'$.event.payload.action.kind')='erased'
                         AND json_extract(e.envelope,'$.event.payload.action.reply')=0))
                   AND NOT EXISTS(SELECT 1 FROM hidden WHERE id=e.id))",
                [parent, community],
                |row| row.get(0),
            )?;
            ensure!(parent_visible, "post or reply unavailable");
        }
        Ok(event)
    }
    fn contact_identity(
        &self,
        community: &str,
        event: &SignedEvent,
        allow_anonymous: bool,
    ) -> Result<(String, String)> {
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
        let name = if crate::usernames::named(event) {
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
        let continuation = match &op {
            SocialOp::FeedReplies { post, before, .. } => Some((post.clone(), before.clone())),
            _ => None,
        };
        let enhanced = matches!(&op, SocialOp::PrivacyFeed { .. }) || continuation.is_some();
        let result = match op {
            SocialOp::PrivacyFeed { nearby }
            | SocialOp::Feed { nearby }
            | SocialOp::FeedReplies { nearby, .. } => {
                if let Some(n) = &nearby {
                    n.validate()?;
                }
                let blocked = self.block_peers(owner)?;
                let policy = crate::privacy::settings(&self.db, owner)?;
                let crate::feed_page::FeedPage {
                    mut posts,
                    events,
                    event_count,
                } = self.personalized_feed_page(
                    community,
                    owner,
                    nearby.as_ref(),
                    &blocked,
                    &policy,
                )?;
                let ages = if enhanced {
                    self.page_ages(&events)?
                } else {
                    Default::default()
                };
                let visibility = |event: &Envelope| crate::privacy::visibility(event, &ages);
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
                if let Some((target, before)) = continuation {
                    let post = posts
                        .into_iter()
                        .find(|post| post.id == target)
                        .context("conversation unavailable; refresh the feed")?;
                    return Ok(Response::Social {
                        result: crate::server::preceding_replies(post.replies, before.as_deref())?,
                    });
                }
                let chain = self.account_chain(owner)?;
                let mut deleted = self.erased_ids(community)?;
                let more = crate::server::fit_feed_page(
                    &mut posts,
                    &mut deleted,
                    serde_json::to_vec(&chain)?.len(),
                )?;
                return Ok(Response::Feed {
                    deleted,
                    posts,
                    event_count,
                    account_chain: chain,
                    direct: true,
                    privacy: enhanced,
                }
                .with_deletion_pages(more));
            }
            SocialOp::SetProfile {
                expected,
                name,
                profile,
            } => serde_json::to_value(self.set_profile(home, owner, expected, name, profile)?)?,
            SocialOp::SetProfilePortrait {
                expected,
                name,
                profile,
                portrait,
            } => serde_json::to_value(self.set_profile_with_portrait(
                home,
                owner,
                expected,
                name,
                profile,
                Some(portrait),
            )?)?,
            SocialOp::ProfilePortrait { event } => {
                let portrait = if let Some(id) = event {
                    match self.existing(&id)? {
                        Some(row)
                            if row.event.payload.community == community
                                && crate::usernames::named(&row.event)
                                && !self.hidden()?.contains(&id)
                                && !self.unavailable(owner, row.event.owner_id())? =>
                        {
                            self.profile_portrait(&row.event.payload.home, row.event.owner_id())?
                        }
                        _ => None,
                    }
                } else {
                    self.profile_portrait(home, owner)?
                };
                json!({"portrait":portrait})
            }
            SocialOp::AttachmentPut {
                id,
                size,
                part,
                ciphertext,
            } => {
                self.put_attachment(owner, &id, size, part, &ciphertext)?;
                json!({})
            }
            SocialOp::AttachmentGet { id, part } => {
                json!({"ciphertext":self.get_attachment(&id, part)?})
            }
            SocialOp::Profiles { events } => {
                ensure!(events.len() <= 100, "too many profile references");
                let legacy_batch = events.len() > crate::model::PROFILE_LOOKUP_BATCH;
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
                        && let Some(mut record) = self.current_profile(&event.payload.home, peer)?
                    {
                        // Older clients request 100 records and cannot render the larger photos.
                        if legacy_batch
                            && record.profile.photo.len()
                                > crate::model::LEGACY_PROFILE_PHOTO_LENGTH
                        {
                            record.profile.photo.clear();
                        }
                        profiles.insert(peer.to_owned(), serde_json::to_value(record)?);
                    }
                }
                serde_json::Value::Object(profiles)
            }
            SocialOp::SetPush {
                provider,
                subscription,
                categories,
            } => self.set_push(owner, device, &provider, subscription, categories)?,
            SocialOp::OpenPush { route } => self.push_route(owner, device, &route)?,
            SocialOp::NotificationThread { target, anchor } => {
                self.activity_thread(owner, &target, &anchor)?
            }
            SocialOp::NotificationInbox => self.notification_inbox(owner, device)?,
            SocialOp::ReadNotification {
                category,
                target,
                through,
            } => {
                self.read_notification(owner, device, &category, &target, through)?;
                json!({})
            }
            SocialOp::MuteNotifications { target, muted } => {
                ensure!(
                    target.len() == 64 && hex::decode(&target).is_ok(),
                    "invalid thread"
                );
                if muted {
                    self.db.execute(
                        "INSERT OR IGNORE INTO inbox_mutes VALUES(?,?)",
                        [owner, &target],
                    )?;
                } else {
                    self.db.execute(
                        "DELETE FROM inbox_mutes WHERE owner=? AND target=?",
                        [owner, &target],
                    )?;
                }
                json!({})
            }
            SocialOp::DismissRequest { peer } => {
                valid_peer(&peer, owner)?;
                ensure!(
                    self.db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM dm_devices WHERE owner=? AND device=?)",
                        [owner, device],
                        |r| r.get::<_, bool>(0)
                    )?,
                    "open this account's messaging installation"
                );
                self.db.execute(
                    "UPDATE dm_mail SET packet=NULL WHERE recipient=? AND sender=? AND held=1",
                    [owner, &peer],
                )?;
                json!({})
            }
            SocialOp::PrivacyStatus => {
                let mut result = self.privacy_status(owner)?;
                result["notifications"] = self.notification_inbox(owner, device)?;
                result["requests"] = json!(
                    result["notifications"]["requests"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| &r["target"])
                        .collect::<Vec<_>>()
                );
                result["pushCapabilities"] = crate::push::capabilities();
                result["pushCategories"] = self.push_categories(owner, device)?;
                result["profilePhotoLimit"] = json!(crate::model::MAX_PROFILE_PHOTO_LENGTH);
                result["profilePortraitLimit"] = json!(crate::model::MAX_PROFILE_PORTRAIT_LENGTH);
                result["currentProfile"] =
                    serde_json::to_value(self.current_profile(home, owner)?)?;
                if !result["currentProfile"].is_null() && self.has_profile_portrait(home, owner)? {
                    result["currentProfile"]["hasPortrait"] = json!(true);
                }
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
                ensure!(
                    !tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM dm_devices WHERE owner=? AND device<>?)",
                        [owner, device],
                        |r| r.get::<_, bool>(0)
                    )?,
                    "open this account’s messaging installation"
                );
                crate::privacy::approve(&tx, owner, &peer)?;
                tx.commit()?;
                json!({})
            }
            SocialOp::Contact { event } => {
                let event = self.visible_contact(community, &event)?;
                let (peer, label) = self.contact_identity(community, &event, false)?;
                valid_peer(&peer, owner)?;
                ensure!(!self.unavailable(owner, &peer)?, "messaging unavailable");
                json!({"peer":peer,"label":label,"event":event})
            }
            SocialOp::Block { target } => {
                let (peer, label) = match target {
                    Target::Event(id) => {
                        let event = self
                            .existing(&id)?
                            .context("post or reply unavailable")?
                            .event;
                        let (peer, label) = self.contact_identity(community, &event, true)?;
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
                tx.execute("INSERT INTO inbox_reads(owner,kind,target,through)
                    SELECT recipient,'message',sender,MAX(seq) FROM dm_mail WHERE
                    (recipient=?1 AND sender=?2) OR (recipient=?2 AND sender=?1) GROUP BY recipient,sender
                    ON CONFLICT(owner,kind,target) DO UPDATE SET through=MAX(through,excluded.through)", [owner,&peer])?;
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
                if !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM dm_mail WHERE id=?)",
                    [&id],
                    |r| r.get::<_, bool>(0),
                )? {
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
                            "SELECT COUNT(*), COUNT(CASE WHEN sender=? THEN 1 END) FROM dm_mail WHERE recipient=? AND held=1 AND packet IS NOT NULL AND created>=?",
                            params![owner,peer,mail_cutoff(now())], |r| Ok((r.get(0)?,r.get(1)?)))?;
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
                self.db.execute("UPDATE dm_mail SET packet=NULL WHERE recipient=? AND seq<=? AND held=0 AND packet IS NOT NULL",params![owner,after])?;
                let mut incoming = Vec::new();
                let mut query=self.db.prepare("SELECT seq,packet,sender,created FROM dm_mail WHERE recipient=? AND seq>? AND held=0 AND packet IS NOT NULL AND created>=? ORDER BY seq LIMIT ?")?;
                let mut rows = query.query(params![owner, after, mail_cutoff(now()), MAIL_PAGE])?;
                while let Some(r) = rows.next()? {
                    incoming.push(Incoming {
                        cursor: r.get(0)?,
                        received: r.get(3)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_visibility_follows_post_and_reply_moderation() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let key = signing_key(&new_secret())?;
        let home = hex::encode(key.verifying_key().to_bytes());
        let community = "neighborhood";
        let sign = |sequence, action| {
            Envelope::accept(
                SignedEvent::create(&key, home.clone(), community.into(), sequence, action)?,
                &key,
            )
        };
        let post = sign(
            1,
            Action::Post {
                body: "Neighborhood meeting".into(),
                display_name: "Neighbor".into(),
                topic: "Plans".into(),
                anonymous: false,
            },
        )?;
        let reply = sign(
            2,
            Action::Reply {
                parent: post.event.id.clone(),
                body: "See you there".into(),
                display_name: "Neighbor".into(),
            },
        )?;
        store.accept(&post)?;
        store.accept(&reply)?;

        let mut staging = Store::from_sqlite(rusqlite::Connection::open_in_memory()?);
        staging.restore_snapshot(&store.snapshot()?)?;
        for store in [store, staging] {
            assert_eq!(
                store.visible_contact(community, &reply.event.id)?.id,
                reply.event.id
            );
            store.moderate(&reply.event.id, true)?;
            assert!(store.visible_contact(community, &reply.event.id).is_err());
            store.moderate(&reply.event.id, false)?;
            store.moderate(&post.event.id, true)?;
            assert!(store.visible_contact(community, &post.event.id).is_err());
            assert!(store.visible_contact(community, &reply.event.id).is_err());
            store.moderate(&post.event.id, false)?;
            assert!(store.visible_contact(community, &reply.event.id).is_ok());
        }
        Ok(())
    }
}
