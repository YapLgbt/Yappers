// SPDX-License-Identifier: MIT OR Apache-2.0
//! A home's explicit delegation of complete service to one additional transport identity.
use crate::model::{public_key, verify};
use anyhow::{Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FullHomeStandby {
    pub home: String,
    pub standby: EndpointAddr,
    pub signature: String,
}

impl FullHomeStandby {
    fn statement(&self) -> Result<Vec<u8>> {
        // Addresses are interchangeable connection hints. Iroh authenticates the signed ID.
        let mut bytes = b"yap:full-home-standby:v1\0".to_vec();
        bytes.extend_from_slice(public_key(&self.home)?.as_bytes());
        bytes.extend_from_slice(self.standby.id.as_bytes());
        Ok(bytes)
    }

    pub fn issue(owner: &SigningKey, standby: EndpointAddr) -> Result<Self> {
        let mut grant = Self {
            home: hex::encode(owner.verifying_key().as_bytes()),
            standby,
            signature: String::new(),
        };
        grant.signature = hex::encode(owner.sign(&grant.statement()?).to_bytes());
        grant.validate(&grant.home)?;
        Ok(grant)
    }

    pub fn validate(&self, enrolled: &str) -> Result<()> {
        ensure!(
            self.home == enrolled && self.standby.id.to_string() != enrolled,
            "standby delegation does not match the enrolled home"
        );
        verify(enrolled, &self.statement()?, &self.signature)
    }
}
