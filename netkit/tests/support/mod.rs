// SPDX-License-Identifier: AGPL-3.0-only
#![allow(dead_code)] // Each integration binary uses a subset of these fixtures.
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn primary_path(home: &Path) -> PathBuf {
    home.with_added_extension("primary.turso")
}

pub async fn primary(home: &Path) -> Result<turso::Connection> {
    let path = primary_path(home).canonicalize()?;
    let db = turso::Builder::new_local(path.to_str().context("non-UTF8 fixture path")?)
        .build()
        .await?;
    let conn = db.connect()?;
    let mode: String = conn
        .query("PRAGMA journal_mode", ())
        .await?
        .next()
        .await?
        .context("missing journal mode")?
        .get(0)?;
    ensure!(mode == "mvcc", "fixture requires the native MVCC primary");
    Ok(conn)
}

/// Seed large capacity fixtures in bounded native transactions. Each statement
/// binds the domain, first ID and last ID and inserts one row per ID.
pub async fn seed_range(
    conn: &turso::Connection,
    sql: &str,
    domain: &str,
    count: i64,
) -> Result<()> {
    let mut stmt = conn.prepare(sql).await?;
    for first in (1..=count).step_by(200) {
        let last = count.min(first + 199);
        let inserted = stmt.execute((domain, first, last)).await?;
        ensure!(
            inserted == (last - first + 1) as u64,
            "incomplete capacity fixture"
        );
    }
    Ok(())
}

// The primary has a SQLite-compatible data file, a WAL, and a distinct MVCC
// log whose extension replaces the data file's extension.
fn raw_files(home: &Path) -> [PathBuf; 4] {
    let main = primary_path(home);
    let mut wal = main.as_os_str().to_owned();
    wal.push("-wal");
    [
        home.to_owned(),
        main.clone(),
        main.with_extension("db-log"),
        wal.into(),
    ]
}

pub fn assert_no_plaintext(home: &Path, needles: &[&str]) -> Result<()> {
    for (i, file) in raw_files(home).into_iter().enumerate() {
        if i >= 2 && !file.try_exists()? {
            continue;
        }
        let bytes = fs::read(&file)?;
        for &needle in needles {
            assert!(!needle.is_empty());
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "plaintext marker in {}",
                file.display()
            );
        }
    }
    Ok(())
}

/// Copy a stopped Home's complete local primary; the caller supplies its guard.
pub fn raw_snapshot(source: &Path, dest: &Path) -> Result<()> {
    for (i, (before, after)) in raw_files(source)
        .into_iter()
        .zip(raw_files(dest))
        .enumerate()
    {
        if i == 3 && !before.try_exists()? {
            continue;
        }
        fs::copy(before, after)?;
    }
    Ok(())
}
