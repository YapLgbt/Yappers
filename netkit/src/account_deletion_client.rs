// SPDX-License-Identifier: MIT OR Apache-2.0
//! Resumable account deletion over the existing client command ABI. Persist
//! the root intent before discovery, and each immutable proof batch and its
//! sequence watermark before sending. Only the Home can acknowledge completion.
use super::Client;
use crate::{
    MAX_FRAME,
    model::{ACCOUNT_DELETE_BATCH, AccountDeleteTarget, Action, Request, Response, SignedEvent},
    portability::{AccountDeletionIntent, AccountProof, AccountRecord, AccountWallet, digest},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{collections::BTreeSet, sync::atomic::Ordering};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedDeletionBatch {
    pub proofs: Vec<SignedEvent>,
    pub next_sequence: u64,
}

fn validate_targets(targets: &[AccountDeleteTarget]) -> Result<()> {
    ensure!(
        (1..=ACCOUNT_DELETE_BATCH).contains(&targets.len()),
        "invalid pending deletion page size"
    );
    let mut ids = BTreeSet::new();
    for t in targets {
        ensure!(
            !t.community.is_empty()
                && t.community.is_ascii()
                && t.community.len() <= 160
                && t.id.len() == 64
                && t.id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && ids.insert(&t.id),
            "invalid or duplicate deletion target"
        );
    }
    Ok(())
}

impl Client {
    fn deletion_registration(&self, intent: &AccountDeletionIntent) -> Result<Vec<AccountRecord>> {
        let chain = self.account_chain()?;
        let record = chain
            .last()
            .context("account deletion requires a registered wallet")?;
        intent.validate(record)?;
        ensure!(
            intent.device == self.author_id()
                && intent.home == self.join.server_addr.id.to_string(),
            "deletion intent belongs to another Home or device"
        );
        Ok(chain)
    }

    /// Create the root-signed intent locally, without contacting the Home.
    pub fn prepare_account_deletion(
        &self,
        wallet: &AccountWallet,
    ) -> Result<AccountDeletionIntent> {
        let intent = AccountDeletionIntent::create(wallet)?;
        self.deletion_registration(&intent)?;
        Ok(intent)
    }

    /// Check the current registration using a device proof scoped to the
    /// read-only probe. Call before persisting an initial deletion journal.
    pub async fn check_account_deletion_availability(
        &self,
        intent: &AccountDeletionIntent,
    ) -> Result<()> {
        let chain = self.deletion_registration(intent)?;
        let proof = AccountProof::signed(
            chain.last().context("missing deletion registration")?,
            &self.author,
            "check-account-delete-v1",
            &intent.home,
            &digest(intent)?,
        )?;
        let response = self
            .home_rpc(&Request::AccountDeleteAvailability {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                chain,
                intent: intent.clone(),
                proof,
            })
            .await
            .context("Could not confirm account deletion support with your Home. You can keep using your account and try again later")?;
        ensure!(
            matches!(response, Response::AccountDeleteAvailable { account } if account == intent.account),
            "Your Home did not confirm account deletion for this account. You can keep using your account and try again later"
        );
        Ok(())
    }

    fn deletion_request(
        &self,
        intent: &AccountDeletionIntent,
        chain: Vec<AccountRecord>,
        deletes: Vec<SignedEvent>,
    ) -> Result<Request> {
        let record = chain.last().context("missing deletion registration")?;
        let proof = AccountProof::signed(
            record,
            &self.author,
            "delete-account-v1",
            &intent.home,
            &digest(&(intent, &deletes))?,
        )?;
        Ok(Request::AccountDelete {
            token: self.join.access_token.clone(),
            community: self.join.community.clone(),
            chain,
            intent: intent.clone(),
            proof,
            deletes,
        })
    }

    /// Sign a prefix that fits one transport frame. The Home returns any
    /// remaining targets after applying it. Each Delete carries its own community.
    pub fn prepare_account_deletion_batch(
        &self,
        intent: &AccountDeletionIntent,
        targets: &[AccountDeleteTarget],
        next_sequence: u64,
    ) -> Result<PreparedDeletionBatch> {
        validate_targets(targets)?;
        ensure!(
            (1..i64::MAX as u64).contains(&next_sequence),
            "invalid next sequence"
        );
        let chain = self.deletion_registration(intent)?;
        let record = chain
            .last()
            .context("missing deletion registration")?
            .clone();
        // The empty array's brackets are already included. All device request
        // signatures and digests have a fixed encoded length.
        let mut request = self.deletion_request(intent, chain, Vec::new())?;
        if self.standby.is_some() {
            request = Request::HomeOperation {
                request_id: "0".repeat(64),
                call: Box::new(request),
            };
        }
        let capacity = MAX_FRAME
            .checked_sub(serde_json::to_vec(&request)?.len())
            .context("deletion registration exceeds frame")?;
        loop {
            let previous = self.sequence.load(Ordering::SeqCst);
            let mut next = previous.max(next_sequence);
            let mut remaining = capacity;
            let mut proofs = Vec::with_capacity(targets.len());
            for t in targets {
                ensure!(next < i64::MAX as u64 - 1, "sequence exhausted");
                let event = SignedEvent::create_for_account(
                    &self.author,
                    intent.home.clone(),
                    t.community.clone(),
                    next,
                    Action::Delete {
                        target: t.id.clone(),
                    },
                    record.clone(),
                )?;
                let size = serde_json::to_vec(&event)?.len() + usize::from(!proofs.is_empty());
                if size > remaining {
                    break;
                }
                remaining -= size;
                proofs.push(event);
                next += 1;
            }
            ensure!(!proofs.is_empty(), "signed deletion exceeds frame");
            if self
                .sequence
                .compare_exchange(previous, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(PreparedDeletionBatch {
                    proofs,
                    next_sequence: next,
                });
            }
        }
    }

    /// An empty batch discovers outstanding targets or retrieves the durable
    /// terminal receipt, including after a lost acknowledgment and relaunch.
    pub async fn account_deletion_step(
        &self,
        intent: &AccountDeletionIntent,
        mut proofs: Vec<SignedEvent>,
    ) -> Result<Response> {
        ensure!(
            proofs.len() <= ACCOUNT_DELETE_BATCH,
            "deletion batch too large"
        );
        let chain = self.deletion_registration(intent)?;
        let mut ids = BTreeSet::new();
        let mut sequences = BTreeSet::new();
        for event in &proofs {
            event.validate()?;
            let Action::Delete { target } = &event.payload.action else {
                bail!("expected a signed Delete");
            };
            ensure!(
                event.payload.version == 4
                    && event.payload.account.as_deref() == chain.last()
                    && event.payload.home == intent.home
                    && event.payload.author == intent.device
                    && ids.insert(target.clone())
                    && sequences.insert(event.payload.sequence),
                "invalid deletion batch binding"
            );
        }
        // A journal can outlive the Home's acceptance window for signed events.
        // Rediscover outstanding targets before signing replacements at fresh sequences.
        let oldest_accepted = crate::model::now().saturating_sub(30 * 86400);
        if proofs
            .iter()
            .any(|e| e.payload.created_at < oldest_accepted)
        {
            proofs.clear();
            ids.clear();
        }
        let request = self.deletion_request(intent, chain, proofs)?;
        let response = self.home_rpc(&request).await?;
        match &response {
            Response::AccountDeleted { account } => ensure!(
                account == &intent.account,
                "deletion receipt account mismatch"
            ),
            Response::AccountDeletePending { targets } => {
                validate_targets(targets)?;
                ensure!(
                    targets.iter().all(|t| !ids.contains(&t.id)),
                    "Home did not apply deletion batch"
                );
            }
            _ => bail!("unexpected account deletion response"),
        }
        Ok(response)
    }

    pub(super) async fn account_deletion_step_value(
        &self,
        intent: &AccountDeletionIntent,
        proofs: Vec<SignedEvent>,
    ) -> Result<serde_json::Value> {
        match self.account_deletion_step(intent, proofs).await? {
            Response::AccountDeleted { account } => {
                Ok(serde_json::json!({"deleted":true,"account":account,"home":intent.home}))
            }
            Response::AccountDeletePending { targets } => {
                Ok(serde_json::json!({"deleted":false,"targets":targets}))
            }
            _ => bail!("unexpected account deletion response"),
        }
    }
}
