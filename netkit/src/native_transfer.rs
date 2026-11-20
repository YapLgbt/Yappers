// SPDX-License-Identifier: AGPL-3.0-only
//! Logical transfer at the standalone Home's administrative boundaries. The
//! portable format is the migrated SQLite schema used by volatile HA staging.
use crate::{
    backup,
    home_sql::HomeDb,
    native_content, native_guard, native_primary,
    store::{Store, migrations},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, MAIN_DB, OptionalExtension, params, types::Value};
use std::{fs, path::Path};

// Column names are part of the portable format. In particular, old v1 databases
// can have the same columns in a different physical order after ALTER TABLE.
// Preserve implicit rowids where they order user-visible blocks or prekey claims.
const DOMAIN: &[(&str, &str)] = &[
    ("peers", "id,generation,cursor,scope"),
    ("account_states", "account,chain"),
    ("deleted_accounts", "account,record_id"),
    ("hidden", "id"),
    ("peer_deliveries", "peer,community,target"),
    ("public_groups", "id,record"),
    ("group_permits", "group_id,host,record"),
    ("group_hosts", "group_id,host,issued_at,expires_at,record"),
    ("hosted_groups", "group_id"),
    ("user_blocks", "rowid,owner,peer,handle,label"),
    ("dm_devices", "owner,device,identity"),
    ("dm_prekeys", "rowid,owner,once_key,bundle"),
    ("dm_mail", "seq,id,sender,recipient,packet,created,held"),
    ("dm_claims", "owner,peer,created"),
    ("private_groups", "id,owner,room"),
    ("private_group_rosters", "group_id,revision,roster"),
    ("private_group_sessions", "group_id,id,packet"),
    (
        "private_group_events",
        "cursor,group_id,id,revision,session,ratchet_index,parent,event",
    ),
    ("public_profiles", "home,owner,revision,name,profile"),
    ("public_profile_portraits", "home,owner,portrait"),
    ("encrypted_attachments", "id,owner,size,created,received"),
    ("encrypted_attachment_chunks", "id,part,ciphertext"),
    ("username_claims", "home,account,name"),
    ("first_seen", "owner,since"),
    ("privacy_settings", "owner,settings"),
    ("dm_approved", "owner,peer"),
    (
        "push_devices",
        "owner,device,provider,subscription,categories",
    ),
    ("push_starts", "owner,device,category,baseline"),
    (
        "push_routes",
        "token,owner,device,category,target,delivered,leasing,retry_at",
    ),
    ("inbox_reads", "owner,kind,target,through"),
    ("inbox_mutes", "owner,target"),
    ("inbox_arrivals", "event,received"),
];
const STREAMS: [&str; 3] = ["events", "dm_mail", "private_group_events"];

fn copy_table(src: &HomeDb, dst: &HomeDb, table: &str, columns: &str) -> Result<()> {
    let count = columns.split(',').count();
    let params = vec!["?"; count].join(",");
    let mut insert = dst.prepare(&format!("INSERT INTO {table}({columns}) VALUES({params})"))?;
    let mut select = src.prepare(&format!("SELECT {columns} FROM {table} ORDER BY rowid"))?;
    let mut rows = select.query([])?;
    while let Some(row) = rows.next()? {
        let values = (0..count)
            .map(|i| row.get::<_, Value>(i))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        insert.execute(values)?;
    }
    Ok(())
}

fn copy_events(src: &Store, dst: &Store, private: bool) -> Result<()> {
    let (table, columns) = if private {
        ("personal_events", "account,event_id,envelope")
    } else {
        ("events", "cursor,id,community,author,author_seq,envelope")
    };
    let mut select = src
        .db
        .prepare(&format!("SELECT {columns} FROM {table} ORDER BY rowid"))?;
    let mut rows = select.query([])?;
    let mut insert = dst.db.prepare(&format!(
        "INSERT INTO {table}({columns}) VALUES({})",
        if private { "?,?,?" } else { "?,?,?,?,?,?" }
    ))?;
    while let Some(row) = rows.next()? {
        let raw: String = row.get("envelope")?;
        let envelope = native_content::hydrate(&src.db, &raw, private)?;
        envelope.validate()?;
        let event = &envelope.event;
        let stored = if dst.db.native().is_some() {
            native_content::encode(&dst.db, &envelope, private)?
        } else {
            serde_json::to_string(&envelope)?
        };
        if private {
            ensure!(
                row.get::<_, String>(0)? == event.owner_id()
                    && row.get::<_, String>(1)? == event.id,
                "private history does not match its signed event"
            );
            insert.execute(params![event.owner_id(), event.id, stored])?;
        } else {
            ensure!(
                row.get::<_, String>(1)? == event.id
                    && row.get::<_, String>(2)? == event.payload.community
                    && row.get::<_, String>(3)? == event.payload.author
                    && row.get::<_, u64>(4)? == event.payload.sequence,
                "feed index does not match its signed event"
            );
            insert.execute(params![
                row.get::<_, i64>(0)?,
                event.id,
                event.payload.community,
                event.payload.author,
                event.payload.sequence,
                stored
            ])?;
            if let Some(receipt) = src.username_receipt(&event.id)? {
                Store::record_username(&dst.db, event, &receipt)?;
            }
        }
    }
    Ok(())
}

fn copy_sequences(src: &Store, dst: &Store) -> Result<()> {
    for stream in STREAMS {
        if let Some(conn) = dst.db.native() {
            let high_water = src
                .db
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name=?",
                    [stream],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .unwrap_or(0);
            futures_lite::future::block_on(tokio::task::unconstrained(
                native_primary::restore_high_water(conn, stream, high_water),
            ))
            .with_context(|| format!("restoring {stream} cursor high water {high_water}"))?;
        } else {
            let high_water: i64 = src.db.query_row(
                "SELECT high_water FROM native_cursor_fences WHERE stream=?",
                [stream],
                |r| r.get(0),
            )?;
            dst.db
                .execute("DELETE FROM sqlite_sequence WHERE name=?", [stream])?;
            dst.db.execute(
                "INSERT INTO sqlite_sequence(name,seq) VALUES(?,?)",
                params![stream, high_water],
            )?;
        }
    }
    Ok(())
}

fn copy_domain(src: &Store, dst: &Store) -> Result<()> {
    copy_table(
        &src.db,
        &dst.db,
        "deletions",
        "target,owner,community,private,proof",
    )?;
    copy_events(src, dst, false)?;
    copy_events(src, dst, true)?;
    for &(table, columns) in DOMAIN {
        copy_table(&src.db, &dst.db, table, columns)
            .with_context(|| format!("copying Home table {table}"))?;
    }
    let receipts = |store: &Store| {
        store
            .db
            .query_row("SELECT count(*) FROM username_receipts", [], |r| {
                r.get::<_, i64>(0)
            })
    };
    ensure!(
        receipts(src)? == receipts(dst)?,
        "unmatched username receipt"
    );
    copy_sequences(src, dst)
}

fn check_empty(db: &HomeDb) -> Result<()> {
    for table in DOMAIN.iter().map(|(table, _)| *table).chain([
        "events",
        "personal_events",
        "username_receipts",
        "deletions",
        "public_projections",
        "home_openings",
        "home_import_complete",
    ]) {
        ensure!(
            !db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table})"), [], |r| {
                r.get::<_, bool>(0)
            })?,
            "Home import requires an empty destination ({table})"
        );
    }
    Ok(())
}

fn copy_legacy_projections(src: &Store, dst: &Store) -> Result<()> {
    let mut select = src
        .db
        .prepare("SELECT community,digest,posts,event_count,verified FROM public_projections")?;
    let mut rows = select.query([])?;
    while let Some(row) = rows.next()? {
        let community: String = row.get(0)?;
        let raw: String = row.get(2)?;
        let posts: Vec<crate::model::RemotePost> = serde_json::from_str(&raw)?;
        let sealed = crate::native_projection::encode(dst, &community, &posts)?;
        dst.db.execute("INSERT INTO public_projections(community,digest,posts,event_count,verified) VALUES(?,?,?,?,?)",
            params![community, row.get::<_,String>(1)?, sealed, row.get::<_,i64>(3)?, row.get::<_,i64>(4)?])?;
    }
    Ok(())
}

/// The boot/restore owner publishes the completion marker and commits this
/// transaction. A retry starts with an empty native domain.
pub(crate) fn import_legacy(source: &Store, destination: &mut Store) -> Result<()> {
    ensure!(
        destination.db.native().is_some() && !destination.db.is_autocommit()?,
        "Home import requires a native destination transaction"
    );
    check_empty(&destination.db)?;
    let db = source
        .db
        .sqlite()
        .context("expected portable SQLite source")?;
    let staged;
    let in_place = db.path().is_some_and(|p| !p.is_empty());
    let src = if in_place {
        let tx = db.unchecked_transaction()?;
        backup::verify_epoch(&tx)?;
        let mut memory = Connection::open_in_memory()?;
        memory.execute_batch("PRAGMA temp_store=MEMORY")?;
        rusqlite::backup::Backup::new(&tx, &mut memory)?.run_to_completion(
            256,
            std::time::Duration::ZERO,
            None,
        )?;
        // Release the attached guard read transaction before native key allocation.
        tx.commit()?;
        staged = Store::from_sqlite(memory);
        &staged
    } else {
        source
    };
    let (id, epoch): (String, i64) = src.db.query_row(
        "SELECT id,epoch FROM deletion_epoch WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let stamp = native_guard::head(&destination.db)?;
    ensure!(
        stamp.guard_id == id && stamp.epoch == epoch,
        "portable Home does not match current deletion guard"
    );
    let guard = &destination
        .db
        .native_home()
        .context("missing native Home")?
        .guard;
    guard.verify(&stamp)?;
    copy_domain(src, destination)?;
    if in_place {
        copy_legacy_projections(src, destination)?;
    }
    guard.verify(&stamp)
}

fn bind_portable(db: &HomeDb, stamp: &native_guard::Stamp) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS deletion_epoch(singleton INTEGER PRIMARY KEY CHECK(singleton=1), id TEXT NOT NULL, epoch INTEGER NOT NULL);
                      DELETE FROM deletion_epoch;")?;
    db.execute(
        "INSERT INTO deletion_epoch(singleton,id,epoch) VALUES(1,?,?)",
        params![stamp.guard_id, stamp.epoch],
    )?;
    Ok(())
}

// Pin the native read first. Each body read consults the current guard. Check
// the stamp once more after the last read, before returning or sealing the
// image. A guard writer can thus complete while the copy is walking the rows.
fn export<T>(
    store: &Store,
    guard_data: impl FnOnce(&Connection) -> Result<T>,
) -> Result<(Vec<u8>, T)> {
    let home = store.db.native_home().context("expected native Home")?;
    let tx = store.db.unchecked_transaction()?;
    let head = native_guard::head(&tx)?;
    let mut memory = Connection::open_in_memory()?;
    memory.execute_batch("PRAGMA temp_store=MEMORY")?;
    migrations::apply(&mut memory)?;
    // The seed carries the authoritative arrival rows, even if a legacy event
    // has none. Disable insertion side effects on this private connection only.
    memory.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER,
        false,
    )?;
    let mut stage = Store::from_sqlite(memory);
    let write = stage.db.unchecked_transaction()?;
    copy_domain(store, &stage)?;
    bind_portable(&write, &head)?;
    write.commit()?;
    let image = stage
        .db
        .sqlite_mut()
        .context("expected volatile SQLite stage")?
        .serialize(MAIN_DB)?
        .to_vec();
    let result = home.guard.pinned(|guard, stamp| {
        ensure!(
            *stamp == head,
            "native Home does not match current deletion guard"
        );
        guard_data(guard)
    })?;
    tx.commit()?;
    Ok((image, result))
}

pub(crate) fn snapshot(store: &Store) -> Result<Vec<u8>> {
    Ok(export(store, |_| Ok(()))?.0)
}

pub(crate) fn archive(store: &Store) -> Result<Vec<u8>> {
    let (image, (memory, key)) = export(store, |guard| {
        let key: Vec<u8> =
            guard.query_row("SELECT key FROM state WHERE singleton=1", [], |r| r.get(0))?;
        // seal_snapshot operates on an attached guard and may mint keys for old
        // inline stores. Native records must already have their scoped live key.
        let mut memory = Connection::open_in_memory()?;
        memory.execute_batch("PRAGMA temp_store=MEMORY; ATTACH DATABASE ':memory:' AS guard")?;
        rusqlite::backup::Backup::new_with_names(guard, MAIN_DB, &mut memory, c"guard")?
            .run_to_completion(256, std::time::Duration::ZERO, None)?;
        memory.execute_batch(
            "CREATE TEMP TRIGGER require_archive_key BEFORE INSERT ON guard.opening_keys
            WHEN NOT EXISTS(SELECT 1 FROM guard.opening_keys WHERE
              (target,owner,community,private)=(NEW.target,NEW.owner,NEW.community,NEW.private))
            BEGIN SELECT RAISE(ABORT,'missing live archive key'); END;",
        )?;
        Ok((memory, key))
    })?;
    backup::seal_snapshot(&memory, &image, &key)
}

pub(crate) fn backup(store: &Store, output: &Path) -> Result<()> {
    backup::publish(&archive(store)?, output)
}

pub(crate) fn restore(guard: &Path, input: &Path, output: &Path) -> Result<()> {
    let path = guard
        .canonicalize()
        .context("missing current deletion guard")?;
    let _destination = backup::serving_lock(output).context("cannot lock restore destination")?;
    let _authority = backup::serving_lock(&path).context("cannot lock current deletion guard")?;
    let bytes = fs::read(input)?;
    let ciphertext = bytes
        .strip_prefix(backup::HEADER)
        .context("invalid protected backup")?;
    // Authenticate before the native opener can apply a migration to an older guard.
    let reader = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let key: Vec<u8> =
        reader.query_row("SELECT key FROM state WHERE singleton=1", [], |r| r.get(0))?;
    let image = backup::unseal(ciphertext, &key)?;
    drop(reader);
    let guard = native_guard::NativeGuard::open(&path)?;
    let mut source = Store::from_sqlite(Connection::open_in_memory()?);
    source.restore_snapshot(&image)?;
    guard.pinned(|authority, stamp| {
        let current: Vec<u8> =
            authority.query_row("SELECT key FROM state WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(current == key, "backup guard changed during restore");
        backup::recover_openings(&mut source, authority)?;
        bind_portable(&source.db, stamp)
    })?;
    Store::restore_from_memory(output, &path, &source)
}

#[cfg(test)]
#[path = "../tests/turso/native_transfer.rs"]
mod tests;
