// SPDX-License-Identifier: AGPL-3.0-only
//! Operator acceptance is evidence. Access comes from the local operator's reviewed policy.
use crate::model::{now, public_key, verify};
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const COVENANT: &str = include_str!("../peering/covenant-v1.txt");
pub const VERSION: &str = "1";
const DOMAIN: &[u8] = b"yap:peer-covenant:v1\0";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Statement {
    version: String,
    covenant_digest: String,
    home: String,
    operator: String,
    contact: String,
    source_url: String,
    revision: String,
    accepted_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    statement: Statement,
    signature: String,
}
impl Statement {
    fn bytes(&self) -> Result<Vec<u8>> {
        Ok([DOMAIN, serde_json::to_vec(self)?.as_slice()].concat())
    }
}
impl Acceptance {
    pub fn sign(
        key: &SigningKey,
        operator: String,
        contact: String,
        source_url: String,
        revision: String,
    ) -> Result<Self> {
        let statement = Statement {
            version: VERSION.into(),
            covenant_digest: blake3::hash(COVENANT.as_bytes()).to_hex().to_string(),
            home: hex::encode(key.verifying_key().as_bytes()),
            operator,
            contact,
            source_url,
            revision,
            accepted_at: now(),
        };
        let signature = hex::encode(key.sign(&statement.bytes()?).to_bytes());
        let receipt = Self {
            statement,
            signature,
        };
        receipt.validate()?;
        Ok(receipt)
    }
    pub fn validate(&self) -> Result<()> {
        let s = &self.statement;
        ensure!(
            s.version == VERSION
                && s.covenant_digest == blake3::hash(COVENANT.as_bytes()).to_hex().as_str(),
            "different covenant text or version"
        );
        ensure!(
            s.accepted_at > 0 && s.accepted_at <= now() + 300,
            "invalid acceptance date"
        );
        for (value, max) in [(&s.operator, 200), (&s.contact, 320), (&s.source_url, 2048)] {
            ensure!(
                !value.trim().is_empty()
                    && value == value.trim()
                    && value.len() <= max
                    && !value.chars().any(char::is_control),
                "invalid operator, contact or source"
            );
        }
        let url = url::Url::parse(&s.source_url)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none(),
            "public HTTPS source URL required"
        );
        ensure!(
            matches!(s.revision.len(), 40 | 64)
                && s.revision
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "use a full Git commit or SHA-256 archive digest"
        );
        verify(&s.home, &s.bytes()?, &self.signature)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    Active,
    Suspended,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovedPeer {
    acceptance: Acceptance,
    communities: BTreeSet<String>,
    status: Access,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Policy {
    managed_homes: BTreeSet<String>,
    peers: Vec<ApprovedPeer>,
}
impl Policy {
    pub(crate) fn load() -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("YAP_PEERING_POLICY") else {
            return Ok(None);
        };
        let policy: Self =
            serde_json::from_reader(std::fs::File::open(path).context("missing peering policy")?)?;
        policy.validate()?;
        Ok(Some(policy))
    }
    pub(crate) fn validate(&self) -> Result<()> {
        let mut seen = self.managed_homes.clone();
        for home in &seen {
            public_key(home)?;
        }
        for peer in &self.peers {
            peer.acceptance.validate()?;
            ensure!(
                seen.insert(peer.acceptance.statement.home.clone()),
                "duplicate peering identity"
            );
            ensure!(
                !peer.communities.is_empty()
                    && peer
                        .communities
                        .iter()
                        .all(|s| !s.is_empty() && s.is_ascii() && s.len() <= 160),
                "invalid admission scopes"
            );
        }
        Ok(())
    }
    pub(crate) fn external(&self, home: &str) -> bool {
        self.peers
            .iter()
            .any(|p| p.acceptance.statement.home == home)
    }
    pub(crate) fn access(&self, home: &str, community: &str) -> Option<Access> {
        if self.managed_homes.contains(home) {
            return Some(Access::Active);
        }
        self.peers
            .iter()
            .find(|peer| {
                peer.acceptance.statement.home == home && peer.communities.contains(community)
            })
            .map(|peer| peer.status)
    }
}
