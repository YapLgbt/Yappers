// SPDX-License-Identifier: MIT OR Apache-2.0
//! User-controlled account authority, signed home/device changes, and private data archives.
//!
//! Account keys are recovery/administrative keys. Devices have independent signing sequences.
//! Signed history is immutable; a personal archive never grants public federation admission.
use crate::model::{Envelope, now, public_key, signing_key, verify};
use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const STATE_DOMAIN: &[u8] = b"yap:account-state:v1\0";
const ARCHIVE_DOMAIN: &[u8] = b"yap:personal-archive:v1\0";
const PROOF_DOMAIN: &[u8] = b"yap:account-request:v1\0";

/// Canonical field order and signature domains are versioned protocol contracts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountPayload {
    pub version: u8,
    pub account: String,
    pub revision: u64,
    pub previous: Option<String>,
    pub home: String,
    pub devices: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountRecord {
    pub payload: AccountPayload,
    pub signature: String,
}
impl AccountRecord {
    pub fn id(&self) -> Result<String> {
        Ok(blake3::hash(&serde_json::to_vec(self)?)
            .to_hex()
            .to_string())
    }
    pub fn validate(&self) -> Result<()> {
        let p = &self.payload;
        ensure!(
            p.version == 1 && p.revision > 0,
            "unsupported account state"
        );
        public_key(&p.account)?;
        public_key(&p.home)?;
        ensure!(!p.devices.is_empty(), "invalid device count");
        for device in &p.devices {
            public_key(device)?;
        }
        ensure!(
            (p.revision == 1) == p.previous.is_none(),
            "invalid account predecessor"
        );
        if let Some(previous) = &p.previous {
            ensure!(
                previous.len() == 64 && hex::encode(hex::decode(previous)?) == *previous,
                "invalid predecessor hash"
            );
        }
        verify(
            &p.account,
            &[STATE_DOMAIN, &serde_json::to_vec(p)?].concat(),
            &self.signature,
        )
    }
    pub fn signed(
        root: &SigningKey,
        previous: Option<&Self>,
        home: String,
        devices: BTreeSet<String>,
    ) -> Result<Self> {
        let account = hex::encode(root.verifying_key().as_bytes());
        if let Some(old) = previous {
            old.validate()?;
            ensure!(old.payload.account == account, "account authority mismatch");
        }
        let payload = AccountPayload {
            version: 1,
            account,
            revision: previous
                .map(|v| {
                    v.payload
                        .revision
                        .checked_add(1)
                        .context("account revision exhausted")
                })
                .transpose()?
                .unwrap_or(1),
            previous: previous.map(Self::id).transpose()?,
            home,
            devices,
        };
        let signature = hex::encode(
            root.sign(&[STATE_DOMAIN, &serde_json::to_vec(&payload)?].concat())
                .to_bytes(),
        );
        let record = Self { payload, signature };
        record.validate()?;
        Ok(record)
    }
}
pub fn validate_chain(chain: &[AccountRecord]) -> Result<&AccountRecord> {
    ensure!(!chain.is_empty(), "invalid account chain length");
    for (index, record) in chain.iter().enumerate() {
        record.validate()?;
        ensure!(
            record.payload.revision == index as u64 + 1,
            "account revision gap"
        );
        if index > 0 {
            let old = &chain[index - 1];
            ensure!(
                record.payload.account == old.payload.account
                    && record.payload.previous.as_deref() == Some(old.id()?.as_str()),
                "account chain fork"
            );
        }
    }
    Ok(chain.last().expect("nonempty checked"))
}

/// Recovery material, never returned by server RPCs or printed by CLI commands.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountWallet {
    pub version: u8,
    pub root_secret: String,
    pub device_secret: String,
    pub next_sequence: u64,
    pub chain: Vec<AccountRecord>,
    #[serde(default)]
    pub pending: Vec<crate::model::SignedEvent>,
}
impl AccountWallet {
    /// Upgrade an existing author's key to account authority, preserving legacy authorship.
    pub fn create(root_secret: String, home: String) -> Result<Self> {
        let root = signing_key(&root_secret)?;
        let device_secret = crate::model::new_secret();
        let device = hex::encode(signing_key(&device_secret)?.verifying_key().as_bytes());
        let record = AccountRecord::signed(&root, None, home, BTreeSet::from([device]))?;
        Ok(Self {
            version: 1,
            root_secret,
            device_secret,
            next_sequence: 1,
            chain: vec![record],
            pending: vec![],
        })
    }
    pub fn validate(&self) -> Result<&AccountRecord> {
        ensure!(
            self.version == 1 && self.next_sequence > 0 && self.next_sequence < i64::MAX as u64,
            "invalid account wallet"
        );
        let record = validate_chain(&self.chain)?;
        ensure!(
            hex::encode(signing_key(&self.root_secret)?.verifying_key().as_bytes())
                == record.payload.account,
            "wrong recovery key"
        );
        let device = hex::encode(signing_key(&self.device_secret)?.verifying_key().as_bytes());
        ensure!(
            record.payload.devices.contains(&device),
            "device is not authorized"
        );
        let mut sequences = BTreeSet::new();
        for event in &self.pending {
            event.validate()?;
            ensure!(
                event.owner_id() == record.payload.account
                    && event.payload.author == device
                    && event.payload.home == record.payload.home
                    && event.payload.sequence < self.next_sequence
                    && sequences.insert(event.payload.sequence),
                "invalid account outbox"
            );
        }
        Ok(record)
    }
    /// Recovery/import always mints a new device key. Never clone an active sequence writer.
    /// A move revokes old devices by default; enrollment may explicitly retain them.
    pub fn transition(&self, home: String, retain_devices: bool) -> Result<Self> {
        self.validate()?;
        ensure!(
            self.pending.is_empty(),
            "deliver pending uploads before moving or enrolling"
        );
        Self::recover(
            self.root_secret.clone(),
            self.chain.clone(),
            home,
            retain_devices,
        )
    }
    /// Recovery authority can replace lost/revoked devices using newer, verified public state.
    pub fn recover(
        root_secret: String,
        chain: Vec<AccountRecord>,
        home: String,
        retain_devices: bool,
    ) -> Result<Self> {
        let old = validate_chain(&chain)?;
        let root = signing_key(&root_secret)?;
        ensure!(
            hex::encode(root.verifying_key().as_bytes()) == old.payload.account,
            "wrong recovery authority"
        );
        ensure!(
            !retain_devices || home == old.payload.home,
            "enrollment cannot change home"
        );
        let device_secret = crate::model::new_secret();
        let device = hex::encode(signing_key(&device_secret)?.verifying_key().as_bytes());
        let mut devices = if retain_devices {
            old.payload.devices.clone()
        } else {
            BTreeSet::new()
        };
        devices.insert(device);
        let record = AccountRecord::signed(&root, Some(old), home, devices)?;
        let mut chain = chain;
        chain.push(record);
        let wallet = Self {
            version: 1,
            root_secret,
            device_secret,
            next_sequence: 1,
            chain,
            pending: vec![],
        };
        wallet.validate()?;
        Ok(wallet)
    }
}

/// Authored events only. References to other people's posts do not include their contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchivePayload {
    pub version: u8,
    pub account: String,
    pub chain: Vec<AccountRecord>,
    pub events: Vec<Envelope>,
    pub local_data: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalArchive {
    pub payload: ArchivePayload,
    pub signature: String,
}
impl PersonalArchive {
    pub fn create(
        wallet: &AccountWallet,
        events: Vec<Envelope>,
        local_data: serde_json::Value,
    ) -> Result<Self> {
        let record = wallet.validate()?;
        let mut events = events;
        let deletions: std::collections::BTreeMap<_, _> = events
            .iter()
            .filter_map(|e| {
                if let crate::Action::Delete { target } = &e.event.payload.action {
                    Some((
                        (
                            target.clone(),
                            e.event.owner_id().to_string(),
                            e.event.payload.community.clone(),
                        ),
                        e.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        for event in &mut events {
            if event.event.payload.version == 4
                && matches!(
                    event.event.payload.action,
                    crate::Action::Post { .. }
                        | crate::Action::PostV2 { .. }
                        | crate::Action::Reply { .. }
                )
                && let Some(proof) = deletions.get(&(
                    event.event.id.clone(),
                    event.event.owner_id().into(),
                    event.event.payload.community.clone(),
                ))
            {
                event.validate()?;
                *event = event.erased(proof)?;
            }
        }
        let payload = ArchivePayload {
            version: 1,
            account: record.payload.account.clone(),
            chain: wallet.chain.clone(),
            events,
            local_data,
        };
        let signature = hex::encode(
            signing_key(&wallet.root_secret)?
                .sign(&[ARCHIVE_DOMAIN, &serde_json::to_vec(&payload)?].concat())
                .to_bytes(),
        );
        let archive = Self { payload, signature };
        archive.validate()?;
        Ok(archive)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_ARCHIVE_BYTES,
            "archive too large"
        );
        let p = &self.payload;
        ensure!(p.version == 1, "unsupported archive version");
        ensure!(
            validate_chain(&p.chain)?.payload.account == p.account,
            "archive identity mismatch"
        );
        ensure!(
            serde_json::to_vec(&p.local_data)?.len() <= 256 * 1024,
            "local data too large"
        );
        let mut ids = BTreeSet::new();
        let mut sequences = BTreeSet::new();
        for envelope in &p.events {
            envelope.validate()?;
            ensure!(
                envelope.event.owner_id() == p.account,
                "archive contains another person's event"
            );
            ensure!(
                ids.insert(&envelope.event.id)
                    && sequences.insert((
                        &envelope.event.payload.author,
                        envelope.event.payload.sequence
                    )),
                "duplicate archive event or sequence"
            );
        }
        verify(
            &p.account,
            &[ARCHIVE_DOMAIN, &serde_json::to_vec(p)?].concat(),
            &self.signature,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountProof {
    pub device: String,
    pub timestamp: i64,
    pub nonce: String,
    pub signature: String,
}
impl AccountProof {
    pub fn create(
        wallet: &AccountWallet,
        operation: &str,
        home: &str,
        digest: &str,
    ) -> Result<Self> {
        Self::signed(
            wallet.validate()?,
            &signing_key(&wallet.device_secret)?,
            operation,
            home,
            digest,
        )
    }
    pub fn signed(
        record: &AccountRecord,
        key: &SigningKey,
        operation: &str,
        home: &str,
        digest: &str,
    ) -> Result<Self> {
        record.validate()?;
        let device = hex::encode(key.verifying_key().as_bytes());
        ensure!(
            record.payload.devices.contains(&device),
            "device is not authorized"
        );
        let mut proof = Self {
            device,
            timestamp: now(),
            nonce: crate::model::new_secret(),
            signature: String::new(),
        };
        proof.signature = hex::encode(
            key.sign(&proof.bytes(&record.payload.account, operation, home, digest)?)
                .to_bytes(),
        );
        Ok(proof)
    }
    fn bytes(&self, account: &str, operation: &str, home: &str, digest: &str) -> Result<Vec<u8>> {
        Ok([
            PROOF_DOMAIN,
            &serde_json::to_vec(&(
                account,
                operation,
                home,
                digest,
                &self.device,
                self.timestamp,
                &self.nonce,
            ))?,
        ]
        .concat())
    }
    pub fn validate(
        &self,
        record: &AccountRecord,
        operation: &str,
        home: &str,
        digest: &str,
    ) -> Result<()> {
        record.validate()?;
        ensure!(
            record.payload.devices.contains(&self.device),
            "device revoked or unauthorized"
        );
        ensure!(
            (now() as i128 - self.timestamp as i128).abs() <= 300,
            "account proof expired"
        );
        ensure!(
            self.nonce.len() == 64 && hex::encode(hex::decode(&self.nonce)?) == self.nonce,
            "invalid proof nonce"
        );
        verify(
            &self.device,
            &self.bytes(&record.payload.account, operation, home, digest)?,
            &self.signature,
        )
    }
}
pub fn digest(value: &impl Serialize) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(value)?)
        .to_hex()
        .to_string())
}

/// Encrypted recovery includes the root key; only the user-controlled file contains it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryBundle {
    pub version: u8,
    pub wallet: AccountWallet,
    pub archive: PersonalArchive,
}
impl RecoveryBundle {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported recovery version");
        self.wallet.validate()?;
        self.archive.validate()?;
        ensure!(
            self.wallet.chain == self.archive.payload.chain,
            "recovery state mismatch"
        );
        Ok(())
    }
}
pub fn encrypt_recovery(bundle: &RecoveryBundle, passphrase: String) -> Result<Vec<u8>> {
    use std::io::Write;
    bundle.validate()?;
    ensure!(
        passphrase.chars().count() >= 12 && passphrase.len() <= 1024,
        "use a passphrase of 12–1024 characters"
    );
    let plaintext = serde_json::to_vec(bundle)?;
    ensure!(plaintext.len() <= MAX_ARCHIVE_BYTES, "recovery too large");
    // Predictable mobile memory use: scrypt N=2^18, r=8, p=1 (~256 MiB).
    let mut recipient = age::scrypt::Recipient::new(age::secrecy::SecretString::from(passphrase));
    recipient.set_work_factor(18);
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))?;
    let mut encrypted = Vec::new();
    let mut writer = encryptor.wrap_output(&mut encrypted)?;
    writer.write_all(&plaintext)?;
    writer.finish()?;
    Ok(encrypted)
}
pub fn decrypt_recovery(bytes: &[u8], passphrase: String) -> Result<RecoveryBundle> {
    use std::io::Read;
    ensure!(
        bytes.len() <= MAX_ARCHIVE_BYTES + 65536,
        "recovery file too large"
    );
    let decryptor = age::Decryptor::new(bytes).context("invalid encrypted recovery")?;
    ensure!(
        decryptor.is_scrypt(),
        "expected passphrase-protected recovery"
    );
    let mut identity = age::scrypt::Identity::new(age::secrecy::SecretString::from(passphrase));
    identity.set_max_work_factor(18);
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
    let mut plaintext = Vec::new();
    reader
        .by_ref()
        .take((MAX_ARCHIVE_BYTES + 1) as u64)
        .read_to_end(&mut plaintext)?;
    ensure!(
        plaintext.len() <= MAX_ARCHIVE_BYTES,
        "recovery expands beyond capacity"
    );
    let bundle: RecoveryBundle = serde_json::from_slice(&plaintext)?;
    bundle.validate()?;
    Ok(bundle)
}

fn recovery_check(home: &str, root: &[u8; 32]) -> Result<String> {
    let mut hash = blake3::Hasher::new_derive_key("Yap browser recovery key v1");
    hash.update(public_key(home)?.as_bytes());
    hash.update(root);
    Ok(hex::encode(&hash.finalize().as_bytes()[..4]))
}

pub fn recovery_key(wallet: &AccountWallet) -> Result<String> {
    let record = wallet.validate()?;
    let seed = signing_key(&wallet.root_secret)?.to_bytes();
    let check = recovery_check(&record.payload.home, &seed)?;
    Ok(format!("yap1-{}-{check}", hex::encode(seed)))
}

pub fn read_recovery_key(input: &str, home: &str) -> Result<serde_json::Value> {
    let invalid =
        || anyhow::anyhow!("Check the recovery key. Paste the whole key saved for this Yap home.");
    let (root, check) = input
        .trim()
        .strip_prefix("yap1-")
        .and_then(|text| text.split_once('-'))
        .ok_or_else(invalid)?;
    let key = signing_key(root).map_err(|_| invalid())?;
    ensure!(recovery_check(home, &key.to_bytes())? == check, invalid());
    Ok(
        serde_json::json!({"root": root, "home": home,"account":hex::encode(key.verifying_key().as_bytes())}),
    )
}

/// Offline client operations exposed identically to Kotlin/Native and JVM. Never networked.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum PortableCommand {
    RecoveryKey {
        wallet: AccountWallet,
    },
    ReadRecoveryKey {
        input: String,
        home: String,
    },
    Recover {
        #[serde(rename = "retainDevices", default)]
        retain_devices: bool,
        #[serde(rename = "rootSecret")]
        root_secret: String,
        chain: Vec<AccountRecord>,
        home: String,
    },
    Create {
        #[serde(rename = "rootSecret")]
        root_secret: String,
        home: String,
    },
    Transition {
        wallet: AccountWallet,
        home: String,
        #[serde(rename = "retainDevices")]
        retain_devices: bool,
    },
    Encrypt {
        wallet: AccountWallet,
        events: Vec<Envelope>,
        #[serde(rename = "localData")]
        local_data: serde_json::Value,
        passphrase: String,
    },
    Decrypt {
        ciphertext: String,
        passphrase: String,
    },
}
pub fn command(command: PortableCommand) -> Result<serde_json::Value> {
    use serde_json::json;
    match command {
        PortableCommand::RecoveryKey { wallet } => Ok(json!({"key": recovery_key(&wallet)?})),
        PortableCommand::ReadRecoveryKey { input, home } => read_recovery_key(&input, &home),
        PortableCommand::Recover {
            retain_devices,
            root_secret,
            chain,
            home,
        } => {
            Ok(json!({"wallet":AccountWallet::recover(root_secret, chain, home, retain_devices)?}))
        }
        PortableCommand::Create { root_secret, home } => {
            Ok(json!({"wallet":AccountWallet::create(root_secret, home)?}))
        }
        PortableCommand::Transition {
            wallet,
            home,
            retain_devices,
        } => Ok(json!({"wallet":wallet.transition(home, retain_devices)?})),
        PortableCommand::Encrypt {
            wallet,
            events,
            local_data,
            passphrase,
        } => {
            let archive = PersonalArchive::create(&wallet, events, local_data)?;
            let encrypted = encrypt_recovery(
                &RecoveryBundle {
                    version: 1,
                    wallet,
                    archive,
                },
                passphrase,
            )?;
            Ok(json!({"ciphertext":hex::encode(encrypted)}))
        }
        PortableCommand::Decrypt {
            ciphertext,
            passphrase,
        } => {
            ensure!(
                ciphertext.len() <= (MAX_ARCHIVE_BYTES + 65536) * 2,
                "encrypted recovery too large"
            );
            Ok(json!({"bundle":decrypt_recovery(&hex::decode(ciphertext)?, passphrase)?}))
        }
    }
}
