// SPDX-License-Identifier: MIT OR Apache-2.0
//! Signed membership and ciphertext-only wire records for single-home groups.
use crate::{
    model::{public_key, verify},
    portability::digest,
};
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use vodozemac::{Curve25519PublicKey, Ed25519PublicKey, megolm::MegolmMessage, olm::OlmMessage};

pub(crate) const MAX_CIPHERTEXT: usize = 24_000;
pub(crate) const SESSION_MESSAGES: u32 = 100;
pub(crate) const SESSION_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed<T> {
    pub value: T,
    pub signature: String,
}

#[doc(hidden)]
pub trait Domain {
    const NAME: &'static str;
}

impl<T: Serialize + Domain> Signed<T> {
    pub(crate) fn new(value: T, key: &SigningKey) -> Result<Self> {
        let signature = hex::encode(key.sign(&Self::bytes(&value)?).to_bytes());
        Ok(Self { value, signature })
    }

    fn bytes(value: &T) -> Result<Vec<u8>> {
        Ok([T::NAME.as_bytes(), b"\0", &serde_json::to_vec(value)?].concat())
    }

    pub(crate) fn verify(&self, key: &str) -> Result<()> {
        verify(key, &Self::bytes(&self.value)?, &self.signature)
    }
}

pub(crate) fn hash_id(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid private group identifier"
    );
    Ok(())
}

pub(crate) fn text(value: &str, maximum: usize, multiline: bool) -> Result<()> {
    ensure!(
        !value.trim().is_empty()
            && value.encode_utf16().count() <= maximum
            && value
                .chars()
                .all(|c| !c.is_control() || (multiline && matches!(c, '\n' | '\t'))),
        "invalid private group text"
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Room {
    pub home: String,
    pub owner: String,
    pub nonce: String,
    pub name: String,
}
impl Domain for Room {
    const NAME: &'static str = "yap:private-group:v1";
}
impl Signed<Room> {
    pub(crate) fn check(&self, home: &str) -> Result<String> {
        let room = &self.value;
        ensure!(room.home == home, "this invitation belongs to another home");
        public_key(home)?;
        hash_id(&room.nonce)?;
        text(&room.name, 60, false)?;
        self.verify(&room.owner)?;
        digest(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Device {
    pub account: String,
    pub signing: String,
    pub identity: Curve25519PublicKey,
    pub fallback: Curve25519PublicKey,
}
impl Device {
    fn check(&self) -> Result<()> {
        public_key(&self.account)?;
        public_key(&self.signing)?;
        ensure!(
            self.identity != self.fallback
                && self.identity.as_bytes() != &[0; 32]
                && self.fallback.as_bytes() != &[0; 32],
            "invalid device messaging keys"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinRequest {
    pub group: String,
    pub device: Device,
}
impl Domain for JoinRequest {
    const NAME: &'static str = "yap:private-group-join:v1";
}
impl Signed<JoinRequest> {
    pub(crate) fn check(&self, group: &str) -> Result<()> {
        ensure!(
            self.value.group == group,
            "join request is for another group"
        );
        self.value.device.check()?;
        self.verify(&self.value.device.signing)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Member {
    pub device: Device,
    pub since: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Roster {
    pub group: String,
    pub revision: u32,
    pub previous: Option<String>,
    pub members: BTreeMap<String, Member>,
}
impl Domain for Roster {
    const NAME: &'static str = "yap:private-group-roster:v1";
}
impl Signed<Roster> {
    pub(crate) fn check(&self, group: &str, owner: &str, prior: Option<&Self>) -> Result<()> {
        self.verify(owner)?;
        let roster = &self.value;
        ensure!(
            roster.group == group
                && roster.revision > 0
                && !roster.members.is_empty()
                && roster.members.contains_key(owner),
            "invalid group roster"
        );
        for (id, member) in &roster.members {
            member.device.check()?;
            ensure!(
                id == &member.device.signing && (1..=roster.revision).contains(&member.since),
                "invalid group member"
            );
        }
        let Some(prior) = prior else {
            ensure!(
                roster.revision == 1 && roster.previous.is_none() && roster.members.len() == 1,
                "invalid initial group roster"
            );
            return Ok(());
        };
        ensure!(
            Some(roster.revision) == prior.value.revision.checked_add(1)
                && roster.previous.as_ref() == Some(&digest(prior)?),
            "private group roster changed"
        );
        let old = &prior.value.members;
        let mut changes = 0;
        for (id, member) in &roster.members {
            match old.get(id) {
                Some(existing) => ensure!(existing == member, "member device keys changed"),
                None => {
                    ensure!(member.since == roster.revision, "invalid history start");
                    changes += 1;
                }
            }
        }
        changes += old
            .keys()
            .filter(|id| !roster.members.contains_key(*id))
            .count();
        ensure!(
            changes == 1,
            "only one membership change is allowed per revision"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub group: String,
    pub roster: String,
    pub sender: String,
    pub id: String,
    pub keys: BTreeMap<String, OlmMessage>,
}
impl Domain for Session {
    const NAME: &'static str = "yap:private-group-session:v1";
}
impl Signed<Session> {
    pub(crate) fn check(&self, roster: &Signed<Roster>) -> Result<()> {
        let session = &self.value;
        ensure!(
            session.group == roster.value.group
                && session.roster == digest(roster)?
                && session.keys.keys().eq(roster.value.members.keys()),
            "session recipients do not match group roster"
        );
        let sender = roster
            .value
            .members
            .get(&session.sender)
            .context("unknown group sender")?;
        Ed25519PublicKey::from_base64(&session.id)?;
        self.verify(&session.sender)?;
        for (id, key) in &session.keys {
            let OlmMessage::PreKey(key) = key else {
                anyhow::bail!("invalid group key packet");
            };
            ensure!(
                key.identity_key() == sender.device.identity
                    && key.one_time_key() == roster.value.members[id].device.fallback
                    && key.to_base64().len() <= 2048,
                "invalid group key packet"
            );
        }
        Ok(())
    }
}

// Session receipt includes just this recipient's Olm envelope and is bound by the
// encrypted context and the sender's signed event. This avoids reading others' envelopes.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionKeyPacket {
    pub group: String,
    pub roster: String,
    pub sender: String,
    pub recipient: String,
    pub session: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventHeader {
    pub group: String,
    pub roster: String,
    pub revision: u32,
    pub sender: String,
    pub session: String,
    pub nonce: String,
    pub parent: Option<String>,
}
impl EventHeader {
    pub(crate) fn id(&self) -> Result<String> {
        // The ID survives re-encryption of a pending event after a membership change.
        digest(&(
            "yap:private-group-event-id:v1",
            &self.group,
            &self.sender,
            &self.nonce,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub header: EventHeader,
    pub ciphertext: String,
}
impl Domain for Event {
    const NAME: &'static str = "yap:private-group-event:v1";
}
impl Signed<Event> {
    pub(crate) fn check(&self, roster: &Signed<Roster>) -> Result<MegolmMessage> {
        let header = &self.value.header;
        ensure!(
            header.group == roster.value.group
                && header.revision == roster.value.revision
                && header.roster == digest(roster)?
                && roster.value.members.contains_key(&header.sender),
            "event does not match its group roster"
        );
        hash_id(&header.nonce)?;
        if let Some(parent) = &header.parent {
            hash_id(parent)?;
        }
        Ed25519PublicKey::from_base64(&header.session)?;
        ensure!(
            self.value.ciphertext.len() <= MAX_CIPHERTEXT,
            "private post is too large"
        );
        self.verify(&header.sender)?;
        let message = MegolmMessage::from_base64(&self.value.ciphertext)?;
        ensure!(
            message.message_index() < SESSION_MESSAGES,
            "group session expired"
        );
        Ok(message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    pub cursor: u64,
    pub event: Signed<Event>,
    pub key: OlmMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Operation {
    Create {
        room: Signed<Room>,
        roster: Signed<Roster>,
    },
    Roster {
        group: String,
    },
    RosterAfter {
        group: String,
        after: u32,
    },
    Change {
        roster: Signed<Roster>,
        request: Option<Signed<JoinRequest>>,
    },
    Publish {
        event: Signed<Event>,
        session: Option<Signed<Session>>,
    },
    Read {
        group: String,
        roster: String,
        before: Option<u64>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Outcome {
    Saved,
    Roster {
        history: Vec<Signed<Roster>>,
    },
    RosterPage {
        history: Vec<Signed<Roster>>,
    },
    Accepted {
        id: String,
        roster: String,
    },
    Page {
        entries: Vec<Delivery>,
        before: Option<u64>,
    },
}

// The local command is sent to Rust through the platform bridge. Draft text and
// device-private state never enter a transport request.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LocalAction {
    List,
    Create { name: String },
    Import { invitation: String },
    Open { group: String, before: Option<u64> },
    Approve { group: String, request: String },
    Remove { group: String, device: String },
    Prepare { group: String, draft: Option<Draft> },
    Publish { group: String },
    Discard { group: String },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub author: String,
    pub headline: Option<String>,
    pub body: String,
    pub parent: Option<String>,
}
impl Draft {
    pub(crate) fn check(&self) -> Result<()> {
        text(&self.author, 30, false)?;
        text(&self.body, 3000, true)?;
        match (&self.headline, &self.parent) {
            (Some(title), None) => text(title, 100, false),
            (None, Some(parent)) => hash_id(parent),
            _ => anyhow::bail!("invalid private group post or reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_accepts_member_seventeen_and_revision_101() -> Result<()> {
        let owner_key = SigningKey::from_bytes(&[1; 32]);
        let owner = hex::encode(owner_key.verifying_key().as_bytes());
        let group = "ab".repeat(32);
        let member = |id: u8, since| {
            let key = SigningKey::from_bytes(&[id; 32]);
            let signing = hex::encode(key.verifying_key().as_bytes());
            let device = Device {
                account: signing.clone(),
                signing: signing.clone(),
                identity: Curve25519PublicKey::from_bytes([id; 32]),
                fallback: Curve25519PublicKey::from_bytes([id + 17; 32]),
            };
            (signing, Member { device, since })
        };
        let prior = Signed::new(
            Roster {
                group: group.clone(),
                revision: 100,
                previous: Some("cd".repeat(32)),
                members: (1..=16).map(|i| member(i, u32::from(i))).collect(),
            },
            &owner_key,
        )?;
        let mut update = prior.value.clone();
        update.revision = 101;
        update.previous = Some(digest(&prior)?);
        update.members.extend([member(17, 101)]);
        Signed::new(update.clone(), &owner_key)?.check(&group, &owner, Some(&prior))?;
        update.previous = None;
        assert!(
            Signed::new(update, &owner_key)?
                .check(&group, &owner, Some(&prior))
                .is_err()
        );
        Ok(())
    }
}
