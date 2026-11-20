// SPDX-License-Identifier: AGPL-3.0-only
//! Home-scoped claims and independently verifiable bylines for named events.
use crate::{model::*, store::Store};
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, HashSet};

const YAP_HOME: &str = "114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b";
pub(crate) fn domain(home: &str) -> Option<String> {
    if home == YAP_HOME {
        return Some("yap.lgbt".into());
    }
    // Each mapping is a local operator trust decision. Never accept a domain from a peer.
    let configured = std::env::var("YAP_USERNAME_DOMAINS").ok()?;
    configured
        .split(',')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, name)| {
            (key == home
                && name != "yap.lgbt"
                && (1..=253).contains(&name.len())
                && name.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                }))
            .then(|| name.to_owned())
        })
}

fn valid_name(name: &str) -> bool {
    (2..=24).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
pub(crate) fn named(event: &SignedEvent) -> bool {
    match &event.payload.action {
        Action::Post { anonymous, .. } | Action::PostV2 { anonymous, .. } => !anonymous,
        Action::Reply { display_name, .. } => display_name != "Anonymous neighbor",
        _ => false,
    }
}
impl UsernameReceipt {
    fn bytes(event: &SignedEvent, name: &str) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "yap/username/byline/v1",
            &event.id,
            &event.payload.home,
            name,
        ))?)
    }
    fn validate(&self, event: &SignedEvent) -> Result<()> {
        ensure!(
            valid_name(&self.name) && named(event) && event.payload.account.is_some(),
            "invalid username byline"
        );
        crate::model::verify(
            &event.payload.home,
            &Self::bytes(event, &self.name)?,
            &self.signature,
        )
    }
}
// A receipt links a named author to one public event. Its ciphertext shares
// that event's retirement boundary, with a separate encryption domain.
fn receipt_header(id: &str, raw: &str) -> Result<Envelope> {
    let header: Envelope = serde_json::from_str(raw)?;
    ensure!(
        header.event.id == id
            && header.deletion.is_none()
            && matches!(header.event.payload.action, Action::Erased { .. }),
        "invalid receipt header"
    );
    Ok(header)
}

fn receipt_key(opening: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("yap native username receipt v1", opening)
}

fn native_receipt_key(db: &crate::home_sql::HomeDb, id: &str) -> Result<[u8; 32]> {
    let raw: String = db.query_row("SELECT envelope FROM events WHERE id=?", [id], |r| r.get(0))?;
    let header = receipt_header(id, &raw)?;
    Ok(receipt_key(&crate::native_guard::opening_key(
        db, &header, false, false,
    )?))
}

fn stored_receipt(db: &crate::home_sql::HomeDb, id: &str) -> Result<Option<UsernameReceipt>> {
    if db.native().is_none() {
        let stored: Option<String> = db
            .query_row(
                "SELECT receipt FROM username_receipts WHERE event_id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        return stored
            .map(|raw| Ok(serde_json::from_str(&raw)?))
            .transpose();
    }
    let stored: Option<Vec<u8>> = db
        .query_row(
            "SELECT receipt FROM username_receipts WHERE event_id=?",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    stored
        .map(|sealed| {
            Ok(serde_json::from_slice(&crate::backup::unseal(
                &sealed,
                &native_receipt_key(db, id)?,
            )?)?)
        })
        .transpose()
}

impl Store {
    pub(crate) fn username(&self, home: &str, account: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT name FROM username_claims WHERE home=? AND account=?",
                [home, account],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub(crate) fn claim_username(&self, home: &str, account: &str, input: &str) -> Result<String> {
        ensure!(
            domain(home).is_some(),
            "This home has not enabled usernames."
        );
        let name = input.trim().to_ascii_lowercase();
        ensure!(
            valid_name(&name),
            "Use 2–24 letters, numbers, or underscores, starting with a letter."
        );
        self.db.execute(
            "INSERT OR IGNORE INTO username_claims(home,account,name) VALUES(?,?,?)",
            params![home, account, name],
        )?;
        match self.username(home, account)? {
            Some(owned) if owned == name => Ok(owned),
            Some(_) => anyhow::bail!("Your account already has a username."),
            None => anyhow::bail!("That username is taken. Try another."),
        }
    }
    pub(crate) fn sign_username(
        &self,
        event: &SignedEvent,
        key: &SigningKey,
    ) -> Result<Option<UsernameReceipt>> {
        if !named(event) || domain(&event.payload.home).is_none() {
            return Ok(None);
        }
        ensure!(
            event.payload.account.is_some(),
            "Claim a username before posting by name."
        );
        let name = self
            .username(&event.payload.home, event.owner_id())?
            .context("Claim a username before posting by name.")?;
        let signature = hex::encode(key.sign(&UsernameReceipt::bytes(event, &name)?).to_bytes());
        Ok(Some(UsernameReceipt { name, signature }))
    }
    pub(crate) fn record_username(
        db: &crate::home_sql::HomeDb,
        event: &SignedEvent,
        receipt: &UsernameReceipt,
    ) -> Result<()> {
        receipt.validate(event)?;
        if crate::account_store::deleted_registration(db, event.owner_id())?.is_some() {
            return Ok(());
        }
        if db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE id=? AND json_extract(envelope,'$.deletion') IS NOT NULL)", [&event.id], |r|r.get::<_,bool>(0))? {return Ok(())}
        if let Some(known) = stored_receipt(db, &event.id)? {
            ensure!(known == *receipt, "conflicting signed username");
            return Ok(());
        }
        let raw = serde_json::to_string(receipt)?;
        let value = if db.native().is_some() {
            rusqlite::types::Value::Blob(crate::backup::seal(
                raw.as_bytes(),
                &native_receipt_key(db, &event.id)?,
            )?)
        } else {
            rusqlite::types::Value::Text(raw)
        };
        db.execute(
            "INSERT INTO username_receipts VALUES(?,?)",
            params![event.id, value],
        )?;
        Ok(())
    }
    pub(crate) fn username_receipt(&self, id: &str) -> Result<Option<UsernameReceipt>> {
        stored_receipt(&self.db, id)
    }

    fn page_usernames(&self, ids: &[&str]) -> Result<HashMap<String, String>> {
        let mut names = HashMap::new();
        for chunk in ids.chunks(crate::MAX_PAGE_ENTRIES as usize) {
            let ids = [serde_json::to_string(chunk)?];
            if !self.is_native() {
                let mut stmt = self.db.prepare_cached(
                    "SELECT r.event_id, r.receipt FROM json_each(?) k CROSS JOIN username_receipts r
                     WHERE r.event_id=CAST(k.value AS TEXT)",
                )?;
                let mut rows = stmt.query(ids)?;
                while let Some(row) = rows.next()? {
                    let receipt: UsernameReceipt = serde_json::from_str(&row.get::<_, String>(1)?)?;
                    names.insert(row.get(0)?, receipt.name);
                }
                continue;
            }
            let mut stmt = self.db.prepare_cached(
                "SELECT r.event_id, r.receipt, e.envelope FROM json_each(?) k CROSS JOIN username_receipts r
                 LEFT JOIN events e ON e.id=r.event_id
                 WHERE r.event_id=CAST(k.value AS TEXT)",
            )?;
            let mut rows = stmt.query(ids)?;
            let mut receipts = Vec::new();
            while let Some(row) = rows.next()? {
                let header = receipt_header(&row.get::<_, String>(0)?, &row.get::<_, String>(2)?)?;
                receipts.push((header, row.get::<_, Vec<u8>>(1)?));
            }
            if !receipts.is_empty() {
                let headers = receipts
                    .iter()
                    .map(|(header, _)| header)
                    .collect::<Vec<_>>();
                crate::native_guard::with_public_opening_keys(&self.db, &headers, |keys| {
                    for ((header, sealed), key) in receipts.iter().zip(keys) {
                        let raw = crate::backup::unseal(sealed, &receipt_key(key))?;
                        let receipt: UsernameReceipt = serde_json::from_slice(&raw)?;
                        names.insert(header.event.id.clone(), receipt.name);
                    }
                    Ok(())
                })?;
            }
        }
        Ok(names)
    }
    pub(crate) fn log_entry(&self, cursor: u64, envelope: Envelope) -> Result<LogEntry> {
        Ok(LogEntry {
            cursor,
            username: self.username_receipt(&envelope.event.id)?,
            envelope,
        })
    }
    pub(crate) fn apply_current_bylines(
        &self,
        posts: &mut [RemotePost],
        events: &[Envelope],
    ) -> Result<()> {
        let visible: HashSet<_> = posts
            .iter()
            .flat_map(|post| {
                std::iter::once(post.id.as_str())
                    .chain(post.replies.iter().map(|reply| reply.id.as_str()))
            })
            .collect();
        let named_events: HashMap<_, _> = events
            .iter()
            .map(|e| &e.event)
            .filter(|event| visible.contains(event.id.as_str()) && named(event))
            .map(|event| {
                (
                    event.id.as_str(),
                    (event.payload.home.as_str(), event.owner_id()),
                )
            })
            .collect();
        let ids: Vec<_> = named_events.keys().copied().collect();
        let owners: HashSet<_> = named_events.values().copied().collect();
        let profiles = self.current_profiles(&owners.into_iter().collect::<Vec<_>>())?;
        let names = self.page_usernames(&ids)?;
        let byline = |id: &str| {
            let Some(key) = named_events.get(id) else {
                return (None, None);
            };
            let username = names
                .get(id)
                .and_then(|local| domain(key.0).map(|domain| format!("{local}@{domain}")));
            (username, profiles.get(key))
        };
        for post in posts {
            let (username, record) = byline(&post.id);
            post.username = username;
            post.profile = record.map(|p| p.profile.clone());
            if let Some(record) = record {
                post.author.clone_from(&record.name);
            }
            for reply in &mut post.replies {
                let (username, record) = byline(&reply.id);
                reply.username = username;
                if let Some(record) = record {
                    reply.author.clone_from(&record.name);
                    reply.profile = Some(record.profile.clone());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portability::AccountWallet;
    use rusqlite::Connection;
    use std::collections::BTreeSet;

    #[test]
    fn unique_claim_survives_retry_restore_and_is_scoped_to_home() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("home");
        let db = Store::open(&file)?;
        assert_eq!(
            db.claim_username(YAP_HOME, "account-a", " Hazel ")?,
            "hazel"
        );
        assert_eq!(db.claim_username(YAP_HOME, "account-a", "HAZEL")?, "hazel");
        assert!(db.claim_username(YAP_HOME, "account-b", "hazel").is_err());
        assert!(
            db.claim_username(YAP_HOME, "account-a", "different")
                .is_err()
        );
        assert!(
            db.claim_username(YAP_HOME, "account-b", "a@yap.lgbt")
                .is_err()
        );
        assert_eq!(db.username(&"0".repeat(64), "account-a")?, None);
        let backup = dir.path().join("before.backup");
        let restored = dir.path().join("restored");
        db.backup(&backup)?;
        drop(db);
        crate::backup::restore(
            &file.with_added_extension("deletion-guard"),
            &backup,
            &restored,
        )?;
        assert_eq!(
            Store::open(&restored)?
                .username(YAP_HOME, "account-a")?
                .as_deref(),
            Some("hazel")
        );
        Ok(())
    }

    #[test]
    fn unopened_restore_keeps_body_and_receipt_bound_to_current_guard() -> Result<()> {
        let home = signing_key(&new_secret())?;
        let home_id = hex::encode(home.verifying_key().as_bytes());
        let wallet = AccountWallet::create(new_secret(), home_id.clone())?;
        let device = signing_key(&wallet.device_secret)?;
        let accepted = |sequence, action| {
            Envelope::accept(
                SignedEvent::create_for_account(
                    &device,
                    home_id.clone(),
                    "public".into(),
                    sequence,
                    action,
                    wallet.chain[0].clone(),
                )?,
                &home,
            )
        };
        let body = "unopened-restore-sensitive-body";
        let post = accepted(
            1,
            Action::Post {
                body: body.into(),
                display_name: "Neighbor".into(),
                anonymous: false,
                topic: "Plans".into(),
            },
        )?;
        let id = &post.event.id;
        let name = "unopened_receipt";
        let receipt = UsernameReceipt {
            name: name.into(),
            signature: hex::encode(
                home.sign(&UsernameReceipt::bytes(&post.event, name)?)
                    .to_bytes(),
            ),
        };
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("origin");
        let mut origin = Store::open(&source)?;
        origin.accept_with_username(&post, Some(&receipt))?;
        origin.accept_with_username(&post, Some(&receipt))?;
        origin.import_personal_events(post.event.owner_id(), std::slice::from_ref(&post))?;

        let mut ha = Store {
            db: Connection::open_in_memory()?.into(),
        };
        ha.restore_snapshot(&origin.snapshot()?)?;
        assert_eq!(ha.username_receipt(id)?.as_ref(), Some(&receipt));
        let archive = dir.path().join("archive");
        origin.backup(&archive)?;
        let guard = source.with_added_extension("deletion-guard");
        let stale = dir.path().join("unopened");
        crate::backup::restore(&guard, &archive, &stale)?;
        // Inspect the physical restored row without opening it as a Home.
        let primary_path = crate::native_primary::path_for(&stale);
        let sealed = futures_lite::future::block_on(async {
            let db = turso::Builder::new_local(primary_path.to_str().unwrap())
                .build()
                .await?;
            let conn = db.connect()?;
            let mut rows = conn
                .query(
                    "SELECT receipt FROM username_receipts WHERE event_id=?",
                    [id.as_str()],
                )
                .await?;
            anyhow::Ok(
                rows.next()
                    .await?
                    .context("missing sealed receipt")?
                    .get::<Vec<u8>>(0)?,
            )
        })?;
        assert!(!sealed.is_empty());
        let again = dir.path().join("again");
        Store::backup_existing(&stale, &again)?;
        let reopened = dir.path().join("reopened");
        crate::backup::restore(&guard, &again, &reopened)?;
        assert_eq!(
            Store::open(&reopened)?.username_receipt(id)?.as_ref(),
            Some(&receipt)
        );

        origin.accept(&accepted(2, Action::Delete { target: id.clone() })?)?;
        assert!(origin.username_receipt(id)?.is_none());
        assert!(Store::open(&stale).is_err());
        let raw = Connection::open_with_flags(&guard, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(
            raw.query_row(
                "SELECT count(*) FROM opening_keys WHERE target=?",
                [id],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        for entry in std::fs::read_dir(dir.path())? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("unopened") {
                let bytes = std::fs::read(entry.path())?;
                for marker in [body, name, &receipt.signature, &post.event.payload.nonce] {
                    assert!(!bytes.windows(marker.len()).any(|w| w == marker.as_bytes()));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn only_attested_named_events_carry_a_username_across_peers() -> Result<()> {
        let home = signing_key(&new_secret())?;
        let home_id = hex::encode(home.verifying_key().as_bytes());
        let wallet = AccountWallet::create(new_secret(), home_id.clone())?;
        let device = signing_key(&wallet.device_secret)?;
        let event = |seq, anonymous| {
            SignedEvent::create_for_account(
                &device,
                home_id.clone(),
                "public".into(),
                seq,
                Action::Post {
                    body: "Hello".into(),
                    topic: "Chatter".into(),
                    anonymous,
                    display_name: if anonymous {
                        "Anonymous neighbor"
                    } else {
                        "Hazel"
                    }
                    .into(),
                },
                wallet.chain[0].clone(),
            )
        };
        let named = Envelope::accept(event(1, false)?, &home)?;
        let anonymous = Envelope::accept(event(2, true)?, &home)?;
        let byline = UsernameReceipt {
            name: "hazel".into(),
            signature: hex::encode(
                home.sign(&UsernameReceipt::bytes(&named.event, "hazel")?)
                    .to_bytes(),
            ),
        };
        let dir = tempfile::tempdir()?;
        let mut origin = Store::open(&dir.path().join("a"))?;
        origin.accept_with_username(&named, Some(&byline))?;
        assert!(
            origin
                .accept_with_username(&anonymous, Some(&byline))
                .is_err()
        );
        origin.accept(&anonymous)?;
        let Response::Page { entries, .. } =
            origin.page(0, 10, &BTreeSet::from(["public".into()]), "a")?
        else {
            unreachable!()
        };
        assert!(entries[1].username.is_none());
        let mut peer = Store::open(&dir.path().join("b"))?;
        let mut altered = entries.clone();
        altered[0].username.as_mut().unwrap().name = "other".into();
        assert!(peer.import_page("a", "a", 2, &altered).is_err());
        assert!(peer.events("public")?.is_empty());
        peer.import_page("a", "a", 2, &entries)?;
        assert_eq!(peer.username_receipt(&named.event.id)?, Some(byline));
        assert!(peer.username_receipt(&anonymous.event.id)?.is_none());

        let other = Envelope::accept(event(3, false)?, &home)?;
        let receipt = UsernameReceipt {
            name: "second".into(),
            signature: hex::encode(
                home.sign(&UsernameReceipt::bytes(&other.event, "second")?)
                    .to_bytes(),
            ),
        };
        peer.accept_with_username(&other, Some(&receipt))?;
        let mut ha = Store::from_sqlite(Connection::open_in_memory()?);
        ha.restore_snapshot(&peer.snapshot()?)?;
        for store in [&mut peer, &mut ha] {
            let ids = [
                other.event.id.as_str(),
                named.event.id.as_str(),
                anonymous.event.id.as_str(),
            ];
            assert_eq!(
                store.page_usernames(&ids)?,
                HashMap::from([
                    (other.event.id.clone(), "second".into()),
                    (named.event.id.clone(), "hazel".into()),
                ])
            );
            // Neither an off-page named post nor an on-page anonymous post needs
            // its receipt. Malformed rows make unintended reads observable.
            store.db.execute(
                "UPDATE username_receipts SET receipt='invalid' WHERE event_id=?",
                [&other.event.id],
            )?;
            store.db.execute(
                "INSERT INTO username_receipts VALUES(?,'invalid')",
                [&anonymous.event.id],
            )?;
            assert!(store.page_usernames(&[&other.event.id]).is_err());
            let events = store.events("public")?;
            let mut visible = project(
                &events,
                "public",
                "",
                &BTreeSet::from([other.event.id.clone()]),
            );
            store.set_profile(
                &home_id,
                named.event.owner_id(),
                0,
                "Current name".into(),
                Default::default(),
            )?;
            store.apply_current_bylines(&mut visible, &events)?;
            assert_eq!(visible.len(), 2);
            let named = visible.iter().find(|p| p.id == named.event.id).unwrap();
            assert_eq!(named.author, "Current name");
            assert!(named.profile.is_some());
            let anonymous = visible.iter().find(|p| p.id == anonymous.event.id).unwrap();
            assert!(anonymous.profile.is_none() && anonymous.username.is_none());
        }
        Ok(())
    }
}
