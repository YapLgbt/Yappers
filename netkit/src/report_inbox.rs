// SPDX-License-Identifier: AGPL-3.0-only
//! An operator-owned SQLite file, separate from every feed, backup and HA ledger.
use crate::{
    model::now,
    report::{Evidence, issued_at},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub struct Inbox {
    db: Connection,
}
const DAY: i64 = 86400;
impl Inbox {
    pub fn open(path: &Path, home: Option<&str>) -> Result<Self> {
        // Resolve directory aliases such as macOS /var, while refusing symlinks at the file.
        let directory = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let path = directory
            .canonicalize()?
            .join(path.file_name().context("missing inbox filename")?);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(home.is_some())
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        ensure!(
            file.metadata()?.is_file() && file.metadata()?.permissions().mode() & 0o077 == 0,
            "report inbox must be a private regular file (0600)"
        );
        let flags =
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let db = Connection::open_with_flags(path, flags)?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        let other: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT IN ('report_home','report_cases'))",
            [], |row| row.get(0),
        )?;
        ensure!(!other, "report inbox must have a dedicated file");
        db.execute_batch("PRAGMA secure_delete=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA; PRAGMA temp_store=MEMORY;")?;
        if let Some(home) = home {
            db.execute_batch("CREATE TABLE IF NOT EXISTS report_home(id INTEGER PRIMARY KEY CHECK(id=1),home TEXT NOT NULL);
              CREATE TABLE IF NOT EXISTS report_cases(id TEXT PRIMARY KEY, created INTEGER NOT NULL, status TEXT NOT NULL, closed INTEGER, evidence TEXT);")?;
            db.execute("INSERT OR IGNORE INTO report_home VALUES(1,?)", [home])?;
            let bound: String =
                db.query_row("SELECT home FROM report_home WHERE id=1", [], |r| r.get(0))?;
            ensure!(bound == home, "report inbox belongs to another home");
        }
        let mut inbox = Self { db };
        inbox.purge(now())?;
        Ok(inbox)
    }
    fn expire(db: &Connection, t: i64) -> Result<()> {
        db.execute("UPDATE report_cases SET evidence=NULL,status='expired' WHERE evidence IS NOT NULL AND created<=?",[t-7*DAY])?;
        db.execute("DELETE FROM report_cases WHERE created<=?", [t - 30 * DAY])?;
        Ok(())
    }
    pub(crate) fn purge(&mut self, t: i64) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::expire(&tx, t)?;
        tx.commit()?;
        Ok(())
    }
    fn id(receipt: &str) -> String {
        hex::encode(blake3::derive_key(
            "Yap private report receipt v1",
            receipt.as_bytes(),
        ))
    }
    fn status(db: &Connection, id: &str) -> Result<Option<Value>> {
        Ok(db
            .query_row(
                "SELECT status,created,closed FROM report_cases WHERE id=?",
                [id],
                |row| {
                    Ok(json!({
                        "case": id,
                        "status": row.get::<_, String>(0)?,
                        "receivedAt": row.get::<_, i64>(1)?,
                        "closedAt": row.get::<_, Option<i64>>(2)?
                    }))
                },
            )
            .optional()?)
    }
    pub(crate) fn submit(
        &mut self,
        home: &str,
        receipt: &str,
        evidence: &Evidence,
        t: i64,
    ) -> Result<Value> {
        let stamp = issued_at(receipt, home)?;
        evidence.validate()?;
        let id = Self::id(receipt);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::expire(&tx, t)?;
        let encoded = serde_json::to_string(evidence)?;
        if let Some(existing) = Self::status(&tx, &id)? {
            let previous: Option<String> = tx.query_row(
                "SELECT evidence FROM report_cases WHERE id=?",
                [&id],
                |row| row.get(0),
            )?;
            ensure!(
                previous.is_none_or(|original| original == encoded),
                "receipt already used for different evidence"
            );
            tx.commit()?;
            return Ok(existing);
        }
        // A delayed or retried submission cannot recreate evidence after the status expires.
        ensure!(
            (t - 3600..=t + 300).contains(&stamp),
            "draft expired; review a new report"
        );
        let recent: i64 = tx.query_row(
            "SELECT count(*) FROM report_cases WHERE created>?",
            [t - 3600],
            |r| r.get(0),
        )?;
        let pending: i64 = tx.query_row(
            "SELECT count(*) FROM report_cases WHERE status='received'",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            recent < 60 && pending < 2000,
            "report inbox is full; try again later"
        );
        tx.execute(
            "INSERT INTO report_cases(id,created,status,evidence) VALUES(?,?,'received',?)",
            params![id, t, encoded],
        )?;
        let result = Self::status(&tx, &id)?.context("case missing")?;
        tx.commit()?;
        Ok(result)
    }
    pub(crate) fn lookup(&mut self, home: &str, receipt: &str, t: i64) -> Result<Value> {
        issued_at(receipt, home)?;
        self.purge(t)?;
        Self::status(&self.db, &Self::id(receipt))?.context("case unknown or expired")
    }
    pub fn pending(&mut self) -> Result<Vec<Value>> {
        self.purge(now())?;
        let mut q = self.db.prepare(
            "SELECT id,created FROM report_cases WHERE status='received' ORDER BY created,id",
        )?;
        Ok(q.query_map([], |r| {
            Ok(json!({"case":r.get::<_,String>(0)?,"receivedAt":r.get::<_,i64>(1)?}))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn inspect(&mut self, id: &str) -> Result<Value> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::expire(&tx, now())?;
        let mut result = Self::status(&tx, id)?.context("unknown case")?;
        let raw: Option<String> =
            tx.query_row("SELECT evidence FROM report_cases WHERE id=?", [id], |r| {
                r.get(0)
            })?;
        result["evidence"] = raw
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or(Value::Null);
        tx.commit()?;
        Ok(result)
    }
    pub fn close(&mut self, id: &str, outcome: &str) -> Result<()> {
        ensure!(
            matches!(outcome, "actionTaken" | "noAction"),
            "invalid outcome"
        );
        let t = now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::expire(&tx, t)?;
        let changed = tx.execute(
            "UPDATE report_cases SET status=?,closed=?,evidence=NULL WHERE id=? AND status='received'",
            params![outcome, t, id],
        )?;
        ensure!(changed == 1, "case unavailable or already closed");
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        new_secret,
        report::{Category, Subject, receipt},
    };
    #[test]
    fn private_evidence_expires_and_stale_retries_cannot_restore_it() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("reports.sqlite");
        let home = new_secret();
        let ticket = receipt(&home);
        let t = now();
        let evidence = Evidence {
            category: Category::Threat,
            subject: Subject::Message,
            text: "SELECTED-TEST-EXCERPT".into(),
            reference: None,
        };
        let mut inbox = Inbox::open(&file, Some(&home))?;
        let id = inbox.submit(&home, &ticket, &evidence, t)?["case"]
            .as_str()
            .unwrap()
            .to_owned();
        ensure!(
            inbox
                .submit(
                    &home,
                    &ticket,
                    &Evidence {
                        text: "different text".into(),
                        ..evidence.clone()
                    },
                    t
                )
                .is_err()
        );
        ensure!(Inbox::open(&file, Some(&new_secret())).is_err());
        ensure!(inbox.lookup(&new_secret(), &ticket, t).is_err());
        inbox.purge(t + 7 * DAY)?;
        ensure!(inbox.inspect(&id)?["evidence"].is_null());
        ensure!(inbox.submit(&home, &ticket, &evidence, t + 7 * DAY)?["status"] == "expired");
        ensure!(inbox.lookup(&home, &ticket, t + 30 * DAY).is_err());
        ensure!(
            inbox
                .submit(&home, &ticket, &evidence, t + 30 * DAY)
                .is_err()
        );
        drop(inbox);
        let bytes = std::fs::read(file)?;
        for secret in [&ticket, &evidence.text] {
            ensure!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
        Ok(())
    }
}
