// SPDX-License-Identifier: AGPL-3.0-only
//! The native transaction commits domain state, its new head and its outbox
//! together. Only a committed outbox may advance this independent SQLite guard.
//! A crash before finalization leaves a replayable transition. A catalog behind
//! the guard (or on a different branch at the same epoch) cannot serve.
use crate::{
    home_sql::HomeDb,
    model::{Action, Envelope, new_secret},
    portability::{AccountRecord, validate_chain},
    store::migrations,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(crate) const NATIVE_SCHEMA: &str = include_str!("../migrations/native/0001_guard.sql");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stamp {
    pub guard_id: String,
    pub epoch: i64,
    pub token: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Intent {
    Delete {
        proof: Box<Envelope>,
        private: bool,
    },
    Account {
        chain: Vec<AccountRecord>,
    },
    Delivery {
        peer: String,
        community: String,
        target: String,
    },
    RetireAccount {
        account: String,
        record_id: String,
    },
    Cutover,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Prepared {
    before: Stamp,
    after: Stamp,
    op_id: String,
    intents: Vec<Intent>,
}

#[derive(Clone, Copy)]
pub(crate) struct Scope<'a> {
    pub target: &'a str,
    pub owner: &'a str,
    pub community: &'a str,
    pub private: bool,
}

pub(crate) struct NativeGuard {
    path: PathBuf,
    writer: Mutex<Connection>,
    publication: RwLock<()>,
    fenced: AtomicBool,
}

fn stamp(db: &Connection) -> Result<Stamp> {
    db.query_row(
        "SELECT s.id,s.epoch,w.token FROM state s JOIN native_watermark w
         ON w.singleton=s.singleton AND w.epoch=s.epoch WHERE s.singleton=1",
        [],
        |r| {
            Ok(Stamp {
                guard_id: r.get(0)?,
                epoch: r.get(1)?,
                token: r.get(2)?,
            })
        },
    )
    .context("missing or stale independent guard watermark")
}

/// A native transaction whose pinned snapshot was overtaken by a committed
/// guard transition can retry from a fresh snapshot. Equal-epoch forks cannot.
#[derive(Debug)]
pub(crate) struct StaleSnapshot;

impl std::fmt::Display for StaleSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native transaction predates current guard epoch")
    }
}

impl std::error::Error for StaleSnapshot {}

fn check(db: &Connection, expected: &Stamp) -> Result<()> {
    let actual = stamp(db)?;
    if actual.guard_id == expected.guard_id && actual.epoch > expected.epoch {
        return Err(StaleSnapshot.into());
    }
    ensure!(
        actual == *expected,
        "catalog is stale or belongs to a different guard or branch"
    );
    Ok(())
}

impl NativeGuard {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .context("missing independent deletion guard")?;
        let mut db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.busy_timeout(Duration::from_secs(5))?;
        migrations::guard_version(&db, "main")?;
        ensure!(
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM state WHERE singleton=1 AND length(id)=64
                 AND epoch>=0 AND typeof(key)='blob' AND length(key)=32)",
                [],
                |r| r.get::<_, bool>(0),
            )?,
            "uninitialized independent deletion guard"
        );
        let mode: String = db.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
        ensure!(
            mode.eq_ignore_ascii_case("delete"),
            "guard requires a DELETE journal"
        );
        db.execute_batch(
            "PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA; PRAGMA temp_store=MEMORY",
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        migrations::apply_guard_in(&tx, "main")?;
        tx.execute(
            "INSERT OR IGNORE INTO native_watermark(singleton,epoch,token,mac_key)
             SELECT singleton,epoch,?,randomblob(32) FROM state WHERE singleton=1",
            [new_secret()],
        )?;
        stamp(&tx)?;
        tx.commit()?;
        Ok(Self {
            path,
            writer: Mutex::new(db),
            publication: RwLock::new(()),
            fenced: AtomicBool::new(false),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn fence(&self) {
        self.fenced.store(true, Ordering::Release);
    }

    fn live(&self) -> Result<()> {
        ensure!(
            !self.fenced.load(Ordering::Acquire),
            "native Home requires guard recovery"
        );
        Ok(())
    }

    fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        let db = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("guard writer poisoned"))?;
        self.live()?;
        Ok(db)
    }

    pub(crate) fn publication(&self) -> Result<RwLockWriteGuard<'_, ()>> {
        let gate = self
            .publication
            .write()
            .map_err(|_| anyhow::anyhow!("guard publication poisoned"))?;
        self.live()?;
        Ok(gate)
    }

    pub(crate) fn read_gate(&self) -> Result<RwLockReadGuard<'_, ()>> {
        let gate = self
            .publication
            .read()
            .map_err(|_| anyhow::anyhow!("guard publication poisoned"))?;
        self.live()?;
        Ok(gate)
    }

    pub(crate) fn stamp(&self) -> Result<Stamp> {
        stamp(&*self.writer()?)
    }

    pub(crate) fn verify(&self, expected: &Stamp) -> Result<()> {
        check(&*self.writer()?, expected)
    }

    /// Administrative reads use their own SQLite snapshot. A callback may
    /// hydrate native rows using the normal key reader without reentering its mutex.
    pub(crate) fn pinned<T>(&self, f: impl FnOnce(&Connection, &Stamp) -> Result<T>) -> Result<T> {
        self.live()?;
        let db = Connection::open_with_flags(self.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(5))?;
        let tx = db.unchecked_transaction()?;
        let anchor = stamp(&tx)?;
        let result = f(&tx, &anchor)?;
        self.live()?;
        tx.commit()?;
        Ok(result)
    }

    fn reserve_key_for(
        &self,
        expected: &Stamp,
        scope: Scope<'_>,
        reservation: Option<(&str, &str)>,
    ) -> Result<[u8; 32]> {
        let mut db = self.writer()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&tx, expected)?;
        ensure!(!retired(&tx, scope.owner)?, "account deleted");
        let args = params![scope.target, scope.owner, scope.community, scope.private];
        ensure!(!tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM known_deletions WHERE target=? AND owner=? AND community=? AND private<=?)",
            args, |r| r.get::<_, bool>(0)
        )?, "opening permanently retired");
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO opening_keys VALUES(?,?,?,?,randomblob(32))",
            args,
        )?;
        if let Some((catalog, operation)) = reservation {
            if inserted != 0 {
                tx.execute("INSERT INTO unconfirmed_opening_keys VALUES(?,?,?,?)", args)?;
            }
            tx.execute(
                "INSERT OR IGNORE INTO opening_reservations
                 SELECT target,owner,community,private,?5,?6 FROM unconfirmed_opening_keys
                 WHERE target=?1 AND owner=?2 AND community=?3 AND private=?4",
                params![
                    scope.target,
                    scope.owner,
                    scope.community,
                    scope.private,
                    catalog,
                    operation
                ],
            )?;
        }
        let key = tx.query_row(
            "SELECT key FROM opening_keys WHERE target=? AND owner=? AND community=? AND private=?",
            args, |r| r.get(0)
        ).context("missing current opening key")?;
        tx.commit()?;
        Ok(key)
    }

    #[cfg(test)]
    pub(crate) fn reserve_key(&self, expected: &Stamp, scope: Scope<'_>) -> Result<[u8; 32]> {
        self.reserve_key_for(expected, scope, None)
    }

    pub(crate) fn key(&self, expected: &Stamp, scope: Scope<'_>) -> Result<[u8; 32]> {
        let db = self.writer()?;
        let key = db
            .prepare_cached(
                "SELECT k.key FROM opening_keys k
             JOIN state s ON s.singleton=1 AND s.id=?5 AND s.epoch=?6
             JOIN native_watermark w ON w.singleton=s.singleton AND w.epoch=s.epoch AND w.token=?7
             WHERE k.target=?1 AND k.owner=?2 AND k.community=?3 AND k.private=?4
               AND NOT EXISTS (SELECT 1 FROM known_deletions d WHERE d.target=?1
                   AND d.owner=?2 AND d.community=?3 AND d.private<=?4)",
            )?
            .query_row(
                params![
                    scope.target,
                    scope.owner,
                    scope.community,
                    scope.private,
                    expected.guard_id,
                    expected.epoch,
                    expected.token
                ],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(key) = key {
            return Ok(key);
        }
        // Distinguish a retryable stale transaction from a missing/retired key.
        check(&db, expected)?;
        anyhow::bail!("missing current opening key")
    }

    fn prepare(&self, before: Stamp, intents: Vec<Intent>) -> Result<Prepared> {
        let db = self.writer()?;
        let tx = db.unchecked_transaction()?;
        check(&tx, &before)?;
        for intent in &intents {
            validate_intent(&tx, intent)?;
        }
        let op_id = new_secret();
        let after = successor(&tx, &before, &op_id, &intents)?;
        tx.commit()?;
        Ok(Prepared {
            before,
            after,
            op_id,
            intents,
        })
    }

    fn finalize(&self, target: &Stamp, ops: &[Prepared]) -> Result<()> {
        let mut db = self.writer()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut current = stamp(&tx)?;
        if &current == target {
            return Ok(());
        }
        for op in ops {
            ensure!(
                op.before == current
                    && op.after == successor(&tx, &current, &op.op_id, &op.intents)?,
                "unverifiable native guard transition"
            );
            for intent in &op.intents {
                apply_intent(&tx, intent)?;
            }
            current = op.after.clone();
        }
        ensure!(
            &current == target,
            "native guard chain does not reach committed head"
        );
        tx.execute(
            "UPDATE state SET epoch=? WHERE singleton=1",
            [current.epoch],
        )?;
        tx.execute(
            "UPDATE native_watermark SET epoch=?,token=? WHERE singleton=1",
            params![current.epoch, current.token],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn successor(db: &Connection, before: &Stamp, op_id: &str, intents: &[Intent]) -> Result<Stamp> {
    let secret: [u8; 32] = db.query_row(
        "SELECT mac_key FROM native_watermark WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let bytes = serde_json::to_vec(&("yap native guard transition v1", before, op_id, intents))?;
    Ok(Stamp {
        guard_id: before.guard_id.clone(),
        epoch: before
            .epoch
            .checked_add(1)
            .context("guard epoch exhausted")?,
        token: blake3::keyed_hash(&secret, &bytes).to_hex().to_string(),
    })
}

fn retired(db: &Connection, account: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM known_deleted_accounts WHERE account=?)",
        [account],
        |r| r.get(0),
    )?)
}

fn validate_intent(db: &Connection, intent: &Intent) -> Result<()> {
    match intent {
        Intent::Delete { proof, .. } => {
            ensure!(
                proof.event.payload.version == 4
                    && proof.deletion.is_none()
                    && matches!(proof.event.payload.action, Action::Delete { .. })
                    && (1..=i64::MAX - 300).contains(&proof.accepted_at),
                "expected signed deletion"
            );
            proof.validate()?;
        }
        Intent::Account { chain } => {
            let latest = validate_chain(chain)?;
            ensure!(!retired(db, &latest.payload.account)?, "account deleted");
            let known: Option<String> = db
                .query_row(
                    "SELECT chain FROM current_accounts WHERE account=?",
                    [&latest.payload.account],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(known) = known {
                let old: Vec<AccountRecord> = serde_json::from_str(&known)?;
                ensure!(
                    chain.starts_with(&old),
                    "regressive or conflicting guard account chain"
                );
            }
        }
        Intent::Delivery {
            peer,
            community,
            target,
        } => {
            ensure!(
                !peer.is_empty() && !community.is_empty() && !target.is_empty(),
                "missing delivery identity"
            );
        }
        Intent::RetireAccount { account, record_id } => {
            let raw: String = db.query_row(
                "SELECT chain FROM current_accounts WHERE account=?",
                [account],
                |r| r.get(0),
            )?;
            let chain: Vec<AccountRecord> = serde_json::from_str(&raw)?;
            ensure!(
                validate_chain(&chain)?.id()? == *record_id,
                "deletion does not match current guard registration"
            );
        }
        Intent::Cutover => {}
    }
    Ok(())
}

fn apply_intent(db: &Connection, intent: &Intent) -> Result<()> {
    validate_intent(db, intent)?;
    match intent {
        Intent::Delete { proof, private } => {
            let Action::Delete { target } = &proof.event.payload.action else {
                unreachable!()
            };
            let scope = params![
                target,
                proof.event.owner_id(),
                proof.event.payload.community,
                private
            ];
            db.execute(
                "INSERT OR IGNORE INTO known_deletions VALUES(?,?,?,?,?)",
                params![
                    target,
                    proof.event.owner_id(),
                    proof.event.payload.community,
                    private,
                    serde_json::to_string(proof)?
                ],
            )?;
            db.execute("DELETE FROM opening_keys WHERE target=? AND owner=? AND community=? AND private>=?", scope)?;
        }
        Intent::Account { chain } => {
            db.execute("INSERT INTO current_accounts(account,chain) VALUES(?,?)
                       ON CONFLICT(account) DO UPDATE SET chain=excluded.chain
                       WHERE json_array_length(excluded.chain)>json_array_length(current_accounts.chain)",
                params![chain[0].payload.account,serde_json::to_string(chain)?])?;
        }
        Intent::Delivery {
            peer,
            community,
            target,
        } => {
            db.execute(
                "INSERT OR IGNORE INTO known_deliveries VALUES(?,?,?)",
                params![peer, community, target],
            )?;
        }
        Intent::RetireAccount { account, record_id } => {
            db.execute(
                "INSERT INTO known_deleted_accounts VALUES(?,?)",
                params![account, record_id],
            )?;
            db.execute("DELETE FROM current_accounts WHERE account=?", [account])?;
            db.execute("DELETE FROM opening_keys WHERE owner=?", [account])?;
        }
        Intent::Cutover => {}
    }
    Ok(())
}

fn guard(db: &HomeDb) -> Result<&NativeGuard> {
    Ok(&db
        .native_home()
        .context("missing native guard authority")?
        .guard)
}

pub(crate) fn head(db: &HomeDb) -> Result<Stamp> {
    db.query_row(
        "SELECT guard_id,epoch,token FROM native_guard_head WHERE singleton=1",
        [],
        |r| {
            Ok(Stamp {
                guard_id: r.get(0)?,
                epoch: r.get(1)?,
                token: r.get(2)?,
            })
        },
    )
    .context("missing native guard head")
}

/// Only fresh homes and explicitly authorized logical imports may bind a head.
/// Existing stores must call reconcile; never initialize from a missing head.
pub(crate) fn initialize_head(db: &HomeDb) -> Result<()> {
    let current = guard(db)?.stamp()?;
    db.execute(
        "INSERT OR IGNORE INTO native_guard_head VALUES(1,?,?,?)",
        params![current.guard_id, current.epoch, current.token],
    )?;
    verify_current(db)
}

pub(crate) fn verify_current(db: &HomeDb) -> Result<()> {
    guard(db)?.verify(&head(db)?)
}

/// Open only the requested public scopes for one bounded page. Keep publication
/// fenced through decryption, but release the SQLite connection before running
/// the caller. Only the prepared SQL is cached.
pub(crate) fn with_public_opening_keys<T>(
    db: &HomeDb,
    headers: &[&Envelope],
    work: impl FnOnce(&[[u8; 32]]) -> Result<T>,
) -> Result<T> {
    ensure!(
        headers.len() <= crate::MAX_PAGE_ENTRIES as usize,
        "opening batch too large"
    );
    let mut scopes = Vec::with_capacity(headers.len());
    for h in headers {
        ensure!(
            h.deletion.is_none() && matches!(h.event.payload.action, Action::Erased { .. }),
            "expected live public header"
        );
        scopes.push((&h.event.id, h.event.owner_id(), &h.event.payload.community));
    }
    let scopes = serde_json::to_string(&scopes)?;
    let guard = guard(db)?;
    let _lease = guard.read_gate()?;
    // Match all three signed identifiers. A native transaction may already
    // contain a deletion that has not yet reached the independent guard.
    let expected = db
        .prepare_cached(
            "SELECT guard_id,epoch,token FROM native_guard_head WHERE singleton=1 AND NOT EXISTS(
           SELECT 1 FROM json_each(?1) q CROSS JOIN deletions d
           WHERE d.target=json_extract(q.value,'$[0]') AND d.owner=json_extract(q.value,'$[1]')
             AND d.community=json_extract(q.value,'$[2]') AND d.private<=0)",
        )?
        .query_row([&scopes], |r| {
            Ok(Stamp {
                guard_id: r.get(0)?,
                epoch: r.get(1)?,
                token: r.get(2)?,
            })
        })
        .context("missing native guard head or public opening deleted in native catalog")?;
    let keys = {
        let conn = guard.writer()?;
        if headers.is_empty() {
            check(&conn, &expected)?;
            Vec::new()
        } else {
            // Keep each scope tuple in SQLite's parsed JSON form for the three
            // indexed comparisons. The input stays ordinary JSON text.
            let mut stmt = conn.prepare_cached(
                "SELECT k.key FROM state s
                 JOIN native_watermark w ON w.singleton=s.singleton AND w.epoch=s.epoch
                 CROSS JOIN jsonb_each(?1) q CROSS JOIN opening_keys k
                 WHERE s.singleton=1 AND s.id=?2 AND s.epoch=?3 AND w.token=?4
                   AND k.target=json_extract(q.value,'$[0]') AND k.owner=json_extract(q.value,'$[1]')
                   AND k.community=json_extract(q.value,'$[2]') AND k.private=0
                   AND NOT EXISTS (SELECT 1 FROM known_deletions d WHERE d.target=k.target
                     AND d.owner=k.owner AND d.community=k.community AND d.private<=0)
                 ORDER BY q.key"
            )?;
            let keys = stmt
                .query_map(
                    params![scopes, expected.guard_id, expected.epoch, expected.token],
                    |r| r.get(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if keys.len() != headers.len() {
                check(&conn, &expected)?;
                anyhow::bail!("missing current public opening key");
            }
            keys
        }
    };
    work(&keys)
}

pub(crate) fn opening_key(
    db: &HomeDb,
    event: &Envelope,
    private: bool,
    create: bool,
) -> Result<[u8; 32]> {
    let scope = Scope {
        target: &event.event.id,
        owner: event.event.owner_id(),
        community: &event.event.payload.community,
        private,
    };
    if create {
        ensure!(
            !db.is_autocommit()?,
            "native opening publication requires a transaction"
        );
        ensure!(
            event.deletion.is_none()
                && matches!(
                    event.event.payload.action,
                    Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
                ),
            "expected live signed opening"
        );
    }
    // Includes a deletion staged by this native transaction before the guard
    // watermark advances. The independent guard also checks retired scopes.
    ensure!(!db.query_row("SELECT EXISTS(SELECT 1 FROM deletions WHERE target=? AND owner=? AND community=? AND private<=?)",
        params![scope.target,scope.owner,scope.community,private], |r| r.get::<_, bool>(0))?,
        "opening deleted in native catalog");
    let current = head(db)?;
    if create {
        let home = db.native_home().context("expected native Home")?;
        db.with_opening_operation(|operation| {
            home.guard
                .reserve_key_for(&current, scope, Some((&home.catalog_id, operation)))
        })
    } else {
        guard(db)?.key(&current, scope)
    }
}

/// A reservation can be resolved after its outer transaction ends, or on startup
/// while the owner's lock guarantees that this catalog has no active writers.
/// A key shared by overlapping writes survives until one commits or all abort.
/// Existing confirmed keys may protect backups and are never reclaimed here.
pub(crate) fn recover_opening_reservations(db: &HomeDb, operation: Option<&str>) -> Result<()> {
    let home = db.native_home().context("expected native Home")?;
    ensure!(
        db.is_autocommit()?,
        "opening recovery requires committed state"
    );
    let pending = {
        let authority = home.guard.writer()?;
        let sql = if operation.is_some() {
            "SELECT target,owner,community,private,operation FROM opening_reservations WHERE catalog=?1 AND operation=?2"
        } else {
            "SELECT target,owner,community,private,operation FROM opening_reservations WHERE catalog=?1 AND ?2 IS NULL"
        };
        authority
            .prepare(sql)?
            .query_map(params![home.catalog_id, operation], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if pending.is_empty() {
        return Ok(());
    }
    let mut opening = db.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM home_openings WHERE target=? AND owner=? AND community=? AND private=?)",
    )?;
    let live = pending
        .iter()
        .map(|(target, owner, community, private, _)| {
            opening.query_row(params![target, owner, community, private], |r| {
                r.get::<_, bool>(0)
            })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut authority = home.guard.writer()?;
    let tx = authority.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for ((target, owner, community, private, operation), live) in pending.into_iter().zip(live) {
        let scope = params![target, owner, community, private];
        if live {
            tx.execute(
                "DELETE FROM unconfirmed_opening_keys WHERE target=? AND owner=? AND community=? AND private=?",
                scope,
            )?;
            tx.execute(
                "DELETE FROM opening_reservations WHERE target=? AND owner=? AND community=? AND private=?",
                scope,
            )?;
        } else {
            tx.execute(
                "DELETE FROM opening_reservations WHERE target=? AND owner=? AND community=? AND private=? AND catalog=? AND operation=?",
                params![target, owner, community, private, home.catalog_id, operation],
            )?;
            tx.execute(
                "DELETE FROM opening_keys WHERE target=?1 AND owner=?2 AND community=?3 AND private=?4
                 AND EXISTS(SELECT 1 FROM unconfirmed_opening_keys WHERE target=?1 AND owner=?2 AND community=?3 AND private=?4)
                 AND NOT EXISTS(SELECT 1 FROM opening_reservations WHERE target=?1 AND owner=?2 AND community=?3 AND private=?4)",
                scope,
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn stage(db: &HomeDb, intent: Intent) -> Result<()> {
    ensure!(
        db.native().is_some() && !db.is_autocommit()?,
        "guard intent requires native transaction"
    );
    db.execute(
        "INSERT INTO native_guard_stage(intent) VALUES(?)",
        [serde_json::to_string(&intent)?],
    )?;
    Ok(())
}

pub(crate) fn stage_delete(db: &HomeDb, proof: &Envelope, private: bool) -> Result<()> {
    ensure!(
        proof.deletion.is_none() && matches!(proof.event.payload.action, Action::Delete { .. }),
        "expected standalone deletion proof"
    );
    stage(
        db,
        Intent::Delete {
            proof: Box::new(proof.clone()),
            private,
        },
    )
}

pub(crate) fn stage_retirement(db: &HomeDb, account: &str, record_id: &str) -> Result<()> {
    stage(
        db,
        Intent::RetireAccount {
            account: account.into(),
            record_id: record_id.into(),
        },
    )
}

pub(crate) fn stage_account(db: &HomeDb, chain: &[AccountRecord]) -> Result<()> {
    stage(
        db,
        Intent::Account {
            chain: chain.to_vec(),
        },
    )
}

pub(crate) fn stage_delivery(db: &HomeDb, peer: &str, community: &str, target: &str) -> Result<()> {
    stage(
        db,
        Intent::Delivery {
            peer: peer.into(),
            community: community.into(),
            target: target.into(),
        },
    )
}

pub(crate) fn stage_cutover(db: &HomeDb) -> Result<()> {
    stage(db, Intent::Cutover)
}

pub(crate) fn has_intents(db: &HomeDb) -> Result<bool> {
    Ok(
        db.query_row("SELECT EXISTS(SELECT 1 FROM native_guard_stage)", [], |r| {
            r.get(0)
        })?,
    )
}

/// Called inside the outermost native transaction with the publication gate
/// held. The CAS on the singleton head also makes competing native writers
/// conflict at commit. Nested savepoints never finalize guard effects.
pub(crate) fn before_commit(db: &HomeDb) -> Result<()> {
    ensure!(
        !db.is_autocommit()?,
        "expected an active native transaction"
    );
    let before = head(db)?;
    let mut stmt = db.prepare("SELECT intent FROM native_guard_stage ORDER BY slot")?;
    let intents = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|raw| Ok(serde_json::from_str(&raw?)?))
        .collect::<Result<Vec<_>>>()?;
    ensure!(!intents.is_empty(), "empty native guard transition");
    let op = guard(db)?.prepare(before, intents)?;
    // Only prune through a verified durable watermark. Never discard an
    // unacknowledged transition that a restarting owner might need to replay.
    db.execute(
        "DELETE FROM native_guard_outbox WHERE epoch<=?",
        [op.before.epoch],
    )?;
    db.execute(
        "INSERT INTO native_guard_outbox(epoch,operation) VALUES(?,?)",
        params![op.after.epoch, serde_json::to_string(&op)?],
    )?;
    ensure!(
        db.execute(
            "UPDATE native_guard_head SET epoch=?,token=?
                        WHERE singleton=1 AND guard_id=? AND epoch=? AND token=?",
            params![
                op.after.epoch,
                op.after.token,
                op.before.guard_id,
                op.before.epoch,
                op.before.token
            ]
        )? == 1,
        "native guard head changed"
    );
    db.execute("DELETE FROM native_guard_stage", [])?;
    Ok(())
}

/// The owner calls this before serving an existing catalog, and after native
/// COMMIT while its publication gate excludes sensitive reads. A caller with
/// speculative changes must never be able to retire a key.
pub(crate) fn reconcile(db: &HomeDb) -> Result<()> {
    ensure!(
        db.is_autocommit()?,
        "refusing to finalize uncommitted native state"
    );
    ensure!(
        !has_intents(db)?,
        "committed native catalog contains unsealed guard intents"
    );
    let authority = guard(db)?;
    let current = authority.stamp()?;
    let committed = head(db)?;
    if current == committed {
        return Ok(());
    }
    ensure!(
        committed.guard_id == current.guard_id && committed.epoch > current.epoch,
        "native catalog predates trusted guard or is on a different branch"
    );
    let mut stmt = db.prepare(
        "SELECT epoch,operation FROM native_guard_outbox WHERE epoch>? AND epoch<=? ORDER BY epoch",
    )?;
    let mut rows = stmt.query([current.epoch, committed.epoch])?;
    let mut ops = Vec::new();
    while let Some(row) = rows.next()? {
        let op: Prepared = serde_json::from_str(&row.get::<_, String>(1)?)?;
        ensure!(
            op.after.epoch == row.get::<_, i64>(0)?,
            "unexpected outbox epoch"
        );
        ops.push(op);
    }
    authority.finalize(&committed, &ops)
}

pub(crate) fn after_commit(db: &HomeDb) -> Result<()> {
    reconcile(db)
}

#[cfg(test)]
#[path = "../tests/turso/native_guard.rs"]
mod tests;

#[cfg(test)]
mod page_tests {
    use super::*;
    use crate::{SignedEvent, signing_key, store::Store};

    #[test]
    fn public_key_batch_requires_current_head_and_live_scope_in_both_databases() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let signer = signing_key(&new_secret())?;
        let home = hex::encode(signer.verifying_key().as_bytes());
        let post = Envelope::accept(
            SignedEvent::create(
                &signer,
                home,
                "nearby".into(),
                1,
                Action::Post {
                    body: "guarded".into(),
                    topic: "Plans".into(),
                    anonymous: true,
                    display_name: "Anonymous neighbor".into(),
                },
            )?,
            &signer,
        )?;
        store.accept(&post)?;
        let mut header = post;
        header.event.payload = header.event.header()?;
        let db = &store.db;
        let read = || with_public_opening_keys(db, &[&header], |keys| Ok(keys[0]));
        let key = opening_key(db, &header, false, false)?;
        assert_eq!(read()?, key);
        let scope = params![
            header.event.id,
            header.event.owner_id(),
            header.event.payload.community
        ];
        let guard = guard(db)?;
        // Deliberately retain the key. A tombstone independently prevents
        // disclosure even if the native catalog still has a readable opening.
        guard.writer()?.execute(
            "INSERT INTO known_deletions VALUES(?,?,?,1,'fixture')",
            scope,
        )?;
        assert_eq!(read()?, key);
        guard.writer()?.execute(
            "INSERT INTO known_deletions VALUES(?,?,?,0,'fixture')",
            scope,
        )?;
        assert!(read().is_err());
        guard.writer()?.execute("DELETE FROM known_deletions", [])?;
        {
            let tx = db.unchecked_transaction()?;
            tx.execute("INSERT INTO deletions VALUES(?,?,?,0,'fixture')", scope)?;
            assert!(read().is_err());
        }
        assert_eq!(read()?, key);
        {
            let tx = db.unchecked_transaction()?;
            tx.execute("UPDATE native_guard_head SET token='fork'", [])?;
            assert!(read().is_err());
            tx.execute("UPDATE native_guard_head SET epoch=epoch-1", [])?;
            assert!(read().unwrap_err().is::<StaleSnapshot>());
        }
        assert_eq!(read()?, key);
        Ok(())
    }
}
