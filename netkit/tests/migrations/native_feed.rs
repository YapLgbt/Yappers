// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use tempfile::tempdir;

const POST_FILTER: &str = "SELECT id FROM events WHERE community='nearby'
    AND NOT EXISTS(SELECT 1 FROM hidden WHERE hidden.id=events.id)
    AND json_extract(envelope,'$.event.payload.action.kind')='erased'
    AND json_extract(envelope,'$.event.payload.action.reply')=0
    AND json_extract(envelope,'$.deletion') IS NULL";
const POST_ORDER: &str = "ORDER BY json_extract(envelope,'$.acceptedAt') DESC,id DESC LIMIT 2";
const REPLIES: &str = "SELECT e.id,o.routing FROM events e LEFT JOIN home_openings o
    ON o.target=e.id AND o.private=0 WHERE e.community='nearby' AND e.cursor>3 AND e.cursor<=4
    AND json_extract(e.envelope,'$.event.payload.action.kind')='erased'
    AND json_extract(e.envelope,'$.event.payload.action.reply')=1
    AND json_extract(e.envelope,'$.deletion') IS NULL ORDER BY e.cursor";
const REPLY_TIP: &str = "SELECT cursor FROM events WHERE community='nearby'
    AND json_extract(envelope,'$.event.payload.action.kind')='erased'
    AND json_extract(envelope,'$.event.payload.action.reply')=1
    AND json_extract(envelope,'$.deletion') IS NULL ORDER BY cursor DESC LIMIT 1";
const DELETED: &str = "SELECT id FROM events WHERE community='nearby' AND id>'a'
    AND json_extract(envelope,'$.deletion') IS NOT NULL ORDER BY id LIMIT 2";

async fn strings(conn: &Connection, sql: &str, column: usize) -> Result<Vec<String>> {
    let mut rows = conn.query(sql, ()).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(row.get(column)?);
    }
    Ok(out)
}

#[tokio::test]
async fn populated_v1_feed_indexes_backfill_and_track_committed_changes() -> Result<()> {
    let dir = tempdir()?;
    let path = path_for(&dir.path().join("home"));
    let posts = format!("{POST_FILTER} {POST_ORDER}");
    let next_posts = format!(
        "{POST_FILTER} AND json_extract(envelope,'$.acceptedAt')<=30
        AND (json_extract(envelope,'$.acceptedAt')<30 OR id<'c') {POST_ORDER}"
    );
    // Build the historical primary from its frozen scripts without opening it
    // through the current runner until its v1 rows are durably committed.
    {
        let db = turso::Builder::new_local(path.to_str().unwrap())
            .build()
            .await?;
        let conn = db.connect()?;
        assert_eq!(mode(&conn, "PRAGMA journal_mode=mvcc").await?, "mvcc");
        conn.execute("BEGIN IMMEDIATE", ()).await?;
        for ddl in MIGRATIONS[0] {
            conn.execute_batch(ddl).await?;
        }
        conn.execute(format!("PRAGMA application_id={APPLICATION_ID}"), ())
            .await?;
        conn.execute("PRAGMA user_version=1", ()).await?;
        for (id, scope, action, at) in [
            ("b", "nearby", r#"{"kind":"erased","reply":false}"#, 30),
            ("c", "nearby", r#"{"kind":"erased","reply":false}"#, 30),
            ("early", "nearby", r#"{"kind":"erased","reply":true}"#, 90),
            ("later", "nearby", r#"{"kind":"erased","reply":true}"#, 60),
            ("a", "nearby", r#"{"kind":"erased","reply":false}"#, 17),
            ("other", "far", r#"{"kind":"erased","reply":false}"#, 80),
            ("foreign", "far", r#"{"kind":"erased","reply":true}"#, 95),
            ("z", "nearby", r#"{"kind":"erased","reply":false}"#, 80),
            ("hidden", "nearby", r#"{"kind":"erased","reply":false}"#, 99),
        ] {
            conn.execute(
                "INSERT INTO events(id,community,author,author_seq,envelope)
                VALUES(?1,?2,?1,1,json_object('acceptedAt',?3,'event',
                  json_object('payload',json_object('action',json(?4)))))",
                (id, scope, at, action),
            )
            .await?;
        }
        conn.execute(
            "INSERT INTO events(id,community,author,author_seq,envelope)
            SELECT 'tip',community,'tip',1,envelope FROM events WHERE id='later'",
            (),
        )
        .await?;
        conn.execute(
            "UPDATE events SET envelope=json_set(envelope,'$.deletion',json('{}')) WHERE id='z'",
            (),
        )
        .await?;
        conn.execute("INSERT INTO hidden VALUES('hidden')", ())
            .await?;
        conn.execute("COMMIT", ()).await?;
        assert_eq!(integer(&conn, "PRAGMA user_version").await?, 1);
    }
    for entry in fs::read_dir(dir.path())? {
        fs::set_permissions(entry?.path(), fs::Permissions::from_mode(0o600))?;
    }

    {
        let primary = Primary::open(&path).await?;
        let conn = primary.connect().await?;
        assert_eq!(version(&conn).await?, MIGRATIONS.len());
        assert_eq!(
            integer(
                &conn,
                "SELECT count(*) FROM sqlite_schema WHERE name='events_reply_parent'"
            )
            .await?,
            0
        );
        for (sql, searches) in [
            (
                posts.as_str(),
                &["feed_posts (community=?)", "hidden_1 (id=?)"][..],
            ),
            (
                next_posts.as_str(),
                &["feed_posts (community=? AND json_extract"][..],
            ),
            (
                REPLIES,
                &[
                    "feed_replies (community=? AND cursor>? AND cursor<=?)",
                    "home_openings_1 (target=? AND private=?) LEFT-JOIN",
                ][..],
            ),
            (REPLY_TIP, &["feed_replies (community=?)"][..]),
            (DELETED, &["erased_by_community (community=? AND id>?)"][..]),
        ] {
            let plan = strings(&conn, &format!("EXPLAIN QUERY PLAN {sql}"), 3).await?;
            for search in searches {
                assert!(
                    plan.iter()
                        .any(|step| step.starts_with("SEARCH ") && step.contains(search)),
                    "expected {search} in {plan:?}"
                );
            }
            assert!(
                plan.iter().all(|step| !step.contains("TEMP B-TREE")),
                "{plan:?}"
            );
        }
        assert_eq!(strings(&conn, &posts, 0).await?, ["c", "b"]);
        assert_eq!(strings(&conn, &next_posts, 0).await?, ["b", "a"]);
        assert_eq!(strings(&conn, REPLIES, 0).await?, ["later"]);
        assert_eq!(
            integer(&conn, REPLY_TIP).await?,
            integer(&conn, "SELECT cursor FROM events WHERE id='tip'").await?
        );
        assert_eq!(strings(&conn, DELETED, 0).await?, ["z"]);

        let redact = "UPDATE events SET envelope=json_set(envelope,'$.deletion',json('{}')) WHERE id IN ('c','later')";
        conn.execute("BEGIN CONCURRENT", ()).await?;
        conn.execute(redact, ()).await?;
        let reader = primary.connect().await?;
        assert_eq!(strings(&reader, &posts, 0).await?, ["c", "b"]);
        conn.execute("ROLLBACK", ()).await?;
        assert_eq!(strings(&conn, REPLIES, 0).await?, ["later"]);
        conn.execute("BEGIN CONCURRENT", ()).await?;
        conn.execute(redact, ()).await?;
        conn.execute(
            "INSERT INTO events(id,community,author,author_seq,envelope)
            SELECT 'new',community,'new',1,json_set(envelope,'$.acceptedAt',40)
            FROM events WHERE id='a'",
            (),
        )
        .await?;
        conn.execute("COMMIT", ()).await?;
    }
    let primary = Primary::open(&path).await?;
    let conn = primary.connect().await?;
    assert_eq!(strings(&conn, &posts, 0).await?, ["new", "b"]);
    assert!(strings(&conn, REPLIES, 0).await?.is_empty());
    assert_eq!(strings(&conn, DELETED, 0).await?, ["c", "later"]);
    Ok(())
}
