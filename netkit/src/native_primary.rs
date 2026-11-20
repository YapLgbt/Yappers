// SPDX-License-Identifier: AGPL-3.0-only
//! The Home process shares one native database handle. Each operation checks out
//! a distinct connection and runs its own transaction. Cursor fences live in
//! the database, so plain domain INSERTs and whole-operation retries agree.

use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use turso::{Connection, Database, transaction::TransactionBehavior};

const APPLICATION_ID: i64 = 0x5941504e; // YAPN
// One outer entry per schema version. Apply all its scripts in one transaction.
const MIGRATIONS: &[&[&str]] = &[
    &[
        include_str!("../migrations/native/0001_domain.sql"),
        crate::native_content::SCHEMA,
        crate::native_guard::NATIVE_SCHEMA,
        include_str!("../migrations/native/0001_cursors.sql"),
    ],
    &[include_str!("../migrations/native/0002_public_feed.sql")],
    &[include_str!("../migrations/native/0003_feed_votes.sql")],
    &[include_str!("../migrations/0005_deleted_accounts.sql")],
];

pub(crate) fn path_for(home: &Path) -> PathBuf {
    home.with_added_extension("primary.turso")
}

#[derive(Clone, Debug)]
pub(crate) struct Primary {
    db: Database,
}

fn private_file(path: &Path) -> Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(path)?;
            ensure!(
                meta.is_file() && meta.permissions().mode() & 0o077 == 0,
                "expected private regular file at {}",
                path.display()
            );
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

async fn integer(conn: &Connection, sql: &str) -> Result<i64> {
    Ok(conn
        .query(sql, ())
        .await?
        .next()
        .await?
        .context("missing integer result")?
        .get(0)?)
}

async fn mode(conn: &Connection, sql: &str) -> Result<String> {
    Ok(conn
        .query(sql, ())
        .await?
        .next()
        .await?
        .context("missing journal mode")?
        .get(0)?)
}

async fn version(conn: &Connection) -> Result<usize> {
    let version = integer(conn, "PRAGMA user_version").await?;
    ensure!(
        (0..=MIGRATIONS.len() as i64).contains(&version),
        "unsupported native Home schema version {version}"
    );
    let id = integer(conn, "PRAGMA application_id").await?;
    if version == 0 {
        ensure!(
            id == 0 && integer(conn, "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name <> '__turso_internal_mvcc_meta')").await? == 0,
            "unrecognized unversioned native database"
        );
    } else {
        ensure!(id == APPLICATION_ID, "database is not a native Home");
    }
    Ok(version as usize)
}

impl Primary {
    /// Open the exact primary path returned by path_for. The service owns the
    /// authority lock across this call and the lifetime of all its connections.
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        let path = std::path::absolute(path)?;
        let name = path.to_str().context("non-UTF8 native Home path")?;
        private_file(&path)?;
        let mut wal = OsString::from(&path);
        wal.push("-wal");
        private_file(Path::new(&wal))?;
        // Turso changes the extension for its MVCC log, unlike its WAL.
        private_file(&path.with_extension("db-log"))?;

        let db = turso::Builder::new_local(name).build().await?;
        let primary = Self { db };
        let mut conn = primary.connect().await?;
        let current = version(&conn).await?;
        if current == 0 {
            ensure!(
                mode(&conn, "PRAGMA journal_mode=mvcc").await? == "mvcc",
                "cannot enable native MVCC"
            );
        } else {
            ensure!(
                mode(&conn, "PRAGMA journal_mode").await? == "mvcc",
                "existing native Home requires MVCC"
            );
        }
        if current < MIGRATIONS.len() {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            let current = version(&tx).await?;
            for (index, scripts) in MIGRATIONS.iter().enumerate().skip(current) {
                for sql in *scripts {
                    tx.execute_batch(sql)
                        .await
                        .with_context(|| format!("native Home migration {}", index + 1))?;
                }
                tx.execute(format!("PRAGMA user_version={}", index + 1), ())
                    .await?;
            }
            tx.execute(format!("PRAGMA application_id={APPLICATION_ID}"), ())
                .await?;
            tx.commit().await?;
            File::open(path.parent().context("missing native Home directory")?)?.sync_all()?;
        }
        Ok(primary)
    }

    pub(crate) async fn connect(&self) -> Result<Connection> {
        let conn = self.db.connect()?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute("PRAGMA synchronous=FULL", ()).await?;
        ensure!(
            integer(&conn, "PRAGMA synchronous").await? == 2,
            "native Home requires synchronous=FULL"
        );
        conn.execute("PRAGMA temp_store=MEMORY", ()).await?;
        Ok(conn)
    }
}

/// Finish an ordered import into an unpublished database. The allocator in
/// Turso 0.8.4 is a sequence; changing sqlite_sequence alone does not seed it.
/// setval may reserve numbers across rollback. Reservations are never published
/// as domain rows, and callers discard a failed import before serving it.
pub(crate) async fn restore_high_water(
    conn: &Connection,
    stream: &str,
    through: i64,
) -> Result<()> {
    ensure!(
        !conn.is_autocommit()?,
        "restore high water inside the import transaction"
    );
    let sql = match stream {
        "events" => "SELECT setval('__turso_internal_autoincrement_events',?,1)",
        "dm_mail" => "SELECT setval('__turso_internal_autoincrement_dm_mail',?,1)",
        "private_group_events" => {
            "SELECT setval('__turso_internal_autoincrement_private_group_events',?,1)"
        }
        _ => anyhow::bail!("unknown native cursor stream {stream}"),
    };
    let high: i64 = conn
        .query(
            "SELECT high_water FROM native_cursor_fences WHERE stream=?",
            [stream],
        )
        .await?
        .next()
        .await?
        .context("missing native cursor fence")?
        .get(0)?;
    ensure!(
        through >= high,
        "restored high water predates imported {stream} rows"
    );
    if through > high {
        let mut rows = conn.query(sql, [through]).await?;
        let seeded: i64 = rows
            .next()
            .await?
            .context("missing sequence value")?
            .get(0)?;
        ensure!(
            seeded == through && rows.next().await?.is_none(),
            "native sequence restore failed"
        );
        drop(rows);
        ensure!(
            conn.execute(
                "UPDATE native_cursor_fences SET high_water=? WHERE stream=? AND high_water=?",
                (through, stream, high)
            )
            .await?
                == 1,
            "concurrent native cursor restore"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Each insert has the same shape as the domain writer and omits its cursor.
    const STREAMS: [(&str, &str, &str); 3] = [
        (
            "events",
            "INSERT INTO events(id,community,author,author_seq,envelope)
             VALUES(?1,'community',?1,1,'{}') RETURNING cursor",
            "SELECT id,cursor FROM events ORDER BY cursor",
        ),
        (
            "dm_mail",
            "INSERT INTO dm_mail(id,sender,recipient,created) VALUES(?1,'s','r',1) RETURNING seq",
            "SELECT id,seq FROM dm_mail ORDER BY seq",
        ),
        (
            "private_group_events",
            "INSERT INTO private_group_events(group_id,id,revision,session,ratchet_index,event)
             VALUES('group',?1,1,?1,1,'sealed') RETURNING cursor",
            "SELECT id,cursor FROM private_group_events ORDER BY cursor",
        ),
    ];

    async fn insert(conn: &Connection, sql: &str, id: &str) -> turso::Result<i64> {
        let mut rows = conn.query(sql, [id]).await?;
        let value = rows.next().await?.unwrap().get(0)?;
        assert!(rows.next().await?.is_none());
        Ok(value)
    }

    fn assert_conflict(error: turso::Error) {
        assert!(
            matches!(error, turso::Error::Busy(_) | turso::Error::BusySnapshot(_))
                || error
                    .to_string()
                    .to_ascii_lowercase()
                    .contains("write-write conflict"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn overlapping_inserts_cannot_publish_past_an_earlier_cursor() -> Result<()> {
        let dir = tempdir()?;
        let path = path_for(&dir.path().join("home.db"));
        assert_eq!(path.file_name().unwrap(), "home.db.primary.turso");
        let primary = Primary::open(&path).await?;
        for (stream, sql, page) in STREAMS {
            let a = primary.connect().await?;
            let b = primary.connect().await?;
            a.execute("BEGIN CONCURRENT", ()).await?;
            b.execute("BEGIN CONCURRENT", ()).await?;
            b.execute("INSERT INTO hidden VALUES(?)", [stream]).await?;
            let first = insert(&a, sql, "A").await?;
            // B attempts to publish first. On 0.8.4 the INSERT itself conflicts.
            assert_conflict(insert(&b, sql, "B").await.unwrap_err());
            if !b.is_autocommit()? {
                b.execute("ROLLBACK", ()).await?;
            }
            let reader = primary.connect().await?;
            assert!(reader.query(page, ()).await?.next().await?.is_none());
            a.execute("COMMIT", ()).await?;
            assert!(
                reader
                    .query("SELECT id FROM hidden WHERE id=?", [stream])
                    .await?
                    .next()
                    .await?
                    .is_none()
            );

            b.execute("BEGIN CONCURRENT", ()).await?;
            b.execute("INSERT INTO hidden VALUES(?)", [stream]).await?;
            let second = insert(&b, sql, "B").await?;
            b.execute("COMMIT", ()).await?;
            assert!(second > first);
            let mut rows = reader.query(page, ()).await?;
            for (id, cursor) in [("A", first), ("B", second)] {
                let row = rows.next().await?.unwrap();
                assert_eq!(row.get::<String>(0)?, id);
                assert_eq!(row.get::<i64>(1)?, cursor);
            }
            assert!(rows.next().await?.is_none());
        }
        Ok(())
    }

    #[tokio::test]
    async fn independent_streams_and_ordinary_writes_commit_in_reverse_order() -> Result<()> {
        let dir = tempdir()?;
        let primary = Primary::open(&path_for(&dir.path().join("home"))).await?;
        let mut writers = Vec::new();
        for (_, sql, _) in STREAMS {
            let conn = primary.connect().await?;
            conn.execute("BEGIN CONCURRENT", ()).await?;
            insert(&conn, sql, "independent").await?;
            writers.push(conn);
        }
        for id in ["one", "two"] {
            let conn = primary.connect().await?;
            conn.execute("BEGIN CONCURRENT", ()).await?;
            conn.execute("INSERT INTO hidden VALUES(?)", [id]).await?;
            writers.push(conn);
        }
        for conn in writers.into_iter().rev() {
            conn.execute("COMMIT", ()).await?;
        }
        let reader = primary.connect().await?;
        assert_eq!(integer(&reader, "SELECT count(*) FROM hidden").await?, 2);
        for (_, _, page) in STREAMS {
            assert_eq!(
                reader
                    .query(page, ())
                    .await?
                    .next()
                    .await?
                    .unwrap()
                    .get::<String>(0)?,
                "independent"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn push_baseline_retries_when_its_source_advanced() -> Result<()> {
        for (category, index) in [("reply", 0), ("message", 1), ("request", 1)] {
            let dir = tempdir()?;
            let primary = Primary::open(&path_for(&dir.path().join("home"))).await?;
            let subscriber = primary.connect().await?;
            let producer = primary.connect().await?;
            let baseline = if index == 0 {
                "SELECT COALESCE(MAX(cursor),0) FROM events"
            } else {
                "SELECT COALESCE(MAX(seq),0) FROM dm_mail"
            };
            subscriber.execute("BEGIN CONCURRENT", ()).await?;
            assert_eq!(integer(&subscriber, baseline).await?, 0);
            producer.execute("BEGIN CONCURRENT", ()).await?;
            let cursor = insert(&producer, STREAMS[index].1, "first").await?;
            producer.execute("COMMIT", ()).await?;
            let sql = "INSERT OR IGNORE INTO push_starts VALUES('owner','device',?,?)";
            assert_conflict(subscriber.execute(sql, (category, 0)).await.unwrap_err());
            if !subscriber.is_autocommit()? {
                subscriber.execute("ROLLBACK", ()).await?;
            }
            subscriber.execute("BEGIN CONCURRENT", ()).await?;
            let latest = integer(&subscriber, baseline).await?;
            subscriber.execute(sql, (category, latest)).await?;
            subscriber.execute("COMMIT", ()).await?;
            assert_eq!(latest, cursor);
        }
        Ok(())
    }

    #[tokio::test]
    async fn restored_deleted_tail_survives_reopen_for_every_stream() -> Result<()> {
        let dir = tempdir()?;
        let path = path_for(&dir.path().join("restored"));
        {
            let primary = Primary::open(&path).await?;
            let tx = primary.connect().await?;
            tx.execute("BEGIN IMMEDIATE", ()).await?;
            for (stream, sql, _) in STREAMS {
                insert(&tx, sql, "preserved").await?;
                restore_high_water(&tx, stream, 90).await?;
                assert!(restore_high_water(&tx, stream, 89).await.is_err());
            }
            tx.execute("COMMIT", ()).await?;
        }
        let primary = Primary::open(&path).await?;
        let tx = primary.connect().await?;
        assert_eq!(mode(&tx, "PRAGMA journal_mode").await?, "mvcc");
        tx.execute("BEGIN CONCURRENT", ()).await?;
        for (_, sql, _) in STREAMS {
            assert!(insert(&tx, sql, "after-restore").await? > 90);
        }
        tx.execute("COMMIT", ()).await?;
        for file in fs::read_dir(dir.path())? {
            let file = file?;
            assert_eq!(
                file.metadata()?.permissions().mode() & 0o077,
                0,
                "{:?}",
                file.path()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/migrations/native_feed.rs"]
mod feed_migration_tests;
