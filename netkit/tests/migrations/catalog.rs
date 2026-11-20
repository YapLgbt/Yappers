// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::store::Store;
use tempfile::tempdir;

#[test]
fn legacy_backfill_runs_once_and_preserves_external_refs() -> Result<()> {
    let mut db = Connection::open_in_memory()?;
    baseline(&db)?;
    db.execute_batch(
        "INSERT INTO peers VALUES('peer','generation',7);
         INSERT INTO account_states VALUES('owner','[]');
         CREATE TABLE home_opening_refs(handle BLOB);
         INSERT INTO home_opening_refs VALUES(x'00010203');",
    )?;
    apply(&mut db)?;
    assert_eq!(version(&db)?, CURRENT_VERSION);
    assert_eq!(
        db.query_row("SELECT scope FROM peers WHERE cursor=7", [], |r| r
            .get::<_, String>(0))?,
        ""
    );
    assert!(
        db.query_row(
            "SELECT since FROM first_seen WHERE owner='owner'",
            [],
            |r| r.get::<_, i64>(0)
        )? > 0
    );
    // The previous server recorded version 1 even after adding scope and held.
    // Removing an observation makes an accidental second backfill visible.
    db.execute_batch("DELETE FROM first_seen; UPDATE schema_version SET version=1")?;
    apply(&mut db)?;
    apply(&mut db)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM first_seen", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        db.query_row("SELECT handle FROM home_opening_refs", [], |r| r
            .get::<_, Vec<u8>>(0))?,
        [0, 1, 2, 3]
    );
    Ok(())
}

#[test]
fn failed_upgrade_rolls_back_ddl_and_retries() -> Result<()> {
    let mut db = Connection::open_in_memory()?;
    baseline(&db)?;
    db.execute_batch(
        "INSERT INTO personal_events VALUES('owner','old','{\"event\":{\"payload\":{\"version\":3}}}');",
    )?;
    assert!(apply(&mut db).is_err());
    assert_eq!(version(&db)?, 1);
    assert!(!has_column(&db, "peers", "scope")?);
    assert!(!has_column(&db, "dm_mail", "held")?);
    db.execute("DELETE FROM personal_events", [])?;
    apply(&mut db)?;
    assert_eq!(version(&db)?, CURRENT_VERSION);
    Ok(())
}

#[test]
fn backup_version_check_works_on_read_only_connections() -> Result<()> {
    let tmp = tempdir()?;
    let path = tmp.path().join("metadata");
    let writer = Connection::open(&path)?;
    writer.execute_batch("CREATE TABLE schema_version(version INTEGER NOT NULL)")?;
    let reader = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(Store::check_backup_version(&reader).is_err());
    for v in 1..=CURRENT_VERSION {
        writer.execute("DELETE FROM schema_version", [])?;
        writer.execute("INSERT INTO schema_version VALUES(?)", [v])?;
        Store::check_backup_version(&reader)?;
    }
    writer.execute("INSERT INTO schema_version VALUES(1)", [])?;
    assert!(Store::check_backup_version(&reader).is_err());
    writer.execute("UPDATE schema_version SET version=?", [CURRENT_VERSION + 1])?;
    writer.execute(
        "DELETE FROM schema_version WHERE rowid=(SELECT min(rowid) FROM schema_version)",
        [],
    )?;
    assert!(Store::check_backup_version(&reader).is_err());
    Ok(())
}

#[test]
fn unrelated_unversioned_databases_are_left_alone() -> Result<()> {
    for ddl in [
        "CREATE TABLE peers(id TEXT PRIMARY KEY,generation TEXT NOT NULL,cursor TEXT NOT NULL)",
        "CREATE TABLE peers(id TEXT PRIMARY KEY,generation TEXT NOT NULL,cursor INTEGER NOT NULL);
         CREATE TABLE unrelated(value TEXT)",
        "CREATE TABLE peers(id TEXT PRIMARY KEY,generation TEXT NOT NULL,cursor INTEGER NOT NULL CHECK(cursor >= 0))",
        "PRAGMA application_id=401; CREATE TABLE peers(id TEXT PRIMARY KEY,generation TEXT NOT NULL,cursor INTEGER NOT NULL)",
        "CREATE VIEW peers AS SELECT 1 AS cursor",
    ] {
        let mut db = Connection::open_in_memory()?;
        db.execute_batch(ddl)?;
        let before = db.serialize(MAIN_DB)?.to_vec();
        assert!(apply(&mut db).is_err(), "adopted {ddl}");
        assert_eq!(db.serialize(MAIN_DB)?.as_ref(), before);
    }
    Ok(())
}

#[test]
fn a_legacy_guard_keeps_its_keys_and_a_newer_guard_refuses_startup_and_backup() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("home");
    drop(Store::open_legacy(&path)?);
    let guard = Connection::open(path.with_added_extension("deletion-guard"))?;
    let key = || guard.query_row("SELECT key FROM state", [], |r| r.get::<_, Vec<u8>>(0));
    let original = key()?;
    // Recreate the pre-versioned guard layout, keeping the real backup KEK.
    guard.execute_batch("DROP TRIGGER native_legacy_epoch; DROP TABLE native_watermark")?;
    guard.pragma_update(None, "user_version", 0)?;
    drop(Store::open(&path)?);
    assert_eq!(guard_version(&guard, "main")?, GUARD_STEPS.len());
    assert_eq!(key()?, original);
    let locator = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    assert!(locator.table_exists(Some(MAIN_DB), c"home_native_locator")?);
    assert!(!locator.table_exists(Some(MAIN_DB), c"events")?);
    // Older catalog readers refuse the locator. The public backup entry point
    // resolves it to the current native primary and guard.
    assert!(Store::check_backup_version(&locator).is_err());
    let supported = dir.path().join("supported.backup");
    Store::backup_existing(&path, &supported)?;
    assert!(supported.metadata()?.len() > 0);
    drop(locator);

    guard.pragma_update(None, "user_version", GUARD_STEPS.len() as i64 + 1)?;
    assert!(
        Store::open(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("guard schema")
    );
    let archive = dir.path().join("archive");
    assert!(Store::backup_existing(&path, &archive).is_err());
    assert!(!archive.exists());
    assert_eq!(key()?, original);
    Ok(())
}
