// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    backup,
    model::{SignedEvent, new_secret, signing_key},
    portability::AccountWallet,
};

fn event(
    wallet: &AccountWallet,
    home: &ed25519_dalek::SigningKey,
    community: &str,
    seq: u64,
    action: Action,
) -> Result<Envelope> {
    let record = wallet.validate()?;
    Envelope::accept(
        SignedEvent::create_for_account(
            &signing_key(&wallet.device_secret)?,
            record.payload.home.clone(),
            community.to_owned(),
            seq,
            action,
            record.clone(),
        )?,
        home,
    )
}

fn post(body: &str) -> Action {
    Action::Post {
        body: body.into(),
        anonymous: false,
        topic: "Plans".into(),
        display_name: "Neighbor".into(),
    }
}

#[test]
fn cross_community_proofs_purge_and_predeletion_archive_agree() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("home");
    let home = signing_key(&new_secret())?;
    let home_id = hex::encode(home.verifying_key().as_bytes());
    let wallet = AccountWallet::create(new_secret(), home_id.clone())?;
    let other = AccountWallet::create(new_secret(), home_id)?;
    let owner = &wallet.validate()?.payload.account;
    let mut store = Store::open(&path)?;
    store.register_account(&wallet.chain)?;
    store.register_account(&other.chain)?;
    let first = event(&wallet, &home, "one", 1, post("public opening"))?;
    let reply = event(
        &wallet,
        &home,
        "two",
        2,
        Action::Reply {
            parent: first.event.id.clone(),
            body: "reply opening".into(),
            display_name: "Neighbor".into(),
        },
    )?;
    let private = event(&wallet, &home, "three", 3, post("private opening"))?;
    let keep = event(&other, &home, "one", 1, post("surviving opening"))?;
    let vote = event(
        &wallet,
        &home,
        "one",
        4,
        Action::Vote {
            target: keep.event.id.clone(),
            value: 1,
        },
    )?;
    for e in [&first, &reply, &keep, &vote] {
        store.accept(e)?;
    }
    store.import_personal_events(owner, &[first.clone(), private.clone()])?;
    assert_eq!(targets(&store.db, owner, 2)?.len(), 2);
    assert_eq!(targets(&store.db, owner, 16)?.len(), 3);
    assert!(store.operation(|s| purge(&s.db, owner)).is_err());
    store.db.execute(
        "INSERT INTO public_profiles VALUES('one',?1,1,'old profile','{}')",
        [owner],
    )?;
    store.db.execute(
        "INSERT INTO username_claims VALUES('one',?1,'oldname')",
        [owner],
    )?;
    for (id, sender, recipient) in [
        ("sent", owner.as_str(), "other"),
        ("held", "other", owner.as_str()),
        ("keep", "other", "third"),
    ] {
        store.db.execute("INSERT INTO dm_mail(id,sender,recipient,packet,created,held) VALUES(?1,?2,?3,'encrypted',?4,1)",params![id,sender,recipient,crate::now()])?;
    }
    for sql in [
        "INSERT INTO dm_devices VALUES(?1,'device','identity')",
        "INSERT INTO dm_prekeys VALUES(?1,'one','bundle')",
        "INSERT INTO dm_claims VALUES('other',?1,1)",
        "INSERT INTO user_blocks VALUES(?1,'other','handle','label')",
        "INSERT INTO dm_approved VALUES('other',?1)",
        "INSERT INTO push_devices VALUES(?1,'device','ios','subscription','[]')",
        "INSERT INTO push_starts VALUES(?1,'device','message',1)",
        "INSERT INTO push_routes VALUES('token','other','dev','request',?1,0,0,0)",
        "INSERT INTO inbox_reads VALUES('other','message',?1,1)",
        "INSERT INTO privacy_settings VALUES(?1,'{}')",
    ] {
        store.db.execute(sql, [owner])?;
    }
    store.db.execute(
        "INSERT INTO inbox_mutes VALUES('other',?)",
        [&first.event.id],
    )?;
    let archive = dir.path().join("old.backup");
    store.backup(&archive)?;
    let delete = |target: &Envelope, n| {
        event(
            &wallet,
            &home,
            &target.event.payload.community,
            n,
            Action::Delete {
                target: target.event.id.clone(),
            },
        )
    };
    let wrong = delete(&keep, 5)?;
    assert!(
        store
            .operation(|s| apply_proof(&s.db, owner, &wrong))
            .is_err()
    );
    let proofs = [delete(&first, 6)?, delete(&reply, 7)?, delete(&private, 8)?];
    for proof in &proofs {
        store.operation(|s| apply_proof(&s.db, owner, proof))?;
        store.operation(|s| apply_proof(&s.db, owner, proof))?;
    }
    assert!(targets(&store.db, owner, 1)?.is_empty());
    assert!(store.existing(&proofs[2].event.id)?.is_none());
    let record = wallet.validate()?.id()?;
    store.operation(|s| {
        s.db.execute(
            "INSERT INTO deleted_accounts VALUES(?,?)",
            params![owner, record],
        )?;
        crate::native_guard::stage_retirement(&s.db, owner, &record)?;
        purge(&s.db, owner)
    })?;
    let verify = |s: &Store| -> Result<()> {
        for sql in [
            "SELECT count(*) FROM public_profiles WHERE owner=?",
            "SELECT count(*) FROM username_claims WHERE account=?",
            "SELECT count(*) FROM dm_mail WHERE sender=?1 OR recipient=?1",
            "SELECT count(*) FROM home_openings WHERE owner=?",
            "SELECT count(*) FROM account_states WHERE account=?",
            "SELECT count(*) FROM personal_events WHERE account=?",
            "SELECT count(*) FROM dm_devices WHERE owner=?",
            "SELECT count(*) FROM dm_prekeys WHERE owner=?",
            "SELECT count(*) FROM dm_claims WHERE owner=?1 OR peer=?1",
            "SELECT count(*) FROM dm_approved WHERE owner=?1 OR peer=?1",
            "SELECT count(*) FROM user_blocks WHERE owner=?1 OR peer=?1",
            "SELECT count(*) FROM push_devices WHERE owner=?",
            "SELECT count(*) FROM push_starts WHERE owner=?",
            "SELECT count(*) FROM push_routes WHERE owner=?1 OR target=?1",
            "SELECT count(*) FROM inbox_reads WHERE owner=?1 OR target=?1",
            "SELECT count(*) FROM privacy_settings WHERE owner=?",
            "SELECT count(*) FROM first_seen WHERE owner=?",
        ] {
            assert_eq!(
                s.db.query_row(sql, [owner], |r| r.get::<_, i64>(0))?,
                0,
                "{sql}"
            );
        }
        let packet: String =
            s.db.query_row("SELECT packet FROM dm_mail WHERE id='keep'", [], |r| {
                r.get(0)
            })?;
        assert_eq!(packet, "encrypted");
        assert_eq!(
            s.db.query_row(
                "SELECT count(*) FROM inbox_mutes WHERE target=?",
                [&first.event.id],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(s.existing(&keep.event.id)?, Some(keep.clone()));
        assert!(s.existing(&vote.event.id)?.is_none());
        for (e, p) in [(&first, &proofs[0]), (&reply, &proofs[1])] {
            let erased = s.existing(&e.event.id)?.unwrap();
            erased.validate()?;
            assert_eq!(erased.deletion.as_deref(), Some(p));
        }
        Ok(())
    };
    verify(&store)?;
    drop(store);
    let guard_path = path.with_added_extension("deletion-guard");
    let guard = rusqlite::Connection::open_with_flags(
        &guard_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    assert_eq!(
        guard.query_row(
            "SELECT count(*) FROM opening_keys WHERE owner=?",
            [owner],
            |r| r.get::<_, usize>(0)
        )?,
        0
    );
    assert_eq!(
        guard.query_row(
            "SELECT record_id FROM known_deleted_accounts WHERE account=?",
            [owner],
            |r| r.get::<_, String>(0)
        )?,
        record
    );
    drop(guard);
    let restored = dir.path().join("restored");
    backup::restore(&guard_path, &archive, &restored)?;
    verify(&Store::open(&restored)?)?;
    Ok(())
}
