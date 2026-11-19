// SPDX-License-Identifier: AGPL-3.0-only
//! Headless user-owned account/recovery workflow. All secrets remain in mode-0600 files.
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use yappers_netkit::{
    client::{Client, ClientConfig},
    model::*,
    portability::*,
};

#[derive(Subcommand)]
pub enum AccountCommand {
    /// Deliver the signed move/revocation to a previous home; no old membership token needed.
    Notify {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        address: PathBuf,
    },
    /// Create an account/device, optionally preserving a legacy CLI author's identity.
    Init {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        legacy_identity: Option<PathBuf>,
    },
    /// Print public identity and home state only.
    Show {
        #[arg(long)]
        wallet: PathBuf,
    },
    /// Post using a delegated device and durable, idempotent outbox.
    Post {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        headline: String,
        #[arg(long)]
        body: String,
        #[arg(long, default_value = "Neighbor")]
        name: String,
    },
    /// Retry exact queued events, never regenerate after an uncertain result.
    Retry {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
    },
    /// Encrypt authored history and account recovery; passphrase is read from a private file.
    Export {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        passphrase_file: PathBuf,
        #[arg(long)]
        local_data: Option<PathBuf>,
        /// Export locally retained receipts without contacting the home.
        #[arg(long)]
        offline: bool,
    },
    /// Recover/move to a chosen home using a fresh device; replay safely after partial transfer.
    Import {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        passphrase_file: PathBuf,
        /// Keep uncertain pending events as private drafts; never silently republish them.
        #[arg(long)]
        pending_as_drafts: bool,
    },
    /// Explicitly add a second device at the same home. Output contains recovery authority.
    Enroll {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Read one's private imported/authored history without printing its content.
    History {
        #[arg(long)]
        wallet: PathBuf,
        #[arg(long)]
        join: PathBuf,
    },
}

/// Lock files use OS advisory locks; separate processes may never sign from one wallet.
struct WalletLock {
    file: fs::File,
}
impl WalletLock {
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let lock = path.with_extension("wallet-lock");
        let mut options = fs::OpenOptions::new();
        options.create(true).write(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(lock)?;
        file.try_lock()
            .context("account wallet is already in use")?;
        Ok(Self { file })
    }
}
impl Drop for WalletLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
fn private_read(path: &Path) -> Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            fs::metadata(path)?.permissions().mode() & 0o077 == 0,
            "private file permissions must be owner-only"
        );
    }
    ensure!(
        fs::metadata(path)?.len() <= (MAX_ARCHIVE_BYTES + 65536) as u64,
        "private input exceeds capacity"
    );
    Ok(fs::read(path)?)
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", &new_secret()[..16]));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}
fn read_wallet(path: &Path) -> Result<AccountWallet> {
    let wallet: AccountWallet = serde_json::from_slice(&private_read(path)?)?;
    wallet.validate()?;
    Ok(wallet)
}
fn save_wallet(path: &Path, wallet: &AccountWallet) -> Result<()> {
    wallet.validate()?;
    private_write(path, &serde_json::to_vec_pretty(wallet)?)
}
fn history(path: &Path) -> Result<Vec<Envelope>> {
    let path = path.with_extension("history.json");
    if path.exists() {
        Ok(serde_json::from_slice(&private_read(&path)?)?)
    } else {
        Ok(vec![])
    }
}
fn merge_history(
    wallet: &AccountWallet,
    left: Vec<Envelope>,
    right: Vec<Envelope>,
) -> Result<Vec<Envelope>> {
    let mut all = std::collections::BTreeMap::<String, Envelope>::new();
    for event in left.into_iter().chain(right) {
        event.validate()?;
        if let Some(old) = all.get(&event.event.id) {
            ensure!(old.same_acceptance(&event)?, "conflicting authored history");
            if old.deletion.is_some() {
                continue;
            }
        }
        all.insert(event.event.id.clone(), event);
    }
    let events: Vec<_> = all.into_values().collect();
    Ok(
        PersonalArchive::create(wallet, events, serde_json::Value::Null)?
            .payload
            .events,
    )
}
fn save_history(path: &Path, wallet: &AccountWallet, events: Vec<Envelope>) -> Result<()> {
    let events = merge_history(wallet, history(path)?, events)?;
    let bytes = serde_json::to_vec(&events)?;
    ensure!(
        bytes.len() <= MAX_ARCHIVE_BYTES,
        "local authored history capacity reached; preserve/export files before continuing"
    );
    private_write(&path.with_extension("history.json"), &bytes)
}
fn join(path: &Path) -> Result<JoinConfig> {
    let value: JoinConfig = serde_json::from_slice(&private_read(path)?)?;
    value.validate()?;
    Ok(value)
}
fn passphrase(path: &Path) -> Result<String> {
    let bytes = private_read(path)?;
    ensure!(bytes.len() <= 1026, "passphrase file too large");
    Ok(String::from_utf8(bytes)?
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}
async fn client(wallet: &AccountWallet, join: JoinConfig) -> Result<Client> {
    wallet.validate()?;
    Client::open(ClientConfig {
        full_home_standby: None,
        join,
        author_secret: wallet.device_secret.clone(),
        next_sequence: wallet.next_sequence,
        account_chain: wallet.chain.clone(),
    })
    .await
}
async fn flush(client: &Client, wallet: &mut AccountWallet, path: &Path) -> Result<()> {
    while let Some(event) = wallet.pending.first().cloned() {
        let receipt = client.submit(event).await?;
        let Response::Accepted {
            envelope: Some(envelope),
            erased,
            ..
        } = receipt
        else {
            anyhow::bail!("missing acceptance receipt")
        };
        let mut receipts = vec![*envelope];
        receipts.extend(erased.map(|target| *target));
        save_history(path, wallet, receipts)?;
        wallet.pending.remove(0);
        save_wallet(path, wallet)?;
    }
    Ok(())
}

pub async fn execute(command: AccountCommand) -> Result<()> {
    match command {
        AccountCommand::Notify {
            wallet,
            join: invitation,
            address,
        } => {
            let wallet = read_wallet(&wallet)?;
            let client = client(&wallet, join(&invitation)?).await?;
            let address = serde_json::from_slice(&fs::read(address)?)?;
            let result = client.notify_account(address, &wallet.chain).await;
            client.close().await;
            result?;
            println!("signed account update delivered to previous home");
        }
        AccountCommand::Init {
            wallet: path,
            join: invitation,
            legacy_identity,
        } => {
            let _lock = WalletLock::acquire(&path)?;
            let invitation = join(&invitation)?;
            let wallet = if path.exists() {
                read_wallet(&path)?
            } else {
                let root = if let Some(legacy) = legacy_identity {
                    let legacy: serde_json::Value =
                        serde_json::from_slice(&private_read(&legacy)?)?;
                    ensure!(
                        legacy["pending"].as_array().is_none_or(|p| p.is_empty()),
                        "deliver legacy outbox before upgrade"
                    );
                    legacy["secret"]
                        .as_str()
                        .context("missing legacy signing key")?
                        .to_owned()
                } else {
                    new_secret()
                };
                let created = AccountWallet::create(root, invitation.server_addr.id.to_string())?;
                save_wallet(&path, &created)?;
                created
            };
            let client = client(&wallet, invitation).await?;
            let result = client.register_account(&wallet).await;
            client.close().await;
            result?;
            println!("account {} ready", wallet.validate()?.payload.account);
        }
        AccountCommand::Show { wallet } => {
            let wallet = read_wallet(&wallet)?;
            let record = wallet.validate()?;
            println!(
                "account {}\nhome {}\nrevision {}\nauthorized devices {}\npending {}",
                record.payload.account,
                record.payload.home,
                record.payload.revision,
                record.payload.devices.len(),
                wallet.pending.len()
            );
        }
        AccountCommand::Post {
            wallet: path,
            join: invitation,
            headline,
            body,
            name,
        } => {
            let _lock = WalletLock::acquire(&path)?;
            let mut wallet = read_wallet(&path)?;
            let client = client(&wallet, join(&invitation)?).await?;
            let result = async {
                ensure!(
                    wallet.pending.is_empty(),
                    "retry the existing outbox before posting again"
                );
                let prepared = client.prepare(Action::PostV2 {
                    area: None,
                    headline,
                    body,
                    topic: "Chatter".into(),
                    anonymous: false,
                    display_name: name,
                    profile: None,
                })?;
                wallet.next_sequence = prepared.next_sequence;
                wallet.pending.push(prepared.event);
                save_wallet(&path, &wallet)?;
                flush(&client, &mut wallet, &path).await
            }
            .await;
            client.close().await;
            result?;
            println!("account post accepted");
        }
        AccountCommand::Retry {
            wallet: path,
            join: invitation,
        } => {
            let _lock = WalletLock::acquire(&path)?;
            let mut wallet = read_wallet(&path)?;
            let client = client(&wallet, join(&invitation)?).await?;
            let result = flush(&client, &mut wallet, &path).await;
            client.close().await;
            result?;
            println!("account outbox delivered");
        }
        AccountCommand::Export {
            wallet: path,
            join: invitation,
            output,
            passphrase_file,
            local_data,
            offline,
        } => {
            ensure!(
                output != path && output != passphrase_file && output != invitation,
                "export must use a separate output file"
            );
            ensure!(!output.exists(), "export output already exists");
            let _lock = WalletLock::acquire(&path)?;
            let wallet = read_wallet(&path)?;
            let data = local_data
                .map(|p| -> Result<serde_json::Value> {
                    Ok(serde_json::from_slice(&private_read(&p)?)?)
                })
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            let invitation = join(&invitation)?;
            ensure!(
                wallet.validate()?.payload.home == invitation.server_addr.id.to_string(),
                "export invitation is for another home"
            );
            let mut events = history(&path)?;
            if !offline {
                let client = client(&wallet, invitation.clone()).await?;
                let remote = client.export_owned(&wallet).await;
                client.close().await;
                events = merge_history(&wallet, events, remote?)?;
                save_history(&path, &wallet, events.clone())?;
            }
            let archive = PersonalArchive::create(
                &wallet,
                events,
                serde_json::json!({
                    "client": data, "sourceHomeAddress": invitation.server_addr,
                    "historyCoverage": if offline { "local-receipts" } else { "local-plus-home" }
                }),
            )?;
            let count = archive.payload.events.len();
            let encrypted = encrypt_recovery(
                &RecoveryBundle {
                    version: 1,
                    wallet,
                    archive,
                },
                passphrase(&passphrase_file)?,
            )?;
            private_write(&output, &encrypted)?;
            println!(
                "encrypted recovery exported: {count} authored events; keep its passphrase separate"
            );
        }
        AccountCommand::Import {
            wallet: path,
            join: invitation_path,
            input,
            passphrase_file,
            pending_as_drafts,
        } => {
            ensure!(
                path != input && path != invitation_path && path != passphrase_file,
                "import wallet must be a separate file"
            );
            let _lock = WalletLock::acquire(&path)?;
            let mut bundle =
                decrypt_recovery(&private_read(&input)?, passphrase(&passphrase_file)?)?;
            ensure!(
                bundle.wallet.pending.is_empty() || pending_as_drafts,
                "recovery has pending uploads; deliver first or explicitly use --pending-as-drafts"
            );
            let drafts = std::mem::take(&mut bundle.wallet.pending);
            let invitation = join(&invitation_path)?;
            let destination = invitation.server_addr.id.to_string();
            let wallet = if path.exists() {
                let saved = read_wallet(&path)?;
                ensure!(
                    saved.validate()?.payload.account == bundle.archive.payload.account
                        && saved.validate()?.payload.home == destination
                        && saved.chain.starts_with(&bundle.wallet.chain)
                        && saved.chain.len() > bundle.wallet.chain.len(),
                    "existing wallet is not this migration; choose another output"
                );
                saved
            } else {
                // Probe destination before signing a new revision; do not blindly fork known history.
                let probe = Client::open(ClientConfig {
                    full_home_standby: None,
                    join: invitation.clone(),
                    author_secret: bundle.wallet.device_secret.clone(),
                    next_sequence: bundle.wallet.next_sequence,
                    account_chain: vec![],
                })
                .await?;
                let known = probe.lookup_account(&bundle.archive.payload.account).await;
                probe.close().await;
                let known = known?;
                ensure!(
                    known.is_empty()
                        || known.starts_with(&bundle.wallet.chain)
                        || bundle.wallet.chain.starts_with(&known),
                    "destination has conflicting account history"
                );
                let latest = if known.len() > bundle.wallet.chain.len() {
                    known
                } else {
                    bundle.wallet.chain.clone()
                };
                let created = AccountWallet::recover(
                    bundle.wallet.root_secret.clone(),
                    latest,
                    destination,
                    false,
                )?;
                // Persist the exact new key/revision before any network side effect. Re-run to resume.
                save_wallet(&path, &created)?;
                created
            };
            let client = client(&wallet, invitation).await?;
            let result = client
                .import_owned(&wallet, &bundle.archive.payload.events)
                .await;
            client.close().await;
            result?;
            save_history(&path, &wallet, bundle.archive.payload.events.clone())?;
            if !drafts.is_empty() {
                private_write(
                    &path.with_extension("pending-drafts.json"),
                    &serde_json::to_vec(&drafts)?,
                )?;
            }
            // Local preferences/profile remain a separate private file, never a public feed event.
            private_write(
                &path.with_extension("local-data.json"),
                &serde_json::to_vec(&bundle.archive.payload.local_data)?,
            )?;
            println!(
                "account {} moved/recovered; {} authored events retained privately; original event IDs/signatures preserved",
                wallet.validate()?.payload.account,
                bundle.archive.payload.events.len()
            );
        }
        AccountCommand::Enroll {
            wallet: path,
            join: invitation,
            output,
        } => {
            ensure!(
                output != path && !output.exists(),
                "device output must be a new file"
            );
            let _lock = WalletLock::acquire(&path)?;
            let wallet = read_wallet(&path)?;
            let enrolled = wallet.transition(wallet.validate()?.payload.home.clone(), true)?;
            // Persist new state on both local wallets before registration. Existing device keeps its counter/key.
            save_wallet(&output, &enrolled)?;
            let mut existing = wallet;
            existing.chain = enrolled.chain.clone();
            save_wallet(&path, &existing)?;
            let client = client(&enrolled, join(&invitation)?).await?;
            let result = client.register_account(&enrolled).await;
            client.close().await;
            result?;
            println!(
                "second device enrolled; recovery authority is included in its private wallet"
            );
        }
        AccountCommand::History {
            wallet,
            join: invitation,
        } => {
            let wallet = read_wallet(&wallet)?;
            let client = client(&wallet, join(&invitation)?).await?;
            let events = client.export_owned(&wallet).await;
            client.close().await;
            println!("{} validated authored events available", events?.len());
        }
    }
    Ok(())
}
