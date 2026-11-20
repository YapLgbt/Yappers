// SPDX-License-Identifier: AGPL-3.0-only
//! Synchronous Home SQL on bounded blocking workers. The native connection
//! and its transactions stay on the owning worker. Rows own SQLite values and
//! cursors advance one row at a time on either engine.

use crate::store::NativeHome;
use rusqlite::{
    Error, Result, ToSql, TransactionBehavior,
    types::{FromSql, FromSqlError, ToSqlOutput, Value, ValueRef},
};
use std::{
    cell::{Cell, RefCell},
    error::Error as StdError,
    future::Future,
    ops::Deref,
    sync::Arc,
};

fn wait<T>(future: impl Future<Output = turso::Result<T>>) -> Result<T> {
    futures_lite::future::block_on(tokio::task::unconstrained(future)).map_err(native_failure)
}

fn native_failure(err: turso::Error) -> Error {
    match err {
        turso::Error::QueryReturnedNoRows => Error::QueryReturnedNoRows,
        other => Error::ToSqlConversionFailure(Box::new(other)),
    }
}

#[derive(Debug)]
struct UnresolvedCommit(anyhow::Error);

impl std::fmt::Display for UnresolvedCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Home commit finalization requires recovery: {}", self.0)
    }
}
impl StdError for UnresolvedCommit {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.0.as_ref())
    }
}

fn guard_error(err: anyhow::Error) -> Error {
    Error::UserFunctionError(err.reallocate_into_boxed_dyn_error_without_backtrace())
}

pub fn retryable(err: &anyhow::Error) -> bool {
    if err.chain().any(|e| e.is::<UnresolvedCommit>()) {
        return false;
    }
    err.chain().any(|e| {
        e.is::<crate::native_guard::StaleSnapshot>()
            || match e.downcast_ref::<turso::Error>() {
                Some(turso::Error::Busy(_) | turso::Error::BusySnapshot(_)) => true,
                // 0.8.4 maps these MVCC engine variants to its generic Error.
                Some(turso::Error::Error(reason)) => matches!(
                    reason.as_str(),
                    "Write-write conflict" | "Database schema conflict"
                ),
                _ => false,
            }
    })
}

fn own(value: ValueRef<'_>, index: usize) -> Result<Value> {
    Value::try_from(value)
        .map_err(|e| Error::FromSqlConversionFailure(index, value.data_type(), Box::new(e)))
}

fn param(value: &dyn ToSql) -> Result<Value> {
    match value.to_sql()? {
        ToSqlOutput::Borrowed(value) => own(value, 0),
        ToSqlOutput::Owned(value) => Ok(value),
        _ => Err(Error::InvalidQuery),
    }
}

pub trait HomeParams {
    fn values(self) -> Result<Vec<Value>>;
}

impl HomeParams for [&dyn ToSql; 0] {
    fn values(self) -> Result<Vec<Value>> {
        Ok(vec![])
    }
}
impl<T: ToSql> HomeParams for &[T] {
    fn values(self) -> Result<Vec<Value>> {
        self.iter().map(|v| param(v)).collect()
    }
}
impl HomeParams for Vec<Value> {
    fn values(self) -> Result<Vec<Value>> {
        Ok(self)
    }
}
impl HomeParams for Vec<&str> {
    fn values(self) -> Result<Vec<Value>> {
        self.as_slice().values()
    }
}

// As with rusqlite, [] must have exactly one possible element type.
macro_rules! arrays {
    ($($n:literal),*) => { $(
        impl<T: ToSql> HomeParams for [T; $n] {
            fn values(self) -> Result<Vec<Value>> { self.as_slice().values() }
        }
    )* };
}
arrays!(1, 2, 3, 4, 5, 6, 7, 8);

fn native_values(values: Vec<Value>) -> Vec<turso::Value> {
    values
        .into_iter()
        .map(|v| match v {
            Value::Null => turso::Value::Null,
            Value::Integer(i) => turso::Value::Integer(i),
            Value::Real(f) => turso::Value::Real(f),
            Value::Text(s) => turso::Value::Text(s),
            Value::Blob(b) => turso::Value::Blob(b),
        })
        .collect()
}

pub struct HomeRow {
    columns: Arc<[String]>,
    values: Vec<Value>,
}

pub trait HomeIndex: Copy {
    fn index(self, columns: &[String]) -> Result<usize>;
}
impl HomeIndex for usize {
    fn index(self, columns: &[String]) -> Result<usize> {
        if self < columns.len() {
            Ok(self)
        } else {
            Err(Error::InvalidColumnIndex(self))
        }
    }
}
impl HomeIndex for &str {
    fn index(self, columns: &[String]) -> Result<usize> {
        columns
            .iter()
            .position(|col| self.eq_ignore_ascii_case(col))
            .ok_or_else(|| Error::InvalidColumnName(self.into()))
    }
}
impl HomeRow {
    pub fn get<I: HomeIndex, T: FromSql>(&self, index: I) -> Result<T> {
        let i = index.index(&self.columns)?;
        let value = ValueRef::from(&self.values[i]);
        T::column_result(value).map_err(|err| match err {
            FromSqlError::InvalidType => {
                Error::InvalidColumnType(i, self.columns[i].clone(), value.data_type())
            }
            FromSqlError::OutOfRange(v) => Error::IntegralValueOutOfRange(i, v),
            FromSqlError::Utf8Error(e) => Error::Utf8Error(i, e),
            FromSqlError::Other(e) => Error::FromSqlConversionFailure(i, value.data_type(), e),
            e => Error::FromSqlConversionFailure(i, value.data_type(), Box::new(e)),
        })
    }
}

enum Backend {
    Sqlite(rusqlite::Connection),
    Native(turso::Connection),
}

pub struct HomeDb {
    backend: Backend,
    home: Option<Arc<NativeHome>>,
    next_savepoint: Cell<u64>,
    opening_operation: RefCell<Option<String>>,
}

impl From<rusqlite::Connection> for HomeDb {
    fn from(conn: rusqlite::Connection) -> Self {
        Self {
            backend: Backend::Sqlite(conn),
            home: None,
            next_savepoint: Cell::new(0),
            opening_operation: RefCell::default(),
        }
    }
}

impl HomeDb {
    pub fn from_native(conn: turso::Connection, home: Arc<NativeHome>) -> Self {
        Self {
            backend: Backend::Native(conn),
            home: Some(home),
            next_savepoint: Cell::new(0),
            opening_operation: RefCell::default(),
        }
    }
    pub fn sqlite(&self) -> Option<&rusqlite::Connection> {
        match &self.backend {
            Backend::Sqlite(c) => Some(c),
            _ => None,
        }
    }
    pub fn sqlite_mut(&mut self) -> Option<&mut rusqlite::Connection> {
        match &mut self.backend {
            Backend::Sqlite(c) => Some(c),
            _ => None,
        }
    }
    pub fn native(&self) -> Option<&turso::Connection> {
        match &self.backend {
            Backend::Native(c) => Some(c),
            _ => None,
        }
    }
    pub fn native_home(&self) -> Option<&Arc<NativeHome>> {
        self.home.as_ref()
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(c) => c.execute_batch(sql),
            Backend::Native(c) => wait(c.execute_batch(sql)),
        }
    }
    pub fn execute(&self, sql: &str, params: impl HomeParams) -> Result<usize> {
        self.prepare(sql)?.execute(params)
    }
    pub fn query_row<T>(
        &self,
        sql: &str,
        params: impl HomeParams,
        f: impl FnOnce(&HomeRow) -> Result<T>,
    ) -> Result<T> {
        self.prepare(sql)?.query_row(params, f)
    }
    pub fn prepare(&self, sql: &str) -> Result<HomeStatement<'_>> {
        Ok(HomeStatement(match &self.backend {
            Backend::Sqlite(c) => Statement::Sqlite(c.prepare(sql)?),
            Backend::Native(c) => Statement::Native(wait(c.prepare(sql))?),
        }))
    }
    pub fn prepare_cached(&self, sql: &str) -> Result<HomeStatement<'_>> {
        Ok(HomeStatement(match &self.backend {
            Backend::Sqlite(c) => Statement::Cached(c.prepare_cached(sql)?),
            Backend::Native(c) => Statement::Native(wait(c.prepare_cached(sql))?),
        }))
    }
    pub fn is_autocommit(&self) -> Result<bool> {
        match &self.backend {
            Backend::Sqlite(c) => Ok(c.is_autocommit()),
            Backend::Native(c) => c.is_autocommit().map_err(native_failure),
        }
    }
    pub fn transaction(&mut self) -> Result<HomeTx<'_>> {
        self.unchecked_transaction()
    }
    pub fn unchecked_transaction(&self) -> Result<HomeTx<'_>> {
        HomeTx::new_unchecked(self, TransactionBehavior::Deferred)
    }
    pub fn transaction_with_behavior(
        &mut self,
        behavior: TransactionBehavior,
    ) -> Result<HomeTx<'_>> {
        HomeTx::new_unchecked(self, behavior)
    }
    pub fn begin_operation(&self) -> Result<()> {
        if !self.is_autocommit()? {
            return Err(Error::InvalidQuery);
        }
        self.execute_batch(if self.native().is_some() {
            "BEGIN CONCURRENT"
        } else {
            "BEGIN IMMEDIATE"
        })
    }

    pub fn rollback_operation(&self) -> Result<()> {
        if !self.is_autocommit()? {
            self.execute_batch("ROLLBACK")?;
        }
        self.finish_opening_operation().map_err(guard_error)
    }

    pub(crate) fn with_opening_operation<T>(&self, reserve: impl FnOnce(&str) -> T) -> T {
        let mut operation = self.opening_operation.borrow_mut();
        reserve(operation.get_or_insert_with(crate::new_secret))
    }

    fn finish_opening_operation(&self) -> anyhow::Result<()> {
        let mut operation = self.opening_operation.borrow_mut();
        if let Some(id) = operation.as_deref() {
            crate::native_guard::recover_opening_reservations(self, Some(id))?;
            *operation = None;
        }
        Ok(())
    }

    pub fn commit_operation(&self) -> Result<()> {
        let Some(home) = self.native_home() else {
            return self.execute_batch("COMMIT");
        };
        use crate::native_guard;
        let has_intents = native_guard::has_intents(self).map_err(guard_error)?;
        if !has_intents && self.opening_operation.borrow().is_none() {
            // The request may have read an account or permission before a
            // concurrent revocation. Validate its pinned head and commit under
            // the same shared gate so publication cannot interleave.
            let _read = home.guard.read_gate().map_err(guard_error)?;
            native_guard::verify_current(self).map_err(guard_error)?;
            return self.execute_batch("COMMIT");
        }

        let _publication = home.guard.publication().map_err(guard_error)?;
        let prepare = if has_intents {
            native_guard::before_commit(self)
        } else {
            native_guard::verify_current(self)
        };
        let result = prepare
            .map_err(guard_error)
            .and_then(|()| self.execute_batch("COMMIT"));
        if let Err(original) = result {
            // COMMIT can fail after the engine has ended the transaction.
            // Reconcile only against committed state while readers are excluded.
            let recovered = self
                .rollback_operation()
                .map_err(anyhow::Error::from)
                .and_then(|()| native_guard::reconcile(self));
            if let Err(recovery) = recovered {
                home.guard.fence();
                let err = recovery.context(format!(
                    "commit failed ({original}) and reconciliation failed"
                ));
                return Err(Error::UserFunctionError(Box::new(UnresolvedCommit(err))));
            }
            return Err(original);
        }

        native_guard::after_commit(self)
            .and_then(|()| self.finish_opening_operation())
            .map_err(|err| {
                // The logical write has committed. Retrying the domain operation
                // could duplicate it even when the guard itself reports Busy.
                home.guard.fence();
                Error::UserFunctionError(Box::new(UnresolvedCommit(err)))
            })
    }
}

enum Statement<'db> {
    Sqlite(rusqlite::Statement<'db>),
    Cached(rusqlite::CachedStatement<'db>),
    Native(turso::Statement),
}
pub struct HomeStatement<'db>(Statement<'db>);

impl HomeStatement<'_> {
    pub fn execute(&mut self, params: impl HomeParams) -> Result<usize> {
        let values = params.values()?;
        match &mut self.0 {
            Statement::Sqlite(s) => s.execute(rusqlite::params_from_iter(values)),
            Statement::Cached(s) => s.execute(rusqlite::params_from_iter(values)),
            Statement::Native(s) => Ok(wait(s.execute(native_values(values)))? as usize),
        }
    }
    pub fn query(&mut self, params: impl HomeParams) -> Result<HomeRows<'_>> {
        let values = params.values()?;
        let (columns, inner) = match &mut self.0 {
            Statement::Native(s) => {
                let rows = wait(s.query(native_values(values)))?;
                (rows.column_names(), Cursor::Native { rows, stmt: s })
            }
            Statement::Sqlite(s) => sqlite_cursor(s, values)?,
            Statement::Cached(s) => sqlite_cursor(s, values)?,
        };
        Ok(HomeRows {
            columns: columns.into(),
            inner,
            current: None,
            finished: false,
        })
    }
    pub fn query_row<T>(
        &mut self,
        params: impl HomeParams,
        f: impl FnOnce(&HomeRow) -> Result<T>,
    ) -> Result<T> {
        f(self
            .query(params)?
            .next()?
            .ok_or(Error::QueryReturnedNoRows)?)
    }
    pub fn query_map<T, F: FnMut(&HomeRow) -> Result<T>>(
        &mut self,
        params: impl HomeParams,
        f: F,
    ) -> Result<HomeMappedRows<'_, F>> {
        Ok(HomeMappedRows {
            rows: self.query(params)?,
            map: f,
        })
    }
}

fn sqlite_cursor<'s>(
    stmt: &'s mut rusqlite::Statement<'_>,
    values: Vec<Value>,
) -> Result<(Vec<String>, Cursor<'s>)> {
    let names = stmt.column_names().map(str::to_owned).collect();
    Ok((
        names,
        Cursor::Sqlite(stmt.query(rusqlite::params_from_iter(values))?),
    ))
}

enum Cursor<'s> {
    Sqlite(rusqlite::Rows<'s>),
    Native {
        rows: turso::Rows,
        stmt: &'s mut turso::Statement,
    },
}

pub struct HomeRows<'s> {
    columns: Arc<[String]>,
    inner: Cursor<'s>,
    current: Option<HomeRow>,
    finished: bool,
}

impl HomeRows<'_> {
    pub fn next(&mut self) -> Result<Option<&HomeRow>> {
        if self.finished {
            return Ok(None);
        }
        self.finished = true;
        let values = match &mut self.inner {
            Cursor::Sqlite(rows) => {
                let Some(row) = rows.next()? else {
                    return Ok(None);
                };
                (0..self.columns.len())
                    .map(|i| own(row.get_ref(i)?, i))
                    .collect::<Result<Vec<_>>>()?
            }
            Cursor::Native { rows, .. } => {
                let Some(row) = wait(rows.next())? else {
                    return Ok(None);
                };
                (0..row.column_count())
                    .map(|i| {
                        Ok(match row.get_value(i).map_err(native_failure)? {
                            turso::Value::Null => Value::Null,
                            turso::Value::Integer(i) => Value::Integer(i),
                            turso::Value::Real(f) => Value::Real(f),
                            turso::Value::Text(s) => Value::Text(s),
                            turso::Value::Blob(b) => Value::Blob(b),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            }
        };
        self.finished = false;
        self.current = Some(HomeRow {
            columns: self.columns.clone(),
            values,
        });
        Ok(self.current.as_ref())
    }
}

impl Drop for HomeRows<'_> {
    fn drop(&mut self) {
        if let Cursor::Native { stmt, .. } = &mut self.inner {
            // Release cursors abandoned by query_row or early iterator exit.
            let _ = stmt.reset();
        }
    }
}

pub struct HomeMappedRows<'s, F> {
    rows: HomeRows<'s>,
    map: F,
}
impl<T, F: FnMut(&HomeRow) -> Result<T>> Iterator for HomeMappedRows<'_, F> {
    type Item = Result<T>;
    fn next(&mut self) -> Option<Self::Item> {
        self.rows
            .next()
            .transpose()
            .map(|row| row.and_then(&mut self.map))
    }
}

pub struct HomeTx<'db> {
    db: &'db HomeDb,
    savepoint: Option<u64>,
    active: bool,
}

impl<'db> HomeTx<'db> {
    pub fn new_unchecked(db: &'db HomeDb, behavior: TransactionBehavior) -> Result<Self> {
        let savepoint = if db.is_autocommit()? {
            let begin = match behavior {
                TransactionBehavior::Deferred => "BEGIN DEFERRED",
                TransactionBehavior::Immediate => "BEGIN IMMEDIATE",
                TransactionBehavior::Exclusive => "BEGIN EXCLUSIVE",
                _ => return Err(Error::InvalidQuery),
            };
            db.execute_batch(begin)?;
            None
        } else {
            let n = db
                .next_savepoint
                .get()
                .checked_add(1)
                .ok_or(Error::InvalidQuery)?;
            db.next_savepoint.set(n);
            db.execute_batch(&format!("SAVEPOINT home_domain_{n}"))?;
            Some(n)
        };
        Ok(Self {
            db,
            savepoint,
            active: true,
        })
    }
    pub fn commit(mut self) -> Result<()> {
        match self.savepoint {
            Some(n) => self.db.execute_batch(&format!("RELEASE home_domain_{n}"))?,
            None => self.db.commit_operation()?,
        }
        self.active = false;
        Ok(())
    }
    fn abort(&mut self) -> Result<()> {
        match self.savepoint {
            Some(n) if !self.db.is_autocommit()? => self.db.execute_batch(&format!(
                "ROLLBACK TO home_domain_{n}; RELEASE home_domain_{n}"
            ))?,
            Some(_) => {}
            None => self.db.rollback_operation()?,
        }
        self.active = false;
        Ok(())
    }
}
impl Deref for HomeTx<'_> {
    type Target = HomeDb;
    fn deref(&self) -> &HomeDb {
        self.db
    }
}
impl Drop for HomeTx<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{OptionalExtension, params};

    fn native_db(db: &turso::Database) -> Result<HomeDb> {
        Ok(HomeDb {
            backend: Backend::Native(db.connect().map_err(native_failure)?),
            home: None,
            next_savepoint: Cell::new(0),
            opening_operation: RefCell::default(),
        })
    }

    fn exercise(db: &mut HomeDb) -> anyhow::Result<()> {
        db.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, label TEXT, payload BLOB)")?;
        db.begin_operation()?;
        let outer = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        outer.execute(
            "INSERT INTO items VALUES(?1,?2,?3)",
            params![1, "one", &[0u8, 255][..]],
        )?;
        {
            let inner = outer.unchecked_transaction()?;
            inner.execute("INSERT INTO items(id) VALUES(?)", [2])?;
        }
        {
            let read = outer.unchecked_transaction()?;
            assert_eq!(
                read.query_row("SELECT count(*) FROM items", [], |r| r.get::<_, usize>(0))?,
                1
            );
        }
        outer.commit()?;
        db.commit_operation()?;

        let mut stmt = db.prepare_cached(
            "SELECT id AS Identifier,label,payload,NULL FROM items WHERE id>=? ORDER BY id",
        )?;
        assert!(
            stmt.query_row([2], |r| r.get::<_, u64>(0))
                .optional()?
                .is_none()
        );
        let found = stmt
            .query_map([1], |r| {
                Ok((
                    r.get::<_, u64>("identifier")?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, Option<i32>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(found, [(1, "one".to_owned(), vec![0, 255], None)]);
        assert!(stmt.query_row([1], |r| r.get::<_, bool>("label")).is_err());
        drop(stmt);
        db.begin_operation()?;
        assert_eq!(
            db.query_row(
                "UPDATE items SET label='leased' WHERE id=1 RETURNING id",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        db.commit_operation()?;
        assert_eq!(
            db.query_row("SELECT label FROM items WHERE id=1", [], |r| r
                .get::<_, String>(0))?,
            "leased"
        );
        db.begin_operation()?;
        assert_eq!(db.execute("DELETE FROM items", [])?, 1);
        db.rollback_operation()?;
        assert!(db.is_autocommit()?);
        assert_eq!(
            db.query_row("SELECT count(*) FROM items", [], |r| r.get::<_, usize>(0))?,
            1
        );
        Ok(())
    }

    #[test]
    fn sqlite_queries_and_nested_transactions() -> anyhow::Result<()> {
        exercise(&mut HomeDb::from(rusqlite::Connection::open_in_memory()?))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_queries_and_nested_concurrent_transactions() -> anyhow::Result<()> {
        let source = turso::Builder::new_local(":memory:").build().await?;
        let mut db = native_db(&source)?;
        db.query_row("PRAGMA journal_mode=mvcc", [], |r| r.get::<_, String>(0))?;
        exercise(&mut db)?;
        Ok(())
    }

    #[test]
    fn guard_advance_rejects_pinned_no_intent_read_and_write() -> anyhow::Result<()> {
        use crate::{native_guard, store::Store};
        let dir = tempfile::tempdir()?;
        let current = Store::open(&dir.path().join("home"))?;
        let old = current.fork()?;
        current
            .db
            .execute_batch("CREATE TABLE guard_commit_probe(n INTEGER)")?;
        for write in [false, true] {
            old.db.begin_operation()?;
            native_guard::verify_current(&old.db)?;
            if write {
                old.db
                    .execute("INSERT INTO guard_commit_probe VALUES(1)", [])?;
            }
            current.db.begin_operation()?;
            native_guard::stage_cutover(&current.db)?;
            current.db.commit_operation()?;
            let err = anyhow::Error::from(old.db.commit_operation().unwrap_err())
                .context("finish old snapshot");
            assert!(
                err.chain().any(|e| e.is::<native_guard::StaleSnapshot>()),
                "{err:#}"
            );
            assert!(retryable(&err));
            old.db.rollback_operation()?;
            old.db.begin_operation()?;
            assert_eq!(
                old.db
                    .query_row("SELECT count(*) FROM guard_commit_probe", [], |r| r
                        .get::<_, usize>(0))?,
                0
            );
            old.db.commit_operation()?;
        }
        Ok(())
    }

    #[test]
    fn mvcc_conflict_survives_error_context() -> anyhow::Result<()> {
        let source = wait(turso::Builder::new_local(":memory:").build())?;
        let a = native_db(&source)?;
        let b = native_db(&source)?;
        a.query_row("PRAGMA journal_mode=mvcc", [], |r| r.get::<_, String>(0))?;
        a.execute_batch("CREATE TABLE balance(id INTEGER PRIMARY KEY, n INTEGER); INSERT INTO balance VALUES(1,0)")?;
        a.begin_operation()?;
        b.begin_operation()?;
        assert_eq!(
            b.query_row("SELECT n FROM balance", [], |r| r.get::<_, i64>(0))?,
            0
        );
        a.execute("UPDATE balance SET n=1 WHERE id=1", [])?;
        a.commit_operation()?;
        let err = b
            .execute("UPDATE balance SET n=2 WHERE id=1", [])
            .and_then(|_| b.commit_operation())
            .unwrap_err();
        let err = anyhow::Error::from(err).context("update balance");
        assert!(err.chain().any(|e| e.is::<turso::Error>()), "{err:#}");
        assert!(retryable(&err), "{err:#}");
        b.rollback_operation()?;
        Ok(())
    }
}
