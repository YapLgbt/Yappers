// SPDX-License-Identifier: AGPL-3.0-only
//! Public reply routing for a pinned native feed read. Only the independent
//! committed reader populates the shared index. Bodies, keys and plaintext
//! parent IDs never enter it. Dropping the Home discards the entire index.
use crate::{home_sql::HomeDb, native_content, native_guard, store::NativeHome};
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::params;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Bound::{Excluded, Unbounded},
    sync::{Arc, LazyLock, Mutex, PoisonError, RwLock},
};

const LIMIT: usize = 200;
const BATCH: usize = 256;
type Tag = [u8; 32];
type Rank = (i64, Arc<str>);

pub(crate) struct FeedReplies {
    secret: [u8; 32],
    communities: Mutex<HashMap<String, Arc<Community>>>,
}

impl Default for FeedReplies {
    fn default() -> Self {
        Self {
            secret: rand::random(),
            communities: Mutex::default(),
        }
    }
}

#[derive(Default)]
struct Community {
    refresh: Mutex<()>,
    routes: RwLock<Routes>,
}

#[derive(Clone, Copy, Default)]
struct Watermarks {
    reply: i64,
    deletion: i64,
}

impl Watermarks {
    fn covers(self, other: Self) -> bool {
        self.reply >= other.reply && self.deletion >= other.deletion
    }
}

#[derive(Default)]
struct Routes {
    through: Watermarks,
    parents: HashMap<Tag, BTreeMap<Rank, i64>>,
    // Find retired replies without having to recover their erased parent.
    by_id: HashMap<Arc<str>, (Tag, i64)>,
}

impl Routes {
    fn add(&mut self, tag: Tag, rank: Rank, cursor: i64) {
        self.by_id.insert(rank.1.clone(), (tag, rank.0));
        self.parents.entry(tag).or_default().insert(rank, cursor);
    }

    fn retire(&mut self, id: &str) {
        if let Some((id, (tag, time))) = self.by_id.remove_entry(id)
            && let Some(replies) = self.parents.get_mut(&tag)
        {
            replies.remove(&(time, id));
            if replies.is_empty() {
                self.parents.remove(&tag);
            }
        }
    }
}

fn poisoned<T>(_: PoisonError<T>) -> anyhow::Error {
    anyhow!("public reply index lock poisoned")
}

// Keep the literal predicates in sync with the native feed_replies partial index.
const LIVE_REPLY: &str = "json_extract(e.envelope,'$.event.payload.action.kind')='erased'
    AND json_extract(e.envelope,'$.event.payload.action.reply')=1
    AND json_extract(e.envelope,'$.deletion') IS NULL";

fn watermarks(db: &HomeDb, community: &str) -> Result<Watermarks> {
    static SQL: LazyLock<String> = LazyLock::new(|| {
        format!(
            "SELECT COALESCE((SELECT e.cursor FROM events e
        WHERE e.community=? AND {LIVE_REPLY} ORDER BY e.cursor DESC LIMIT 1),0),
        COALESCE((SELECT MAX(rowid) FROM deletions),0)"
        )
    });
    Ok(db.prepare_cached(&SQL)?.query_row([community], |r| {
        Ok(Watermarks {
            reply: r.get(0)?,
            deletion: r.get(1)?,
        })
    })?)
}

impl Community {
    fn refresh(&self, home: &Arc<NativeHome>, community: &str, wanted: Watermarks) -> Result<()> {
        if self.routes.read().map_err(poisoned)?.through.covers(wanted) {
            return Ok(());
        }
        let _refresh = self.refresh.lock().map_err(poisoned)?;
        let (from, has_routes) = {
            let routes = self.routes.read().map_err(poisoned)?;
            (routes.through, !routes.by_id.is_empty())
        };
        if from.covers(wanted) {
            return Ok(());
        }

        let reader = home.connection()?;
        let tx = reader.db.unchecked_transaction()?;
        // A speculative cursor may be reused after rollback. Never advance the
        // shared watermark past the committed allocator, even for an empty scan.
        let committed: (i64, i64) = tx.query_row(
            "SELECT high_water,COALESCE((SELECT MAX(rowid) FROM deletions),0)
             FROM native_cursor_fences WHERE stream='events'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            wanted.reply <= committed.0 && wanted.deletion <= committed.1,
            "reply index requires a committed feed snapshot"
        );

        if wanted.reply > from.reply {
            let sql = format!(
                "SELECT e.cursor,e.id,e.envelope,o.routing FROM events e
                LEFT JOIN home_openings o ON o.target=e.id AND o.private=0
                WHERE e.community=? AND e.cursor>? AND e.cursor<=? AND {LIVE_REPLY}
                ORDER BY e.cursor"
            );
            let mut stmt = tx.prepare(&sql)?;
            let mut rows = stmt.query(params![community, from.reply, wanted.reply])?;
            let mut additions = Vec::with_capacity(BATCH);
            while let Some(row) = rows.next()? {
                let header: crate::Envelope = serde_json::from_str(&row.get::<_, String>(2)?)?;
                ensure!(
                    header.event.id == row.get::<_, String>(1)?
                        && header.event.payload.community == community,
                    "reply header binding mismatch"
                );
                // The routing AEAD binds the complete accepted, signed header.
                let (parent, _) =
                    native_content::reply_route(&tx, &header, &row.get::<_, Vec<u8>>(3)?)?;
                let tag = home.feed_replies.tag(&parent);
                additions.push((
                    tag,
                    (header.accepted_at, Arc::from(header.event.id)),
                    row.get(0)?,
                ));
                if additions.len() == BATCH {
                    self.publish(&mut additions)?;
                }
            }
            self.publish(&mut additions)?;
        }

        if has_routes && wanted.deletion > from.deletion {
            // The journal only drives memory reclamation. Visibility always
            // uses live event rows in the caller's snapshot. A deletion from
            // a different owner can name the same target without retiring it.
            let mut stmt = tx.prepare(
                "SELECT e.id FROM deletions d
                CROSS JOIN events e ON e.id=d.target AND e.community=d.community
                WHERE d.rowid>? AND d.rowid<=? AND d.private=0 AND d.community=?
                AND json_extract(e.envelope,'$.deletion') IS NOT NULL",
            )?;
            let mut rows = stmt.query(params![from.deletion, wanted.deletion, community])?;
            while let Some(row) = rows.next()? {
                self.routes
                    .write()
                    .map_err(poisoned)?
                    .retire(&row.get::<_, String>(0)?);
            }
        }

        // A failed scan may leave a committed prefix. Re-inserting it on the
        // next attempt is idempotent. No data from the caller is ever published.
        let mut routes = self.routes.write().map_err(poisoned)?;
        routes.through.reply = from.reply.max(wanted.reply);
        routes.through.deletion = from.deletion.max(wanted.deletion);
        Ok(())
    }

    fn publish(&self, additions: &mut Vec<(Tag, Rank, i64)>) -> Result<()> {
        let mut routes = self.routes.write().map_err(poisoned)?;
        for (tag, rank, cursor) in additions.drain(..) {
            routes.add(tag, rank, cursor);
        }
        Ok(())
    }
}

impl FeedReplies {
    fn tag(&self, parent: &str) -> Tag {
        *blake3::keyed_hash(&self.secret, parent.as_bytes()).as_bytes()
    }
}

// Probe only proposed IDs using their unique index, regardless of the size
// of the community or moderation tables. A hide can be reverted without a new cursor.
// Turso 0.8.4 otherwise prefers a community-prefix scan for every JSON value.
const VISIBLE: &str = "SELECT e.id FROM json_each(?) q
        CROSS JOIN events e ON e.id=CAST(q.value AS TEXT) WHERE CAST(e.community AS TEXT)=?
        AND json_extract(e.envelope,'$.event.payload.action.kind')='erased'
        AND json_extract(e.envelope,'$.event.payload.action.reply')=1
        AND json_extract(e.envelope,'$.deletion') IS NULL
        AND NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=e.id)";

fn visible(db: &HomeDb, community: &str, ids: &[&str]) -> Result<HashSet<String>> {
    let mut visible = HashSet::new();
    let mut stmt = db.prepare_cached(VISIBLE)?;
    for chunk in ids.chunks(BATCH) {
        let mut rows = stmt.query(params![serde_json::to_string(chunk)?, community])?;
        while let Some(row) = rows.next()? {
            visible.insert(row.get(0)?);
        }
    }
    Ok(visible)
}

struct Window {
    parent: String,
    tag: Tag,
    before: Option<Rank>,
    ids: Vec<String>,
    exhausted: bool,
}

/// Return the newest 200 visible live replies to each selected public post,
/// displayed oldest first. Missing keys have no visible replies. The caller
/// supplies distinct roots already selected as visible in this same snapshot
/// and completes its guarded read transaction before disclosing the result.
pub(crate) fn latest(
    db: &HomeDb,
    community: &str,
    parents: &[String],
) -> Result<HashMap<String, Vec<String>>> {
    ensure!(
        !db.is_autocommit()?,
        "reply lookup requires a pinned native transaction"
    );
    let home = db
        .native_home()
        .context("reply lookup requires a native Home")?;
    if parents.is_empty() {
        return Ok(HashMap::new());
    }
    let wanted = watermarks(db, community)?;
    let index = {
        let mut communities = home.feed_replies.communities.lock().map_err(poisoned)?;
        if wanted.reply == 0 {
            communities.get(community).cloned()
        } else {
            Some(communities.entry(community.to_owned()).or_default().clone())
        }
    };
    let Some(index) = index else {
        return Ok(HashMap::new());
    };
    index.refresh(home, community, wanted)?;
    if wanted.reply == 0 {
        return Ok(HashMap::new());
    }
    let mut windows: Vec<_> = parents
        .iter()
        .map(|parent| Window {
            tag: home.feed_replies.tag(parent),
            parent: parent.clone(),
            before: None,
            ids: Vec::new(),
            exhausted: false,
        })
        .collect();

    loop {
        let mut candidates = Vec::new();
        {
            let routes = index.routes.read().map_err(poisoned)?;
            for (i, w) in windows.iter_mut().enumerate() {
                if w.exhausted || w.ids.len() == LIMIT {
                    continue;
                }
                let Some(replies) = routes.parents.get(&w.tag) else {
                    w.exhausted = true;
                    continue;
                };
                let remaining = LIMIT - w.ids.len();
                let upper = w.before.as_ref().map_or(Unbounded, Excluded);
                let start = candidates.len();
                for (rank, _) in replies
                    .range((Unbounded, upper))
                    .rev()
                    .filter(|(_, cursor)| **cursor <= wanted.reply)
                    .take(remaining)
                {
                    candidates.push((i, rank.clone()));
                }
                w.exhausted = candidates.len() - start < remaining;
                if !w.exhausted {
                    w.before = candidates.last().map(|(_, rank)| rank.clone());
                }
            }
        }
        if candidates.is_empty() {
            break;
        }
        let ids: Vec<&str> = candidates.iter().map(|(_, rank)| rank.1.as_ref()).collect();
        let found = visible(db, community, &ids)?;
        for (i, (_, id)) in candidates {
            if found.contains(id.as_ref()) {
                windows[i].ids.push(id.to_string());
            }
        }
    }
    native_guard::verify_current(db)?;
    Ok(windows
        .into_iter()
        .filter(|w| !w.ids.is_empty())
        .map(|mut w| {
            w.ids.reverse();
            (w.parent, w.ids)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Action, Envelope, SignedEvent, new_secret, signing_key},
        store::Store,
    };
    use ed25519_dalek::SigningKey;

    fn event(key: &SigningKey, seq: u64, community: &str, action: Action) -> Result<Envelope> {
        Envelope::accept(
            SignedEvent::create(
                key,
                hex::encode(key.verifying_key().as_bytes()),
                community.into(),
                seq,
                action,
            )?,
            key,
        )
    }

    fn parent(key: &SigningKey, seq: u64) -> Result<Envelope> {
        event(
            key,
            seq,
            "public",
            Action::Post {
                body: "root".into(),
                anonymous: true,
                topic: "Plans".into(),
                display_name: "Anonymous neighbor".into(),
            },
        )
    }

    fn reply(key: &SigningKey, seq: u64, community: &str, parent: &str) -> Result<Envelope> {
        event(
            key,
            seq,
            community,
            Action::Reply {
                parent: parent.into(),
                body: "body".into(),
                display_name: "Anonymous neighbor".into(),
            },
        )
    }

    fn read(store: &mut Store, roots: &[String]) -> Result<HashMap<String, Vec<String>>> {
        store.operation(|s| latest(&s.db, "public", roots))
    }

    #[test]
    fn native_visibility_seeks_event_and_moderation_by_id() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("home"))?;
        let sql = format!("EXPLAIN QUERY PLAN {VISIBLE}");
        let plan = store
            .db
            .prepare(&sql)?
            .query_map([r#"["reply-id"]"#, "public"], |row| row.get::<_, String>(3))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for table in ["e", "h"] {
            let search = format!("SEARCH {table} USING ");
            assert!(
                plan.iter()
                    .any(|step| step.contains(&search) && step.contains("(id=?)")),
                "{plan:#?}"
            );
        }
        Ok(())
    }

    #[test]
    fn committed_backfill_is_safe_for_older_snapshots_and_rollback() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("home");
        let mut writer = Store::open(&path)?;
        let old = writer.fork()?;
        let key = signing_key(&new_secret())?;
        let root = parent(&key, 1)?;
        let id = &root.event.id;
        let roots = std::slice::from_ref(id);
        writer.accept(&root)?;
        assert!(read(&mut writer, roots)?.is_empty());
        let home = writer.db.native_home().unwrap().clone();
        let index = &home.feed_replies;
        assert!(index.communities.lock().unwrap().is_empty());

        let first = reply(&key, 2, "public", id)?;
        writer.accept(&first)?;
        old.db.begin_operation()?;
        native_guard::verify_current(&old.db)?; // Pin the earlier MVCC snapshot.
        let newer = reply(&key, 3, "public", id)?;
        writer.accept(&newer)?;
        assert_eq!(read(&mut writer, roots)?[id].len(), 2);
        assert_eq!(
            latest(&old.db, "public", roots)?[id],
            std::slice::from_ref(&first.event.id)
        );
        old.db.commit_operation()?;

        let speculative = reply(&key, 4, "public", id)?;
        writer.db.begin_operation()?;
        writer.accept(&speculative)?;
        assert!(latest(&writer.db, "public", roots).is_err());
        writer.db.rollback_operation()?;
        let replacement = reply(&key, 4, "public", id)?;
        writer.accept(&replacement)?;
        let result = read(&mut writer, roots)?;
        assert_eq!(result[id].len(), 3);
        assert!(result[id].contains(&replacement.event.id));
        assert!(!result[id].contains(&speculative.event.id));

        let community = index.communities.lock().unwrap()["public"].clone();
        old.db.begin_operation()?;
        native_guard::verify_current(&old.db)?;
        for (i, r) in [first, newer, replacement].into_iter().enumerate() {
            writer.accept(&event(
                &key,
                i as u64 + 5,
                "public",
                Action::Delete { target: r.event.id },
            )?)?;
        }
        assert!(read(&mut writer, roots)?.is_empty());
        assert!(community.routes.read().unwrap().parents.is_empty());
        assert!(community.routes.read().unwrap().by_id.is_empty());
        let stale = latest(&old.db, "public", roots).unwrap_err();
        assert!(crate::home_sql::retryable(&stale), "{stale:#}");
        old.db.rollback_operation()?;
        Ok(())
    }

    #[test]
    fn backfill_authenticates_new_routes_and_advances_only_after_success() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let key = signing_key(&new_secret())?;
        let post = parent(&key, 1)?;
        let parent = &post.event.id;
        let roots = std::slice::from_ref(parent);
        let first = reply(&key, 2, "public", parent)?;
        let second = reply(&key, 3, "public", parent)?;
        store.accept(&post)?;
        store.accept(&first)?;
        assert_eq!(read(&mut store, roots)?[parent].len(), 1);
        store.accept(&second)?;
        let select = "SELECT routing FROM home_openings WHERE private=0 AND target=?";
        let update = "UPDATE home_openings SET routing=? WHERE private=0 AND target=?";
        let sealed = |id: &str| store.db.query_row(select, [id], |r| r.get::<_, Vec<u8>>(0));
        let old = sealed(&first.event.id)?;
        let new = sealed(&second.event.id)?;
        store.db.execute(update, params![old, second.event.id])?;
        assert!(read(&mut store, roots).is_err());
        store.db.execute(update, params![new, second.event.id])?;
        // An incremental retry must only open the newly committed suffix.
        store.db.execute(update, params![new, first.event.id])?;
        assert_eq!(read(&mut store, roots)?[parent].len(), 2);
        Ok(())
    }

    #[test]
    fn latest_visible_window_backfills_hidden_replies_and_survives_restart() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("home");
        let mut store = Store::open(&path)?;
        let key = signing_key(&new_secret())?;
        let root = parent(&key, 1)?;
        let second = parent(&key, 2)?;
        let id = &root.event.id;
        let other = &second.event.id;
        let roots = [id.clone(), other.clone()];
        let orphan_parent = new_secret();
        let early = reply(&key, 3, "public", other)?;
        store.accept(&early)?; // Delivery can precede the parent.
        store.accept(&root)?;
        store.accept(&second)?;
        store.accept(&reply(&key, 4, "elsewhere", id)?)?;
        store.accept(&reply(&key, 5, "public", &orphan_parent)?)?;
        let mut replies = (0..LIMIT + 3)
            .map(|i| reply(&key, i as u64 + 6, "public", id))
            .collect::<Result<Vec<_>>>()?;
        replies.sort_by(|a, b| (a.accepted_at, &a.event.id).cmp(&(b.accepted_at, &b.event.id)));
        store.operation(|s| {
            for r in replies.iter().rev() {
                s.accept(r)?;
            }
            Ok(())
        })?;
        let ids: Vec<_> = replies.into_iter().map(|r| r.event.id).collect();
        let first = read(&mut store, &roots)?;
        assert_eq!(first[id], ids[3..]);
        assert_eq!(first[other], std::slice::from_ref(&early.event.id));
        let hidden = ids.last().unwrap();
        store.moderate(hidden, true)?;
        let moderated = read(&mut store, &roots)?;
        assert_eq!(moderated[id], ids[2..ids.len() - 1]);
        store.moderate(hidden, false)?;
        assert_eq!(read(&mut store, &roots)?, first);
        drop(store);
        let mut reopened = Store::open(&path)?;
        assert_eq!(read(&mut reopened, &roots)?, first);
        for entry in std::fs::read_dir(dir.path())? {
            let path = entry?.path();
            if path.is_file() {
                let bytes = std::fs::read(&path)?;
                assert!(
                    !bytes
                        .windows(orphan_parent.len())
                        .any(|w| w == orphan_parent.as_bytes()),
                    "plaintext routing in {path:?}"
                );
            }
        }
        Ok(())
    }
}
