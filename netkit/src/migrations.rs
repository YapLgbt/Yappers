// SPDX-License-Identifier: AGPL-3.0-only
//! Ordered migrations for the Home catalog and its independently retained deletion guard.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, MAIN_DB, TransactionBehavior};

const STEPS: &[fn(&Connection) -> Result<()>] =
    &[baseline, peer_scope, mail_requests, inbox, deleted_accounts];
pub(crate) const CURRENT_VERSION: i64 = STEPS.len() as i64;

// Version zero includes guards retained from releases before versioned migrations.
const GUARD_STEPS: &[&str] = &[
    include_str!("../migrations/guard/0001_guard.sql"),
    include_str!("../migrations/guard/0002_native_head.sql"),
    include_str!("../migrations/guard/0003_deleted_accounts.sql"),
];

pub(crate) fn guard_version(db: &Connection, schema: &str) -> Result<usize> {
    let version: i64 = db.pragma_query_value(Some(schema), "user_version", |r| r.get(0))?;
    ensure!(
        (0..=GUARD_STEPS.len() as i64).contains(&version),
        "unsupported deletion guard schema version {version}"
    );
    Ok(version as usize)
}

pub(crate) fn apply_guard(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    apply_guard_in(tx, "guard")
}

pub(crate) fn apply_guard_in(tx: &rusqlite::Transaction<'_>, schema: &'static str) -> Result<()> {
    ensure!(matches!(schema, "main" | "guard"), "invalid guard database");
    let current = guard_version(tx, schema)?;
    for (index, sql) in GUARD_STEPS.iter().enumerate().skip(current) {
        tx.execute_batch(&sql.replace("guard.", &format!("{schema}.")))?;
        tx.pragma_update(Some(schema), "user_version", (index + 1) as i64)?;
    }
    Ok(())
}

/// Read one supported marker from an initialized catalog, including legacy v1.
pub(crate) fn version(db: &Connection) -> Result<i64> {
    let mut stmt = db.prepare("SELECT version FROM main.schema_version LIMIT 2")?;
    let mut rows = stmt.query([])?;
    let version: i64 = rows
        .next()?
        .context("missing Home schema version")?
        .get(0)?;
    ensure!(rows.next()?.is_none(), "ambiguous Home schema version");
    ensure!(
        (1..=CURRENT_VERSION).contains(&version),
        "unsupported Home schema version {version}"
    );
    Ok(version)
}

pub(crate) fn apply(db: &mut Connection) -> Result<()> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing = if tx.table_exists(Some(MAIN_DB), c"schema_version")? {
        version(&tx)?
    } else {
        check_unversioned(&tx)?;
        0
    };
    // Earlier releases kept version=1 while adding optional subsystem tables.
    // Install those idempotent definitions before the first numbered upgrade.
    if existing == 1 {
        baseline(&tx)?;
    }
    for (index, step) in STEPS.iter().enumerate().skip(existing as usize) {
        let next = (index + 1) as i64;
        step(&tx).with_context(|| format!("applying Home schema migration {next}"))?;
        tx.execute("UPDATE main.schema_version SET version=?", [next])?;
    }
    let legacy: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE json_extract(envelope,'$.event.payload.version')<>4
         UNION ALL SELECT 1 FROM personal_events WHERE json_extract(envelope,'$.event.payload.version')<>4)",
        [], |r| r.get(0),
    )?;
    ensure!(!legacy, "legacy content present; use a fresh v5 store");
    tx.commit()?;
    Ok(())
}

// The first standalone checkpoint database contained exactly the peers table.
// Match its complete DDL, allowing differences in whitespace and SQL keyword case.
fn check_unversioned(db: &Connection) -> Result<()> {
    let unmarked: bool = db.query_row(
        "SELECT application_id=0 AND user_version=0 FROM pragma_application_id, pragma_user_version",
        [], |r| r.get(0),
    )?;
    ensure!(unmarked, "unrecognized unversioned database metadata");
    let mut stmt = db.prepare(
        "SELECT coalesce(sql,'') FROM main.sqlite_schema
         WHERE NOT(type='index' AND name='sqlite_autoindex_peers_1' AND tbl_name='peers' AND sql IS NULL)
         LIMIT 2",
    )?;
    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(());
    };
    let ddl: String = row.get(0)?;
    let tokens = ddl
        .to_ascii_lowercase()
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace(',', " , ");
    ensure!(rows.next()?.is_none() && tokens.split_ascii_whitespace().eq(
        "create table peers ( id text primary key , generation text not null , cursor integer not null )".split(' ')
    ), "unrecognized Home database without schema version");
    Ok(())
}

fn deleted_accounts(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("../migrations/0005_deleted_accounts.sql"))?;
    Ok(())
}

fn baseline(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("../migrations/0001_home.sql"))?;
    Ok(())
}

fn has_column(db: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?) WHERE name=?)",
        [table, column],
        |r| r.get(0),
    )?)
}

fn peer_scope(db: &Connection) -> Result<()> {
    if !has_column(db, "peers", "scope")? {
        db.execute_batch(include_str!("../migrations/0002_peer_scope.sql"))?;
    }
    Ok(())
}

fn mail_requests(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("../migrations/0003_mail_requests.sql"))?;
    // A legacy database with held mail has already run the one-time approval
    // backfill. Repeating it could reapprove a sender the account removed.
    if !has_column(db, "dm_mail", "held")? {
        db.execute_batch(include_str!("../migrations/0003_backfill.sql"))?;
    }
    Ok(())
}

fn inbox(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("../migrations/0004_inbox.sql"))?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/migrations/catalog.rs"]
mod tests;
