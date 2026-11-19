// SPDX-License-Identifier: AGPL-3.0-only
//! Durable event headers and erasable content, local moderation, and per-peer replication checkpoints.
//!
//! This layer verifies signatures and storage invariants. The server is responsible for
//! membership, origin trust, community scope, and page ordering before it calls imports.
//! A conflicting envelope or reused author sequence must roll back the whole page.

use crate::{MAX_PAGE_ENTRIES, model::*};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeSet;

/// SQLite store with secure deletion and atomic rollback journals. Callers serialize access.
pub struct Store {
    pub(crate) db: Connection,
}
impl Store {
    /// Open/create a private database. Unsupported schemas fail; no implicit migration occurs.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        ensure!(
            !path.with_added_extension("ha-managed").try_exists()?,
            "this database is HA staging; export committed state from the authority"
        );
        Self::open_staging(path)
    }
    pub(crate) fn open_staging(path: &std::path::Path) -> Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        drop(file);
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA; PRAGMA temp_store=MEMORY;
            CREATE TABLE IF NOT EXISTS schema_version(version INTEGER NOT NULL);
            INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM schema_version);
            CREATE TABLE IF NOT EXISTS events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
                community TEXT NOT NULL, author TEXT NOT NULL, author_seq INTEGER NOT NULL, envelope TEXT NOT NULL,
                UNIQUE(author, author_seq));
            CREATE INDEX IF NOT EXISTS events_community ON events(community, cursor);
            CREATE TABLE IF NOT EXISTS peers(id TEXT PRIMARY KEY, generation TEXT NOT NULL, cursor INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS account_states(account TEXT PRIMARY KEY, chain TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS personal_events(account TEXT NOT NULL, event_id TEXT NOT NULL, envelope TEXT NOT NULL, PRIMARY KEY(account,event_id));
            CREATE TABLE IF NOT EXISTS hidden(id TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS public_projections(community TEXT PRIMARY KEY, digest TEXT NOT NULL, posts TEXT NOT NULL, event_count INTEGER NOT NULL, verified INTEGER NOT NULL);")?;
        let version: i64 = db.query_row("SELECT version FROM schema_version", [], |r| r.get(0))?;
        ensure!(version == 1, "unsupported database schema");
        let has_scope: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('peers') WHERE name='scope')",
            [],
            |r| r.get(0),
        )?;
        if !has_scope {
            db.execute(
                "ALTER TABLE peers ADD COLUMN scope TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        db.execute_batch(crate::redaction::SCHEMA)?;
        ensure!(!db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE json_extract(envelope,'$.event.payload.version')<>4 UNION ALL SELECT 1 FROM personal_events WHERE json_extract(envelope,'$.event.payload.version')<>4)",[],|r|r.get::<_,bool>(0))?, "legacy content present; use a fresh v5 store");
        db.execute_batch(crate::directory::SCHEMA)?;
        db.execute_batch(crate::mailbox::SCHEMA)?;
        db.execute_batch(crate::private_group_store::SCHEMA)?;
        db.execute_batch(crate::profiles::SCHEMA)?;
        db.execute_batch(crate::usernames::SCHEMA)?;
        crate::privacy::install(&db)?;
        crate::notifications::install(&db)?;
        crate::backup::attach(&mut db, path)?;
        crate::backup::watch_deletes(&db)?;
        Ok(Self { db })
    }
    /// Read the full home attestation so a duplicate can be checked for equivocation.
    pub fn existing(&self, id: &str) -> Result<Option<Envelope>> {
        let text: Option<String> = self
            .db
            .query_row("SELECT envelope FROM events WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        text.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }
    pub(crate) fn insert(db: &Connection, envelope: &Envelope) -> Result<()> {
        let envelope = &Self::reconcile(db, envelope, false)?;
        crate::privacy::observe(db, envelope.event.owner_id())?;
        let existing: Option<String> = db
            .query_row(
                "SELECT envelope FROM events WHERE id=?1",
                [&envelope.event.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            ensure!(
                serde_json::from_str::<Envelope>(&existing)?.same_acceptance(envelope)?,
                "conflicting home attestation"
            );
            return Ok(());
        }
        db.execute(
            "INSERT INTO events(id,community,author,author_seq,envelope) VALUES(?1,?2,?3,?4,?5)",
            params![
                envelope.event.id,
                envelope.event.payload.community,
                envelope.event.payload.author,
                envelope.event.payload.sequence as i64,
                serde_json::to_string(envelope)?
            ],
        )?;
        Ok(())
    }
    /// Durably accept one validated envelope; exact duplicates are idempotent.
    pub fn accept(&mut self, envelope: &Envelope) -> Result<()> {
        self.accept_with_username(envelope, None)
    }
    pub(crate) fn accept_with_username(
        &mut self,
        envelope: &Envelope,
        username: Option<&UsernameReceipt>,
    ) -> Result<()> {
        envelope.validate()?;
        let tx = self.db.transaction()?;
        Self::insert(&tx, envelope)?;
        if let Some(receipt) = username {
            Self::record_username(&tx, &envelope.event, receipt)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Load a community's ingestion log; projection order is defined separately in model.
    pub fn events(&self, community: &str) -> Result<Vec<Envelope>> {
        let mut statement = self
            .db
            .prepare("SELECT envelope FROM events WHERE community=?1 ORDER BY cursor")?;
        let rows = statement.query_map([community], |r| r.get::<_, String>(0))?;
        rows.map(|s| Ok(serde_json::from_str(&s?)?)).collect()
    }
    /// Read instance-local moderation decisions, never replicated peer policy.
    pub fn hidden(&self) -> Result<BTreeSet<String>> {
        self.db
            .prepare("SELECT id FROM hidden")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }
    /// Hide/unhide locally without modifying an immutable signed envelope.
    pub fn moderate(&self, id: &str, hide: bool) -> Result<()> {
        if hide {
            self.db
                .execute("INSERT OR IGNORE INTO hidden VALUES(?1)", [id])?;
        } else {
            self.db.execute("DELETE FROM hidden WHERE id=?1", [id])?;
        }
        Ok(())
    }
    /// Return the last committed serving generation and its local ingestion cursor.
    pub fn cursor(&self, peer: &str) -> Result<(Option<String>, u64)> {
        let cursor = self
            .db
            .query_row(
                "SELECT generation,cursor FROM peers WHERE id=?1",
                [peer],
                |r| Ok((Some(r.get::<_, String>(0)?), r.get::<_, i64>(1)? as u64)),
            )
            .optional()?;
        Ok(cursor.unwrap_or((None, 0)))
    }
    /// A changed authorized community set requires replaying previously filtered history.
    pub fn scoped_cursor(&self, peer: &str, scope: &str) -> Result<(Option<String>, u64)> {
        let saved: Option<String> = self
            .db
            .query_row("SELECT scope FROM peers WHERE id=?1", [peer], |r| r.get(0))
            .optional()?;
        if saved.as_deref() == Some(scope) {
            self.cursor(peer)
        } else {
            Ok((None, 0))
        }
    }

    /// Commit all envelopes and their checkpoint atomically. Caller must authorize origin/scope and validate page ordering.
    pub fn import_page(
        &mut self,
        peer: &str,
        generation: &str,
        next: u64,
        entries: &[LogEntry],
    ) -> Result<()> {
        self.import_scoped_page(peer, generation, next, entries, "")
    }
    /// Atomically checkpoint the serving generation, local scope, and complete imported page.
    pub fn import_scoped_page(
        &mut self,
        peer: &str,
        generation: &str,
        next: u64,
        entries: &[LogEntry],
        scope: &str,
    ) -> Result<()> {
        let (old_generation, old_cursor) = self.scoped_cursor(peer, scope)?;
        ensure!(
            next <= i64::MAX as u64
                && (old_generation.as_deref() != Some(generation) || next >= old_cursor),
            "invalid sync cursor"
        );
        for entry in entries {
            entry.envelope.validate()?;
        }
        let tx = self.db.transaction()?;
        for entry in entries {
            Self::insert(&tx, &entry.envelope)?;
            if let Some(receipt) = &entry.username {
                Self::record_username(&tx, &entry.envelope.event, receipt)?;
            }
        }
        tx.execute(
            "INSERT INTO peers(id,generation,cursor,scope) VALUES(?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,cursor=excluded.cursor,scope=excluded.scope
             WHERE peers.generation<>excluded.generation OR peers.cursor<>excluded.cursor OR peers.scope<>excluded.scope",
            params![peer, generation, next as i64, scope],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Scan global log rows, filter authorized communities, and advance over filtered rows too.
    pub fn page(
        &self,
        after: u64,
        limit: u32,
        communities: &BTreeSet<String>,
        generation: &str,
    ) -> Result<Response> {
        self.peer_page(after, limit, communities, generation, None)
    }
    pub(crate) fn remember_deliveries<'a>(
        &self,
        peer: &str,
        deliveries: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO peer_deliveries(peer,community,target) VALUES(?,?,?)",
            )?;
            for (community, target) in deliveries {
                insert.execute(params![peer, community, target])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn peer_page(
        &self,
        after: u64,
        limit: u32,
        communities: &BTreeSet<String>,
        generation: &str,
        suspended_peer: Option<&str>,
    ) -> Result<Response> {
        ensure!(
            after <= i64::MAX as u64 && (1..=MAX_PAGE_ENTRIES).contains(&limit),
            "invalid page bounds"
        );
        let mut statement = self.db.prepare(
            "SELECT cursor,envelope FROM events WHERE cursor>?1 ORDER BY cursor LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after as i64, limit], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
        })?;
        let mut next = after;
        let mut entries = Vec::new();
        for row in rows {
            let (cursor, json) = row?;
            next = cursor;
            let envelope: Envelope = serde_json::from_str(&json)?;
            if communities.contains(&envelope.event.payload.community) {
                if let Some(peer) = suspended_peer {
                    let proof = if matches!(envelope.event.payload.action, Action::Delete { .. }) {
                        Some(envelope)
                    } else {
                        envelope.deletion.map(|e| *e)
                    };
                    if let Some(envelope)=proof && let Action::Delete{target}=&envelope.event.payload.action
                        && self.db.query_row("SELECT EXISTS(SELECT 1 FROM peer_deliveries WHERE peer=? AND community=? AND target=?)",params![peer,envelope.event.payload.community,target],|r|r.get::<_,bool>(0))? {
                        entries.push(LogEntry {cursor,envelope,username:None});
                    }
                } else {
                    entries.push(self.log_entry(cursor, envelope)?);
                }
            }
        }
        let more: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE cursor>?1)",
            [next as i64],
            |r| r.get(0),
        )?;
        Ok(Response::Page {
            generation: generation.to_owned(),
            entries,
            next,
            more,
        })
    }
}

impl Store {
    /// SQLite's backup API includes every committed WAL page in one consistent image.
    pub(crate) fn snapshot(&self) -> Result<Vec<u8>> {
        let mut memory = Connection::open_in_memory()?;
        rusqlite::backup::Backup::new(&self.db, &mut memory)?.run_to_completion(
            256,
            std::time::Duration::ZERO,
            None,
        )?;
        let mut image = memory.serialize(rusqlite::MAIN_DB)?.to_vec();
        // A deserialized in-memory DB cannot open an on-disk WAL. The private backup
        // has already incorporated all WAL pages; describe it as rollback-journal format.
        ensure!(
            image.len() >= 100 && &image[..16] == b"SQLite format 3\0",
            "invalid SQLite backup"
        );
        image[18] = 1;
        image[19] = 1;
        Ok(image)
    }

    pub(crate) fn restore_snapshot(&mut self, image: &[u8]) -> Result<()> {
        ensure!(
            image.len() >= 100 && &image[..16] == b"SQLite format 3\0",
            "invalid SQLite snapshot"
        );
        self.db
            .execute_batch("PRAGMA secure_delete=ON; PRAGMA temp_store=MEMORY;")?;
        ensure!(
            self.db.path().is_none_or(|path| path.is_empty()),
            "raw snapshots may only enter volatile staging"
        );
        let mut source = Connection::open_in_memory()?;
        source.deserialize_read_exact(rusqlite::MAIN_DB, image, image.len(), true)?;
        rusqlite::backup::Backup::new(&source, &mut self.db)?.run_to_completion(
            256,
            std::time::Duration::ZERO,
            None,
        )?;

        self.db.execute_batch(crate::redaction::SCHEMA)?;
        self.db.execute_batch(crate::usernames::SCHEMA)?;
        self.db.execute_batch(crate::profiles::SCHEMA)?;
        crate::notifications::install(&self.db)?;
        Ok(())
    }
}
