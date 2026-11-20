// SPDX-License-Identifier: AGPL-3.0-only
//! One process owns each MVCC file. Independent connections share that owner.
//! The tiny catalog locator is published only after a durable native import.
use super::Store;
use crate::{
    backup,
    home_sql::HomeDb,
    native_guard::{self, NativeGuard},
    native_primary::Primary,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::{
    collections::HashMap,
    fs::{self, File},
    future::Future,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, OnceLock, Weak},
    time::Duration,
};

const LOCATOR: &str = "CREATE TABLE home_native_locator(singleton INTEGER PRIMARY KEY CHECK(singleton=1), id TEXT NOT NULL);
    CREATE TABLE schema_version(version INTEGER NOT NULL); INSERT INTO schema_version VALUES(2147483647)";
static HOMES: LazyLock<Mutex<HashMap<PathBuf, Weak<NativeHome>>>> = LazyLock::new(Mutex::default);

pub(crate) struct NativeHome {
    pub(crate) primary: Primary,
    pub(crate) guard: Arc<NativeGuard>,
    pub(crate) path: PathBuf,
    pub(crate) catalog_id: String,
    pub(crate) disclosure: Arc<crate::server::DisclosureFence>,
    pub(crate) feed_replies: crate::feed_replies::FeedReplies,
    admin: OnceLock<crate::native_admin::Admin>,
    _owner: File,
}

fn wait<F: Future>(future: F) -> F::Output {
    futures_lite::future::block_on(tokio::task::unconstrained(future))
}

fn canonical(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let path = std::path::absolute(path)?;
    Ok(path
        .parent()
        .context("missing Home directory")?
        .canonicalize()?
        .join(path.file_name().context("invalid Home file name")?))
}

fn read_marker(path: &Path, locator: bool) -> Result<Option<String>> {
    if !path.exists() || path.metadata()?.len() == 0 {
        return Ok(None);
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let table = if locator {
        c"home_native_locator"
    } else {
        c"native_adoption"
    };
    if !db.table_exists(Some(rusqlite::MAIN_DB), table)? {
        return Ok(None);
    }
    let sql = format!("SELECT id FROM {} WHERE singleton=1", table.to_str()?);
    let id: String = db.query_row(&sql, [], |r| r.get(0))?;
    ensure!(
        id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid Home locator"
    );
    Ok(Some(id))
}

fn owner_lock(path: &Path) -> Result<File> {
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.with_added_extension("native-owner"))?;
    lock.try_lock()
        .context("another process owns this Turso Home")?;
    Ok(lock)
}

fn write_locator(db: &mut Connection, id: &str) -> Result<()> {
    db.execute_batch("PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let objects = tx.prepare("SELECT type,name FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'
        AND type IN ('trigger','view','table') ORDER BY CASE type WHEN 'trigger' THEN 0 WHEN 'view' THEN 1 ELSE 2 END")?
        .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (kind, name) in objects {
        tx.execute_batch(&format!(
            "DROP {} IF EXISTS \"{}\"",
            kind,
            name.replace('"', "\"\"")
        ))?;
    }
    tx.execute_batch(LOCATOR)?;
    tx.execute("INSERT INTO home_native_locator VALUES(1,?)", [id])?;
    tx.commit()?;
    db.execute_batch("VACUUM")?;
    Ok(())
}

impl NativeHome {
    pub(crate) fn primary_path(path: &Path) -> PathBuf {
        crate::native_primary::path_for(path)
    }
    pub(crate) fn is_locator(path: &Path) -> Result<bool> {
        Ok(read_marker(path, true)?.is_some())
    }

    fn own(path: &Path, guard_path: &Path, owner: File, catalog_id: &str) -> Result<Arc<Self>> {
        let guard = Arc::new(NativeGuard::open(guard_path)?);
        let primary = wait(Primary::open(&Self::primary_path(path)))?;
        Ok(Arc::new(Self {
            primary,
            guard,
            path: path.to_owned(),
            catalog_id: catalog_id.to_owned(),
            disclosure: Arc::default(),
            feed_replies: Default::default(),
            admin: OnceLock::new(),
            _owner: owner,
        }))
    }

    pub(crate) fn connection(self: &Arc<Self>) -> Result<Store> {
        Ok(Store {
            db: HomeDb::from_native(wait(self.primary.connect())?, self.clone()),
        })
    }

    pub(crate) fn open(path: &Path) -> Result<Store> {
        let path = canonical(path)?;
        let mut homes = HOMES
            .lock()
            .map_err(|_| anyhow::anyhow!("Home owner registry unavailable"))?;
        homes.retain(|_, home| home.strong_count() != 0);
        if let Some(home) = homes.get(&path).and_then(Weak::upgrade) {
            let store = home.connection()?;
            store.verify_authority()?;
            return Ok(store);
        }
        let owner = owner_lock(&path)?;
        let guard_path = path.with_added_extension("deletion-guard");
        let primary_path = Self::primary_path(&path);
        let (home, store) = if let Some(id) = read_marker(&path, true)? {
            ensure!(
                primary_path.is_file() && primary_path.metadata()?.len() > 0,
                "missing native Home primary"
            );
            let home = Self::own(&path, &guard_path, owner, &id)?;
            let store = home.connection()?;
            store.verify_binding(&id)?;
            native_guard::reconcile(&store.db)?;
            native_guard::recover_opening_reservations(&store.db, None)?;
            (home, store)
        } else {
            let existing_guard_lock = guard_path
                .try_exists()?
                .then(|| backup::lock_authority(&path))
                .transpose()?;
            let pending = read_marker(&path, false)?;
            ensure!(
                pending.is_some() || !primary_path.try_exists()?,
                "unrecognized native Home destination"
            );
            // A restart after the native commit can see an advanced guard. Resume
            // from the matching durable import receipt before opening the old Home.
            if pending.is_none() {
                let empty = !path.exists() || path.metadata()?.len() == 0;
                if empty {
                    ensure!(
                        !guard_path.try_exists()?,
                        "missing catalog for existing deletion guard"
                    );
                    File::options()
                        .create(true)
                        .truncate(false)
                        .write(true)
                        .mode(0o600)
                        .open(&path)?;
                } else {
                    // Validate a mainline catalog before recording any conversion intent.
                    drop(Store::open_legacy(&path)?);
                }
                let mut db = Connection::open(&path)?;
                db.execute_batch("PRAGMA synchronous=EXTRA; PRAGMA secure_delete=ON")?;
                let tx = db.transaction()?;
                tx.execute_batch(
                    "CREATE TABLE native_adoption(singleton INTEGER PRIMARY KEY,id TEXT NOT NULL)",
                )?;
                if empty {
                    tx.execute_batch("CREATE TABLE native_fresh(singleton INTEGER PRIMARY KEY)")?;
                }
                tx.execute(
                    "INSERT INTO native_adoption VALUES(1,?)",
                    [crate::model::new_secret()],
                )?;
                tx.commit()?;
            }
            let mut binding = Connection::open(&path)?;
            let fresh = binding.table_exists(Some(rusqlite::MAIN_DB), c"native_fresh")?;
            if fresh {
                // Atomically bind the new guard and locator without creating a legacy Home.
                backup::attach(&mut binding, &path)?;
            }
            drop(binding);
            let _quiescent = match existing_guard_lock {
                Some(lock) => lock,
                None => backup::lock_authority(&path)?,
            };
            let id = read_marker(&path, false)?.context("missing adoption record")?;
            let home = Self::own(&path, &guard_path, owner, &id)?;
            let mut store = home.connection()?;
            let prior: Option<String> = store
                .db
                .query_row(
                    "SELECT id FROM home_import_complete WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(prior) = prior {
                ensure!(prior == id, "native Home belongs to another catalog");
                native_guard::reconcile(&store.db)?;
                native_guard::recover_opening_reservations(&store.db, None)?;
            } else if fresh {
                native_guard::initialize_head(&store.db)?;
                store
                    .db
                    .execute("INSERT INTO home_import_complete VALUES(1,?)", [&id])?;
            } else {
                let source = Store::open_legacy(&path)?;
                store.adopt(&source, &id, true)?;
            }
            write_locator(&mut Connection::open(&path)?, &id)?;
            (home, store)
        };
        let admin = crate::native_admin::Admin::start(&home)?;
        home.admin
            .set(admin)
            .map_err(|_| anyhow::anyhow!("duplicate Home admin listener"))?;
        homes.insert(path, Arc::downgrade(&home));
        Ok(store)
    }
}

impl Store {
    pub(crate) fn is_native(&self) -> bool {
        self.db.native().is_some()
    }

    pub(crate) fn fork(&self) -> Result<Self> {
        self.db
            .native_home()
            .context("only native Homes can open an independent connection")?
            .connection()
    }

    fn verify_binding(&self, id: &str) -> Result<()> {
        let found: String = self.db.query_row(
            "SELECT id FROM home_import_complete WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(found == id, "native Home does not match its locator");
        Ok(())
    }

    pub(crate) fn verify_authority(&self) -> Result<()> {
        if self.is_native() {
            native_guard::verify_current(&self.db)
        } else {
            backup::verify_epoch(self.db.sqlite().context("expected SQLite staging")?)
        }
    }

    pub(crate) fn operation<T>(
        &mut self,
        mut work: impl FnMut(&mut Store) -> Result<T>,
    ) -> Result<T> {
        if !self.is_native() || !self.db.is_autocommit()? {
            return work(self);
        }
        let start = std::time::Instant::now();
        let mut delay = Duration::from_millis(1);
        loop {
            let attempt = (|| {
                self.db.begin_operation()?;
                native_guard::verify_current(&self.db)?;
                let value = work(self)?;
                self.db.commit_operation()?;
                Ok(value)
            })();
            if let Err(err) = &attempt {
                self.db.rollback_operation()?;
                if crate::home_sql::retryable(err) && start.elapsed() < Duration::from_secs(4) {
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_millis(32));
                    continue;
                }
            }
            return attempt;
        }
    }

    // The HA authority has durably accepted the seed. Invalidate all access
    // to the source's old backups and ciphertext before removing its locator.
    pub(crate) fn retire_seed(self, path: &Path) -> Result<()> {
        let home = self.db.native_home().context("expected native seed")?;
        let _gate = home.guard.publication()?;
        self.verify_authority()?;
        let conn =
            Connection::open_with_flags(home.guard.path(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.execute_batch("PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA")?;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch("DELETE FROM opening_keys; UPDATE state SET key=randomblob(32),epoch=epoch+1 WHERE singleton=1")?;
        tx.commit()?;
        home.guard.fence();
        drop(_gate);
        drop(self);
        let primary = NativeHome::primary_path(path);
        for p in [
            primary.with_extension("db-log"),
            suffix(&primary, "-wal"),
            primary,
            path.to_owned(),
        ] {
            if p.try_exists()? {
                fs::remove_file(&p)?;
            }
        }
        File::open(path.parent().context("missing seed directory")?)?.sync_all()?;
        Ok(())
    }

    fn adopt(&mut self, source: &Store, id: &str, cutover: bool) -> Result<()> {
        native_guard::initialize_head(&self.db)?;
        native_guard::recover_opening_reservations(&self.db, None)?;
        // setval restores deleted cursor tails and requires exclusive access to the allocator.
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            crate::native_transfer::import_legacy(source, self)?;
            self.db
                .execute("INSERT INTO home_import_complete VALUES(1,?)", [id])?;
            if cutover {
                native_guard::stage_cutover(&self.db)?;
            }
            self.db.commit_operation()?;
            Ok(())
        })();
        if result.is_err() {
            self.db.rollback_operation()?;
        }
        result
    }

    pub(crate) fn restore_from_memory(output: &Path, guard: &Path, source: &Store) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        let output = canonical(output)?;
        let parent = output.parent().context("missing restore directory")?;
        let id = crate::model::new_secret();
        let stage = parent.join(format!(".yap-restore-{id}"));
        let vault = NativeHome::primary_path(&output);
        let link = output.with_added_extension("deletion-guard");
        let paths = [
            output.clone(),
            link.clone(),
            vault.clone(),
            suffix(&vault, "-wal"),
            vault.with_extension("db-log"),
            suffix(&vault, "-shm"),
            output.with_added_extension("ha-managed"),
        ];
        for path in &paths {
            absent(path)?;
        }
        fs::DirBuilder::new().mode(0o700).create(&stage)?;
        let mut published = Vec::new();
        let result = (|| {
            let staged = stage.join("home");
            let home = NativeHome::own(&staged, guard, owner_lock(&staged)?, &id)?;
            let mut dest = home.connection()?;
            dest.adopt(source, &id, false)?;
            drop(dest);
            drop(home);
            let marker_file = File::options()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&staged)?;
            drop(marker_file);
            write_locator(&mut Connection::open(&staged)?, &id)?;
            let data = NativeHome::primary_path(&staged);
            for ext in ["", "-wal", "-log", "-shm"] {
                let src = if ext == "-log" {
                    data.with_extension("db-log")
                } else {
                    suffix(&data, ext)
                };
                let dst = if ext == "-log" {
                    vault.with_extension("db-log")
                } else {
                    suffix(&vault, ext)
                };
                if ext.is_empty() || src.try_exists()? {
                    File::open(&src)?.sync_all()?;
                    fs::hard_link(src, &dst)?;
                    published.push(dst);
                }
            }
            std::os::unix::fs::symlink(guard, &link)?;
            published.push(link);
            File::open(parent)?.sync_all()?;
            fs::hard_link(&staged, &output)?;
            published.push(output.clone());
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            for path in published.into_iter().rev() {
                fs::remove_file(path)?;
            }
        }
        fs::remove_dir_all(stage)?;
        result
    }
}

fn suffix(path: &Path, ext: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(ext);
    value.into()
}

fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
        Ok(_) => anyhow::bail!("restore destination already exists: {}", path.display()),
    }
}

#[cfg(test)]
#[path = "../tests/turso/native_home.rs"]
mod tests;
