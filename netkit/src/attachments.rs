// SPDX-License-Identifier: MIT OR Apache-2.0
//! Attachment metadata travels inside Olm or Megolm. The Home only receives sealed chunks.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};

pub const MAX_ATTACHMENT_BYTES: usize = 20_000_000;
pub const ATTACHMENT_CHUNK_BYTES: usize = 256 * 1024;
const NONCE_BYTES: usize = 24;
pub(crate) const CHUNK_OVERHEAD: usize = NONCE_BYTES + 16;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: String,
    pub key: String,
    pub name: String,
    pub size: usize,
}

pub(crate) fn id_bytes(id: &str) -> Result<[u8; 32]> {
    ensure!(
        id.len() == 64
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid attachment identifier"
    );
    hex::decode(id)?
        .try_into()
        .ok()
        .context("invalid attachment identifier")
}

pub(crate) fn chunk_count(size: usize) -> Result<usize> {
    ensure!(
        (1..=MAX_ATTACHMENT_BYTES).contains(&size),
        "choose a file within 20 MB"
    );
    Ok(size.div_ceil(ATTACHMENT_CHUNK_BYTES))
}
pub(crate) fn chunk_size(size: usize, part: usize) -> Result<usize> {
    ensure!(part < chunk_count(size)?, "invalid attachment part");
    Ok((size - part * ATTACHMENT_CHUNK_BYTES).min(ATTACHMENT_CHUNK_BYTES))
}

impl Attachment {
    pub(crate) fn create(name: String, size: usize) -> Result<Self> {
        let attachment = Self {
            id: crate::model::new_secret(),
            key: crate::model::new_secret(),
            name,
            size,
        };
        attachment.check()?;
        Ok(attachment)
    }

    pub(crate) fn check(&self) -> Result<()> {
        id_bytes(&self.id)?;
        id_bytes(&self.key)?;
        chunk_count(self.size)?;
        ensure!(
            !self.name.trim().is_empty()
                && self.name.encode_utf16().count() <= 255
                && !self.name.contains(['/', '\\', ':'])
                && !self.name.chars().any(char::is_control)
                && !matches!(self.name.as_str(), "." | ".."),
            "invalid attachment filename"
        );
        Ok(())
    }

    fn materials(&self, part: usize) -> Result<([u8; 32], Vec<u8>)> {
        self.check()?;
        chunk_size(self.size, part)?;
        let mut aad = b"yap:encrypted-attachment:v1".to_vec();
        aad.extend(id_bytes(&self.id)?);
        aad.extend((self.size as u64).to_be_bytes());
        aad.extend((part as u64).to_be_bytes());
        let key = id_bytes(&self.key)?;
        Ok((key, aad))
    }

    pub(crate) fn seal(&self, part: usize, encoded: &str) -> Result<String> {
        let size = chunk_size(self.size, part)?;
        ensure!(
            encoded.len() <= size.div_ceil(3) * 4,
            "invalid attachment part"
        );
        let plain = STANDARD.decode(encoded)?;
        ensure!(plain.len() == size, "invalid attachment part");
        let (key, aad) = self.materials(part)?;
        // The same part can be retried. Different input must yield a different
        // nonce even when a file provider changes the bytes between attempts.
        let nonce_key = blake3::derive_key("yap encrypted attachment v1 chunk nonce", &key);
        let mut hasher = blake3::Hasher::new_keyed(&nonce_key);
        hasher.update(&aad);
        hasher.update(&plain);
        let digest = hasher.finalize();
        let nonce = &digest.as_bytes()[..NONCE_BYTES];
        let cipher = XChaCha20Poly1305::new((&key).into());
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: &plain,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("could not encrypt attachment"))?;
        Ok(STANDARD.encode([nonce, &ciphertext].concat()))
    }

    pub(crate) fn open(&self, part: usize, encoded: &str) -> Result<String> {
        let size = chunk_size(self.size, part)? + CHUNK_OVERHEAD;
        ensure!(
            encoded.len() <= size.div_ceil(3) * 4,
            "invalid encrypted attachment part"
        );
        let ciphertext = STANDARD.decode(encoded)?;
        ensure!(
            ciphertext.len() == size,
            "invalid encrypted attachment part"
        );
        let (nonce, ciphertext) = ciphertext.split_at(NONCE_BYTES);
        let (key, aad) = self.materials(part)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        let plain = cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("could not authenticate attachment"))?;
        Ok(STANDARD.encode(plain))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_chunks_are_bounded_and_authenticated() -> Result<()> {
        assert_eq!(chunk_count(MAX_ATTACHMENT_BYTES)?, 77);
        assert!(chunk_count(MAX_ATTACHMENT_BYTES + 1).is_err());
        let file = Attachment::create("example.txt".into(), ATTACHMENT_CHUNK_BYTES + 3)?;
        let clear = STANDARD.encode(b"Yap");
        let sealed = file.seal(1, &clear)?;
        assert_ne!(clear, sealed);
        assert_eq!(file.seal(1, &clear)?, sealed);
        assert_ne!(
            STANDARD.decode(file.seal(1, &STANDARD.encode(b"Bap"))?)?[..NONCE_BYTES],
            STANDARD.decode(&sealed)?[..NONCE_BYTES]
        );
        assert_eq!(file.open(1, &sealed)?, clear);
        assert!(file.open(0, &sealed).is_err());
        let mut tampered = STANDARD.decode(&sealed)?;
        tampered[0] ^= 1;
        assert!(file.open(1, &STANDARD.encode(tampered)).is_err());
        let different = Attachment::create("example.txt".into(), file.size)?;
        assert!(different.open(1, &sealed).is_err());
        Ok(())
    }
}
