// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    model::{Action, Envelope, SignedEvent, new_secret, signing_key},
    portability::AccountWallet,
};
use std::collections::BTreeSet;

fn event(secret: &str, sequence: u64, action: Action) -> Result<Envelope> {
    let key = signing_key(secret)?;
    let signed = SignedEvent::create(
        &key,
        hex::encode(key.verifying_key().as_bytes()),
        "nearby".into(),
        sequence,
        action,
    )?;
    Envelope::accept(signed, &key)
}

fn post(body: &str) -> Action {
    Action::Post {
        body: body.into(),
        anonymous: true,
        display_name: "Anonymous neighbor".into(),
        topic: "Plans".into(),
    }
}

#[test]
fn portable_format_covers_every_migrated_domain_table_and_column() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let portable = Store::open_legacy(&tmp.path().join("sqlite"))?;
    let native = Store::open(&tmp.path().join("native"))?;
    let custom = [
        ("events", "cursor,id,community,author,author_seq,envelope"),
        ("personal_events", "account,event_id,envelope"),
        ("deletions", "target,owner,community,private,proof"),
        ("username_receipts", "event_id,receipt"),
        (
            "public_projections",
            "community,digest,posts,event_count,verified",
        ),
    ];
    for store in [&portable, &native] {
        let db = &store.db;
        let tables = db
            .prepare(
                "SELECT name FROM main.sqlite_schema WHERE type='table'
            AND name NOT GLOB 'sqlite_*' AND name NOT GLOB '__turso_internal_*'",
            )?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        // All application-owned tables must be copied, transformed or explicitly
        // classified as engine-specific metadata. Never ignore a Home name prefix.
        let metadata: &[&str] = if store.is_native() {
            &[
                "feed_event_counts",
                "feed_device_votes",
                "feed_owner_votes",
                "feed_vote_scores",
                "native_cursor_fences",
                "native_guard_head",
                "native_guard_outbox",
                "native_guard_stage",
                "home_openings",
                "home_import_complete",
            ]
        } else {
            &["schema_version", "deletion_epoch"]
        };
        let mapped = DOMAIN
            .iter()
            .chain(&custom)
            .map(|(table, _)| *table)
            .chain(metadata.iter().copied())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        assert_eq!(tables, mapped);
        for &(table, columns) in DOMAIN.iter().chain(&custom) {
            let actual = db
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<BTreeSet<_>>>()?;
            let copied = columns
                .split(',')
                .filter(|column| *column != "rowid")
                .map(str::to_owned)
                .collect();
            assert_eq!(actual, copied, "{table}");
        }
        let sql = if store.is_native() {
            "SELECT stream FROM native_cursor_fences"
        } else {
            "SELECT name FROM main.sqlite_schema WHERE type='table' AND upper(sql) LIKE '%AUTOINCREMENT%'"
        };
        let streams = db
            .prepare(sql)?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        assert_eq!(streams, STREAMS.into_iter().map(str::to_owned).collect());
    }
    Ok(())
}

#[test]
fn native_restart_and_seed_keep_rows_and_deleted_tail_cursors() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("legacy");
    let mut old = Store::open_legacy(&path)?;
    let secret = new_secret();
    let public = event(&secret, 1, post("live public portable body"))?;
    let personal = event(&secret, 2, post("live private portable body"))?;
    old.accept(&public)?;
    old.import_personal_events(personal.event.owner_id(), std::slice::from_ref(&personal))?;
    old.db.execute_batch(
        "INSERT INTO peers VALUES('neighbor','generation',123,'nearby');
        INSERT INTO dm_mail VALUES(39,'packet','sender','recipient',NULL,12,1);
        INSERT INTO private_group_events VALUES(73,'room','id',8,'session',7,NULL,'opaque');
        INSERT INTO private_group_rosters VALUES('room',8,'encrypted membership');
        INSERT INTO peer_deliveries VALUES('neighbor','nearby','target');
        INSERT INTO push_devices VALUES('account','device','web','subscription','[]');
        INSERT INTO push_starts VALUES('account','device','reply',23);
        INSERT INTO push_routes VALUES('token','account','device','reply','target',1,7,123);
        UPDATE inbox_arrivals SET received=34;
        UPDATE sqlite_sequence SET seq=seq+100;",
    )?;
    let mut cached = crate::model::project(
        std::slice::from_ref(&public),
        "nearby",
        "",
        &BTreeSet::new(),
    );
    cached[0].author = "legacy cached byline".into();
    old.db.execute(
        "INSERT INTO public_projections VALUES('nearby','digest',?,1,10)",
        [serde_json::to_string(&cached)?],
    )?;
    drop(old);
    drop(Store::open(&path)?);
    let native = Store::open(&path)?;
    assert!(native.is_native());
    let sealed:String=native.db.query_row("SELECT posts FROM public_projections WHERE digest='digest' AND event_count=1 AND verified=10", [], |r|r.get(0))?;
    assert_eq!(
        crate::native_projection::decode(&native, "nearby", &sealed)?,
        cached
    );
    let seed = native.snapshot()?;
    let mut stage = Store::from_sqlite(Connection::open_in_memory()?);
    stage.restore_snapshot(&seed)?;
    assert_eq!(stage.existing(&public.event.id)?, Some(public.clone()));
    assert_eq!(
        stage.scoped_cursor("neighbor", "nearby")?,
        (Some("generation".to_owned()), 123)
    );
    assert!(
        stage
            .owned_events(personal.event.owner_id())?
            .contains(&personal)
    );
    for (table, column, value) in [
        ("inbox_arrivals", "received", 34),
        ("private_group_events", "cursor", 73),
        ("dm_mail", "seq", 39),
        ("private_group_rosters", "revision", 8),
        ("push_starts", "baseline", 23),
        ("push_routes", "retry_at", 123),
    ] {
        assert_eq!(
            stage
                .db
                .query_row(&format!("SELECT {column} FROM {table}"), [], |r| r
                    .get::<_, i64>(0))?,
            value
        );
    }
    for (stream, expected) in [
        ("events", 101),
        ("dm_mail", 139),
        ("private_group_events", 173),
    ] {
        assert_eq!(
            stage.db.query_row(
                "SELECT seq FROM sqlite_sequence WHERE name=?",
                [stream],
                |r| r.get::<_, i64>(0)
            )?,
            expected
        );
    }
    assert_eq!(
        stage
            .db
            .query_row("SELECT target FROM peer_deliveries", [], |r| r
                .get::<_, String>(0))?,
        "target"
    );
    assert_eq!(
        stage
            .db
            .query_row("SELECT count(*) FROM public_projections", [], |r| r
                .get::<_, i64>(0))?,
        0
    );
    drop(native);
    let native = Store::open(&path)?;
    for entry in fs::read_dir(tmp.path())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        for marker in [
            "live public portable body",
            "live private portable body",
            "legacy cached byline",
        ] {
            assert!(
                !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
                "plaintext in {}",
                entry.path().display()
            );
        }
    }
    assert_eq!(native.existing(&public.event.id)?, Some(public));
    Ok(())
}

#[test]
fn historical_managed_archive_reconciles_later_deletion_revision_and_delivery() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("original");
    let guard = path.with_added_extension("deletion-guard");
    let file = tmp.path().join("version-one.backup");
    let dest = tmp.path().join("restored");
    let mut old = Store::open_legacy(&path)?;
    let secret = new_secret();
    let old_post = event(&secret, 1, post("retired before native adoption"))?;
    let live = event(&secret, 2, post("surviving historical body"))?;
    let deleted = event(
        &secret,
        3,
        Action::Delete {
            target: old_post.event.id.clone(),
        },
    )?;
    let wallet = AccountWallet::create(new_secret(), old_post.event.payload.home.clone())?;
    let revised = wallet.transition(old_post.event.payload.home.clone(), true)?;
    old.register_account(&wallet.chain)?;
    old.accept(&old_post)?;
    old.accept(&live)?;
    old.import_personal_events(live.event.owner_id(), std::slice::from_ref(&old_post))?;
    old.backup(&file)?;
    // The first managed format kept schema_version=1 even as optional tables
    // were added. Its archive used the same guard key and per-event ciphertext.
    let key: Vec<u8> = old
        .db
        .query_row("SELECT key FROM guard.state", [], |r| r.get(0))?;
    let protected = fs::read(&file)?;
    let bytes = backup::unseal(protected.strip_prefix(backup::HEADER).unwrap(), &key)?;
    let mut v1 = Connection::open_in_memory()?;
    v1.deserialize_read_exact(MAIN_DB, bytes.as_slice(), bytes.len(), false)?;
    v1.execute_batch("UPDATE schema_version SET version=1; DROP TABLE peer_deliveries; DROP TABLE inbox_reads; DROP TABLE push_routes;")?;
    fs::write(
        &file,
        [
            backup::HEADER,
            &backup::seal(&v1.serialize(MAIN_DB)?, &key)?,
        ]
        .concat(),
    )?;
    drop(old);
    let mut source = Store::open(&path)?;
    source.accept(&deleted)?;
    source.register_account(&revised.chain)?;
    source.operation(|s| {
        s.db.execute(
            "INSERT INTO peer_deliveries VALUES('peer','nearby',?)",
            [&live.event.id],
        )?;
        crate::native_guard::stage_delivery(&s.db, "peer", "nearby", &live.event.id)
    })?;
    drop(source);

    let wrong = tmp.path().join("other");
    drop(Store::open(&wrong)?);
    for bad in [
        tmp.path().join("missing_guard"),
        wrong.with_added_extension("deletion-guard"),
    ] {
        assert!(restore(&bad, &file, &dest).is_err());
        assert!(!dest.exists());
    }
    restore(&guard, &file, &dest)?;
    let restored = Store::open(&dest)?;
    assert!(restored.is_native());
    assert_eq!(restored.existing(&live.event.id)?, Some(live.clone()));
    assert_eq!(
        restored.existing(&old_post.event.id)?,
        Some(old_post.erased(&deleted)?)
    );
    assert_eq!(
        restored.account_chain(&wallet.chain[0].payload.account)?,
        revised.chain
    );
    let delivered: String = restored.db.query_row(
        "SELECT target FROM peer_deliveries WHERE peer='peer'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(delivered, live.event.id);
    assert!(
        restored
            .owned_events(live.event.owner_id())?
            .contains(&old_post.erased(&deleted)?)
    );
    let new_file = tmp.path().join("native.backup");
    restored.backup(&new_file)?;
    drop(restored);
    let second = tmp.path().join("second");
    restore(&guard, &new_file, &second)?;
    assert_eq!(Store::open(&second)?.existing(&live.event.id)?, Some(live));
    Ok(())
}

#[test]
fn restore_uses_a_version_zero_guard_without_a_delivery_table() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("old-home");
    let archive = tmp.path().join("old-archive");
    let mut legacy = Store::open_legacy(&path)?;
    let live = event(&new_secret(), 1, post("old managed content"))?;
    legacy.accept(&live)?;
    legacy.backup(&archive)?;
    drop(legacy);

    let guard = path.with_added_extension("deletion-guard");
    let old = Connection::open(&guard)?;
    old.execute_batch(
        "DROP TRIGGER IF EXISTS native_legacy_epoch;
        DROP TABLE native_watermark; DROP TABLE known_deliveries; PRAGMA user_version=0",
    )?;
    let key: Vec<u8> = old.query_row(
        "SELECT key FROM opening_keys WHERE target=?",
        [&live.event.id],
        |r| r.get(0),
    )?;
    drop(old);

    let missing_key = tmp.path().join("missing-key");
    fs::copy(&guard, &missing_key)?;
    Connection::open(&missing_key)?.execute("DELETE FROM opening_keys", [])?;
    let dest = tmp.path().join("recovered");
    assert!(restore(&missing_key, &archive, &dest).is_err());
    assert!(!dest.exists());
    assert!(!dest.with_added_extension("deletion-guard").exists());
    assert!(!native_primary::path_for(&dest).exists());
    restore(&guard, &archive, &dest)?;
    let native = Store::open(&dest)?;
    assert_eq!(native.existing(&live.event.id)?, Some(live.clone()));
    let retained: Vec<u8> = Connection::open(&guard)?.query_row(
        "SELECT key FROM opening_keys WHERE target=?",
        [&live.event.id],
        |r| r.get(0),
    )?;
    assert_eq!(retained, key);
    Ok(())
}
