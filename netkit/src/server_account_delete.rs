// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    account_erasure,
    account_store::deleted_registration,
    portability::{AccountDeletionIntent, AccountProof, AccountRecord, digest, validate_chain},
};

impl CatalogOp<'_> {
    /// The terminal marker pins the registration hash, so a lost acknowledgment
    /// can be recovered without retaining the account chain or any device secrets.
    pub(super) fn check_account_deletion<'a>(
        &self,
        chain: &'a [AccountRecord],
        intent: &AccountDeletionIntent,
        proof: &AccountProof,
        operation: &str,
        digest: &str,
    ) -> Result<(&'a AccountRecord, bool)> {
        let record = validate_chain(chain)?;
        intent.validate(record)?;
        let home = self.logical_addr().id.to_string();
        ensure!(
            intent.home == home && proof.device == intent.device,
            "wrong deletion home or device"
        );
        proof.validate(record, operation, &home, digest)?;
        if let Some(retired) = deleted_registration(&self.store.db, &intent.account)? {
            ensure!(
                retired == intent.record_id,
                "account deleted at a different registration"
            );
            return Ok((record, true));
        }
        ensure!(
            self.store.account_chain(&intent.account)?.last() == Some(record),
            "account deletion requires the current registered device"
        );
        Ok((record, false))
    }

    pub(super) fn delete_account(
        &self,
        chain: &[AccountRecord],
        intent: &AccountDeletionIntent,
        proof: &AccountProof,
        deletes: &[SignedEvent],
    ) -> Result<Response> {
        ensure!(
            deletes.len() <= ACCOUNT_DELETE_BATCH,
            "oversized account deletion batch"
        );
        let (record, retired) = self.check_account_deletion(
            chain,
            intent,
            proof,
            "delete-account-v1",
            &digest(&(intent, deletes))?,
        )?;
        let account = &intent.account;
        if retired {
            return Ok(Response::AccountDeleted {
                account: account.clone(),
            });
        }
        // Acknowledge only after every previously authorized operation has committed.
        // The exclusive retry reads the registration and enumerates remaining targets again.
        self.disclosure.restrict_if(true)?;
        let tx = self.store.db.unchecked_transaction()?;
        for event in deletes {
            ensure!(
                event.payload.version == 4
                    && event.payload.account.as_deref() == Some(record)
                    && event.payload.home == intent.home
                    && event.payload.author == proof.device
                    && matches!(event.payload.action, Action::Delete { .. }),
                "invalid account deletion event"
            );
            event.validate()?;
            let envelope = Envelope::accept(event.clone(), &self.state.key)?;
            account_erasure::apply_proof(&tx, account, &envelope)?;
        }
        let targets = account_erasure::targets(&tx, account, ACCOUNT_DELETE_BATCH)?;
        let result = if targets.is_empty() {
            tx.execute(
                "INSERT INTO deleted_accounts VALUES(?,?)",
                [account, &intent.record_id],
            )?;
            account_erasure::purge(&tx, account)?;
            if tx.native().is_some() {
                crate::native_guard::stage_retirement(&tx, account, &intent.record_id)?;
            }
            Response::AccountDeleted {
                account: account.clone(),
            }
        } else {
            Response::AccountDeletePending {
                targets: targets
                    .into_iter()
                    .map(|(id, community)| AccountDeleteTarget { id, community })
                    .collect(),
            }
        };
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portability::AccountWallet;

    fn deletion(
        wallet: &AccountWallet,
        intent: &AccountDeletionIntent,
        deletes: Vec<SignedEvent>,
        check: bool,
        token: &str,
    ) -> Result<Request> {
        let (operation, hash) = if check {
            ("check-account-delete-v1", digest(intent)?)
        } else {
            ("delete-account-v1", digest(&(intent, &deletes))?)
        };
        let proof = AccountProof::create(wallet, operation, &intent.home, &hash)?;
        Ok(if check {
            Request::AccountDeleteAvailability {
                token: token.into(),
                community: "test".into(),
                chain: wallet.chain.clone(),
                intent: intent.clone(),
                proof,
            }
        } else {
            Request::AccountDelete {
                token: token.into(),
                community: "test".into(),
                chain: wallet.chain.clone(),
                intent: intent.clone(),
                proof,
                deletes,
            }
        })
    }

    #[tokio::test]
    async fn preflight_and_terminal_retry_require_current_root_and_designated_device() -> Result<()>
    {
        let dir = tempfile::tempdir()?;
        let cfg = federation_tests::config(dir.path().join("home"));
        let token = cfg.communities["test"].clone();
        let server = Server::start(cfg).await?;
        let home = server.endpoint.id().to_string();
        let initial = AccountWallet::create(new_secret(), home.clone())?;
        let stale = AccountDeletionIntent::create(&initial)?;
        let latest = initial.transition(home, true)?;
        let mut other_device = latest.clone();
        other_device.device_secret = initial.device_secret.clone();
        let intent = AccountDeletionIntent::create(&latest)?;
        let call = |req: Request| {
            server
                .state
                .operation(|op| op.handle("remote", req.clone()))
        };
        assert!(call(deletion(&latest, &intent, vec![], true, &token)?).is_err());
        server
            .state
            .operation(|op| op.store.register_account(&latest.chain))?;
        let mut forged = intent.clone();
        forged.signature = "00".repeat(64);
        for check in [true, false] {
            for (wallet, intent) in [
                (&initial, &stale),
                (&other_device, &intent),
                (&latest, &forged),
            ] {
                assert!(call(deletion(wallet, intent, vec![], check, &token)?).is_err());
            }
        }
        let check = deletion(&latest, &intent, vec![], true, &token)?;
        assert!(matches!(
            call(check.clone())?,
            Response::AccountDeleteAvailable { .. }
        ));
        assert!(
            server
                .state
                .operation(|op| Ok(op.store.account_chain(&intent.account)?.len()))?
                > 0
        );
        assert!(matches!(
            call(deletion(&latest, &intent, vec![], false, &token)?)?,
            Response::AccountDeleted { .. }
        ));
        assert!(matches!(
            call(check)?,
            Response::AccountDeleteAvailable { .. }
        ));
        assert!(call(deletion(&other_device, &intent, vec![], false, &token)?).is_err());
        assert!(call(deletion(&initial, &stale, vec![], false, &token)?).is_err());
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn finalization_waits_for_authorized_writes_and_batches_roll_back_together() -> Result<()>
    {
        use tokio::time::timeout;
        let dir = tempfile::tempdir()?;
        let cfg = federation_tests::config(dir.path().join("home"));
        let token = cfg.communities["test"].clone();
        let server = Server::start(cfg).await?;
        let home = server.endpoint.id().to_string();
        let wallet = AccountWallet::create(new_secret(), home.clone())?;
        let intent = AccountDeletionIntent::create(&wallet)?;
        server
            .state
            .operation(|op| op.store.register_account(&wallet.chain))?;
        let sign = |seq, action| {
            SignedEvent::create_for_account(
                &signing_key(&wallet.device_secret)?,
                home.clone(),
                "test".into(),
                seq,
                action,
                wallet.validate()?.clone(),
            )
        };
        let post = sign(
            1,
            Action::Post {
                body: "concurrent".into(),
                topic: "Plans".into(),
                display_name: "Anonymous neighbor".into(),
                anonymous: true,
            },
        )?;
        let request = Request::Submit {
            token: token.clone(),
            event: post.clone(),
            account_chain: wallet.chain.clone(),
        };
        let state = server.state.clone();
        let (started, running) = tokio::sync::oneshot::channel();
        let (release, waiting) = std::sync::mpsc::channel();
        let submit = tokio::spawn(async move {
            let mut started = Some(started);
            state
                .blocking(move |op| {
                    let result = op.handle("remote", request.clone())?;
                    if let Some(started) = started.take() {
                        started.send(()).unwrap();
                        waiting.recv_timeout(Duration::from_secs(8))?;
                    }
                    Ok(result)
                })
                .await
        });
        timeout(Duration::from_secs(8), running).await??;
        let discover = deletion(&wallet, &intent, vec![], false, &token)?;
        let mut next = Box::pin(server.state.dispatch("remote", discover));
        assert!(
            timeout(Duration::from_millis(100), &mut next)
                .await
                .is_err()
        );
        release.send(())?;
        submit.await??;
        assert!(
            matches!(next.await?, Response::AccountDeletePending { targets } if targets.len()==1 && targets[0].id==post.id)
        );
        let good = sign(
            2,
            Action::Delete {
                target: post.id.clone(),
            },
        )?;
        let bad = sign(
            3,
            Action::Delete {
                target: "ff".repeat(32),
            },
        )?;
        let fail = deletion(&wallet, &intent, vec![good.clone(), bad], false, &token)?;
        assert!(server.state.dispatch("remote", fail).await.is_err());
        let stored = server
            .state
            .operation(|op| op.store.existing(&post.id))?
            .unwrap();
        assert!(stored.deletion.is_none() && stored.event == post);
        let ok = deletion(&wallet, &intent, vec![good], false, &token)?;
        assert!(matches!(
            server.state.dispatch("remote", ok).await?,
            Response::AccountDeleted { .. }
        ));
        server.close().await;
        Ok(())
    }
}
