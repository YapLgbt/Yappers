// SPDX-License-Identifier: AGPL-3.0-only
//! Account inbox state is part of the home database and its committed HA image.
//! Views join the current moderated events, so federation replay, erasure and
//! out-of-order arrival do not create a second notification history.
use crate::{model::now, store::Store};
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

pub(crate) fn install(db: &Connection) -> Result<()> {
    db.execute_batch(crate::push::SCHEMA)?;
    db.execute_batch("
        CREATE TABLE IF NOT EXISTS inbox_reads(
            owner TEXT, kind TEXT, target TEXT, through INTEGER NOT NULL,
            PRIMARY KEY(owner,kind,target));
        CREATE TABLE IF NOT EXISTS inbox_mutes(
            owner TEXT, target TEXT, PRIMARY KEY(owner,target));
        CREATE TABLE IF NOT EXISTS inbox_arrivals(event TEXT PRIMARY KEY, received INTEGER NOT NULL);
        CREATE TRIGGER IF NOT EXISTS inbox_event_arrived AFTER INSERT ON events
        WHEN json_extract(NEW.envelope,'$.event.payload.action.kind') IN ('post','postV2','reply')
        BEGIN INSERT OR IGNORE INTO inbox_arrivals VALUES(NEW.id,unixepoch()); END;
        CREATE INDEX IF NOT EXISTS events_reply_parent
            ON events(json_extract(envelope,'$.event.payload.action.parent'));
        CREATE INDEX IF NOT EXISTS events_account_owner
            ON events(COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author));
        CREATE VIEW IF NOT EXISTS inbox_replies AS
        SELECT p.id AS target, p.community AS scope, r.id AS reply, r.cursor AS seq,
            COALESCE(json_extract(p.envelope,'$.event.payload.account.payload.account'),p.author) AS owner,
            COALESCE(json_extract(r.envelope,'$.event.payload.account.payload.account'),r.author) AS actor,
            json_extract(r.envelope,'$.event.payload.action.displayName')='Anonymous neighbor' AS anonymous,
            COALESCE(json_extract(p.envelope,'$.event.payload.action.headline'),
                     substr(json_extract(p.envelope,'$.event.payload.action.body'),1,80)) AS title,
            json_extract(r.envelope,'$.acceptedAt') AS created
        FROM events p JOIN events r ON json_extract(r.envelope,'$.event.payload.action.parent')=p.id
            AND r.community=p.community
        WHERE json_extract(p.envelope,'$.event.payload.action.kind') IN ('post','postV2')
            AND json_extract(r.envelope,'$.event.payload.action.kind')='reply'
            AND owner<>actor AND NOT EXISTS(SELECT 1 FROM hidden WHERE id IN (r.id,p.id));
        CREATE VIEW IF NOT EXISTS inbox_visible AS
        SELECT n.* FROM inbox_replies n
        LEFT JOIN privacy_settings prefs ON prefs.owner=n.owner
        LEFT JOIN first_seen age ON age.owner=n.actor
        WHERE NOT EXISTS(SELECT 1 FROM user_blocks b WHERE
            (b.owner=n.owner AND b.peer=n.actor) OR (b.peer=n.owner AND b.owner=n.actor))
        AND NOT EXISTS(SELECT 1 FROM inbox_mutes m WHERE m.owner=n.owner AND m.target=n.target)
        AND (NOT n.anonymous OR NOT COALESCE(json_extract(prefs.settings,'$.hideAnonymous'),0))
        AND (unixepoch()-COALESCE(age.since,unixepoch()))/86400 >= COALESCE(json_extract(prefs.settings,'$.feedDays'),0);
    ")?;
    Ok(())
}

impl Store {
    pub(crate) fn notification_inbox(&self, owner: &str, device: &str) -> Result<Value> {
        let mut stmt = self.db.prepare("WITH rows AS (
            SELECT n.*,seq>COALESCE(r.through,0) AS unread FROM inbox_visible n
            LEFT JOIN inbox_reads r ON r.owner=n.owner AND r.kind='reply' AND r.target=n.target
            WHERE n.owner=?1), grouped AS (
            SELECT target,scope,title,MAX(seq) AS latest,MAX(created) AS created,SUM(unread) AS unread,
                MIN(CASE WHEN unread THEN seq END) AS first FROM rows GROUP BY target)
            SELECT g.target,g.scope,g.title,g.latest,
                MAX(COALESCE(ra.received,n.created),COALESCE(pa.received,0)),g.unread,
                (SELECT reply FROM rows WHERE seq=COALESCE(g.first,g.latest)), COALESCE(g.first,g.latest),
                n.anonymous,COALESCE(p.name,json_extract(e.envelope,'$.event.payload.action.displayName')),
                substr(json_extract(e.envelope,'$.event.payload.action.body'),1,160),json_extract(p.profile,'$.photo'),n.reply
            FROM grouped g JOIN rows n ON n.seq=g.latest JOIN events e ON e.id=n.reply
            LEFT JOIN inbox_arrivals ra ON ra.event=n.reply LEFT JOIN inbox_arrivals pa ON pa.event=n.target
            LEFT JOIN public_profiles p ON NOT n.anonymous AND p.owner=n.actor AND p.home=json_extract(e.envelope,'$.event.payload.home')
            ORDER BY g.latest DESC")?;
        let mut photo_bytes = 0;
        let activity = stmt.query_map([owner], |r| {
            let photo=r.get::<_,Option<String>>(11)?.filter(|photo| {
                if photo_bytes + photo.len() > 128*1024 {return false}
                photo_bytes+=photo.len(); true
            });
            Ok(json!({
            "target":r.get::<_,String>(0)?, "scope":r.get::<_,String>(1)?, "title":r.get::<_,String>(2)?,
            "through":r.get::<_,i64>(3)?, "created":r.get::<_,i64>(4)?, "unread":r.get::<_,i64>(5)?,
            "first":r.get::<_,String>(6)?, "firstThrough":r.get::<_,i64>(7)?,
            "anonymous":r.get::<_,bool>(8)?, "byline":r.get::<_,String>(9)?,
            "preview":r.get::<_,String>(10)?, "photo":photo, "last":r.get::<_,String>(12)?
        }))})?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mailbox = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM dm_devices WHERE owner=? AND device=?)",
            [owner, device],
            |r| r.get::<_, bool>(0),
        )?;
        let mut messages = Vec::new();
        let mut requests = Vec::new();
        if mailbox {
            let mut stmt = self.db.prepare("SELECT m.sender, m.held, MAX(m.seq), COUNT(*),MAX(m.created),
                (SELECT newest.id FROM dm_mail newest WHERE newest.recipient=m.recipient AND newest.sender=m.sender AND newest.held=m.held ORDER BY newest.seq DESC LIMIT 1)
                FROM dm_mail m LEFT JOIN inbox_reads r ON r.owner=m.recipient AND r.kind='message' AND r.target=m.sender
                WHERE m.recipient=? AND m.created>=? AND ((m.held=0 AND m.seq>COALESCE(r.through,0))
                    OR (m.held=1 AND m.packet IS NOT NULL))
                    AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE
                    (b.owner=m.recipient AND b.peer=m.sender) OR (b.owner=m.sender AND b.peer=m.recipient))
                GROUP BY m.sender,m.held ORDER BY MAX(m.seq) DESC")?;
            let mut rows = stmt.query(params![owner, now() - 30 * 86400])?;
            while let Some(r) = rows.next()? {
                let row = json!({"target":r.get::<_,String>(0)?,"through":r.get::<_,i64>(2)?,
                    "last":r.get::<_,String>(5)?,"unread":r.get::<_,i64>(3)?,"created":r.get::<_,i64>(4)?});
                if r.get::<_, bool>(1)? {
                    requests.push(row);
                } else {
                    messages.push(row);
                }
            }
        }
        Ok(json!({"activity":activity,"messages":messages,"requests":requests}))
    }

    pub(crate) fn read_notification(
        &self,
        owner: &str,
        device: &str,
        kind: &str,
        target: &str,
        through: i64,
    ) -> Result<()> {
        ensure!(
            target.len() == 64 && hex::decode(target).is_ok() && through > 0,
            "invalid inbox read position"
        );
        let valid: bool = match kind {
            "reply" => self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM inbox_visible WHERE owner=? AND target=? AND seq=?)",
                params![owner, target, through],
                |r| r.get(0),
            )?,
            "message" => self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM dm_mail m JOIN dm_devices d ON d.owner=m.recipient
                WHERE d.device=? AND m.recipient=? AND m.sender=? AND m.seq=? AND m.held=0)",
                params![device, owner, target, through],
                |r| r.get(0),
            )?,
            _ => anyhow::bail!("unknown inbox destination"),
        };
        // A vanished event makes an offline acknowledgement obsolete. Never move
        // the cursor to a newer event the reader hasn't actually opened.
        if valid {
            self.db.execute(
                "INSERT INTO inbox_reads VALUES(?,?,?,?) ON CONFLICT(owner,kind,target)
                DO UPDATE SET through=excluded.through WHERE excluded.through>inbox_reads.through",
                params![owner, kind, target, through],
            )?;
        }
        Ok(())
    }
}

impl Store {
    /// Fetch an owned thread even when it falls outside Nearby, the 200-post feed
    /// window, or the current group. Anchor keeps an older unread reply in view.
    pub(crate) fn activity_thread(&self, owner: &str, target: &str, anchor: &str) -> Result<Value> {
        use crate::model::{Action, project};
        let missing = || anyhow::anyhow!("This conversation is no longer available.");
        let parent = self.existing(target)?.ok_or_else(missing)?;
        let scope = &parent.event.payload.community;
        let hidden = self.hidden()?;
        ensure!(
            parent.event.owner_id() == owner
                && !hidden.contains(target)
                && matches!(
                    parent.event.payload.action,
                    Action::Post { .. } | Action::PostV2 { .. }
                ),
            "This conversation is no longer available."
        );
        let mut query = self.db.prepare(
            "SELECT e.cursor,e.envelope FROM events e
            WHERE e.community=?1 AND (json_extract(e.envelope,'$.event.payload.action.parent')=?2
            OR json_extract(e.envelope,'$.event.payload.action.target')=?2) ORDER BY e.cursor",
        )?;
        let mut visible = self
            .db
            .prepare("SELECT reply FROM inbox_visible WHERE owner=? AND target=?")?;
        let allowed = visible
            .query_map([owner, target], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        let mut events = vec![parent.clone()];
        let mut replies = Vec::new();
        let mut cursor = std::collections::HashMap::new();
        for row in query.query_map([scope, target], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })? {
            let (seq, raw) = row?;
            let event: crate::model::Envelope = serde_json::from_str(&raw)?;
            match &event.event.payload.action {
                Action::Reply { .. }
                    if allowed.contains(&event.event.id)
                        || (event.event.owner_id() == owner
                            && !hidden.contains(&event.event.id)) =>
                {
                    cursor.insert(event.event.id.clone(), seq);
                    replies.push(event);
                }
                Action::Vote { .. } => events.push(event),
                _ => {}
            }
        }
        let index = replies
            .iter()
            .position(|r| r.event.id == anchor)
            .ok_or_else(missing)?;
        let start = index.saturating_sub(10);
        events.extend(replies.into_iter().skip(start).take(80));
        let mut posts = project(&events, scope, owner, &hidden);
        self.apply_current_bylines(&mut posts, &events)?;
        let post = posts.first_mut().ok_or_else(missing)?;
        let ages = self.ages()?;
        post.visibility = Some(crate::privacy::visibility(&parent, &ages));
        for reply in &mut post.replies {
            if let Some(event) = events.iter().find(|e| e.event.id == reply.id) {
                reply.visibility = Some(crate::privacy::visibility(event, &ages));
                reply.mine = event.event.owner_id() == owner;
            }
        }
        // An unusually large profile photo can limit how many replies fit in an RPC.
        // Remove neighbors farthest along the read window first, keeping its anchor.
        let anchor_seq = cursor[anchor];
        while serde_json::to_vec(&post)?.len() > crate::MAX_FRAME - 4096 {
            let removable = post
                .replies
                .iter()
                .enumerate()
                .filter(|(_, r)| r.id != anchor)
                .max_by_key(|(_, r)| cursor[&r.id])
                .map(|(index, _)| index)
                .ok_or_else(missing)?;
            post.replies.remove(removable);
        }
        let through = post
            .replies
            .iter()
            .filter(|r| allowed.contains(&r.id))
            .map(|r| cursor[&r.id])
            .max()
            .unwrap_or(anchor_seq);
        Ok(json!({"post":post,"scope":scope,"through":through}))
    }
}
