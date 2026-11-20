// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    model::{SignedEvent, signing_key},
    portability::AccountWallet,
    store::Store,
};
use std::{
    fs::{self, File},
    os::unix::fs::OpenOptionsExt,
};
use tempfile::tempdir;

fn deletion(target: &str) -> Result<Envelope> {
    let signer = signing_key(&new_secret())?;
    Envelope::accept(
        SignedEvent::create(
            &signer,
            hex::encode(signer.verifying_key().as_bytes()),
            "nearby".into(),
            1,
            Action::Delete {
                target: target.to_owned(),
            },
        )?,
        &signer,
    )
}

#[test]
fn unsigned_or_forked_transition_never_retires_a_key() -> Result<()> {
    let dir = tempdir()?;
    let catalog = dir.path().join("catalog");
    let path = catalog.with_added_extension("deletion-guard");
    assert!(NativeGuard::open(&path).is_err());
    crate::backup::attach(&mut Connection::open_in_memory()?, &catalog)?;
    let authority = NativeGuard::open(&path)?;
    let base = authority.stamp()?;
    let target = new_secret();
    let proof = deletion(&target)?;
    let scope = Scope {
        target: &target,
        owner: proof.event.owner_id(),
        community: "nearby",
        private: false,
    };
    let private = Scope {
        private: true,
        ..scope
    };
    let other = Scope {
        target: "other",
        ..scope
    };
    let live_key = authority.reserve_key(&base, scope)?;
    let private_key = authority.reserve_key(&base, private)?;
    let survivor = authority.reserve_key(&base, other)?;
    assert_ne!(live_key, private_key);
    assert_eq!(authority.reserve_key(&base, scope)?, live_key);
    let mut op = authority.prepare(
        base.clone(),
        vec![Intent::Delete {
            proof: Box::new(proof.clone()),
            private: true,
        }],
    )?;
    let fork = authority.prepare(base.clone(), vec![Intent::Cutover])?;
    // Possessing the ciphertext catalog cannot expand a private-only proof
    // into a public retirement, or create a different next head.
    if let Intent::Delete { private, .. } = &mut op.intents[0] {
        *private = false;
    }
    assert!(
        authority
            .finalize(&op.after, std::slice::from_ref(&op))
            .is_err()
    );
    assert_eq!(authority.stamp()?, base);
    assert_eq!(authority.key(&base, private)?, private_key);
    if let Intent::Delete { private, .. } = &mut op.intents[0] {
        *private = true;
    }
    authority.finalize(&op.after, std::slice::from_ref(&op))?;
    authority.finalize(&op.after, std::slice::from_ref(&op))?;
    assert!(
        authority
            .finalize(&fork.after, std::slice::from_ref(&fork))
            .is_err()
    );
    assert!(authority.verify(&base).is_err());
    assert!(authority.reserve_key(&op.after, private).is_err());
    assert_eq!(authority.key(&op.after, scope)?, live_key);
    let public_delete = authority.prepare(
        op.after,
        vec![Intent::Delete {
            proof: Box::new(proof.clone()),
            private: false,
        }],
    )?;
    authority.finalize(&public_delete.after, std::slice::from_ref(&public_delete))?;
    let current = authority.stamp()?;
    assert!(authority.reserve_key(&current, scope).is_err());
    assert!(authority.reserve_key(&current, private).is_err());
    assert_eq!(authority.key(&current, other)?, survivor);
    authority.fence();
    assert!(authority.key(&current, other).is_err());
    assert!(authority.read_gate().is_err());
    assert!(authority.publication().is_err());
    drop(authority);
    let reopened = NativeGuard::open(&path)?;
    assert_eq!(reopened.stamp()?, current);
    assert_eq!(reopened.key(&current, other)?, survivor);
    Ok(())
}

#[test]
fn legacy_epoch_change_fences_a_native_branch_and_preserves_current_keys() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("catalog");
    let mut legacy = Connection::open_in_memory()?;
    crate::backup::attach(&mut legacy, &path)?;
    let authority = NativeGuard::open(&path.with_added_extension("deletion-guard"))?;
    let old = authority.stamp()?;
    let scope = Scope {
        target: "test",
        owner: "owner",
        community: "nearby",
        private: false,
    };
    let key = authority.reserve_key(&old, scope)?;
    let abandoned = authority.prepare(old.clone(), vec![Intent::Cutover])?;
    let tx = legacy.transaction()?;
    tx.execute("UPDATE guard.state SET epoch=epoch+1 WHERE singleton=1", [])?;
    tx.execute(
        "UPDATE deletion_epoch SET epoch=epoch+1 WHERE singleton=1",
        [],
    )?;
    tx.commit()?;
    let from_legacy = authority.stamp()?;
    assert_eq!(from_legacy.guard_id, old.guard_id);
    assert_eq!(from_legacy.epoch, abandoned.after.epoch);
    assert_ne!(from_legacy.token, abandoned.after.token);
    assert!(
        authority
            .finalize(&abandoned.after, std::slice::from_ref(&abandoned))
            .is_err()
    );
    assert!(authority.verify(&old).unwrap_err().is::<StaleSnapshot>());
    assert_eq!(authority.key(&from_legacy, scope)?, key);
    let home = hex::encode(signing_key(&new_secret())?.verifying_key().as_bytes());
    let wallet = AccountWallet::create(new_secret(), home.clone())?;
    let moved = wallet.transition(home.clone(), true)?;
    let move_op = authority.prepare(from_legacy, vec![Intent::Account { chain: moved.chain }])?;
    authority.finalize(&move_op.after, std::slice::from_ref(&move_op))?;
    let fork = wallet.transition(home, true)?;
    for invalid in [wallet.chain, fork.chain] {
        assert!(
            authority
                .prepare(
                    move_op.after.clone(),
                    vec![Intent::Account { chain: invalid }]
                )
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn rolled_back_post_can_retry_but_cannot_resurrect_a_retired_key() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("catalog");
    let mut store = Store::open(&path)?;
    let signer = signing_key(&new_secret())?;
    let home = hex::encode(signer.verifying_key().as_bytes());
    let accepted = |seq, action| {
        Envelope::accept(
            SignedEvent::create(&signer, home.clone(), "nearby".into(), seq, action)?,
            &signer,
        )
    };
    let post = accepted(
        1,
        Action::Post {
            body: "rolled back opening".into(),
            anonymous: true,
            topic: "Plans".into(),
            display_name: "Anonymous neighbor".into(),
        },
    )?;
    let id = &post.event.id;
    let scope = Scope {
        target: id,
        owner: post.event.owner_id(),
        community: "nearby",
        private: false,
    };
    let base = head(&store.db)?;
    store.db.begin_operation()?;
    store.accept(&post)?;
    let reserved = guard(&store.db)?.key(&base, scope)?;
    store.db.rollback_operation()?;
    assert!(store.existing(id)?.is_none());
    assert_eq!(
        store.db.query_row(
            "SELECT count(*) FROM home_openings WHERE target=?",
            [id],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    assert!(guard(&store.db)?.key(&base, scope).is_err());
    store.accept(&post)?;
    assert_ne!(guard(&store.db)?.key(&base, scope)?, reserved);
    let deleted = accepted(2, Action::Delete { target: id.clone() })?;
    store.accept(&deleted)?;
    store.accept(&post)?;
    assert_eq!(
        store.existing(id)?.unwrap().deletion.as_deref(),
        Some(&deleted)
    );
    assert!(
        guard(&store.db)?
            .reserve_key(&head(&store.db)?, scope)
            .is_err()
    );

    let durable = accepted(3, post.event.payload.action.clone())?;
    let abandoned = accepted(4, post.event.payload.action.clone())?;
    // Leave both reservation records across reopen, with only one committed body.
    store.db.begin_operation()?;
    store.accept(&durable)?;
    store.db.execute_batch("COMMIT")?;
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.existing(&durable.event.id)?, Some(durable));
    store.db.begin_operation()?;
    store.accept(&abandoned)?;
    store.db.execute_batch("ROLLBACK")?;
    drop(store);
    let store = Store::open(&path)?;
    assert!(store.existing(&abandoned.event.id)?.is_none());
    let authority = guard(&store.db)?.writer()?;
    assert_eq!(
        authority.query_row("SELECT count(*) FROM opening_reservations", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        authority.query_row("SELECT count(*) FROM opening_keys", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[test]
fn startup_replays_only_committed_native_guard_intents() -> Result<()> {
    use std::{env, process::Command};
    const PATH: &str = "YAP_NATIVE_GUARD_CRASH_TEST_DIR";
    const MODE: &str = "YAP_NATIVE_GUARD_CRASH_TEST_MODE";
    if let Some(dir) = env::var_os(PATH) {
        let dir = PathBuf::from(dir);
        let post: Envelope = serde_json::from_slice(&fs::read(dir.join("post"))?)?;
        let proof: Envelope = serde_json::from_slice(&fs::read(dir.join("proof"))?)?;
        let chain: Vec<AccountRecord> = serde_json::from_slice(&fs::read(dir.join("chain"))?)?;
        let mut store = Store::open(&dir.join("home"))?;
        store.db.begin_operation()?;
        store.accept(&proof)?;
        store.register_account(&chain)?;
        store.remember_deliveries("recorded-peer", [("nearby", post.event.id.as_str())])?;
        before_commit(&store.db)?;
        assert!(reconcile(&store.db).is_err());
        if env::var(MODE)?.as_str() == "committed" {
            // Crash precisely after durable native COMMIT, before after_commit.
            store.db.execute_batch("COMMIT")?;
        }
        std::process::exit(53);
    }
    for committed in [false, true] {
        let dir = tempdir()?;
        let path = dir.path().join("home");
        let signer = signing_key(&new_secret())?;
        let home = hex::encode(signer.verifying_key().as_bytes());
        let event = |seq, action| {
            Envelope::accept(
                SignedEvent::create(&signer, home.clone(), "nearby".into(), seq, action)?,
                &signer,
            )
        };
        let post = event(
            1,
            Action::Post {
                body: "guard crash regression".into(),
                anonymous: true,
                topic: "Plans".into(),
                display_name: "Anonymous neighbor".into(),
            },
        )?;
        let id = &post.event.id;
        let proof = event(2, Action::Delete { target: id.clone() })?;
        let wallet = AccountWallet::create(new_secret(), home.clone())?;
        fs::write(dir.path().join("post"), serde_json::to_vec(&post)?)?;
        fs::write(dir.path().join("proof"), serde_json::to_vec(&proof)?)?;
        fs::write(dir.path().join("chain"), serde_json::to_vec(&wallet.chain)?)?;
        let mut store = Store::open(&path)?;
        store.accept(&post)?;
        store.import_personal_events(post.event.owner_id(), std::slice::from_ref(&post))?;
        let base = head(&store.db)?;
        drop(store);
        // The MVCC log replaces the final extension (home.primary.db-log).
        let prefix = "home.primary.";
        let mut old = Vec::new();
        for entry in fs::read_dir(dir.path())? {
            let file = entry?;
            if file.file_name().to_string_lossy().starts_with(prefix) {
                old.push((file.path(), fs::read(file.path())?));
            }
        }
        assert!(old.iter().any(|(p, _)| p.ends_with("home.primary.db-log")));
        let status = Command::new(env::current_exe()?)
            .args([
                "--exact",
                "native_guard::tests::startup_replays_only_committed_native_guard_intents",
            ])
            .env(PATH, dir.path())
            .env(
                MODE,
                if committed {
                    "committed"
                } else {
                    "uncommitted"
                },
            )
            .status()?;
        assert_eq!(status.code(), Some(53));
        // Inspect the independent authority before any startup reconciliation.
        let check = NativeGuard::open(&path.with_added_extension("deletion-guard"))?;
        assert_eq!(check.stamp()?, base);
        drop(check);
        let store = Store::open(&path)?;
        let authority = guard(&store.db)?;
        let current = authority.stamp()?;
        assert_eq!(current.epoch, base.epoch + i64::from(committed));
        let found = store.existing(id)?.unwrap();
        assert_eq!(found.deletion.as_deref(), committed.then_some(&proof));
        authority.pinned(|db, _| {
            for (table, expected) in [
                ("known_deletions", usize::from(committed)),
                ("current_accounts", usize::from(committed)),
                ("known_deliveries", usize::from(committed)),
                ("opening_keys", if committed { 0 } else { 2 }),
            ] {
                assert_eq!(
                    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, usize>(0))?,
                    expected,
                    "{table}"
                );
            }
            Ok(())
        })?;
        reconcile(&store.db)?;
        assert_eq!(authority.stamp()?, current);
        drop(store);
        if committed {
            // Roll back the entire raw native file set, leaving the independent
            // guard and locator intact. The old headers must never be served.
            for entry in fs::read_dir(dir.path())? {
                let file = entry?;
                if file.file_name().to_string_lossy().starts_with(prefix) {
                    fs::remove_file(file.path())?;
                }
            }
            for (file, bytes) in old {
                use std::io::Write;
                let mut out = File::options()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(file)?;
                out.write_all(&bytes)?;
            }
            let err = Store::open(&path)
                .err()
                .context("stale raw native Home unexpectedly opened")?;
            assert!(format!("{err:#}").contains("guard"), "{err:#}");
        }
    }
    Ok(())
}
