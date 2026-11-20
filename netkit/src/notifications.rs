// SPDX-License-Identifier: AGPL-3.0-only
//! Account inbox state is part of the home database and its committed HA image.
//! Views join the current moderated events, so federation replay, erasure and
//! out-of-order arrival do not create a second notification history.
use crate::{model::now, store::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};

impl Store {
    fn sqlite_activity(&self, owner: &str) -> Result<Vec<Value>> {
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
        Ok(activity)
    }

    pub(crate) fn notification_inbox(&self, owner: &str, device: &str) -> Result<Value> {
        let activity = if self.db.native().is_some() {
            self.native_activity(owner)?
        } else {
            self.sqlite_activity(owner)?
        };
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
            "reply" if self.db.native().is_some() => {
                let mut visible = false;
                self.native_replies(owner, Some(target), Some(through), false, |_| {
                    visible = true;
                    Ok(())
                })?;
                visible
            }
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
        let (events, allowed, cursor) = if self.db.native().is_some() {
            self.native_thread_rows(owner, target, anchor, &parent)?
        } else {
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
            (events, allowed, cursor)
        };
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

struct ReplyRoute {
    id: String,
    target: String,
    cursor: i64,
    received: i64,
    through: i64,
    visible_in_inbox: bool,
}

type ThreadRows = (Vec<crate::Envelope>, HashSet<String>, HashMap<String, i64>);

impl Store {
    /// A reply's parent and anonymity belong to its erasable action. Scan
    /// authenticated routing capsules for the owner's communities, or one
    /// cursor for an acknowledgement. No plaintext routing index survives a
    /// deletion. Stream the scan so memory depends on the requested result.
    fn native_replies(
        &self,
        owner: &str,
        target: Option<&str>,
        cursor: Option<i64>,
        include_own: bool,
        mut visit: impl FnMut(ReplyRoute) -> Result<()>,
    ) -> Result<()> {
        let prefs = crate::privacy::settings(&self.db, owner)?;
        let time = now();
        let cursor_filter = if cursor.is_some() {
            "e.cursor=?3"
        } else {
            "?3 IS NULL"
        };
        let parent_filter = if target.is_some() {
            "p.id=?2"
        } else {
            "?2 IS NULL"
        };
        let mut candidates = self.db.prepare(&format!(
            "SELECT e.envelope,e.cursor,o.routing,ar.received
             FROM events e LEFT JOIN home_openings o ON o.target=e.id AND o.private=0
             LEFT JOIN inbox_arrivals ar ON ar.event=e.id
             WHERE {cursor_filter}
               AND e.community IN (SELECT p.community FROM events p
                 WHERE COALESCE(json_extract(p.envelope,'$.event.payload.account.payload.account'),p.author)=?1
                   AND {parent_filter}
                   AND json_extract(p.envelope,'$.event.payload.action.reply')=0
                   AND json_extract(p.envelope,'$.deletion') IS NULL)
               AND json_extract(e.envelope,'$.event.payload.action.reply')=1
               AND json_extract(e.envelope,'$.deletion') IS NULL
               AND NOT EXISTS(SELECT 1 FROM hidden WHERE id=e.id)
             ORDER BY e.cursor"
        ))?;
        let mut parent_query = self.db.prepare(
            "SELECT COALESCE(a.received,0),COALESCE(r.through,0),
                    EXISTS(SELECT 1 FROM inbox_mutes m WHERE m.owner=?1 AND m.target=p.id)
             FROM events p LEFT JOIN inbox_arrivals a ON a.event=p.id
             LEFT JOIN inbox_reads r ON r.owner=?1 AND r.kind='reply' AND r.target=p.id
             WHERE p.id=?2 AND p.community=?3
               AND COALESCE(json_extract(p.envelope,'$.event.payload.account.payload.account'),p.author)=?1
               AND json_extract(p.envelope,'$.event.payload.action.reply')=0
               AND json_extract(p.envelope,'$.deletion') IS NULL
               AND NOT EXISTS(SELECT 1 FROM hidden WHERE id=p.id)",
        )?;
        let mut actor = self.db.prepare(
            "SELECT NOT EXISTS(SELECT 1 FROM user_blocks WHERE
                 (owner=?1 AND peer=?2) OR (owner=?2 AND peer=?1))
             AND (?3-COALESCE((SELECT since FROM first_seen WHERE owner=?2),?3))/86400>=?4",
        )?;
        let mut rows = candidates.query(params![owner, target, cursor])?;
        while let Some(row) = rows.next()? {
            let header: crate::Envelope = serde_json::from_str(&row.get::<_, String>(0)?)?;
            let own = header.event.owner_id() == owner;
            if own && !include_own {
                continue;
            }
            let (parent, anonymous) =
                crate::native_content::reply_route(&self.db, &header, &row.get::<_, Vec<u8>>(2)?)?;
            if target.is_some_and(|id| parent != id) {
                continue;
            }
            let Some((arrived, through, muted)) = parent_query
                .query_row(
                    params![owner, parent, header.event.payload.community],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, bool>(2)?,
                        ))
                    },
                )
                .optional()?
            else {
                continue;
            };
            if !own
                && (muted
                    || (anonymous && prefs.hide_anonymous)
                    || !actor.query_row(
                        params![owner, header.event.owner_id(), time, prefs.feed_days],
                        |r| r.get::<_, bool>(0),
                    )?)
            {
                continue;
            }
            visit(ReplyRoute {
                id: header.event.id,
                target: parent,
                cursor: row.get(1)?,
                received: arrived.max(row.get::<_, Option<i64>>(3)?.unwrap_or(header.accepted_at)),
                through,
                visible_in_inbox: !own,
            })?;
        }
        Ok(())
    }

    fn native_activity(&self, owner: &str) -> Result<Vec<Value>> {
        struct Group {
            latest: ReplyRoute,
            first: Option<(String, i64)>,
            unread: usize,
        }
        let mut groups = HashMap::<String, Group>::new();
        self.native_replies(owner, None, None, false, |reply| {
            let unread = reply.cursor > reply.through;
            let first = unread.then(|| (reply.id.clone(), reply.cursor));
            match groups.entry(reply.target.clone()) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(Group {
                        latest: reply,
                        first,
                        unread: usize::from(unread),
                    });
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    let group = o.get_mut();
                    group.latest = reply;
                    group.unread += usize::from(unread);
                    if group.first.is_none() {
                        group.first = first;
                    }
                }
            }
            Ok(())
        })?;
        let mut groups: Vec<_> = groups.into_values().collect();
        groups.sort_unstable_by_key(|g| std::cmp::Reverse(g.latest.cursor));
        let mut photo_bytes = 0;
        groups.into_iter().map(|g| {
            let last = g.latest;
            let parent = self.existing(&last.target)?.context("missing inbox post")?;
            let reply = self.existing(&last.id)?.context("missing inbox reply")?;
            let title = match parent.event.payload.action {
                crate::Action::Post { body, .. } => body.chars().take(80).collect(),
                crate::Action::PostV2 { headline, .. } => headline,
                _ => anyhow::bail!("invalid inbox parent"),
            };
            let crate::Action::Reply { body, display_name, .. } = &reply.event.payload.action
                else { anyhow::bail!("invalid inbox reply") };
            let anonymous = display_name == "Anonymous neighbor";
            let profile = if anonymous { None } else {
                self.current_profile(&reply.event.payload.home, reply.event.owner_id())?
            };
            let byline = profile.as_ref().map_or(display_name, |p| &p.name);
            let photo = profile.as_ref().map(|p| &p.profile.photo).filter(|s| {
                if photo_bytes + s.len() > 128 * 1024 { return false }
                photo_bytes += s.len();
                true
            });
            let first = g.first.unwrap_or_else(|| (last.id.clone(),last.cursor));
            Ok(json!({
                "target":last.target, "scope":parent.event.payload.community, "title":title,
                "through":last.cursor, "created":last.received, "unread":g.unread,
                "first":first.0, "firstThrough":first.1, "anonymous":anonymous, "byline":byline,
                "preview":body.chars().take(160).collect::<String>(), "photo":photo, "last":last.id
            }))
        }).collect()
    }

    fn native_thread_rows(
        &self,
        owner: &str,
        target: &str,
        anchor: &str,
        parent: &crate::Envelope,
    ) -> Result<ThreadRows> {
        let mut window = VecDeque::<ReplyRoute>::new();
        let mut found = false;
        self.native_replies(owner, Some(target), None, true, |reply| {
            if !found && reply.id == anchor {
                found = true;
            }
            if !found && window.len() == 10 {
                window.pop_front();
            }
            if window.len() < 80 {
                window.push_back(reply);
            }
            Ok(())
        })?;
        ensure!(found, "This conversation is no longer available.");
        let mut events = vec![parent.clone()];
        let mut votes = self.db.prepare(
            "SELECT envelope FROM events WHERE community=? AND json_extract(envelope,'$.event.payload.action.target')=?
               AND json_extract(envelope,'$.event.payload.action.kind')='vote' ORDER BY cursor"
        )?;
        for raw in votes.query_map(params![parent.event.payload.community, target], |r| {
            r.get::<_, String>(0)
        })? {
            events.push(serde_json::from_str(&raw?)?);
        }
        let mut visible = HashSet::new();
        let mut cursors = HashMap::new();
        for reply in window {
            events.push(self.existing(&reply.id)?.context("missing thread reply")?);
            if reply.visible_in_inbox {
                visible.insert(reply.id.clone());
            }
            cursors.insert(reply.id, reply.cursor);
        }
        Ok((events, visible, cursors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Envelope, SignedEvent, new_secret, signing_key};

    #[test]
    fn late_parent_and_private_routing_keep_inbox_and_thread_in_sync() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let home = signing_key(&new_secret())?;
        let home_id = hex::encode(home.verifying_key().as_bytes());
        let author = signing_key(&new_secret())?;
        let reader = hex::encode(author.verifying_key().as_bytes());
        let neighbor = signing_key(&new_secret())?;
        let make = |key: &ed25519_dalek::SigningKey, seq, action| {
            Envelope::accept(
                SignedEvent::create(key, home_id.clone(), "scope".into(), seq, action)?,
                &home,
            )
        };
        let post = make(
            &author,
            1,
            Action::Post {
                body: "Thread title".into(),
                topic: "Plans".into(),
                anonymous: false,
                display_name: "Owner".into(),
            },
        )?;
        let reply = make(
            &neighbor,
            1,
            Action::Reply {
                parent: post.event.id.clone(),
                body: "Visible reply".into(),
                display_name: "Neighbor".into(),
            },
        )?;
        let own = make(
            &author,
            2,
            Action::Reply {
                parent: post.event.id.clone(),
                body: "My response".into(),
                display_name: "Owner".into(),
            },
        )?;
        store.accept(&reply)?;
        assert_eq!(
            store.notification_inbox(&reader, "device")?["activity"],
            json!([])
        );
        store.accept(&post)?;
        store.accept(&own)?;
        let inbox = store.notification_inbox(&reader, "device")?;
        assert_eq!(inbox["activity"][0]["title"], "Thread title");
        assert_eq!(inbox["activity"][0]["last"], reply.event.id);
        let thread = store.activity_thread(&reader, &post.event.id, &reply.event.id)?;
        assert_eq!(thread["post"]["replies"].as_array().unwrap().len(), 2);
        assert_eq!(thread["through"], inbox["activity"][0]["through"]);
        store.moderate(&reply.event.id, true)?;
        assert_eq!(
            store.notification_inbox(&reader, "device")?["activity"],
            json!([])
        );
        assert!(
            store
                .activity_thread(&reader, &post.event.id, &reply.event.id)
                .is_err()
        );
        assert_eq!(
            store.activity_thread(&reader, &post.event.id, &own.event.id)?["post"]["replies"][0]["id"],
            own.event.id
        );
        Ok(())
    }
}
