// SPDX-License-Identifier: AGPL-3.0-only
//! Durable event headers and erasable content, local moderation, and per-peer replication checkpoints.
//!
//! This layer verifies signatures and storage invariants. The server is responsible for
//! membership, origin trust, community scope, and page ordering before it calls imports.
//! A conflicting envelope or reused author sequence must roll back the whole page.

use crate::home_sql::HomeDb;
use crate::{MAX_FRAME, MAX_PAGE_ENTRIES, model::*};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeSet;
#[path = "native_home.rs"]
mod native_home;
pub(crate) use native_home::NativeHome;

#[path = "migrations.rs"]
pub(crate) mod migrations;

/// A Home SQL connection. Standalone connections share a native database and independent guard.
pub struct Store {
    pub(crate) db: HomeDb,
}
impl Store {
    /// Check an initialized Home's schema marker before backup attaches its guard.
    /// This also works on a connection opened with SQLITE_OPEN_READ_ONLY.
    pub(crate) fn check_backup_version(db: &Connection) -> Result<()> {
        migrations::version(db).map(|_| ())
    }

    /// Open/create a private database. Apply pending migrations before opening the deletion guard.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        ensure!(
            !path.with_added_extension("ha-managed").try_exists()?,
            "this database is HA staging; export committed state from the authority"
        );
        NativeHome::open(path)
    }
    pub(crate) fn open_staging(path: &std::path::Path) -> Result<Self> {
        NativeHome::open(path)
    }
    pub(crate) fn from_sqlite(db: Connection) -> Self {
        Self { db: db.into() }
    }
    pub(crate) fn open_legacy(path: &std::path::Path) -> Result<Self> {
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
        db.execute_batch(
            "PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA; PRAGMA temp_store=MEMORY;",
        )?;
        migrations::apply(&mut db)?;
        db.execute_batch("PRAGMA journal_mode=DELETE")?;
        crate::backup::attach(&mut db, path)?;
        crate::backup::watch_deletes(&db)?;
        Ok(Self::from_sqlite(db))
    }
    /// Read the full home attestation so a duplicate can be checked for equivocation.
    pub fn existing(&self, id: &str) -> Result<Option<Envelope>> {
        let text: Option<String> = self
            .db
            .query_row("SELECT envelope FROM events WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        text.map(|s| crate::native_content::hydrate(&self.db, &s, false))
            .transpose()
    }
    pub(crate) fn insert(db: &HomeDb, envelope: &Envelope) -> Result<()> {
        let envelope = &Self::reconcile(db, envelope, false)?;
        if crate::account_store::deleted_registration(db, envelope.event.owner_id())?.is_some() {
            if envelope.deletion.is_none()
                && !matches!(envelope.event.payload.action, Action::Delete { .. })
            {
                // Consume a late federation page without recreating the account or its content.
                return Ok(());
            }
        } else {
            crate::privacy::observe(db, envelope.event.owner_id())?;
        }
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
                crate::native_content::encode(db, envelope, false)?
            ],
        )?;
        if db.native().is_some()
            && crate::native_content::is_content(&envelope.event.payload.action)
        {
            db.execute(
                "INSERT OR IGNORE INTO inbox_arrivals(event,received) VALUES(?,?)",
                params![envelope.event.id, now()],
            )?;
        }
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
        ensure!(
            serde_json::to_vec(&LogEntry {
                cursor: i64::MAX as u64,
                envelope: envelope.clone(),
                username: username.cloned(),
            })?
            .len()
                <= MAX_FRAME - 256,
            "accepted event exceeds federation frame"
        );
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
        rows.map(|s| crate::native_content::hydrate(&self.db, &s?, false))
            .collect()
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
        ensure!(next <= i64::MAX as u64, "invalid sync cursor");
        for entry in entries {
            entry.envelope.validate()?;
        }
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Direct callers can hold different connections. Compare against the
        // committed checkpoint after acquiring the same lock used to advance it.
        let prior: Option<i64> = tx
            .query_row(
                "SELECT cursor FROM peers WHERE id=? AND scope=? AND generation=?",
                [peer, scope, generation],
                |row| row.get(0),
            )
            .optional()?;
        ensure!(
            prior.is_none_or(|cursor| next as i64 >= cursor),
            "invalid sync cursor"
        );
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
                let changed = insert.execute(params![peer, community, target])?;
                if changed != 0 && tx.native().is_some() {
                    crate::native_guard::stage_delivery(&tx, peer, community, target)?;
                }
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
        let capacity = MAX_FRAME
            .checked_sub(
                serde_json::to_vec(&Response::Page {
                    generation: generation.to_owned(),
                    entries: Vec::new(),
                    next: i64::MAX as u64,
                    more: false,
                })?
                .len(),
            )
            .ok_or_else(|| anyhow::anyhow!("federation page metadata exceeds frame"))?;
        let mut remaining = capacity;
        let mut next = after;
        let mut entries = Vec::new();
        for row in rows {
            let (cursor, json) = row?;
            let envelope: Envelope = serde_json::from_str(&json)?;
            if communities.contains(&envelope.event.payload.community) {
                let entry = if let Some(peer) = suspended_peer {
                    let proof = if matches!(envelope.event.payload.action, Action::Delete { .. }) {
                        Some(envelope)
                    } else {
                        envelope.deletion.map(|e| *e)
                    };
                    if let Some(envelope)=proof && let Action::Delete{target}=&envelope.event.payload.action
                        && self.db.query_row("SELECT EXISTS(SELECT 1 FROM peer_deliveries WHERE peer=? AND community=? AND target=?)",params![peer,envelope.event.payload.community,target],|r|r.get::<_,bool>(0))? {
                        Some(LogEntry {cursor,envelope,username:None})
                    } else {
                        None
                    }
                } else {
                    // Filter scope and suspension against the catalog before fetching a body.
                    let envelope = if envelope.deletion.is_none()
                        && matches!(envelope.event.payload.action, Action::Erased { .. })
                    {
                        self.existing(&envelope.event.id)?
                            .ok_or_else(|| anyhow::anyhow!("missing catalog event"))?
                    } else {
                        envelope
                    };
                    Some(self.log_entry(cursor, envelope)?)
                };
                if let Some(mut entry) = entry {
                    let mut bytes = serde_json::to_vec(&entry)?.len() + 1;
                    if bytes > capacity
                        && let Some(proof) = entry.envelope.deletion.take()
                    {
                        // A standalone signed proof retires the opening by target, owner
                        // and scope independently of its original acceptance header.
                        entry = LogEntry {
                            cursor,
                            envelope: *proof,
                            username: None,
                        };
                        bytes = serde_json::to_vec(&entry)?.len() + 1;
                    }
                    if bytes > remaining {
                        ensure!(next > after, "single federation entry exceeds frame");
                        break;
                    }
                    remaining -= bytes;
                    entries.push(entry);
                }
            }
            // Advance over filtered rows, but leave the first unshipped row for the next page.
            next = cursor;
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
        if self.is_native() {
            return crate::native_transfer::snapshot(self);
        }
        let db = self.db.sqlite().context("expected SQLite staging")?;
        let mut memory = Connection::open_in_memory()?;
        memory.execute_batch("PRAGMA temp_store=MEMORY")?;
        rusqlite::backup::Backup::new(db, &mut memory)?.run_to_completion(
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
        let db = self.db.sqlite_mut().context("expected SQLite staging")?;
        db.execute_batch("PRAGMA secure_delete=ON; PRAGMA temp_store=MEMORY;")?;
        ensure!(
            db.path().is_none_or(|path| path.is_empty()),
            "raw snapshots may only enter volatile staging"
        );
        let mut source = Connection::open_in_memory()?;
        source.deserialize_read_exact(rusqlite::MAIN_DB, image, image.len(), false)?;
        // Validate and upgrade the incoming image before replacing live staging.
        migrations::apply(&mut source)?;
        rusqlite::backup::Backup::new(&source, db)?.run_to_completion(
            256,
            std::time::Duration::ZERO,
            None,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod federation_page_tests {
    use super::*;
    use crate::portability::AccountRecord;

    #[test]
    fn federation_pages_preserve_rows_at_the_byte_boundary() -> Result<()> {
        let mut db = Connection::open_in_memory()?;
        migrations::apply(&mut db)?;
        let mut store = Store::from_sqlite(db);
        let author = signing_key(&new_secret())?;
        let home = signing_key(&new_secret())?;
        let home_id = hex::encode(home.verifying_key().as_bytes());
        let devices = (0..210)
            .map(|_| {
                signing_key(&new_secret()).map(|key| hex::encode(key.verifying_key().as_bytes()))
            })
            .chain(std::iter::once(Ok(hex::encode(
                author.verifying_key().as_bytes(),
            ))))
            .collect::<Result<BTreeSet<_>>>()?;
        let account = AccountRecord::signed(&author, None, home_id.clone(), devices)?;
        for sequence in 1..=u64::from(MAX_PAGE_ENTRIES) {
            let community = if sequence % 11 == 1 {
                "other"
            } else {
                "public"
            };
            let event = SignedEvent::create_for_account(
                &author,
                home_id.clone(),
                community.into(),
                sequence,
                Action::Post {
                    body: "Neighborhood notice".into(),
                    topic: "Plans".into(),
                    display_name: "Neighbor".into(),
                    anonymous: false,
                },
                account.clone(),
            )?;
            store.accept(&Envelope::accept(event, &home)?)?;
        }

        let mut cursor = 0;
        let scopes = BTreeSet::from(["public".to_string()]);
        let generation = new_secret();
        let mut observed = Vec::new();
        let mut page_count = 0;
        loop {
            let response = store.page(cursor, MAX_PAGE_ENTRIES, &scopes, &generation)?;
            assert!(serde_json::to_vec(&response)?.len() <= MAX_FRAME);
            let Response::Page {
                entries,
                next,
                more,
                ..
            } = response
            else {
                anyhow::bail!("expected federation page");
            };
            assert!(next > cursor);
            observed.extend(entries.into_iter().map(|entry| entry.cursor));
            page_count += 1;
            cursor = next;
            if !more {
                break;
            }
        }
        let expected = (1..=u64::from(MAX_PAGE_ENTRIES))
            .filter(|seq| seq % 11 != 1)
            .collect::<Vec<_>>();
        assert_eq!(observed, expected);
        assert_eq!(cursor, u64::from(MAX_PAGE_ENTRIES));
        assert!(page_count > 1);
        Ok(())
    }
}
