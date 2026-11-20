// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use std::sync::mpsc;

#[test]
fn independent_home_writes_commit_while_another_writer_is_open() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = Store::open(&dir.path().join("home"))?;
    let mut first = root.fork()?;
    let mut second = root.fork()?;
    let (started, waiting) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        first.operation(|store| {
            store.moderate("first", true)?;
            started.send(())?;
            finish.recv_timeout(Duration::from_secs(10))?;
            Ok(())
        })
    });
    waiting.recv_timeout(Duration::from_secs(10))?;
    second.operation(|store| store.moderate("second", true))?;
    let visible = root.hidden()?;
    release.send(())?;
    worker.join().unwrap()?;
    assert!(!visible.contains("first"));
    assert!(visible.contains("second"));
    assert_eq!(root.hidden()?.len(), 2);
    Ok(())
}

#[test]
fn conflicting_home_operations_retry_from_a_fresh_snapshot() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = Store::open(&dir.path().join("home"))?;
    root.db
        .execute("INSERT INTO first_seen VALUES('same-owner',10)", [])?;
    let mut first = root.fork()?;
    let mut second = root.fork()?;
    let (started, waiting) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut attempts = 0;
        first.operation(|store| {
            attempts += 1;
            let seen: i64 = store.db.query_row(
                "SELECT since FROM first_seen WHERE owner='same-owner'",
                [],
                |row| row.get(0),
            )?;
            if attempts == 1 {
                started.send(())?;
                finish.recv_timeout(Duration::from_secs(10))?;
            }
            store.db.execute(
                "UPDATE first_seen SET since=? WHERE owner='same-owner'",
                [seen + 1],
            )?;
            Ok(())
        })?;
        anyhow::Ok(attempts)
    });
    waiting.recv_timeout(Duration::from_secs(10))?;
    second.operation(|store| {
        store.db.execute(
            "UPDATE first_seen SET since=since+1 WHERE owner='same-owner'",
            [],
        )?;
        Ok(())
    })?;
    release.send(())?;
    assert!(worker.join().unwrap()? >= 2);
    assert_eq!(
        root.db.query_row(
            "SELECT since FROM first_seen WHERE owner='same-owner'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        12
    );
    Ok(())
}
