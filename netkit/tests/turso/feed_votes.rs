// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use crate::{
    model::{Action, Envelope, LogEntry, SignedEvent, new_secret, now, project, signing_key},
    portability::AccountRecord,
};
use ed25519_dalek::{Signer, SigningKey};
use std::collections::BTreeSet;
use tempfile::tempdir;

fn id(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().as_bytes())
}

fn history() -> Result<Vec<Envelope>> {
    let home = signing_key(&new_secret())?;
    let a = signing_key(&new_secret())?;
    let b = signing_key(&new_secret())?;
    let root = signing_key(&new_secret())?;
    let account = AccountRecord::signed(&root, None, id(&home), BTreeSet::from([id(&a), id(&b)]))?;
    let post = Envelope::accept(
        SignedEvent::create(
            &home,
            id(&home),
            "scope".into(),
            1,
            Action::Post {
                body: "post".into(),
                topic: "Plans".into(),
                anonymous: true,
                display_name: "Anonymous neighbor".into(),
            },
        )?,
        &home,
    )?;
    let base = now() - 15;
    let vote = |key: &SigningKey, seq, offset, value, delegated, scope: &str| -> Result<Envelope> {
        let action = Action::Vote {
            target: post.event.id.clone(),
            value,
        };
        let event = if delegated {
            SignedEvent::create_for_account(
                key,
                id(&home),
                scope.into(),
                seq,
                action,
                account.clone(),
            )?
        } else {
            SignedEvent::create(key, id(&home), scope.into(), seq, action)?
        };
        let mut accepted = Envelope::accept(event, &home)?;
        accepted.accepted_at = base + offset;
        let signed = serde_json::to_vec(&(
            &accepted.event.id,
            &accepted.event.payload.home,
            accepted.accepted_at,
        ))?;
        accepted.home_signature = hex::encode(
            home.sign(&[b"yappers:home:v1\0".as_slice(), &signed].concat())
                .to_bytes(),
        );
        accepted.validate()?;
        Ok(accepted)
    };
    Ok(vec![
        post.clone(),
        vote(&a, 1, 9, 1, true, "scope")?,
        vote(&b, 1, 8, -1, true, "scope")?,
        vote(&a, 3, 3, 0, true, "scope")?, // Device A advances but loses to B's acceptance time.
        vote(&a, 2, 12, 1, true, "scope")?, // Later acceptance cannot supersede A's sequence 3.
        vote(&home, 2, 5, 1, false, "scope")?, // A separate owner; aggregate with the delegated vote.
        vote(&b, 2, 3, -1, true, "scope")?,    // Equal acceptance is resolved by device identity.
        vote(&home, 3, 1, -1, false, "elsewhere")?,
    ])
}

fn compare(store: &Store, events: &[Envelope], post: &str, viewer: &str) -> Result<()> {
    let expected = project(events, "scope", viewer, &BTreeSet::new());
    let post_view = expected.iter().find(|p| p.id == post).unwrap();
    let values = store.feed_votes(
        "scope",
        &[post.into(), "missing".into(), post.into()],
        viewer,
    )?;
    assert_eq!(values[post], (post_view.score, post_view.vote));
    assert_eq!(values["missing"], (0, 0));
    assert_eq!(
        store.feed_event_count("scope")?,
        events
            .iter()
            .filter(|e| e.event.payload.community == "scope")
            .count()
    );
    assert_eq!(
        store.feed_event_count("elsewhere")?,
        events
            .iter()
            .filter(|e| e.event.payload.community == "elsewhere")
            .count()
    );
    assert_eq!(store.feed_event_count("no events")?, 0);
    Ok(())
}

#[test]
fn native_votes_match_the_model_through_replays_removals_and_restore() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("home");
    let mut store = Store::open(&path)?;
    let plan = store
        .db
        .prepare(&format!("EXPLAIN QUERY PLAN {BATCH_VOTES}"))?
        .query_map(["[]", "scope", "viewer"], |r| r.get::<_, String>(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for keys in [
        "(community=? AND target=?)",
        "(community=? AND target=? AND owner=?)",
    ] {
        assert!(
            plan.iter()
                .any(|step| step.contains("SEARCH") && step.contains(keys)),
            "{plan:?}"
        );
    }
    let events = history()?;
    let post = &events[0].event.id;
    let owner = events[1].event.owner_id();
    let second = events[0].event.owner_id();
    let mut present = Vec::new();
    for (i, event) in events.iter().enumerate() {
        store.accept(event)?;
        present.push(event.clone());
        store.import_page(
            "peer",
            "generation",
            i as u64 + 1,
            &[LogEntry {
                cursor: i as u64 + 1,
                envelope: event.clone(),
                username: None,
            }],
        )?;
        compare(&store, &present, post, owner)?;
        compare(&store, &present, post, second)?;
    }
    let before = store.feed_event_count("scope")?;
    {
        let _tx = store.db.unchecked_transaction()?;
        store
            .db
            .execute("DELETE FROM events WHERE id=?", [&events[6].event.id])?;
    }
    assert_eq!(store.feed_event_count("scope")?, before);
    compare(&store, &present, post, owner)?;
    let archive = dir.path().join("backup");
    store.backup(&archive)?;
    drop(store);
    let restored = dir.path().join("restored");
    crate::backup::restore(
        &path.with_added_extension("deletion-guard"),
        &archive,
        &restored,
    )?;
    let store = Store::open(&restored)?;
    compare(&store, &present, post, owner)?;
    for index in [6, 3, 1, 2, 4, 5, 7] {
        store
            .db
            .execute("DELETE FROM events WHERE id=?", [&events[index].event.id])?;
        present.retain(|e| e.event.id != events[index].event.id);
        compare(&store, &present, post, owner)?;
        compare(&store, &present, post, second)?;
    }
    Ok(())
}

#[tokio::test]
async fn existing_native_votes_backfill_from_both_earlier_versions() -> Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};
    let events = history()?;
    let owner = events[1].event.owner_id();
    let projected = project(&events, "scope", owner, &BTreeSet::new());
    for version in [1, 2] {
        let dir = tempdir()?;
        let path = dir.path().join("previous.turso");
        {
            let db = turso::Builder::new_local(path.to_str().unwrap())
                .build()
                .await?;
            let conn = db.connect()?;
            conn.query("PRAGMA journal_mode=mvcc", ())
                .await?
                .next()
                .await?;
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            for sql in [
                include_str!("../../migrations/native/0001_domain.sql"),
                include_str!("../../migrations/native/0001_guard.sql"),
                include_str!("../../migrations/native/0001_content.sql"),
                include_str!("../../migrations/native/0001_cursors.sql"),
            ] {
                conn.execute_batch(sql).await?;
            }
            if version == 2 {
                conn.execute_batch(include_str!("../../migrations/native/0002_public_feed.sql"))
                    .await?;
            }
            for original in events.iter().rev() {
                let mut stored = original.clone();
                stored.event.payload = stored.event.header()?;
                conn.execute(
                    "INSERT INTO events(id,community,author,author_seq,envelope) VALUES(?,?,?,?,?)",
                    (
                        stored.event.id.as_str(),
                        stored.event.payload.community.as_str(),
                        stored.event.payload.author.as_str(),
                        stored.event.payload.sequence as i64,
                        serde_json::to_string(&stored)?,
                    ),
                )
                .await?;
            }
            conn.execute_batch(&format!(
                "PRAGMA application_id=0x5941504e; PRAGMA user_version={version}; COMMIT"
            ))
            .await?;
        }
        for file in fs::read_dir(dir.path())? {
            fs::set_permissions(file?.path(), fs::Permissions::from_mode(0o600))?;
        }
        let primary = crate::native_primary::Primary::open(&path).await?;
        let conn = primary.connect().await?;
        let row = conn
            .query(
                "SELECT s.score,v.value,c.event_count FROM feed_vote_scores s
            JOIN feed_owner_votes v ON v.community=s.community AND v.target=s.target
            JOIN feed_event_counts c ON c.community=s.community
            WHERE s.community='scope' AND s.target=? AND v.owner=?",
                (events[0].event.id.as_str(), owner),
            )
            .await?
            .next()
            .await?
            .unwrap();
        assert_eq!(row.get::<i64>(0)?, projected[0].score);
        assert_eq!(row.get::<i64>(1)?, i64::from(projected[0].vote));
        assert_eq!(row.get::<i64>(2)?, 7);
    }
    Ok(())
}
