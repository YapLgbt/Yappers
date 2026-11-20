// SPDX-License-Identifier: AGPL-3.0-only
//! Native Home keeps the signed header and the immutable sealed opening in
//! one Turso transaction. Only the independent guard supplies an opening key.
use crate::{
    MAX_FRAME,
    home_sql::HomeDb,
    model::{Action, Envelope},
};
use anyhow::{Context, Result, ensure};
use rusqlite::params;

pub(crate) const SCHEMA: &str = include_str!("../migrations/native/0001_content.sql");

pub(crate) fn is_content(action: &Action) -> bool {
    matches!(
        action,
        Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
    )
}

/// Separate the body, inbox routing and receipt encryption domains even when
/// importing an event whose key already protects a historical portable backup.
fn content_key(key: &[u8; 32], header: &Envelope, private: bool, domain: &str) -> Result<[u8; 32]> {
    let mut hash = blake3::Hasher::new_keyed(key);
    hash.update(&serde_json::to_vec(&(domain, private, header))?);
    Ok(*hash.finalize().as_bytes())
}

/// Insert only after duplicate and deletion reconciliation, inside the
/// transaction that inserts the returned header at its authoritative cursor.
pub(crate) fn encode(db: &HomeDb, event: &Envelope, private: bool) -> Result<String> {
    if db.native().is_none() || !is_content(&event.event.payload.action) {
        return Ok(serde_json::to_string(event)?);
    }
    ensure!(
        !db.is_autocommit()?,
        "opening publication requires a transaction"
    );
    ensure!(
        event.event.payload.version == 4 && event.deletion.is_none(),
        "invalid live opening"
    );
    event.validate()?;
    let mut header = event.clone();
    header.event.payload = event.event.header()?;
    let raw = serde_json::to_vec(&(&event.event.payload.nonce, &event.event.payload.action))?;
    ensure!(raw.len() <= MAX_FRAME, "event opening exceeds frame limit");
    let key = crate::native_guard::opening_key(db, event, private, true)?;
    let sealed = crate::backup::seal(
        &raw,
        &content_key(&key, &header, private, "home opening v1")?,
    )?;
    // A reply can arrive before its parent. Keep enough authenticated routing
    // data to match it later without opening its body or retaining its link
    // or anonymity bit in the native WAL after key destruction.
    let routing = match &event.event.payload.action {
        Action::Reply {
            parent,
            display_name,
            ..
        } if !private => Some(crate::backup::seal(
            &serde_json::to_vec(&(parent, display_name == "Anonymous neighbor"))?,
            &content_key(&key, &header, private, "home reply route v1")?,
        )?),
        _ => None,
    };
    db.execute(
        "INSERT INTO home_openings(target,private,owner,community,sealed,routing) VALUES(?,?,?,?,?,?)",
        params![event.event.id, private, event.event.owner_id(), event.event.payload.community, sealed, routing],
    )?;
    Ok(serde_json::to_string(&header)?)
}

pub(crate) fn hydrate(db: &HomeDb, stored_json: &str, private: bool) -> Result<Envelope> {
    let event: Envelope = serde_json::from_str(stored_json)?;
    if db.native().is_none() {
        return Ok(event);
    }
    ensure!(
        !is_content(&event.event.payload.action),
        "plaintext opening in native catalog"
    );
    if event.deletion.is_some() || !matches!(event.event.payload.action, Action::Erased { .. }) {
        return Ok(event);
    }
    let key = crate::native_guard::opening_key(db, &event, private, false)?;
    let sealed: Vec<u8> = db.query_row(
        "SELECT sealed FROM home_openings WHERE target=? AND private=? AND owner=? AND community=?
           AND length(sealed) BETWEEN 40 AND ?",
        params![event.event.id, private, event.event.owner_id(), event.event.payload.community, MAX_FRAME + 40],
        |r| r.get(0),
    ).context("missing or invalid signed opening")?;
    open_body(event, private, &key, &sealed)
}

fn open_body(
    mut event: Envelope,
    private: bool,
    key: &[u8; 32],
    sealed: &[u8],
) -> Result<Envelope> {
    // encode is the sole writer in this domain, including logical import.
    // It verifies the author, account and Home before sealing. Both signatures
    // and all header fields enter the AEAD key derivation. Opening a different
    // header, a different scope or a modified body fails authentication.
    let raw = crate::backup::unseal(
        sealed,
        &content_key(key, &event, private, "home opening v1")?,
    )?;
    let (nonce, action) = serde_json::from_slice(&raw)?;
    ensure!(is_content(&action), "invalid opening action");
    let signed_header = event.event.payload.clone();
    event.event.payload.nonce = nonce;
    event.event.payload.action = action;
    ensure!(
        event.event.header()? == signed_header,
        "opening does not match signed header"
    );
    // The only validation predicate that can change since ingest.
    ensure!(
        event.accepted_at <= crate::model::now() + 300,
        "invalid home clock"
    );
    Ok(event)
}

/// The caller supplies scope-filtered public headers in page order. Publish
/// replacements only after the entire page succeeds. A standalone caller may
/// also use this outside the server's enclosing operation transaction.
pub(crate) fn hydrate_public_page(db: &HomeDb, events: &mut [Envelope]) -> Result<()> {
    let native = db.native().is_some();
    let mut live = Vec::new();
    for (index, event) in events.iter().enumerate() {
        ensure!(
            !native || !is_content(&event.event.payload.action),
            "plaintext opening in native catalog"
        );
        if native
            && event.deletion.is_none()
            && matches!(event.event.payload.action, Action::Erased { .. })
        {
            live.push((index, event));
        } else {
            // Neither an inline envelope nor a deletion proof is authenticated
            // by a live opening. Verify its signatures directly.
            event.validate()?;
        }
    }
    if !native {
        return Ok(());
    }
    let tx = if db.is_autocommit()? {
        Some(db.unchecked_transaction()?)
    } else {
        None
    };
    if live.is_empty() {
        crate::native_guard::verify_current(db)?;
    }
    let mut opened = Vec::with_capacity(live.len());
    for chunk in live.chunks(crate::MAX_PAGE_ENTRIES as usize) {
        let headers: Vec<_> = chunk.iter().map(|&(_, h)| h).collect();
        crate::native_guard::with_public_opening_keys(db, &headers, |keys| {
            let scopes: Vec<_> = headers
                .iter()
                .map(|h| (&h.event.id, h.event.owner_id(), &h.event.payload.community))
                .collect();
            let mut stmt = db.prepare_cached(
                "SELECT q.key,o.sealed FROM json_each(?1) q CROSS JOIN home_openings o
                 WHERE o.target=json_extract(q.value,'$[0]') AND o.private=0
                   AND o.owner=json_extract(q.value,'$[1]') AND o.community=json_extract(q.value,'$[2]')
                   AND length(o.sealed) BETWEEN 40 AND ?2"
            )?;
            let mut rows = stmt.query(params![serde_json::to_string(&scopes)?, MAX_FRAME + 40])?;
            let before = opened.len();
            while let Some(row) = rows.next()? {
                let ordinal: usize = row.get(0)?;
                let (index, header) = chunk[ordinal];
                opened.push((
                    index,
                    open_body(
                        header.clone(),
                        false,
                        &keys[ordinal],
                        &row.get::<_, Vec<u8>>(1)?,
                    )?,
                ));
            }
            ensure!(
                opened.len() - before == chunk.len(),
                "missing or invalid signed opening"
            );
            Ok(())
        })?;
    }
    if let Some(tx) = tx {
        tx.commit()?;
    }
    for (index, event) in opened {
        events[index] = event;
    }
    Ok(())
}

pub(crate) fn reply_route(db: &HomeDb, header: &Envelope, sealed: &[u8]) -> Result<(String, bool)> {
    ensure!(
        sealed.len() <= 256
            && header.deletion.is_none()
            && matches!(
                header.event.payload.action,
                Action::Erased { reply: true, .. }
            ),
        "invalid reply routing record"
    );
    let key = crate::native_guard::opening_key(db, header, false, false)?;
    Ok(serde_json::from_slice(&crate::backup::unseal(
        sealed,
        &content_key(&key, header, false, "home reply route v1")?,
    )?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SignedEvent, new_secret, signing_key, store::Store};

    #[test]
    fn public_page_authenticates_live_openings_and_verifies_inline_rows() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let signer = signing_key(&new_secret())?;
        let home = hex::encode(signer.verifying_key().as_bytes());
        let accept = |seq, action| {
            Envelope::accept(
                SignedEvent::create(&signer, home.clone(), "nearby".into(), seq, action)?,
                &signer,
            )
        };
        let post = accept(
            1,
            Action::Post {
                body: "page opening".into(),
                anonymous: true,
                topic: "Plans".into(),
                display_name: "Anonymous neighbor".into(),
            },
        )?;
        let reply = accept(
            2,
            Action::Reply {
                body: "reply opening".into(),
                parent: post.event.id.clone(),
                display_name: "neighbor".into(),
            },
        )?;
        let vote = accept(
            3,
            Action::Vote {
                target: post.event.id.clone(),
                value: 1,
            },
        )?;
        store.accept(&post)?;
        store.accept(&reply)?;
        let header = |e: &Envelope| -> Result<Envelope> {
            let mut h = e.clone();
            h.event.payload = e.event.header()?;
            Ok(h)
        };
        let a = header(&post)?;
        let b = header(&reply)?;
        // Repeated scopes straddle the SQL batch boundary and retain page order.
        let mut page = vec![b.clone(); crate::MAX_PAGE_ENTRIES as usize];
        page.extend([vote.clone(), a.clone()]);
        hydrate_public_page(&store.db, &mut page)?;
        assert_eq!(
            &page[..crate::MAX_PAGE_ENTRIES as usize],
            vec![reply.clone(); crate::MAX_PAGE_ENTRIES as usize]
        );
        assert_eq!(
            &page[crate::MAX_PAGE_ENTRIES as usize..],
            &[vote.clone(), post.clone()]
        );

        let mut bad_vote = vote;
        bad_vote.home_signature = post.home_signature.clone();
        let mut bad_header = b.clone();
        bad_header.event.signature = post.event.signature.clone();
        for invalid in [bad_vote, bad_header, post.clone()] {
            let original = vec![a.clone(), invalid];
            let mut page = original.clone();
            assert!(hydrate_public_page(&store.db, &mut page).is_err());
            assert_eq!(page, original);
        }

        // An invalid imported signature cannot mint an authenticated opening.
        let mut forged = reply;
        forged.home_signature = post.home_signature.clone();
        let tx = store.db.unchecked_transaction()?;
        assert!(encode(&tx, &forged, true).is_err());
        // Even a genuine ciphertext for another verified row cannot be spliced.
        tx.execute("UPDATE home_openings SET sealed=(SELECT sealed FROM home_openings WHERE target=? AND private=0) WHERE target=? AND private=0", params![post.event.id, forged.event.id])?;
        let mut spliced = [a.clone(), b.clone()];
        assert!(hydrate_public_page(&tx, &mut spliced).is_err());
        assert_eq!(spliced, [a, b]);
        Ok(())
    }

    #[test]
    fn deleted_reply_loses_body_routing_and_private_keys() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let dbpath = dir.path().join("home");
        let mut store = Store::open(&dbpath)?;
        ensure!(
            store.db.native().is_some(),
            "test requires the native primary"
        );
        let author = signing_key(&new_secret())?;
        let home = hex::encode(author.verifying_key().as_bytes());
        let accept = |seq, action| {
            Envelope::accept(
                SignedEvent::create(&author, home.clone(), "scope".to_string(), seq, action)?,
                &author,
            )
        };
        let parent = new_secret(); // Never ingested; only the reply knows its ID.
        let reply = accept(
            1,
            Action::Reply {
                parent: parent.clone(),
                body: "native-content-body-marker".into(),
                display_name: "native-content-byline-marker".into(),
            },
        )?;
        let id = &reply.event.id;
        store.accept(&reply)?;
        store.import_personal_events(&home, std::slice::from_ref(&reply))?;
        let header: String =
            store
                .db
                .query_row("SELECT envelope FROM events WHERE id=?", [id], |r| r.get(0))?;
        let route: Vec<u8> = store.db.query_row(
            "SELECT routing FROM home_openings WHERE target=? AND private=0",
            [id],
            |r| r.get(0),
        )?;
        let bare = serde_json::from_str(&header)?;
        assert_eq!(
            reply_route(&store.db, &bare, &route)?,
            (parent.clone(), false)
        );
        assert_eq!(hydrate(&store.db, &header, false)?, reply);
        assert_eq!(hydrate(&store.db, &header, true)?, reply);
        for entry in std::fs::read_dir(dir.path())? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            for marker in [
                &*parent,
                &reply.event.payload.nonce,
                "native-content-body-marker",
                "native-content-byline-marker",
            ] {
                assert!(
                    !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
                    "plaintext in {path:?}"
                );
            }
        }
        let proof = accept(2, Action::Delete { target: id.clone() })?;
        store.import_personal_events(&home, std::slice::from_ref(&proof))?;
        assert!(hydrate(&store.db, &header, true).is_err());
        assert_eq!(hydrate(&store.db, &header, false)?, reply);
        store.accept(&proof)?;
        assert!(reply_route(&store.db, &bare, &route).is_err());
        assert!(hydrate(&store.db, &header, false).is_err());
        drop(store);
        let mut reopened = Store::open(&dbpath)?;
        reopened.accept(&reply)?;
        let erased = reopened.existing(id)?.context("missing tombstone")?;
        erased.validate()?;
        assert_eq!(erased.deletion.as_deref(), Some(&proof));
        Ok(())
    }
}
