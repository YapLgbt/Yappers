// SPDX-License-Identifier: AGPL-3.0-only
//! Durable account state and private authored-history storage. Never a federation trust grant.
use crate::home_sql::HomeDb;
use crate::store::Store;
use crate::{
    model::Envelope,
    portability::{AccountRecord, validate_chain},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::collections::BTreeMap;

pub(crate) fn deleted_registration(db: &HomeDb, account: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT record_id FROM deleted_accounts WHERE account=?",
            [account],
            |r| r.get(0),
        )
        .optional()?)
}

pub(crate) fn require_live(db: &HomeDb, account: &str) -> Result<()> {
    ensure!(
        deleted_registration(db, account)?.is_none(),
        "account deleted"
    );
    Ok(())
}

fn account_chain(db: &HomeDb, account: &str) -> Result<Vec<AccountRecord>> {
    require_live(db, account)?;
    let value: Option<String> = db
        .query_row(
            "SELECT chain FROM account_states WHERE account=?1",
            [account],
            |r| r.get(0),
        )
        .optional()?;
    value
        .map(|v| Ok(serde_json::from_str(&v)?))
        .transpose()
        .map(|v| v.unwrap_or_default())
}

impl Store {
    pub fn account_chain(&self, account: &str) -> Result<Vec<AccountRecord>> {
        account_chain(&self.db, account)
    }
    /// Prefix replays are idempotent; forks at any previously seen revision fail closed.
    pub fn register_account(&mut self, chain: &[AccountRecord]) -> Result<AccountRecord> {
        let latest = validate_chain(chain)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known = account_chain(&tx, &latest.payload.account)?;
        for (old, incoming) in known.iter().zip(chain) {
            ensure!(old == incoming, "conflicting account history");
        }
        if known.len() >= chain.len() {
            return Ok(known.last().context("missing account state")?.clone());
        }
        crate::privacy::observe(&tx, &latest.payload.account)?;
        tx.execute("INSERT INTO account_states(account,chain) VALUES(?1,?2) ON CONFLICT(account) DO UPDATE SET chain=excluded.chain", params![latest.payload.account, serde_json::to_string(chain)?])?;
        if tx.native().is_some() {
            crate::native_guard::stage_account(&tx, chain)?;
        }
        tx.commit()?;
        Ok(latest.clone())
    }
    pub fn owned_events(&self, account: &str) -> Result<Vec<Envelope>> {
        self.owned_after(account, "", -1)
    }

    pub(crate) fn owned_after(
        &self,
        account: &str,
        after: &str,
        limit: i64,
    ) -> Result<Vec<Envelope>> {
        let mut events = BTreeMap::new();
        // Read at most one page from each ordered source, then merge by event ID.
        // Comparing duplicates retains the strongest known deletion proof.
        let mut shared = self.db.prepare("SELECT envelope FROM events
            WHERE COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author)=? AND id>?
            ORDER BY id LIMIT ?")?;
        for value in shared.query_map(params![account, after, limit], |r| r.get::<_, String>(0))? {
            let event = crate::native_content::hydrate(&self.db, &value?, false)?;
            ensure!(event.event.owner_id() == account, "invalid owned event");
            events.insert(event.event.id.clone(), event);
        }
        let mut statement = self.db.prepare(
            "SELECT envelope FROM personal_events
            WHERE account=? AND event_id>? ORDER BY event_id LIMIT ?",
        )?;
        for value in
            statement.query_map(params![account, after, limit], |r| r.get::<_, String>(0))?
        {
            let envelope = crate::native_content::hydrate(&self.db, &value?, true)?;
            if let Some(existing) = events.get(&envelope.event.id) {
                ensure!(
                    existing.same_acceptance(&envelope)?,
                    "conflicting owned history attestations"
                );
            }
            if events
                .get(&envelope.event.id)
                .is_none_or(|e| e.deletion.is_none())
            {
                events.insert(envelope.event.id.clone(), envelope);
            }
        }
        Ok(events.into_values().collect())
    }
    /// A complete validated chunk commits atomically. Historical signatures remain unchanged.
    pub fn import_personal_events(&mut self, account: &str, events: &[Envelope]) -> Result<()> {
        self.import_history(account, events, &[])
    }
    pub(crate) fn import_history(
        &mut self,
        account: &str,
        events: &[Envelope],
        admitted_deletes: &[&Envelope],
    ) -> Result<()> {
        ensure!(events.len() <= 50, "personal import chunk too large");
        let tx = self.db.transaction()?;
        require_live(&tx, account)?;
        for deletion in admitted_deletes {
            ensure!(
                deletion.event.owner_id() == account
                    && matches!(deletion.event.payload.action, crate::Action::Delete { .. }),
                "invalid shared deletion"
            );
            Self::insert(&tx, deletion)?;
        }
        for envelope in events {
            envelope.validate()?;
            ensure!(
                envelope.event.owner_id() == account,
                "cannot import another person's event"
            );
            let envelope = &Self::reconcile(&tx, envelope, true)?;
            let old: Option<String> = tx
                .query_row(
                    "SELECT envelope FROM personal_events WHERE account=?1 AND event_id=?2",
                    params![account, envelope.event.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old {
                ensure!(
                    serde_json::from_str::<Envelope>(&old)?.same_acceptance(envelope)?,
                    "conflicting personal history"
                );
            } else {
                tx.execute(
                    "INSERT INTO personal_events(account,event_id,envelope) VALUES(?1,?2,?3)",
                    params![
                        account,
                        envelope.event.id,
                        crate::native_content::encode(&tx, envelope, true)?
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
