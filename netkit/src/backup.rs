// SPDX-License-Identifier: AGPL-3.0-only
//! The guard lives outside the backup set. It retires individual opening keys atomically
//! with deletion. Old backups recover live records through individual opening keys.
use crate::{
    model::{Action, Envelope, EventPayload, UsernameReceipt, new_secret},
    store::Store,
};
use anyhow::{Context, Result, ensure};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use rand::{RngCore, rngs::OsRng};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{io::Write, os::unix::fs::OpenOptionsExt, path::Path};

pub(crate) const HEADER: &[u8] = b"YAP-HOME-BACKUP-1\0";

/// Restored paths may alias the same live guard. Only one of them may serve.
pub(crate) fn lock_authority(path: &Path) -> Result<std::fs::File> {
    let guard = path.with_added_extension("deletion-guard").canonicalize()?;
    serving_lock(&guard).context("another home is already serving with this deletion guard")
}
pub(crate) fn serving_lock(path: &Path) -> Result<std::fs::File> {
    let lock = path.with_added_extension("serve-lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lock)?;
    ensure!(file.metadata()?.is_file(), "expected a regular serve lock");
    file.try_lock()?;
    Ok(file)
}

pub(crate) fn attach(db: &mut Connection, path: &Path) -> Result<()> {
    let guard = path.with_added_extension("deletion-guard");
    db.execute_batch("CREATE TABLE IF NOT EXISTS deletion_epoch(singleton INTEGER PRIMARY KEY CHECK(singleton=1), id TEXT NOT NULL, epoch INTEGER NOT NULL)")?;
    let initialized: bool =
        db.query_row("SELECT EXISTS(SELECT 1 FROM deletion_epoch)", [], |r| {
            r.get(0)
        })?;
    let _file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(!initialized)
        .mode(0o600)
        .open(&guard)
        .context("missing independent deletion guard; restore the current guard before serving")?;
    db.execute(
        "ATTACH DATABASE ? AS guard",
        [guard
            .canonicalize()?
            .to_str()
            .context("non-UTF8 guard path")?],
    )?;
    db.execute_batch("PRAGMA guard.journal_mode=DELETE; PRAGMA guard.secure_delete=ON; PRAGMA guard.synchronous=EXTRA;")?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    crate::store::migrations::apply_guard(&tx)?;
    if !initialized {
        let id = new_secret();
        tx.execute(
            "INSERT INTO guard.state VALUES(1,?,0,randomblob(32))",
            [&id],
        )
        .context("existing deletion guard belongs to another database")?;
        tx.execute("INSERT INTO deletion_epoch VALUES(1,?,0)", [&id])?;
    }
    verify_epoch(&tx)?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn verify_epoch(db: &Connection) -> Result<()> {
    ensure!(
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM deletion_epoch JOIN guard.state USING(id,epoch))",
            [],
            |r| r.get::<_, bool>(0)
        )?,
        "database predates known deletion state or belongs to a different guard"
    );
    Ok(())
}

pub(crate) fn watch_deletes(db: &Connection) -> Result<()> {
    // TEMP triggers may update attached databases. Both databases use rollback
    // journals so SQLite commits data, epoch, and key destruction as one unit.
    db.execute_batch("CREATE TEMP TRIGGER remember_delivery AFTER INSERT ON main.peer_deliveries BEGIN
        INSERT OR IGNORE INTO known_deliveries VALUES(NEW.peer,NEW.community,NEW.target);
        UPDATE state SET epoch=epoch+1; UPDATE deletion_epoch SET epoch=epoch+1;
        END;
        CREATE TEMP TRIGGER advance_deletion AFTER INSERT ON main.deletions BEGIN
        INSERT OR IGNORE INTO known_deletions VALUES(NEW.target,NEW.owner,NEW.community,NEW.private,NEW.proof);
        DELETE FROM opening_keys WHERE target=NEW.target AND owner=NEW.owner AND community=NEW.community AND private>=NEW.private;
        UPDATE state SET epoch=epoch+1;
        UPDATE deletion_epoch SET epoch=epoch+1;
        END;")?;
    for op in ["INSERT", "UPDATE"] {
        db.execute_batch(&format!("CREATE TEMP TRIGGER remember_account_{op} AFTER {op} ON main.account_states BEGIN
            INSERT INTO current_accounts VALUES(NEW.account,NEW.chain) ON CONFLICT(account) DO UPDATE SET chain=excluded.chain;
            UPDATE state SET epoch=epoch+1; UPDATE deletion_epoch SET epoch=epoch+1; END;"))?;
    }
    Ok(())
}

pub(crate) fn remember(
    db: &Connection,
    target: &str,
    owner: &str,
    community: &str,
    private: bool,
    proof: &str,
) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO guard.known_deletions VALUES(?,?,?,?,?)",
        rusqlite::params![target, owner, community, private, proof],
    )?;
    db.execute(
        "DELETE FROM guard.opening_keys WHERE target=? AND owner=? AND community=? AND private>=?",
        rusqlite::params![target, owner, community, private],
    )?;
    db.execute("UPDATE guard.state SET epoch=epoch+1", [])?;
    db.execute("UPDATE deletion_epoch SET epoch=epoch+1", [])?;
    Ok(())
}

fn cipher(key: &[u8]) -> Result<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid backup key"))
}
pub(crate) fn seal(bytes: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let encrypted = cipher(key)?
        .encrypt(XNonce::from_slice(&nonce), bytes)
        .map_err(|_| anyhow::anyhow!("cannot encrypt backup"))?;
    Ok([nonce.as_slice(), &encrypted].concat())
}
pub(crate) fn unseal(bytes: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let (nonce, data) = bytes
        .split_at_checked(24)
        .context("invalid protected backup")?;
    cipher(key)?
        .decrypt(XNonce::from_slice(nonce), data)
        .map_err(|_| anyhow::anyhow!("backup authentication failed"))
}

// Only committed feed and private-history envelopes contain content openings.
// Derived projection caches can always be rebuilt from the current home.
const TABLES: [(&str, &str, bool); 2] = [
    ("events", "id", false),
    ("personal_events", "event_id", true),
];

fn contents(db: &Connection, table: &str) -> Result<Vec<Envelope>> {
    let mut stmt = db.prepare(&format!("SELECT envelope FROM {table}"))?;
    stmt.query_map([], |r| r.get::<_, String>(0))?
        .map(|row| Ok(serde_json::from_str(&row?)?))
        .collect()
}

pub(crate) fn seal_snapshot(db: &Connection, image: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let mut copy = Store::from_sqlite(Connection::open_in_memory()?);
    copy.restore_snapshot(image)?;
    copy.db.execute_batch("DELETE FROM public_projections;
        CREATE TABLE backup_openings(target TEXT,private INTEGER, ciphertext BLOB,PRIMARY KEY(target,private));")?;
    for (table, id_column, private) in TABLES {
        for mut e in contents(copy.db.sqlite().context("expected SQLite staging")?, table)? {
            ensure!(
                e.event.payload.version == 4,
                "unsupported historical event in backup"
            );
            if matches!(e.event.payload.action, Action::Erased { .. }) {
                e.validate()
                    .context("missing or invalid deletion proof in snapshot")?;
            }
            if !matches!(
                e.event.payload.action,
                Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
            ) {
                continue;
            }
            let args = rusqlite::params![
                e.event.id,
                e.event.owner_id(),
                e.event.payload.community,
                private
            ];
            ensure!(!db.query_row("SELECT EXISTS(SELECT 1 FROM guard.known_deletions WHERE target=? AND owner=? AND community=? AND private<=?)",args,|r|r.get::<_,bool>(0))?,"snapshot contains deleted content");
            db.execute(
                "INSERT OR IGNORE INTO guard.opening_keys VALUES(?,?,?,?,randomblob(32))",
                args,
            )?;
            let opening_key:Vec<u8>=db.query_row("SELECT key FROM guard.opening_keys WHERE target=? AND owner=? AND community=? AND private=?",args,|r|r.get(0))?;
            let byline = if private {
                None
            } else {
                let receipt = copy.username_receipt(&e.event.id)?;
                copy.db.execute(
                    "DELETE FROM username_receipts WHERE event_id=?",
                    [&e.event.id],
                )?;
                receipt
            };
            let ciphertext = seal(
                &serde_json::to_vec(&(&e.event.payload, byline))?,
                &opening_key,
            )?;
            e.event.payload = e.event.header()?;
            copy.db.execute(
                "INSERT INTO backup_openings VALUES(?,?,?)",
                rusqlite::params![e.event.id, private, ciphertext],
            )?;
            copy.db.execute(
                &format!("UPDATE {table} SET envelope=? WHERE {id_column}=?"),
                rusqlite::params![serde_json::to_string(&e)?, e.event.id],
            )?;
        }
    }
    copy.db.execute_batch("VACUUM")?;
    Ok([HEADER, &seal(&copy.snapshot()?, key)?].concat())
}

pub(crate) fn recover_openings(copy: &mut Store, guard: &Connection) -> Result<()> {
    for (table, id_column, private) in TABLES {
        for mut e in contents(copy.db.sqlite().context("expected SQLite staging")?, table)? {
            if e.deletion.is_some() || !matches!(e.event.payload.action, Action::Erased { .. }) {
                e.validate()?;
                continue;
            }
            let args = rusqlite::params![
                e.event.id,
                e.event.owner_id(),
                e.event.payload.community,
                private
            ];
            let proof:Option<String>=guard.query_row("SELECT proof FROM known_deletions WHERE target=? AND owner=? AND community=? AND private<=? ORDER BY private LIMIT 1",args,|r|r.get(0)).optional()?;
            if let Some(proof) = proof {
                e = e.erased(&serde_json::from_str(&proof)?)?;
                copy.db.execute(
                    "DELETE FROM username_receipts WHERE event_id=?",
                    [&e.event.id],
                )?;
            } else {
                let key:Vec<u8>=guard.query_row("SELECT key FROM opening_keys WHERE target=? AND owner=? AND community=? AND private=?",args,|r|r.get(0)).context("missing live opening key")?;
                let encrypted: Vec<u8> = copy.db.query_row(
                    "SELECT ciphertext FROM backup_openings WHERE target=? AND private=?",
                    rusqlite::params![e.event.id, private],
                    |r| r.get(0),
                )?;
                let (payload, byline): (EventPayload, Option<UsernameReceipt>) =
                    serde_json::from_slice(&unseal(&encrypted, &key)?)?;
                e.event.payload = payload;
                if let Some(receipt) = byline {
                    Store::record_username(&copy.db, &e.event, &receipt)?;
                }
            }
            e.validate()?;
            copy.db.execute(
                &format!("UPDATE {table} SET envelope=? WHERE {id_column}=?"),
                rusqlite::params![serde_json::to_string(&e)?, e.event.id],
            )?;
        }
    }
    copy.db.execute_batch("DROP TABLE backup_openings")?;
    copy_deliveries(
        guard,
        copy.db.sqlite().context("expected SQLite staging")?,
        "known_deliveries",
        "peer_deliveries",
    )?;
    let mut accounts = guard.prepare("SELECT chain FROM current_accounts")?;
    for raw in accounts.query_map([], |r| r.get::<_, String>(0))? {
        let chain: Vec<crate::portability::AccountRecord> = serde_json::from_str(&raw?)?;
        copy.register_account(&chain)?;
    }
    let mut stmt = guard.prepare("SELECT private,proof FROM known_deletions")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, bool>(0)?, r.get::<_, String>(1)?)))? {
        let (private, raw) = row?;
        let proof: Envelope = serde_json::from_str(&raw)?;
        if private {
            copy.import_personal_events(proof.event.owner_id(), std::slice::from_ref(&proof))?;
        } else {
            copy.accept(&proof)?;
        }
    }
    Ok(())
}

/// Copies only routing IDs; guards and images created before the covenant have no delivery table.
pub(crate) fn copy_deliveries(
    src: &Connection,
    dst: &Connection,
    from: &str,
    to: &str,
) -> Result<usize> {
    if !src.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?)",
        [from],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(0);
    }
    let mut read = src.prepare(&format!("SELECT peer,community,target FROM {from}"))?;
    let mut write = dst.prepare(&format!(
        "INSERT OR IGNORE INTO {to}(peer,community,target) VALUES(?,?,?)"
    ))?;
    let mut added = 0;
    let mut rows = read.query([])?;
    while let Some(row) = rows.next()? {
        added += write.execute(rusqlite::params![
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?
        ])?;
    }
    Ok(added)
}

pub(crate) fn publish(bytes: &[u8], output: &Path) -> Result<()> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = parent.join(format!(".yap-backup-{}", new_secret()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::hard_link(&tmp, output)?;
        std::fs::File::open(parent)?.sync_all()?;
        anyhow::Ok(())
    })();
    std::fs::remove_file(tmp)?;
    result
}

impl Store {
    /// Back up an initialized standalone store without running database creation or migrations.
    pub fn backup_existing(source: &Path, output: &Path) -> Result<()> {
        ensure!(
            !source.with_added_extension("ha-managed").try_exists()?,
            "use ha-export for an HA home"
        );
        if crate::native_admin::backup_existing(source, output)? {
            return Ok(());
        }
        let db = Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .context("open existing backup source")?;
        Self::check_backup_version(&db).context("unrecognized standalone backup source")?;
        ensure!(
            !db.query_row(
                "SELECT EXISTS(SELECT 1 FROM main.sqlite_schema WHERE name='head')",
                [],
                |r| r.get::<_, bool>(0)
            )?,
            "unsupported standalone backup source"
        );
        let guard = source
            .with_added_extension("deletion-guard")
            .canonicalize()
            .context("missing current backup guard")?;
        db.execute(
            "ATTACH DATABASE ? AS guard",
            [guard.to_str().context("invalid guard path")?],
        )?;
        db.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA guard.secure_delete=ON; PRAGMA guard.synchronous=EXTRA")?;
        Self::from_sqlite(db).backup(output)
    }

    /// A coherent offline backup encrypted under this home's current deletion epoch.
    pub fn backup(&self, output: &Path) -> Result<()> {
        if self.is_native() {
            return crate::native_transfer::backup(self, output);
        }

        let tx = self
            .db
            .sqlite()
            .context("expected SQLite staging")?
            .unchecked_transaction()?;
        crate::store::migrations::guard_version(&tx, "guard")?;
        let key: Vec<u8> = tx.query_row(
            "SELECT key FROM guard.state JOIN deletion_epoch USING(id,epoch)",
            [],
            |r| r.get(0),
        )?;
        let sealed = seal_snapshot(&tx, &self.snapshot()?, &key)?;
        tx.commit()?;
        publish(&sealed, output)
    }
}

/// Restore surviving records with the current guard into a fresh native Home.
pub fn restore(guard: &Path, input: &Path, output: &Path) -> Result<()> {
    crate::native_transfer::restore(guard, input, output)
}
