// SPDX-License-Identifier: MIT OR Apache-2.0
//! Public group identity and verifiable automatic federation records.
//!
//! Descriptors are immutable and owner signed. A host needs an owner-signed permit;
//! a self-signed address announcement alone never grants federation authority.
use crate::model::{new_secret, now, signing_key};
use anyhow::{Result, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

pub const MAX_GROUPS: usize = 64;
pub const MAX_PROVIDERS: usize = 128;
pub const MAX_GROUP_PROVIDERS: usize = 16;
pub const ANNOUNCEMENT_LIFETIME: i64 = 3600;
const GROUP_DOMAIN: &[u8] = b"yappers:group:v1\0";
const PERMIT_DOMAIN: &[u8] = b"yappers:group-host:v1\0";
const HOST_DOMAIN: &[u8] = b"yappers:host-address:v1\0";

fn text(value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty()
            && value.encode_utf16().count() <= max
            && !value.chars().any(char::is_control),
        "invalid group text"
    );
    Ok(())
}
fn sign<T: Serialize>(key: &SigningKey, domain: &[u8], value: &T) -> Result<String> {
    let bytes = [domain, &serde_json::to_vec(value)?].concat();
    Ok(hex::encode(key.sign(&bytes).to_bytes()))
}
fn verify<T: Serialize>(key: &str, signature: &str, domain: &[u8], value: &T) -> Result<()> {
    let bytes: [u8; 32] = hex::decode(key)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid owner key"))?;
    ensure!(hex::encode(bytes) == key, "noncanonical owner key");
    let decoded = hex::decode(signature)?;
    ensure!(
        hex::encode(&decoded) == signature,
        "noncanonical group signature"
    );
    VerifyingKey::from_bytes(&bytes)?.verify_strict(
        &[domain, &serde_json::to_vec(value)?].concat(),
        &Signature::from_slice(&decoded)?,
    )?;
    Ok(())
}

/// Manually chosen coarse place; neither GPS coordinates nor proof of residence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Locality {
    pub city: String,
    pub region: String,
    pub country: String,
}
impl Locality {
    pub fn validate(&self) -> Result<()> {
        text(&self.city, 60)?;
        ensure!(
            self.region.encode_utf16().count() <= 60 && !self.region.chars().any(char::is_control),
            "invalid region"
        );
        ensure!(
            self.country.len() == 2 && self.country.bytes().all(|c| c.is_ascii_uppercase()),
            "use a two-letter uppercase country code"
        );
        Ok(())
    }
    /// Catalog matching key; labels remain human-readable and are not geocoding claims.
    pub fn key(&self) -> String {
        let region = self.region.trim().to_lowercase();
        let city = self.city.trim().to_lowercase();
        format!("{}|{}|{}|{}", self.country, region.len(), region, city)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewGroup {
    pub name: String,
    pub description: String,
    pub category: String,
    pub locality: Option<Locality>,
}
impl NewGroup {
    pub fn validate(&self) -> Result<()> {
        text(&self.name, 60)?;
        text(&self.description.replace(['\n', '\t'], " "), 300)?;
        ensure!(
            crate::model::TOPICS.contains(&self.category.as_str()),
            "invalid group category"
        );
        if let Some(locality) = &self.locality {
            locality.validate()?;
        }
        Ok(())
    }
}

/// Every v1 public group permits automatic provider enrollment at its authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupPayload {
    pub version: u8,
    pub owner: String,
    pub nonce: String,
    pub created_at: i64,
    pub details: NewGroup,
    pub federation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedGroup {
    pub id: String,
    pub payload: GroupPayload,
    pub signature: String,
}
impl SignedGroup {
    pub fn create(key: &SigningKey, details: NewGroup) -> Result<Self> {
        details.validate()?;
        let owner = hex::encode(key.verifying_key().as_bytes());
        let payload = GroupPayload {
            version: 1,
            owner,
            nonce: new_secret(),
            created_at: now(),
            details,
            federation: "open".into(),
        };
        let id = Self::id_for(&payload)?;
        let signature = sign(key, GROUP_DOMAIN, &payload)?;
        Ok(Self {
            id,
            payload,
            signature,
        })
    }
    fn id_for(payload: &GroupPayload) -> Result<String> {
        Ok(format!(
            "{}/groups/{}",
            payload.owner,
            blake3::hash(&serde_json::to_vec(payload)?).to_hex()
        ))
    }
    pub fn validate(&self) -> Result<()> {
        self.payload.details.validate()?;
        ensure!(
            self.payload.version == 1 && self.payload.federation == "open",
            "unsupported group policy"
        );
        ensure!(
            self.payload.created_at > 0 && self.payload.created_at <= now() + 300,
            "invalid group creation time"
        );
        ensure!(
            self.payload.nonce.len() == 64 && hex::decode(&self.payload.nonce).is_ok(),
            "invalid group nonce"
        );
        ensure!(
            self.id == Self::id_for(&self.payload)?,
            "group identity mismatch"
        );
        verify(
            &self.payload.owner,
            &self.signature,
            GROUP_DOMAIN,
            &self.payload,
        )
    }
}

/// Stable permission for one home to originate/replicate this public group.
/// It survives address expiry so previously accepted signed history remains verifiable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostPermit {
    pub group: String,
    pub host: String,
    pub signature: String,
}
impl HostPermit {
    pub fn create(key: &SigningKey, group: &SignedGroup, host: String) -> Result<Self> {
        ensure!(
            hex::encode(key.verifying_key().as_bytes()) == group.payload.owner,
            "only the group authority can admit hosts"
        );
        let signature = sign(key, PERMIT_DOMAIN, &(&group.id, &host))?;
        Ok(Self {
            group: group.id.clone(),
            host,
            signature,
        })
    }
    pub fn validate(&self, group: &SignedGroup) -> Result<()> {
        ensure!(
            self.group == group.id,
            "host permit belongs to another group"
        );
        let key: [u8; 32] = hex::decode(&self.host)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid host key"))?;
        ensure!(hex::encode(key) == self.host, "noncanonical host key");
        VerifyingKey::from_bytes(&key)?;
        verify(
            &group.payload.owner,
            &self.signature,
            PERMIT_DOMAIN,
            &(&self.group, &self.host),
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostPayload {
    pub group: String,
    pub address: EndpointAddr,
    pub issued_at: i64,
    pub expires_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostAnnouncement {
    pub payload: HostPayload,
    pub signature: String,
}
impl HostAnnouncement {
    pub fn create(key: &SigningKey, group: String, address: EndpointAddr) -> Result<Self> {
        ensure!(
            address.id.to_string() == hex::encode(key.verifying_key().as_bytes()),
            "address identity mismatch"
        );
        Self::create_after(key, group, address, 0)
    }
    pub fn create_after(
        key: &SigningKey,
        group: String,
        address: EndpointAddr,
        previous: i64,
    ) -> Result<Self> {
        ensure!(
            address.id.to_string() == hex::encode(key.verifying_key().as_bytes()),
            "address identity mismatch"
        );
        let issued_at = now().max(previous.saturating_add(1));
        ensure!(issued_at <= now() + 300, "announcement clock too far ahead");
        let payload = HostPayload {
            group,
            address,
            issued_at,
            expires_at: issued_at + ANNOUNCEMENT_LIFETIME,
        };
        let signature = sign(key, HOST_DOMAIN, &payload)?;
        Ok(Self { payload, signature })
    }
    pub fn validate(&self, permit: &HostPermit) -> Result<()> {
        ensure!(
            self.payload.group == permit.group
                && self.payload.address.id.to_string() == permit.host,
            "announcement identity mismatch"
        );
        ensure!(
            self.payload.issued_at > 0
                && self.payload.issued_at <= now() + 300
                && self.payload.expires_at > now()
                && self.payload.expires_at > self.payload.issued_at
                && self.payload.expires_at <= self.payload.issued_at + ANNOUNCEMENT_LIFETIME,
            "expired or invalid host announcement"
        );
        ensure!(
            serde_json::to_vec(&self.payload.address)?.len() <= 4096,
            "host address too large"
        );
        verify(&permit.host, &self.signature, HOST_DOMAIN, &self.payload)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectorySnapshot {
    pub groups: Vec<SignedGroup>,
    pub permits: Vec<HostPermit>,
    pub hosts: Vec<HostAnnouncement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub locality: Option<Locality>,
    pub homes: usize,
    pub hosted: bool,
}

// Keep the existing private-key parsing contract in one place for operator tooling.
pub fn authority_id(secret: &str) -> Result<String> {
    Ok(hex::encode(signing_key(secret)?.verifying_key().as_bytes()))
}
