// SPDX-License-Identifier: MIT OR Apache-2.0
//! Private home mailbox protocol. Only signed public prekeys and sealed Olm packets cross the wire.
use crate::{
    model::{public_key, verify},
    portability::AccountRecord,
};
use anyhow::{Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use vodozemac::{Curve25519PublicKey, olm::OlmMessage};

pub const PREKEY_BATCH: usize = 12;
pub const MAIL_PAGE: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed<T> {
    pub record: AccountRecord,
    pub device: String,
    pub value: T,
    pub signature: String,
}

pub trait Payload: Serialize {
    const DOMAIN: &'static str;
}
impl<T: Payload> Signed<T> {
    fn bytes(record: &AccountRecord, device: &str, value: &T) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(T::DOMAIN, record, device, value))?)
    }
    pub fn sign(record: &AccountRecord, key: &SigningKey, value: T) -> Result<Self> {
        let device = hex::encode(key.verifying_key().as_bytes());
        let signature = hex::encode(key.sign(&Self::bytes(record, &device, &value)?).to_bytes());
        Ok(Self {
            record: record.clone(),
            device,
            value,
            signature,
        })
    }
    pub fn check(&self, owner: &str, home: &str) -> Result<()> {
        self.record.validate()?;
        ensure!(
            self.record.payload.account == owner
                && self.record.payload.home == home
                && self.record.payload.devices.contains(&self.device),
            "untrusted messaging device"
        );
        verify(
            &self.device,
            &Self::bytes(&self.record, &self.device, &self.value)?,
            &self.signature,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prekey {
    pub identity: Curve25519PublicKey,
    pub once: Curve25519PublicKey,
}
impl Payload for Prekey {
    const DOMAIN: &'static str = "yap:olm-prekey:v1";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Letter {
    pub to: String,
    pub identity: Curve25519PublicKey,
    pub recipient_key: Curve25519PublicKey,
    pub session: String,
    pub ciphertext: OlmMessage,
}
impl Payload for Letter {
    const DOMAIN: &'static str = "yap:olm-letter:v1";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SocialOp {
    PrivacyFeed { nearby: Option<crate::Nearby> },
    PrivacyStatus,
    NotificationInbox,
    SetPush { provider:String, subscription:serde_json::Value, categories:Vec<String> },
    OpenPush { route:String },
    NotificationThread { target: String, anchor: String },
    ReadNotification { #[serde(rename = "category")] category: String, target: String, through: i64 },
    MuteNotifications { target: String, muted: bool },
    DismissRequest { peer: String },
    SetProfile { expected: u64, name: String, profile: crate::CommunityProfile },
    Profiles { events: Vec<String> },
    ClaimUsername { name: String },
    SetPrivacy { settings: PrivacySettings },
    Approve { peer: String },
    Feed { nearby: Option<crate::Nearby> },
    Contact { event: String },
    Block { target: Target },
    Unblock { id: String },
    Blocks,
    Publish { keys: Vec<Signed<Prekey>> },
    Claim { peer: String },
    Deliver { letter: Box<Signed<Letter>> },
    Inbox { after: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Target {
    Event(String),
    Account(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Incoming {
    pub cursor: i64,
    pub letter: Signed<Letter>,
    #[serde(default)]
    pub age: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivacySettings {
    pub hide_anonymous: bool,
    pub feed_days: u8,
    pub dm_days: u8,
    pub approved_only: bool,
}
pub fn valid_peer(peer: &str, owner: &str) -> Result<()> {
    public_key(peer)?;
    ensure!(peer != owner, "choose another account");
    Ok(())
}
