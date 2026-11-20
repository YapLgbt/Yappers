// SPDX-License-Identifier: MIT OR Apache-2.0
//! Closed legacy v1/v2 and portable-account v3 event schemas, canonical signing bytes, and deterministic projection.
//!
//! Rust struct field order is part of the signing contract. Do not reorder signed fields
//! or change serde attributes without a new event version/ALPN and conformance tests.
//! Ingestion order and local numeric UI IDs never determine federation convergence.

use anyhow::{Context, Result, bail, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use n0_future::time::SystemTime;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u8 = 1;
pub const TOPICS: [&str; 4] = ["Chatter", "Friends", "Missed connections", "Plans"];
const EVENT_DOMAIN: &[u8] = b"yappers:event:v1\0";
const SIGN_DOMAIN: &[u8] = b"yappers:author:v1\0";
const HOME_DOMAIN: &[u8] = b"yappers:home:v1\0";

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn new_secret() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub fn signing_key(secret: &str) -> Result<SigningKey> {
    let bytes: [u8; 32] = hex::decode(secret)
        .context("invalid secret encoding")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("secret must be 32 bytes"))?;
    Ok(SigningKey::from_bytes(&bytes))
}
pub(crate) fn public_key(value: &str) -> Result<VerifyingKey> {
    let bytes: [u8; 32] = hex::decode(value)
        .context("invalid public key")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    ensure!(
        hex::encode(bytes) == value,
        "public keys must use canonical lowercase hex"
    );
    Ok(VerifyingKey::from_bytes(&bytes)?)
}
pub(crate) fn verify(key: &str, bytes: &[u8], signature: &str) -> Result<()> {
    let decoded = hex::decode(signature).context("invalid signature encoding")?;
    ensure!(
        hex::encode(&decoded) == signature,
        "signatures must use canonical lowercase hex"
    );
    let sig = Signature::from_slice(&decoded)?;
    public_key(key)?
        .verify_strict(bytes, &sig)
        .context("invalid signature")
}
fn text(value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.encode_utf16().count() <= max,
        "invalid text length"
    );
    ensure!(
        !value
            .chars()
            .any(|c| c == '\0' || (c.is_control() && c != '\n' && c != '\t')),
        "invalid control character"
    );
    Ok(())
}
fn id(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64 && hex::decode(value).is_ok(),
        "invalid event id"
    );
    Ok(())
}

/// Details of a public account profile, resolved separately from signed posts.
/// Photos are small JPEG thumbnails (base64, at most 4096 ASCII bytes), not URLs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommunityProfile {
    pub pronouns: String,
    pub bio: String,
    pub identity: String,
    pub interests: String,
    pub here_for: Vec<String>,
    pub photo: String,
}
impl CommunityProfile {
    pub(crate) fn validate(&self) -> Result<()> {
        for (value, max) in [
            (&self.pronouns, 40),
            (&self.bio, 280),
            (&self.identity, 80),
            (&self.interests, 120),
        ] {
            if !value.is_empty() {
                text(value, max)?;
            }
        }
        ensure!(
            self.here_for.len() <= 6
                && self.here_for.iter().all(|v| [
                    "Friends",
                    "Dates",
                    "Community",
                    "Creative pals",
                    "Chosen family",
                    "Just yapping"
                ]
                .contains(&v.as_str())),
            "invalid profile intentions"
        );
        ensure!(
            self.photo.len() <= 4096
                && self
                    .photo
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
            "invalid profile photo"
        );
        Ok(())
    }
}

/// Coordinates are rounded to hundredths of a degree before leaving the device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostArea {
    pub label: String,
    pub lat100: i32,
    pub lon100: i32,
}

impl PostArea {
    fn validate(&self) -> Result<()> {
        text(&self.label, 160)?;
        ensure!(
            (-9000..=9000).contains(&self.lat100) && (-18000..=18000).contains(&self.lon100),
            "invalid area"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Nearby {
    pub center: PostArea,
    pub radius_miles: u16,
}

impl Nearby {
    pub fn validate(&self) -> Result<()> {
        self.center.validate()?;
        ensure!((1..=250).contains(&self.radius_miles), "invalid radius");
        Ok(())
    }

    pub fn contains(&self, area: &PostArea) -> bool {
        let radians = |v: i32| (f64::from(v) / 100.0).to_radians();
        let a = radians(self.center.lat100);
        let b = radians(area.lat100);
        let h = ((b - a) / 2.0).sin().powi(2)
            + a.cos()
                * b.cos()
                * (radians(area.lon100 - self.center.lon100) / 2.0)
                    .sin()
                    .powi(2);
        7917.52 * h.clamp(0.0, 1.0).sqrt().asin() <= f64::from(self.radius_miles)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    /// v2 post. The original Post variant remains byte-for-byte stable for old signatures.
    PostV2 {
        body: String,
        topic: String,
        anonymous: bool,
        #[serde(rename = "displayName")]
        display_name: String,
        headline: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Box<CommunityProfile>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        area: Option<PostArea>,
    },
    Post {
        body: String,
        topic: String,
        anonymous: bool,
        #[serde(rename = "displayName")]
        display_name: String,
    },
    Reply {
        parent: String,
        body: String,
        #[serde(rename = "displayName")]
        display_name: String,
    },
    Vote {
        target: String,
        value: i8,
    },
    Delete {
        target: String,
    },
    /// Permanent v4 header. The opening (action and random nonce) is discarded.
    Erased {
        commitment: String,
        reply: bool,
    },
}
impl Action {
    fn validate(&self) -> Result<()> {
        match self {
            Self::PostV2 {
                body,
                topic,
                anonymous,
                display_name,
                headline,
                profile,
                area,
            } => {
                if let Some(area) = area {
                    area.validate()?;
                }
                text(headline, 80)?;
                Self::Post {
                    body: body.clone(),
                    topic: topic.clone(),
                    anonymous: *anonymous,
                    display_name: display_name.clone(),
                }
                .validate()?;
                ensure!(
                    !anonymous || profile.is_none(),
                    "anonymous posts must omit profile details"
                );
                if let Some(profile) = profile {
                    profile.validate()?;
                }
            }
            Self::Post {
                body,
                topic,
                anonymous,
                display_name,
            } => {
                text(body, 500)?;
                text(display_name, 30)?;
                ensure!(TOPICS.contains(&topic.as_str()), "unknown topic");
                ensure!(
                    !anonymous || display_name == "Anonymous neighbor",
                    "anonymous posts must omit the display name"
                );
            }
            Self::Reply {
                parent,
                body,
                display_name,
            } => {
                id(parent)?;
                text(body, 500)?;
                text(display_name, 30)?;
            }
            Self::Vote { target, value } => {
                id(target)?;
                ensure!((-1..=1).contains(value), "invalid vote");
            }
            Self::Delete { target } => id(target)?,
            Self::Erased { commitment, .. } => id(commitment)?,
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventPayload {
    pub version: u8,
    pub home: String,
    pub community: String,
    pub author: String,
    pub sequence: u64,
    pub created_at: i64,
    pub nonce: String,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Box<crate::portability::AccountRecord>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedEvent {
    pub id: String,
    pub payload: EventPayload,
    pub signature: String,
}
impl SignedEvent {
    pub fn create(
        key: &SigningKey,
        home: String,
        community: String,
        sequence: u64,
        action: Action,
    ) -> Result<Self> {
        ensure!(
            !matches!(action, Action::Erased { .. }),
            "cannot author a redacted event"
        );
        let payload = EventPayload {
            version: 4,
            home,
            community,
            author: hex::encode(key.verifying_key().as_bytes()),
            sequence,
            created_at: now(),
            nonce: new_secret(),
            action,
            account: None,
        };
        let mut event = Self {
            id: String::new(),
            payload,
            signature: String::new(),
        };
        event.id = event.expected_id()?;
        event.signature = hex::encode(
            key.sign(&[event.sign_domain(), event.id.as_bytes()].concat())
                .to_bytes(),
        );
        event.validate()?;
        Ok(event)
    }
    /// Delegated authoring preserves device signatures and stable account ownership.
    pub fn create_for_account(
        key: &SigningKey,
        home: String,
        community: String,
        sequence: u64,
        action: Action,
        account: crate::portability::AccountRecord,
    ) -> Result<Self> {
        let mut event = Self::create(key, home, community, sequence, action)?;
        event.payload.version = 4;
        event.payload.account = Some(Box::new(account));
        event.id = event.expected_id()?;
        event.signature = hex::encode(
            key.sign(&[event.sign_domain(), event.id.as_bytes()].concat())
                .to_bytes(),
        );
        event.validate()?;
        Ok(event)
    }
    pub fn owner_id(&self) -> &str {
        self.payload
            .account
            .as_ref()
            .map_or(self.payload.author.as_str(), |record| {
                record.payload.account.as_str()
            })
    }
    fn sign_domain(&self) -> &'static [u8] {
        if self.payload.version == 4 {
            b"yap:author:v4:profiles-1\0"
        } else if self.payload.version == 3 {
            b"yap:author:v3\0"
        } else if self.payload.version == 2 {
            b"yappers:author:v2\0"
        } else {
            SIGN_DOMAIN
        }
    }
    /// The nonce is a private 256-bit opening for content actions. Only the digest
    /// and the post/reply distinction enter the permanent signed header.
    pub fn header(&self) -> Result<EventPayload> {
        let mut header = self.payload.clone();
        if header.version == 4
            && matches!(
                header.action,
                Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
            )
        {
            let mut hash = blake3::Hasher::new_derive_key("yap content commitment v4");
            hash.update(&hex::decode(&header.nonce)?);
            hash.update(&serde_json::to_vec(&header.action)?);
            header.action = Action::Erased {
                commitment: hash.finalize().to_hex().to_string(),
                reply: matches!(header.action, Action::Reply { .. }),
            };
            header.nonce.clear();
        }
        Ok(header)
    }

    fn expected_id(&self) -> Result<String> {
        Ok(blake3::hash(
            &[
                if self.payload.version == 4 {
                    b"yap:event:v4:profiles-1\0"
                } else if self.payload.version == 3 {
                    b"yap:event:v3\0"
                } else if self.payload.version == 2 {
                    b"yappers:event:v2\0"
                } else {
                    EVENT_DOMAIN
                },
                &serde_json::to_vec(&self.header()?)?,
            ]
            .concat(),
        )
        .to_hex()
        .to_string())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.payload.version, 1..=4),
            "unsupported event version"
        );
        ensure!(
            self.payload.version >= 3
                || (self.payload.version == 2)
                    == matches!(self.payload.action, Action::PostV2 { .. }),
            "action does not match event version"
        );
        public_key(&self.payload.home)?;
        if self.payload.version == 3 || self.payload.version == 4 && self.payload.account.is_some()
        {
            let account = self
                .payload
                .account
                .as_ref()
                .context("missing account authorization")?;
            account.validate()?;
            ensure!(
                account.payload.home == self.payload.home
                    && account.payload.devices.contains(&self.payload.author),
                "account did not authorize device/home"
            );
        } else {
            ensure!(
                self.payload.account.is_none(),
                "legacy event cannot carry account state"
            );
        }
        ensure!(
            self.payload.community.len() <= 160
                && !self.payload.community.is_empty()
                && self.payload.community.is_ascii(),
            "invalid community"
        );
        ensure!(
            self.payload.sequence > 0 && self.payload.sequence <= i64::MAX as u64,
            "invalid sequence"
        );
        if matches!(self.payload.action, Action::Erased { .. }) {
            ensure!(
                self.payload.version == 4 && self.payload.nonce.is_empty(),
                "invalid erased header"
            );
        } else {
            id(&self.payload.nonce)?;
            ensure!(
                self.payload.version != 4
                    || hex::encode(hex::decode(&self.payload.nonce)?) == self.payload.nonce,
                "noncanonical nonce"
            );
        }
        self.payload.action.validate()?;
        ensure!(self.id == self.expected_id()?, "event id mismatch");
        verify(
            &self.payload.author,
            &[self.sign_domain(), self.id.as_bytes()].concat(),
            &self.signature,
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope {
    pub event: SignedEvent,
    pub accepted_at: i64,
    pub home_signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion: Option<Box<Envelope>>,
}
impl Envelope {
    /// Both sides must already have passed validation; multiple owner deletes may attest to the same erasure.
    pub fn same_acceptance(&self, other: &Self) -> Result<bool> {
        Ok(self.event.id == other.event.id
            && self.home_signature == other.home_signature
            && self.accepted_at == other.accepted_at
            && self.event.signature == other.event.signature
            && self.event.header()? == other.event.header()?)
    }
    pub fn deleted_by(&self, proof: &Envelope) -> bool {
        matches!(&proof.event.payload.action, Action::Delete { target } if target == &self.event.id)
            && proof.event.owner_id() == self.event.owner_id()
            && proof.event.payload.community == self.event.payload.community
    }
    pub fn erased(&self, proof: &Envelope) -> Result<Self> {
        ensure!(self.deleted_by(proof), "deletion does not own target");
        let mut erased = self.clone();
        erased.event.payload = self.event.header()?;
        ensure!(
            matches!(erased.event.payload.action, Action::Erased { .. }),
            "target cannot be erased"
        );
        erased.deletion = Some(Box::new(proof.clone()));
        Ok(erased)
    }
    fn bytes(&self) -> Result<Vec<u8>> {
        Ok([
            HOME_DOMAIN,
            &serde_json::to_vec(&(&self.event.id, &self.event.payload.home, self.accepted_at))?,
        ]
        .concat())
    }
    pub fn accept(event: SignedEvent, home: &SigningKey) -> Result<Self> {
        event.validate()?;
        ensure!(
            !matches!(event.payload.action, Action::Erased { .. }),
            "cannot accept a missing opening"
        );
        ensure!(
            hex::encode(home.verifying_key().as_bytes()) == event.payload.home,
            "wrong home instance"
        );
        let mut envelope = Self {
            event,
            accepted_at: now(),
            home_signature: String::new(),
            deletion: None,
        };
        envelope.home_signature = hex::encode(home.sign(&envelope.bytes()?).to_bytes());
        Ok(envelope)
    }
    pub fn validate(&self) -> Result<()> {
        match (&self.event.payload.action, &self.deletion) {
            (Action::Erased { .. }, Some(proof)) => {
                ensure!(
                    self.deleted_by(proof)
                        && proof.event.payload.version == 4
                        && proof.deletion.is_none(),
                    "invalid deletion proof"
                );
                proof.validate()?;
            }
            (Action::Erased { .. }, None) | (_, Some(_)) => {
                bail!("missing or unexpected deletion proof")
            }
            _ => {}
        }
        self.event.validate()?;
        ensure!(
            self.event.payload.created_at > 0
                && self.event.payload.created_at <= self.accepted_at + 300,
            "invalid client clock"
        );
        ensure!(
            self.event.payload.created_at >= self.accepted_at - 30 * 86400,
            "event too old at acceptance"
        );
        ensure!(
            self.accepted_at > 0 && self.accepted_at <= now() + 300,
            "invalid home clock"
        );
        verify(
            &self.event.payload.home,
            &self.bytes()?,
            &self.home_signature,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinConfig {
    pub protocol_version: u8,
    pub server_addr: iroh::EndpointAddr,
    pub community: String,
    pub access_token: String,
    #[serde(default)]
    pub network_mode: NetworkMode,
}
impl JoinConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.protocol_version, 1..=crate::PROTOCOL_VERSION),
            "unsupported join configuration version"
        );
        ensure!(
            self.access_token.len() >= 32 && self.access_token.len() <= 256,
            "invalid access token"
        );
        ensure!(
            !self.community.is_empty() && self.community.len() <= 160 && self.community.is_ascii(),
            "invalid community"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkMode {
    #[default]
    Local,
    N0,
}
pub const ACCOUNT_DELETE_BATCH: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountDeleteTarget {
    pub community: String,
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    ReportAvailability,
    SubmitReport {
        token: String,
        community: String,
        receipt: String,
        evidence: crate::report::Evidence,
    },
    ReportStatus {
        receipt: String,
    },
    ServerSource,
    HomeOperation {
        request_id: String,
        call: Box<Request>,
    },
    /// Read an explicitly published replica projection. No credentials or viewer metadata.
    PublicReplicaFeed {
        community: String,
    },
    DeletionsAfter {
        community: String,
        token: Option<String>,
        after: String,
    },
    /// Current profile revisions from the authenticated origin for named authors in a shared scope.
    PeerProfiles {
        community: String,
        known: std::collections::BTreeMap<String, u64>,
    },
    /// Peer-only conditional pull of the founding home's public moderation projection.
    PullPublicProjection {
        community: String,
        digest: Option<String>,
    },
    Social {
        token: String,
        community: String,
        account: String,
        proof: crate::portability::AccountProof,
        op: Box<crate::messaging::SocialOp>,
    },
    PrivateGroup {
        token: String,
        community: String,
        account: String,
        proof: crate::portability::AccountProof,
        op: Box<crate::private_groups::Operation>,
    },
    AnnounceAccount {
        chain: Vec<crate::portability::AccountRecord>,
    },
    AccountLookup {
        token: String,
        community: String,
        account: String,
    },
    AccountRegister {
        token: String,
        community: String,
        chain: Vec<crate::portability::AccountRecord>,
        proof: crate::portability::AccountProof,
    },
    AccountDeleteAvailability {
        token: String,
        community: String,
        chain: Vec<crate::portability::AccountRecord>,
        intent: crate::portability::AccountDeletionIntent,
        proof: crate::portability::AccountProof,
    },
    AccountDelete {
        token: String,
        community: String,
        chain: Vec<crate::portability::AccountRecord>,
        intent: crate::portability::AccountDeletionIntent,
        proof: crate::portability::AccountProof,
        deletes: Vec<SignedEvent>,
    },
    AccountExport {
        token: String,
        community: String,
        chain: Vec<crate::portability::AccountRecord>,
        proof: crate::portability::AccountProof,
        after: Option<String>,
    },
    AccountImport {
        token: String,
        community: String,
        chain: Vec<crate::portability::AccountRecord>,
        proof: crate::portability::AccountProof,
        events: Vec<Envelope>,
    },
    Directory {
        snapshot: crate::groups::DirectorySnapshot,
    },
    EnrollHost {
        group: String,
    },
    Groups {
        token: String,
        community: String,
    },
    GroupsAfter {
        token: String,
        community: String,
        after: String,
    },
    CreateGroup {
        token: String,
        community: String,
        details: crate::groups::NewGroup,
    },
    JoinGroup {
        token: String,
        community: String,
        group: String,
    },
    Feed {
        token: String,
        community: String,
        author: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nearby: Option<Nearby>,
    },
    Submit {
        token: String,
        event: SignedEvent,
        #[serde(default, rename = "accountChain")]
        account_chain: Vec<crate::portability::AccountRecord>,
    },
    Pull {
        after: u64,
        generation: Option<String>,
        limit: u32,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    ReportAvailable,
    PrivateGroup {
        result: Box<crate::private_groups::Outcome>,
    },
    ReportResult {
        result: serde_json::Value,
    },
    ServerSource {
        #[serde(rename = "sourceUrl")]
        source_url: String,
        revision: String,
    },
    PublicReplicaFeed {
        #[serde(default)]
        deleted: Vec<String>,
        posts: Vec<RemotePost>,
        #[serde(rename = "eventCount")]
        event_count: usize,
        #[serde(rename = "asOf")]
        as_of: i64,
    },
    FeedWithMoreDeletions {
        feed: Box<Response>,
    },
    DeletionPage {
        ids: Vec<String>,
        more: bool,
    },
    ProjectionUnchanged,
    Social {
        result: serde_json::Value,
    },
    AccountDeleteAvailable {
        account: String,
    },
    AccountDeletePending {
        targets: Vec<AccountDeleteTarget>,
    },
    AccountDeleted {
        account: String,
    },
    AccountState {
        #[serde(default)]
        privacy: bool,
        #[serde(default)]
        social: bool,
        chain: Vec<crate::portability::AccountRecord>,
    },
    OwnedEvents {
        events: Vec<Envelope>,
        next: Option<String>,
        more: bool,
    },
    PersonalImported {
        count: usize,
    },

    Directory {
        snapshot: crate::groups::DirectorySnapshot,
    },
    HostPermit {
        permit: crate::groups::HostPermit,
    },
    Groups {
        groups: Vec<crate::groups::GroupSummary>,
    },
    GroupPage {
        groups: Vec<crate::groups::GroupSummary>,
    },
    GroupJoined {
        group: Box<crate::groups::GroupSummary>,
        join: JoinConfig,
    },
    Feed {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        direct: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        privacy: bool,
        #[serde(default)]
        deleted: Vec<String>,
        posts: Vec<RemotePost>,
        #[serde(rename = "eventCount")]
        event_count: usize,
        #[serde(
            default,
            rename = "accountChain",
            skip_serializing_if = "Vec::is_empty"
        )]
        account_chain: Vec<crate::portability::AccountRecord>,
    },
    Accepted {
        #[serde(rename = "eventId")]
        event_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        envelope: Option<Box<Envelope>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        erased: Option<Box<Envelope>>,
    },
    Page {
        generation: String,
        entries: Vec<LogEntry>,
        next: u64,
        more: bool,
    },
    Error {
        code: String,
        message: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsernameReceipt {
    pub(crate) name: String,
    pub(crate) signature: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<UsernameReceipt>,
    pub cursor: u64,
    pub envelope: Envelope,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorVisibility {
    pub peer: Option<String>,
    pub anonymous: bool,
    pub age: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<CommunityProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<AuthorVisibility>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mine: bool,
    pub id: String,
    pub author: String,
    pub body: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemotePost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<AuthorVisibility>,
    pub headline: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<PostArea>,
    pub profile: Option<CommunityProfile>,
    pub id: String,
    pub author: String,
    pub body: String,
    pub topic: String,
    pub score: i64,
    pub vote: i8,
    pub mine: bool,
    pub replies: Vec<RemoteReply>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub more_replies: bool,
    pub accepted_at: i64,
}

pub(crate) const FEED_POST_LIMIT: usize = 200;

/// Projection is a deterministic function of the event set, independent of import order.
pub fn project(
    envelopes: &[Envelope],
    community: &str,
    viewer: &str,
    hidden: &BTreeSet<String>,
) -> Vec<RemotePost> {
    project_nearby(envelopes, community, viewer, hidden, None)
}

pub fn project_nearby(
    envelopes: &[Envelope],
    community: &str,
    viewer: &str,
    hidden: &BTreeSet<String>,
    nearby: Option<&Nearby>,
) -> Vec<RemotePost> {
    let events: Vec<_> = envelopes
        .iter()
        .filter(|e| e.event.payload.community == community)
        .collect();
    let owners: BTreeMap<_, _> = events
        .iter()
        .filter(|e| {
            matches!(
                e.event.payload.action,
                Action::Post { .. } | Action::PostV2 { .. } | Action::Reply { .. }
            )
        })
        .map(|e| (e.event.id.as_str(), e.event.owner_id()))
        .collect();
    let mut deleted = hidden.clone();
    for e in &events {
        if let Action::Delete { target } = &e.event.payload.action
            && owners.get(target.as_str()) == Some(&e.event.owner_id())
        {
            deleted.insert(target.clone());
        }
    }
    // First resolve each device's own causal sequence, then resolve concurrent devices
    // by signed home acceptance time. One account contributes at most one vote per post.
    let mut device_votes = BTreeMap::new();
    for e in &events {
        if let Action::Vote { target, value } = &e.event.payload.action {
            let key = (
                target.as_str(),
                e.event.owner_id(),
                e.event.payload.author.as_str(),
            );
            let sequence = (e.event.payload.sequence, e.event.id.as_str());
            let value = (
                sequence,
                (
                    e.accepted_at,
                    e.event.payload.author.as_str(),
                    e.event.id.as_str(),
                ),
                *value,
            );
            let entry = device_votes.entry(key).or_insert(value);
            if sequence > entry.0 {
                *entry = value;
            }
        }
    }
    let mut votes = BTreeMap::new();
    for ((target, owner, _), (_, order, value)) in device_votes {
        let entry = votes.entry((target, owner)).or_insert((order, value));
        if order > entry.0 {
            *entry = (order, value);
        }
    }
    let mut scores = BTreeMap::new();
    for ((target, _), (_, value)) in &votes {
        *scores.entry(*target).or_insert(0i64) += i64::from(*value);
    }
    let mut replies_by_parent: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for reply in &events {
        if let Action::Reply {
            parent,
            body,
            display_name,
        } = &reply.event.payload.action
            && !deleted.contains(&reply.event.id)
        {
            replies_by_parent.entry(parent).or_default().push((
                reply.accepted_at,
                RemoteReply {
                    profile: None,
                    username: None,
                    visibility: None,
                    mine: false,
                    id: reply.event.id.clone(),
                    author: display_name.clone(),
                    body: body.clone(),
                },
            ));
        }
    }
    let mut posts = Vec::new();
    for e in &events {
        if let Action::Post {
            body,
            topic,
            display_name,
            ..
        }
        | Action::PostV2 {
            body,
            topic,
            display_name,
            ..
        } = &e.event.payload.action
        {
            let (headline, profile, area) = match &e.event.payload.action {
                Action::PostV2 {
                    headline,
                    profile,
                    area,
                    ..
                } => (headline.clone(), profile.as_deref().cloned(), area.clone()),
                _ => (String::new(), None, None),
            };
            if nearby
                .is_some_and(|filter| !area.as_ref().is_some_and(|place| filter.contains(place)))
            {
                continue;
            }
            if deleted.contains(&e.event.id) {
                continue;
            }
            let mut replies = replies_by_parent
                .remove(e.event.id.as_str())
                .unwrap_or_default();
            replies.sort_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
            // Keep the newest bounded window, still displayed in chronological order.
            // Taking the oldest 200 would make newly submitted comments disappear on sync.
            let skip_replies = replies.len().saturating_sub(200);
            posts.push(RemotePost {
                username: None,
                visibility: None,
                area,
                headline,
                profile,
                id: e.event.id.clone(),
                author: display_name.clone(),
                body: body.clone(),
                topic: topic.clone(),
                score: scores.get(e.event.id.as_str()).copied().unwrap_or(0),
                vote: votes
                    .get(&(e.event.id.as_str(), viewer))
                    .map(|(_, v)| *v)
                    .unwrap_or(0),
                mine: e.event.owner_id() == viewer,
                replies: replies
                    .into_iter()
                    .skip(skip_replies)
                    .map(|(_, r)| r)
                    .collect(),
                more_replies: false,
                accepted_at: e.accepted_at,
            });
        }
    }
    posts.sort_by(|a, b| (b.accepted_at, &b.id).cmp(&(a.accepted_at, &a.id)));
    posts.truncate(FEED_POST_LIMIT);
    posts
}

pub fn require_response(response: Response) -> Result<Response> {
    if let Response::Error { code, message } = response {
        bail!("{code}: {message}");
    }
    Ok(response)
}
