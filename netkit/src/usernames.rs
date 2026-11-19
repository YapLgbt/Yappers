// SPDX-License-Identifier: AGPL-3.0-only
//! Home-scoped claims and independently verifiable bylines for named events.
use crate::{model::*, store::Store};
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use rusqlite::{Connection, OptionalExtension, params};

const YAP_HOME: &str = "114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b";
pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS username_claims(home TEXT NOT NULL, account TEXT NOT NULL, name TEXT NOT NULL,
    PRIMARY KEY(home,account), UNIQUE(home,name));
CREATE TABLE IF NOT EXISTS username_receipts(event_id TEXT PRIMARY KEY, receipt TEXT NOT NULL);";

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
fn named(event: &SignedEvent) -> bool {
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
        db: &Connection,
        event: &SignedEvent,
        receipt: &UsernameReceipt,
    ) -> Result<()> {
        receipt.validate(event)?;
        if db.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE id=? AND json_extract(envelope,'$.deletion') IS NOT NULL)", [&event.id], |r|r.get::<_,bool>(0))? {return Ok(())}
        let raw = serde_json::to_string(receipt)?;
        db.execute(
            "INSERT OR IGNORE INTO username_receipts(event_id,receipt) VALUES(?,?)",
            params![event.id, raw],
        )?;
        let stored: String = db.query_row(
            "SELECT receipt FROM username_receipts WHERE event_id=?",
            [&event.id],
            |r| r.get(0),
        )?;
        ensure!(stored == raw, "conflicting signed username");
        Ok(())
    }
    pub(crate) fn username_receipt(&self, id: &str) -> Result<Option<UsernameReceipt>> {
        let raw: Option<String> = self
            .db
            .query_row(
                "SELECT receipt FROM username_receipts WHERE event_id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }
    pub(crate) fn log_entry(&self, cursor: u64, envelope: Envelope) -> Result<LogEntry> {
        Ok(LogEntry {
            cursor,
            username: self.username_receipt(&envelope.event.id)?,
            envelope,
        })
    }
    pub(crate) fn add_usernames(
        &self,
        posts: &mut [RemotePost],
        events: &[Envelope],
    ) -> Result<()> {
        let by_id: std::collections::HashMap<_, _> = events
            .iter()
            .map(|e| (e.event.id.as_str(), &e.event))
            .collect();
        let byline = |id: &str| -> Result<Option<String>> {
            let Some(event) = by_id.get(id) else {
                return Ok(None);
            };
            if !named(event) {
                return Ok(None);
            }
            let Some(receipt) = self.username_receipt(id)? else {
                return Ok(None);
            };
            Ok(domain(&event.payload.home).map(|domain| format!("{}@{}", receipt.name, domain)))
        };
        for post in posts {
            post.username = byline(&post.id)?;
            for reply in &mut post.replies {
                reply.username = byline(&reply.id)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portability::AccountWallet;
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
        Ok(())
    }
}
