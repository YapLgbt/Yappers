// SPDX-License-Identifier: MIT OR Apache-2.0
//! Outbound mobile/CLI client. Preparation signs locally; submission is a separate RPC.
//!
//! Callers must persist the returned next sequence and immutable event before submit.
//! A failed/timed-out submit may already be accepted remotely: retry that exact event.

use crate::{model::*, transport};
use anyhow::{Result, ensure};
use ed25519_dalek::SigningKey;
use iroh::Endpoint;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientConfig {
    #[serde(default)]
    pub full_home_standby: Option<crate::takeover::FullHomeStandby>,
    pub join: JoinConfig,
    pub author_secret: String,
    pub next_sequence: u64,
    #[serde(default)]
    pub account_chain: Vec<crate::portability::AccountRecord>,
}
pub struct Client {
    standby: Option<crate::takeover::FullHomeStandby>,
    standby_until: AtomicU64,
    #[cfg(test)]
    probe_address: Option<iroh::EndpointAddr>,
    pub endpoint: Endpoint,
    pub join: JoinConfig,
    pub(crate) author: SigningKey,
    sequence: AtomicU64,
    account_chain: std::sync::RwLock<Vec<crate::portability::AccountRecord>>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    ReportDraft,
    SubmitReport {
        receipt: String,
        evidence: crate::report::Evidence,
    },
    ReportStatus {
        receipt: String,
    },
    Social {
        op: crate::messaging::SocialOp,
    },
    Direct {
        state: Option<serde_json::Value>,
        action: crate::dm_client::LocalAction,
    },
    PrivateGroup {
        state: Option<serde_json::Value>,
        action: crate::private_groups::LocalAction,
    },
    AccountNotify {
        chain: Vec<crate::portability::AccountRecord>,
        address: iroh::EndpointAddr,
    },
    Prepare {
        action: Action,
    },
    AccountLookup {
        account: String,
    },
    AccountRegister {
        wallet: crate::portability::AccountWallet,
    },
    AccountExport {
        wallet: crate::portability::AccountWallet,
    },
    AccountImport {
        wallet: crate::portability::AccountWallet,
        events: Vec<Envelope>,
    },
    Submit {
        event: SignedEvent,
    },
    Feed,
    FeedWithFailover {
        replica: iroh::EndpointAddr,
    },
    NearbyFeed {
        nearby: Nearby,
    },
    Groups,
    CreateGroup {
        details: crate::groups::NewGroup,
    },
    JoinGroup {
        group: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub event: SignedEvent,
    pub next_sequence: u64,
}
impl Client {
    pub async fn open(config: ClientConfig) -> Result<Self> {
        config.join.validate()?;
        if let Some(standby) = &config.full_home_standby {
            standby.validate(&config.join.server_addr.id.to_string())?;
        }
        ensure!(
            config.next_sequence > 0 && config.next_sequence < i64::MAX as u64,
            "invalid next sequence"
        );
        let author = signing_key(&config.author_secret)?;
        if !config.account_chain.is_empty() {
            let account = crate::portability::validate_chain(&config.account_chain)?;
            ensure!(
                account.payload.home == config.join.server_addr.id.to_string()
                    && account
                        .payload
                        .devices
                        .contains(&hex::encode(author.verifying_key().as_bytes())),
                "account/device/home mismatch"
            );
        }
        let endpoint = transport::bind(config.join.network_mode, None, false).await?;
        Ok(Self {
            endpoint,
            standby: config.full_home_standby,
            standby_until: AtomicU64::new(0),
            #[cfg(test)]
            probe_address: None,
            join: config.join,
            author,
            sequence: AtomicU64::new(config.next_sequence),
            account_chain: std::sync::RwLock::new(config.account_chain),
        })
    }
    pub(crate) fn account_chain(&self) -> Result<Vec<crate::portability::AccountRecord>> {
        self.account_chain
            .read()
            .map(|v| v.clone())
            .map_err(|_| anyhow::anyhow!("account state unavailable"))
    }
    fn refresh_account_chain(&self, known: &[crate::portability::AccountRecord]) -> Result<()> {
        let local = self.account_chain()?;
        if local.is_empty() || known.is_empty() {
            return Ok(());
        }
        let latest = crate::portability::validate_chain(known)?;
        ensure!(
            known.starts_with(&local) || local.starts_with(known),
            "conflicting account history from home"
        );
        if known.len() > local.len() {
            ensure!(
                latest.payload.home == self.join.server_addr.id.to_string()
                    && latest.payload.devices.contains(&self.author_id()),
                "account moved or device revoked"
            );
            *self
                .account_chain
                .write()
                .map_err(|_| anyhow::anyhow!("account state unavailable"))? = known.to_vec();
        }
        Ok(())
    }
    pub fn author_id(&self) -> String {
        hex::encode(self.author.verifying_key().as_bytes())
    }
    pub fn prepare(&self, action: Action) -> Result<Prepared> {
        let sequence = self
            .sequence
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                n.checked_add(1).filter(|n| *n < i64::MAX as u64)
            })
            .map_err(|_| anyhow::anyhow!("sequence exhausted"))?;
        let chain = self.account_chain()?;
        let event = if let Some(account) = chain.last() {
            SignedEvent::create_for_account(
                &self.author,
                self.join.server_addr.id.to_string(),
                self.join.community.clone(),
                sequence,
                action,
                account.clone(),
            )?
        } else {
            SignedEvent::create(
                &self.author,
                self.join.server_addr.id.to_string(),
                self.join.community.clone(),
                sequence,
                action,
            )?
        };
        Ok(Prepared {
            event,
            next_sequence: sequence + 1,
        })
    }
    pub async fn submit(&self, event: SignedEvent) -> Result<Response> {
        event.validate()?;
        ensure!(
            event.payload.author == self.author_id()
                && event.payload.home == self.join.server_addr.id.to_string()
                && event.payload.community == self.join.community,
            "outbox event belongs to another identity or instance"
        );
        let id = event.id.clone();
        let expected = event.clone();
        let response = self
            .home_rpc(&Request::Submit {
                token: self.join.access_token.clone(),
                event,
                account_chain: self.account_chain()?,
            })
            .await?;
        ensure!(
            matches!(&response, Response::Accepted { event_id, .. } if event_id == &id),
            "invalid acceptance receipt"
        );
        let Response::Accepted {
            envelope: Some(envelope),
            erased,
            ..
        } = &response
        else {
            anyhow::bail!("missing durable signed acceptance receipt")
        };
        envelope.validate()?;
        if let Some(target) = erased {
            target.validate()?;
            ensure!(
                target.deletion.is_some() && target.deleted_by(envelope),
                "invalid erased target receipt"
            );
        }
        ensure!(
            envelope.event.header()? == expected.header()?
                && envelope.event.signature == expected.signature,
            "acceptance receipt event mismatch"
        );
        Ok(response)
    }
    pub(crate) async fn home_rpc(&self, request: &Request) -> Result<Response> {
        #[cfg(test)]
        if let Some(physical) = &self.probe_address {
            return transport::rpc(&self.endpoint, physical.clone(), request).await;
        }
        let Some(grant) = &self.standby else {
            return transport::rpc(&self.endpoint, self.join.server_addr.clone(), request).await;
        };
        let operation = Request::HomeOperation {
            request_id: new_secret(),
            call: Box::new(request.clone()),
        };
        let now = now().max(0) as u64;
        // Probe again after recovery, without making each RPC in a mailbox poll wait for a dead primary.
        let prefer_standby = self.standby_until.load(Ordering::Relaxed) > now;
        let (first, second) = if prefer_standby {
            (&grant.standby, &self.join.server_addr)
        } else {
            (&self.join.server_addr, &grant.standby)
        };
        let first_try = n0_future::time::timeout(
            std::time::Duration::from_secs(5),
            transport::rpc(&self.endpoint, first.clone(), &operation),
        )
        .await;
        match first_try {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(error)) if !error.is::<transport::Unavailable>() => return Err(error),
            _ => {}
        }
        let response = transport::rpc(&self.endpoint, second.clone(), &operation).await?;
        self.standby_until
            .store(if prefer_standby { 0 } else { now + 20 }, Ordering::Relaxed);
        Ok(response)
    }
    pub async fn feed(&self) -> Result<Response> {
        self.feed_for(None).await
    }

    pub async fn feed_with_failover(&self, replica: iroh::EndpointAddr) -> Result<Response> {
        ensure!(
            self.join.community == format!("{}/public", self.join.server_addr.id)
                && replica.id != self.join.server_addr.id,
            "failover requires the enrolled public home and a distinct replica"
        );
        // Bound an outage probe separately from normal multi-RPC personalized reads.
        // The fallback carries public content only. Application refusals never select it.
        if self.standby.is_some() {
            // An anonymous public projection cannot know which accounts another device just
            // blocked. Keep the last authenticated view if both private endpoints are unreachable.
            return self.feed().await;
        }
        let probe = n0_future::time::timeout(std::time::Duration::from_secs(6), self.feed()).await;
        match probe {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(error)) if !error.is::<transport::Unavailable>() => return Err(error),
            _ => {}
        }
        let response = transport::rpc(
            &self.endpoint,
            replica,
            &Request::PublicReplicaFeed {
                community: self.join.community.clone(),
            },
        )
        .await?;
        ensure!(
            matches!(&response, Response::PublicReplicaFeed { .. }),
            "invalid replica feed"
        );
        Ok(response)
    }

    async fn feed_for(&self, nearby: Option<Nearby>) -> Result<Response> {
        let chain = self.account_chain()?;
        if let Some(record) = chain.last() {
            let response = self
                .home_rpc(&Request::AccountRegister {
                    token: self.join.access_token.clone(),
                    community: self.join.community.clone(),
                    chain: chain.clone(),
                    proof: crate::portability::AccountProof::signed(
                        record,
                        &self.author,
                        "register",
                        &self.join.server_addr.id.to_string(),
                        &crate::portability::digest(&chain)?,
                    )?,
                })
                .await?;
            let Response::AccountState {
                chain,
                social,
                privacy,
            } = response
            else {
                anyhow::bail!("invalid account registration response")
            };
            self.refresh_account_chain(&chain)?;
            if privacy {
                return self
                    .social(crate::messaging::SocialOp::PrivacyFeed { nearby })
                    .await;
            }
            if social {
                return self
                    .social(crate::messaging::SocialOp::Feed { nearby })
                    .await;
            }
        }
        let mut response = self
            .home_rpc(&Request::Feed {
                nearby,
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                author: self
                    .account_chain()?
                    .last()
                    .map_or_else(|| self.author_id(), |r| r.payload.account.clone()),
            })
            .await?;
        ensure!(
            matches!(&response, Response::Feed { .. }),
            "invalid feed response"
        );
        if let Response::Feed { account_chain, .. } = &mut response {
            self.refresh_account_chain(account_chain)?;
            *account_chain = self.account_chain()?;
        }
        Ok(response)
    }
    pub async fn groups(&self) -> Result<Response> {
        let response = self
            .home_rpc(&Request::Groups {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
            })
            .await?;
        ensure!(
            matches!(response, Response::Groups { .. }),
            "invalid groups response"
        );
        Ok(response)
    }
    pub async fn create_group(&self, details: crate::groups::NewGroup) -> Result<Response> {
        details.validate()?;
        let response = self
            .home_rpc(&Request::CreateGroup {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                details,
            })
            .await?;
        self.validate_group_join(&response)?;
        Ok(response)
    }
    pub async fn join_group(&self, group: String) -> Result<Response> {
        let response = self
            .home_rpc(&Request::JoinGroup {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                group: group.clone(),
            })
            .await?;
        self.validate_group_join(&response)?;
        ensure!(
            matches!(&response, Response::GroupJoined { group: summary, .. } if summary.id == group),
            "group join identity mismatch"
        );
        Ok(response)
    }
    fn validate_group_join(&self, response: &Response) -> Result<()> {
        let Response::GroupJoined { group, join } = response else {
            anyhow::bail!("invalid group join response");
        };
        join.validate()?;
        ensure!(
            join.server_addr.id == self.join.server_addr.id && join.community == group.id,
            "group join changed home or community"
        );
        Ok(())
    }
    pub async fn command(&self, command: Command) -> Result<serde_json::Value> {
        match command {
            Command::ReportDraft => {
                ensure!(
                    matches!(
                        transport::rpc(
                            &self.endpoint,
                            self.join.server_addr.clone(),
                            &Request::ReportAvailability
                        )
                        .await?,
                        Response::ReportAvailable
                    ),
                    "home report inbox unavailable"
                );
                let home = self.join.server_addr.id.to_string();
                Ok(serde_json::json!({"home":home,"receipt":crate::report::receipt(&home)}))
            }
            Command::SubmitReport { receipt, evidence } => {
                crate::report::issued_at(&receipt, &self.join.server_addr.id.to_string())?;
                evidence.validate()?;
                self.report_request(Request::SubmitReport {
                    token: self.join.access_token.clone(),
                    community: self.join.community.clone(),
                    receipt,
                    evidence,
                })
                .await
            }
            Command::ReportStatus { receipt } => {
                crate::report::issued_at(&receipt, &self.join.server_addr.id.to_string())?;
                self.report_request(Request::ReportStatus { receipt }).await
            }
            Command::Direct { state, action } => self.direct(state, action).await,
            Command::PrivateGroup { state, action } => self.private_group(state, action).await,
            Command::Social { op } => {
                use crate::messaging::SocialOp::*;
                ensure!(
                    matches!(
                        op,
                        Blocks
                            | Contact { .. }
                            | Block { .. }
                            | Unblock { .. }
                            | ClaimUsername { .. }
                            | SetProfile { .. }
                            | Profiles { .. }
                            | SetPush { .. } | OpenPush { .. } | NotificationThread { .. } | NotificationInbox | ReadNotification { .. } | MuteNotifications { .. } | DismissRequest { .. }
                            | PrivacyStatus
                            | SetPrivacy { .. }
                            | Approve { .. }
                    ),
                    "invalid social command"
                );
                if let Contact { event } = op {
                    let mut result = self
                        .social_value(Contact {
                            event: event.clone(),
                        })
                        .await?;
                    let signed: SignedEvent = serde_json::from_value(result["event"].take())?;
                    signed.validate()?;
                    ensure!(
                        signed.id == event && signed.payload.community == self.join.community,
                        "contact event mismatch"
                    );
                    let label = match &signed.payload.action {
                        Action::Post {
                            anonymous: false,
                            display_name,
                            ..
                        }
                        | Action::PostV2 {
                            anonymous: false,
                            display_name,
                            ..
                        } => display_name,
                        Action::Reply { display_name, .. }
                            if display_name != "Anonymous neighbor" =>
                        {
                            display_name
                        }
                        _ => anyhow::bail!("choose a named author"),
                    };
                    Ok(serde_json::json!({"peer":signed.owner_id(),"label":label}))
                } else {
                    self.social_value(op).await
                }
            }
            Command::AccountNotify { chain, address } => {
                self.notify_account(address, &chain).await?;
                Ok(serde_json::json!({"notified":true}))
            }
            Command::AccountLookup { account } => {
                Ok(serde_json::json!({"chain": self.lookup_account(&account).await?}))
            }
            Command::AccountRegister { wallet } => {
                self.register_account(&wallet).await?;
                Ok(serde_json::json!({"registered":true}))
            }
            Command::AccountExport { wallet } => {
                Ok(serde_json::json!({"events":self.export_owned(&wallet).await?}))
            }
            Command::AccountImport { wallet, events } => {
                self.import_owned(&wallet, &events).await?;
                Ok(
                    serde_json::json!({"imported":events.len(),"events":self.export_owned(&wallet).await?}),
                )
            }
            Command::Groups => Ok(serde_json::to_value(self.groups().await?)?),
            Command::CreateGroup { details } => {
                Ok(serde_json::to_value(self.create_group(details).await?)?)
            }
            Command::JoinGroup { group } => {
                Ok(serde_json::to_value(self.join_group(group).await?)?)
            }
            Command::Prepare { action } => Ok(serde_json::to_value(self.prepare(action)?)?),
            Command::Submit { event } => Ok(serde_json::to_value(self.submit(event).await?)?),
            Command::NearbyFeed { nearby } => {
                Ok(serde_json::to_value(self.feed_for(Some(nearby)).await?)?)
            }
            Command::FeedWithFailover { replica } => Ok(serde_json::to_value(
                self.feed_with_failover(replica).await?,
            )?),
            Command::Feed => Ok(serde_json::to_value(self.feed().await?)?),
        }
    }
    async fn report_request(&self, request: Request) -> Result<serde_json::Value> {
        match transport::rpc(&self.endpoint, self.join.server_addr.clone(), &request).await? {
            Response::ReportResult { result } => Ok(result),
            _ => anyhow::bail!("unexpected report response"),
        }
    }
    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

impl Client {
    pub async fn lookup_account(
        &self,
        account: &str,
    ) -> Result<Vec<crate::portability::AccountRecord>> {
        let response = self
            .home_rpc(&Request::AccountLookup {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                account: account.into(),
            })
            .await?;
        let Response::AccountState { chain, .. } = response else {
            anyhow::bail!("invalid account lookup response")
        };
        if !chain.is_empty() {
            ensure!(
                crate::portability::validate_chain(&chain)?.payload.account == account,
                "account lookup mismatch"
            );
        }
        Ok(chain)
    }
    pub async fn register_account(&self, wallet: &crate::portability::AccountWallet) -> Result<()> {
        let own = self.join.server_addr.id.to_string();
        let response = self
            .home_rpc(&Request::AccountRegister {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                chain: wallet.chain.clone(),
                proof: crate::portability::AccountProof::create(
                    wallet,
                    "register",
                    &own,
                    &crate::portability::digest(&wallet.chain)?,
                )?,
            })
            .await?;
        ensure!(
            matches!(response, Response::AccountState { chain, .. } if chain == wallet.chain),
            "invalid account registration response"
        );
        Ok(())
    }
    pub async fn export_owned(
        &self,
        wallet: &crate::portability::AccountWallet,
    ) -> Result<Vec<Envelope>> {
        let mut events = Vec::new();
        let mut after = None;
        loop {
            let digest = crate::portability::digest(&(&self.join.community, &after))?;
            let response = self
                .home_rpc(&Request::AccountExport {
                    token: self.join.access_token.clone(),
                    community: self.join.community.clone(),
                    chain: wallet.chain.clone(),
                    proof: crate::portability::AccountProof::create(
                        wallet,
                        "export",
                        &self.join.server_addr.id.to_string(),
                        &digest,
                    )?,
                    after: after.clone(),
                })
                .await?;
            let Response::OwnedEvents {
                events: page,
                next,
                more,
            } = response
            else {
                anyhow::bail!("invalid owned history response")
            };
            ensure!(
                page.len() <= 50 && (!more || !page.is_empty()),
                "invalid owned history page"
            );
            for event in &page {
                event.validate()?;
                ensure!(
                    event.event.owner_id() == wallet.validate()?.payload.account
                        && after.as_ref().is_none_or(|id| event.event.id > *id),
                    "invalid owned history event/cursor"
                );
            }
            ensure!(
                next == page.last().map(|e| e.event.id.clone()),
                "owned history cursor mismatch"
            );
            events.extend(page);
            ensure!(
                events.len() <= crate::portability::MAX_ARCHIVE_EVENTS,
                "owned history exceeds export capacity"
            );
            if !more {
                break;
            }
            after = next;
        }
        // A malicious home may omit data, but cannot make third-party or altered events ours.
        let _ = crate::portability::PersonalArchive::create(
            wallet,
            events.clone(),
            serde_json::Value::Null,
        )?;
        Ok(events)
    }
    pub async fn import_owned(
        &self,
        wallet: &crate::portability::AccountWallet,
        events: &[Envelope],
    ) -> Result<()> {
        // Validate every event locally before the first upload; the server validates each page again.
        let events = crate::portability::PersonalArchive::create(
            wallet,
            events.to_vec(),
            serde_json::Value::Null,
        )?
        .payload
        .events;
        self.register_account(wallet).await?;
        let mut cursor = 0;
        while cursor < events.len() {
            let mut end = (cursor + 50).min(events.len());
            while serde_json::to_vec(&events[cursor..end])?.len() > crate::MAX_FRAME / 2
                && end > cursor
            {
                end -= 1;
            }
            ensure!(end > cursor, "single import event exceeds frame");
            let chunk = events[cursor..end].to_vec();
            let response = self
                .home_rpc(&Request::AccountImport {
                    token: self.join.access_token.clone(),
                    community: self.join.community.clone(),
                    chain: wallet.chain.clone(),
                    proof: crate::portability::AccountProof::create(
                        wallet,
                        "import",
                        &self.join.server_addr.id.to_string(),
                        &crate::portability::digest(&chunk)?,
                    )?,
                    events: chunk,
                })
                .await?;
            ensure!(
                matches!(response, Response::PersonalImported { count } if count == end - cursor),
                "import receipt mismatch"
            );
            cursor = end;
        }
        Ok(())
    }
}

impl Client {
    /// Signed home changes can reach an old home after its membership credential changes.
    pub async fn notify_account(
        &self,
        address: iroh::EndpointAddr,
        chain: &[crate::portability::AccountRecord],
    ) -> Result<()> {
        let record = crate::portability::validate_chain(chain)?;
        ensure!(
            chain
                .iter()
                .any(|r| r.payload.home == address.id.to_string()),
            "notification target is not an account home"
        );
        let request = Request::AnnounceAccount {
            chain: chain.to_vec(),
        };
        let response = if address.id == self.join.server_addr.id {
            self.home_rpc(&request).await?
        } else {
            transport::rpc(&self.endpoint, address, &request).await?
        };
        let Response::AccountState { chain: known, .. } = response else {
            anyhow::bail!("invalid account notification response")
        };
        ensure!(
            crate::portability::validate_chain(&known)?.payload.account == record.payload.account
                && known.starts_with(chain),
            "account notification fork/rollback"
        );
        Ok(())
    }
}

#[cfg(all(test, feature = "server"))]
mod deployment_checks {
    use super::*;
    use crate::{portability::AccountWallet, takeover::FullHomeStandby};
    use serde_json::{Value, json};

    async fn direct(c: &Client, state: &Value, action: Value) -> Result<Value> {
        Ok(c.command(serde_json::from_value(
            json!({"type":"direct","state":state,"action":action}),
        )?)
        .await?["state"]
            .take())
    }

    /// Two disposable accounts, one encrypted packet in each direction, no public posts.
    /// Explicit physical routing prevents an apparently successful check against the other node.
    #[tokio::test]
    #[ignore = "writes two disposable accounts and encrypted test mail to YAP_SMOKE_ENROLLMENT"]
    async fn deployed_full_home_private_exchange() -> Result<()> {
        let manifest: Value =
            serde_json::from_reader(std::fs::File::open(std::env::var("YAP_SMOKE_ENROLLMENT")?)?)?;
        let join: JoinConfig = serde_json::from_value(manifest["join"].clone())?;
        let grant: FullHomeStandby = serde_json::from_value(manifest["fullHomeStandby"].clone())?;
        let home = join.server_addr.id.to_string();
        grant.validate(&home)?;
        ensure!(
            join.community == format!("{home}/public"),
            "unexpected logical home scope"
        );
        let open = async |wallet: &AccountWallet, standby: bool| {
            let mut client = Client::open(ClientConfig {
                join: join.clone(),
                full_home_standby: Some(grant.clone()),
                next_sequence: wallet.next_sequence,
                author_secret: wallet.device_secret.clone(),
                account_chain: wallet.chain.clone(),
            })
            .await?;
            client.probe_address = Some(if standby {
                grant.standby.clone()
            } else {
                join.server_addr.clone()
            });
            anyhow::Ok(client)
        };
        let alice = AccountWallet::create(new_secret(), home.clone())?;
        let bob = AccountWallet::create(new_secret(), home)?;
        let a = open(&alice, false).await?;
        let b = open(&bob, true).await?;
        let aid = &alice.validate()?.payload.account;
        let bid = &bob.validate()?.payload.account;
        ensure!(
            matches!(a.feed().await?, Response::Feed { privacy: true, .. })
                && matches!(b.feed().await?, Response::Feed { privacy: true, .. }),
            "expected private home feeds"
        );
        let init = json!({"kind":"initialize"});
        let sync = json!({"kind":"sync"});
        let mut sa = direct(&a, &Value::Null, init.clone()).await?;
        let mut sb = direct(&b, &Value::Null, init).await?;
        sa = direct(&a, &sa, sync.clone()).await?;
        sb = direct(&b, &sb, sync.clone()).await?;
        let send = |peer: &str, body: &str| json!({"kind":"prepare","peer":peer,"label":"HA verification","name":"HA verification","body":body});
        let from_primary = new_secret();
        sa = direct(&a, &sa, send(bid, &from_primary)).await?;
        sa = direct(&a, &sa, sync.clone()).await?;
        sb = direct(&b, &sb, sync.clone()).await?;
        ensure!(
            sb["messages"][0]["body"] == from_primary,
            "standby failed to serve primary ciphertext"
        );
        let from_standby = new_secret();
        sb = direct(&b, &sb, send(aid, &from_standby)).await?;
        direct(&b, &sb, sync.clone()).await?;
        sa = direct(&a, &sa, sync).await?;
        ensure!(
            sa["messages"][1]["body"] == from_standby,
            "primary failed to serve standby ciphertext"
        );
        b.command(serde_json::from_value(
            json!({"type":"social","op":{"kind":"block","target":{"account":aid}}}),
        )?)
        .await?;
        let other = open(&bob, false).await?;
        let status = other
            .command(serde_json::from_value(
                json!({"type":"social","op":{"kind":"privacyStatus"}}),
            )?)
            .await?;
        ensure!(
            status["hasBlocks"] == true,
            "standby block missing from primary"
        );
        a.close().await;
        b.close().await;
        other.close().await;
        println!(
            "Verified authenticated private feeds, bidirectional encrypted mail and cross-node private blocking"
        );
        Ok(())
    }
}
