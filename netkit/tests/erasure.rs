#![cfg(feature = "server")]
use anyhow::Result;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use std::collections::BTreeSet;
use tempfile::tempdir;
use yappers_netkit::{backup, model::*, portability::AccountWallet, store::Store};

fn accepted(home_secret: &str, wallet: &AccountWallet, n: u64, action: Action) -> Result<Envelope> {
    let record = wallet.validate()?;
    let event = SignedEvent::create_for_account(
        &signing_key(&wallet.device_secret)?,
        record.payload.home.clone(),
        "nearby".into(),
        n,
        action,
        record.clone(),
    )?;
    Envelope::accept(event, &signing_key(home_secret)?)
}

#[test]
fn full_home_still_accepts_an_owners_deletion() -> Result<()> {
    let dir = tempdir()?;
    let home = new_secret();
    let home_id = hex::encode(signing_key(&home)?.verifying_key().as_bytes());
    let wallet = AccountWallet::create(new_secret(), home_id)?;
    let path = dir.path().join("home.db");
    let mut store = Store::open(&path)?;
    let post = accepted(
        &home,
        &wallet,
        1,
        Action::Post {
            body: "Erase me at capacity".into(),
            anonymous: true,
            topic: "Plans".into(),
            display_name: "Anonymous neighbor".into(),
        },
    )?;
    store.accept(&post)?;
    // Fill unrelated log slots directly to exercise admission at its production bound.
    let db = rusqlite::Connection::open(&path)?;
    db.execute(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<?)
        INSERT INTO events(id, community, author, author_seq, envelope)
        SELECT 'fixture-' || i, 'other', 'fixture', i, '{}' FROM n",
        [yappers_netkit::store::MAX_EVENTS - 2],
    )?;
    let vote = accepted(
        &home,
        &wallet,
        2,
        Action::Vote {
            target: post.event.id.clone(),
            value: 1,
        },
    )?;
    assert!(store.accept(&vote).is_err());
    let deletion = accepted(
        &home,
        &wallet,
        3,
        Action::Delete {
            target: post.event.id.clone(),
        },
    )?;
    store.accept(&deletion)?;
    assert_eq!(
        store.existing(&post.event.id)?.unwrap().deletion.as_deref(),
        Some(&deletion)
    );
    Ok(())
}

#[test]
fn erased_payload_stays_gone_across_replay_new_peers_private_import_and_backup_restore()
-> Result<()> {
    let dir = tempdir()?;
    let home = new_secret();
    let home_id = hex::encode(signing_key(&home)?.verifying_key().as_bytes());
    let first = AccountWallet::create(new_secret(), home_id.clone())?;
    let owner = first.validate()?.payload.account.clone();
    let replacement = first.transition(home_id.clone(), false)?;
    let db = dir.path().join("home.db");
    let backup_path = dir.path().join("before.backup");
    let stale = dir.path().join("stale.db");
    let mut store = Store::open(&db)?;
    store.register_account(&first.chain)?;
    let create = accepted(
        &home,
        &first,
        1,
        Action::PostV2 {
            body: "sensitive_body_marker".into(),
            headline: "sensitive_headline_marker".into(),
            profile: Some(Box::new(CommunityProfile {
                photo: "ThumbnailMarker".into(),
                ..Default::default()
            })),
            area: Some(PostArea {
                label: "SensitiveLocation".into(),
                lat100: 123,
                lon100: -456,
            }),
            anonymous: false,
            topic: "Friends".into(),
            display_name: "BylineMarker".into(),
        },
    )?;
    let reply = accepted(
        &home,
        &first,
        2,
        Action::Reply {
            parent: create.event.id.clone(),
            body: "sensitive_reply_marker".into(),
            display_name: "Neighbor".into(),
        },
    )?;
    let live = accepted(
        &home,
        &first,
        3,
        Action::Post {
            body: "A surviving post".into(),
            anonymous: true,
            topic: "Plans".into(),
            display_name: "Anonymous neighbor".into(),
        },
    )?;
    for e in [&create, &reply, &live] {
        store.accept(e)?;
        store.import_personal_events(&owner, std::slice::from_ref(e))?;
    }
    store.backup(&backup_path)?;
    std::fs::copy(&db, &stale)?;
    let guard = db.with_added_extension("deletion-guard");
    let mut offline = Store::open(&dir.path().join("offline.db"))?;
    offline.accept(&create)?;
    let other = AccountWallet::create(new_secret(), home_id.clone())?;
    let wrong = accepted(
        &home,
        &other,
        1,
        Action::Delete {
            target: create.event.id.clone(),
        },
    )?;
    // Unrelated valid signatures cannot redact the target.
    store.accept(&wrong)?;
    assert_eq!(store.existing(&create.event.id)?.unwrap(), create);
    store.register_account(&replacement.chain)?;
    let delete = accepted(
        &home,
        &replacement,
        1,
        Action::Delete {
            target: create.event.id.clone(),
        },
    )?;
    let delete_reply = accepted(
        &home,
        &replacement,
        2,
        Action::Delete {
            target: reply.event.id.clone(),
        },
    )?;
    store.accept(&delete)?;
    store.accept(&delete_reply)?;
    let markers = [
        &*create.event.payload.nonce,
        &*reply.event.payload.nonce,
        "sensitive_body_marker",
        "sensitive_headline_marker",
        "sensitive_reply_marker",
        "ThumbnailMarker",
        "SensitiveLocation",
        "BylineMarker",
    ];
    // Even someone who can unwrap the backup image with the current guard
    // cannot recover an erased opening from either the feed or private history.
    let guard_db = rusqlite::Connection::open(&guard)?;
    for (event, expected_keys) in [(&create, 0), (&reply, 0), (&live, 2)] {
        let keys: usize = guard_db.query_row(
            "SELECT COUNT(*) FROM opening_keys WHERE target=?",
            [&event.event.id],
            |row| row.get(0),
        )?;
        assert_eq!(keys, expected_keys);
    }
    let key: Vec<u8> = guard_db.query_row("SELECT key FROM state", [], |row| row.get(0))?;
    let backup_bytes = std::fs::read(&backup_path)?;
    let sealed = backup_bytes.strip_prefix(b"YAP-HOME-BACKUP-1\0").unwrap();
    let (nonce, encrypted) = sealed.split_at(24);
    let image = XChaCha20Poly1305::new_from_slice(&key)
        .unwrap()
        .decrypt(XNonce::from_slice(nonce), encrypted)
        .unwrap();
    for marker in markers {
        assert!(!image.windows(marker.len()).any(|b| b == marker.as_bytes()));
    }
    let redacted = store.existing(&create.event.id)?.unwrap();
    redacted.validate()?;
    assert_eq!(redacted.event.id, create.event.id);
    assert_eq!(redacted.home_signature, create.home_signature);
    assert_eq!(redacted.deletion.as_deref(), Some(&delete));
    assert_ne!(redacted.event.payload.author, delete.event.payload.author);
    assert!(redacted.event.payload.nonce.is_empty());
    let mut forged = redacted.clone();
    forged.deletion = Some(Box::new(wrong));
    assert!(forged.validate().is_err());
    forged.deletion = None;
    assert!(forged.validate().is_err());
    let mut altered = redacted.clone();
    altered.event.payload.nonce = new_secret();
    assert!(altered.validate().is_err());
    let mut fresh = Store::open(&dir.path().join("fresh.db"))?;
    let scopes = BTreeSet::from(["nearby".to_string()]);
    for i in 0..6 {
        let Response::Page { entries, next, .. } = store.page(i, 1, &scopes, "generation")? else {
            unreachable!()
        };
        for marker in markers {
            assert!(!serde_json::to_string(&entries)?.contains(marker));
        }
        fresh.import_page("peer", "generation", next, &entries)?;
        offline.import_page("peer", "generation", next, &entries)?;
    }
    let restored_path = dir.path().join("restored.db");
    backup::restore(&guard, &backup_path, &restored_path)?;
    let mut restored = Store::open(&restored_path)?;
    assert_eq!(restored.account_chain(&owner)?, replacement.chain);
    assert_eq!(restored.existing(&live.event.id)?, Some(live.clone()));
    // A raw database rollback is rejected even when pointed at the correct current guard.
    std::os::unix::fs::symlink(&guard, stale.with_added_extension("deletion-guard"))?;
    assert!(Store::open(&stale).is_err());
    for replica in [&mut store, &mut restored, &mut offline, &mut fresh] {
        replica.accept(&create)?;
        replica.import_personal_events(&owner, std::slice::from_ref(&create))?;
        for e in replica.owned_events(&owner)? {
            e.validate()?;
            for marker in markers {
                assert!(!serde_json::to_string(&e)?.contains(marker));
            }
        }
        assert_eq!(replica.existing(&create.event.id)?, Some(redacted.clone()));
    }
    for path in [&db, &guard, &restored_path] {
        let raw = std::fs::read(path)?;
        for marker in markers {
            assert!(
                !raw.windows(marker.len()).any(|b| b == marker.as_bytes()),
                "remnant in {path:?}: {marker}"
            );
        }
        for suffix in ["-wal", "-journal"] {
            assert!(!std::path::PathBuf::from(format!("{}{suffix}", path.display())).exists());
        }
    }
    let early_path = dir.path().join("early.db");
    let mut early = Store::open(&early_path)?;
    early.accept(&delete)?;
    early.accept(&create)?;
    assert_eq!(early.existing(&create.event.id)?, Some(redacted));
    Ok(())
}
