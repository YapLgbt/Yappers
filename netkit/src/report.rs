// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reports are addressed to one home. A private receipt authorizes status lookup.
use crate::model::{new_secret, now};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Harassment,
    Outing,
    Stalking,
    Threat,
    IntimateImage,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Subject {
    Post,
    Reply,
    Account,
    Message,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub category: Category,
    pub subject: Subject,
    pub reference: Option<String>,
    pub text: String,
}
impl Evidence {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.reference.as_deref().is_none_or(is_hex_id),
            "invalid subject identifier"
        );
        ensure!(
            self.text.chars().count() <= 2000
                && (self.reference.is_some() || !self.text.trim().is_empty()),
            "include an identifier or up to 2000 characters of evidence"
        );
        ensure!(
            !self.text.to_ascii_lowercase().contains("yap1-") && !self.text.contains("yr1."),
            "remove recovery keys and private receipts before sending"
        );
        Ok(())
    }
}
fn is_hex_id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn receipt(home: &str) -> String {
    format!("yr1.{home}.{}.{}", now(), new_secret())
}
pub fn issued_at(receipt: &str, home: &str) -> Result<i64> {
    let mut parts = receipt.split('.');
    ensure!(
        parts.next() == Some("yr1") && parts.next() == Some(home),
        "receipt belongs to another home or is invalid"
    );
    let stamp = parts.next().context("incomplete receipt")?;
    ensure!(
        stamp.len() <= 12 && stamp.bytes().all(|c| c.is_ascii_digit()),
        "invalid receipt date"
    );
    ensure!(
        parts.next().is_some_and(is_hex_id) && parts.next().is_none(),
        "invalid receipt"
    );
    Ok(stamp.parse()?)
}
