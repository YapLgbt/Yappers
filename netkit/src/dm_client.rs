// SPDX-License-Identifier: MIT OR Apache-2.0
//! Device-only Olm state. Each returned state is committed by the platform before the next step.
//! In particular, prepare never submits a ciphertext and sync only acknowledges a persisted cursor.
use crate::{
    client::Client,
    messaging::*,
    model::*,
    portability::{AccountProof, digest},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use vodozemac::{
    Curve25519PublicKey,
    olm::{Account, AccountPickle, OlmMessage, Session, SessionConfig, SessionPickle},
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LocalAction {
    Initialize,
    Sync {
        #[serde(default)]
        blocked: Vec<String>,
    },
    Prepare {
        peer: String,
        label: String,
        name: String,
        body: String,
    },
}

#[derive(Serialize, Deserialize)]
struct Thread {
    identity: Curve25519PublicKey,
    active: String,
    sessions: BTreeMap<String, SessionPickle>,
}

#[derive(Serialize, Deserialize)]
struct Message {
    id: String,
    peer: String,
    label: String,
    body: String,
    outgoing: bool,
    #[serde(default)]
    age: u8,
    status: String,
}
#[derive(Serialize, Deserialize)]
struct PrivateState {
    version: u8,
    owner: String,
    device: String,
    home: String,
    account: AccountPickle,
    threads: BTreeMap<String, Thread>,
    messages: Vec<Message>,
    outbox: Vec<Signed<Letter>>,
    cursor: i64,
    unreadable: u32,
    #[serde(default)]
    delivery_paused: bool,
}
#[derive(Serialize, Deserialize)]
struct Content {
    name: String,
    body: String,
}
impl Content {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.encode_utf16().count() <= 30,
            "invalid sender name"
        );
        ensure!(
            !self.body.trim().is_empty() && self.body.encode_utf16().count() <= 1000,
            "write between 1 and 1000 characters"
        );
        Ok(())
    }
}
impl PrivateState {
    fn append(&mut self, line: Message) {
        // Retain a bounded local history; queued items always remain available for retry.
        if self.messages.len() >= 500
            && let Some(i) = self.messages.iter().position(|m| m.status != "queued")
        {
            self.messages.remove(i);
        }
        self.messages.push(line);
    }
    fn receive(&mut self, account: &mut Account, letter: &Signed<Letter>, age: u8) -> Result<()> {
        let peer = &letter.record.payload.account;
        let v = &letter.value;
        valid_peer(peer, &self.owner)?;
        letter.check(peer, &self.home)?;
        ensure!(
            v.to == self.owner && v.recipient_key == account.curve25519_key(),
            "wrong message recipient"
        );
        let id = digest(letter)?;
        if self.messages.iter().any(|m| m.id == id) {
            return Ok(());
        }
        let thread = self.threads.get(peer);
        if let Some(t) = &thread {
            ensure!(t.identity == v.identity, "contact messaging key changed");
        }
        let (session, plain) =
            if let Some(pickle) = thread.as_ref().and_then(|t| t.sessions.get(&v.session)) {
                // Stage this operation: an invalid packet must not discard the current session.
                let mut session =
                    Session::from_pickle(serde_json::from_value(serde_json::to_value(pickle)?)?);
                let plain = session.decrypt(&v.ciphertext)?;
                (session, plain)
            } else {
                ensure!(
                    self.threads.len() < 256 || thread.is_some(),
                    "conversation capacity reached"
                );
                if let Some(t) = &thread {
                    ensure!(t.sessions.len() < 8, "conversation session limit reached");
                }
                let OlmMessage::PreKey(pre) = &v.ciphertext else {
                    bail!("unknown session")
                };
                ensure!(pre.session_id() == v.session, "session mismatch");
                let inbound =
                    account.create_inbound_session(SessionConfig::version_1(), v.identity, pre)?;
                (inbound.session, inbound.plaintext)
            };
        ensure!(session.session_id() == v.session, "session mismatch");
        let content: Content = serde_json::from_slice(&plain)?;
        content.validate()?;
        let t = self.threads.entry(peer.clone()).or_insert_with(|| Thread {
            identity: v.identity,
            active: v.session.clone(),
            sessions: BTreeMap::new(),
        });
        t.active = v.session.clone();
        t.sessions.insert(v.session.clone(), session.pickle());
        self.append(Message {
            id,
            peer: peer.clone(),
            label: content.name,
            body: content.body,
            outgoing: false,
            age,
            status: "received".into(),
        });
        Ok(())
    }
}
impl Client {
    pub(crate) async fn social(&self, op: SocialOp) -> Result<Response> {
        let chain = self.account_chain()?;
        let record = chain.last().context("a portable account is required")?;
        let home = self.join.server_addr.id.to_string();
        let hash = digest(&(self.endpoint.id().to_string(), &self.join.community, &op))?;
        let proof = AccountProof::signed(record, &self.author, "social-v1", &home, &hash)?;
        self.home_rpc(&Request::Social {
            token: self.join.access_token.clone(),
            community: self.join.community.clone(),
            account: record.payload.account.clone(),
            proof,
            op: Box::new(op),
        })
        .await
    }
    pub(crate) async fn social_value(&self, op: SocialOp) -> Result<Value> {
        let Response::Social { result } = self.social(op).await? else {
            bail!("unexpected messaging response")
        };
        Ok(result)
    }
    pub(crate) async fn direct(&self, saved: Option<Value>, action: LocalAction) -> Result<Value> {
        let chain = self.account_chain()?;
        let record = chain
            .last()
            .context("connect a portable account before using messages")?;
        let owner = &record.payload.account;
        let device = self.author_id();
        let home = self.join.server_addr.id.to_string();
        let mut state: PrivateState = if let Some(value) = saved {
            serde_json::from_value(value)?
        } else {
            ensure!(
                matches!(action, LocalAction::Initialize),
                "messaging keys unavailable"
            );
            let mut account = Account::new();
            account.generate_one_time_keys(PREKEY_BATCH);
            PrivateState {
                version: 1,
                owner: owner.clone(),
                device: device.clone(),
                home: home.clone(),
                account: account.pickle(),
                threads: BTreeMap::new(),
                messages: vec![],
                outbox: vec![],
                cursor: 0,
                unreadable: 0,
                delivery_paused: false,
            }
        };
        ensure!(
            state.version == 1
                && state.owner == *owner
                && state.device == device
                && state.home == home,
            "wrong messaging identity"
        );
        let mut account = Account::from_pickle(serde_json::from_value(serde_json::to_value(
            &state.account,
        )?)?);
        match action {
            LocalAction::Initialize => {}
            LocalAction::Sync { blocked } => {
                let keys = account
                    .one_time_keys()
                    .into_values()
                    .map(|once| {
                        Signed::sign(
                            record,
                            &self.author,
                            Prekey {
                                identity: account.curve25519_key(),
                                once,
                            },
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                let response = self.social_value(SocialOp::Publish { keys }).await?;
                let remaining = response["remaining"]
                    .as_u64()
                    .context("invalid key count")?;
                account.mark_keys_as_published();
                let mut inbox = self
                    .social_value(SocialOp::Inbox {
                        after: state.cursor,
                    })
                    .await?;
                let items: Vec<Incoming> = serde_json::from_value(inbox["incoming"].take())?;
                ensure!(items.len() <= MAIL_PAGE, "invalid mailbox page");
                for item in items {
                    ensure!(item.cursor > state.cursor, "invalid mailbox cursor");
                    if !blocked.contains(&item.letter.record.payload.account)
                        && state.receive(&mut account, &item.letter, item.age).is_err()
                    {
                        state.unreadable = state.unreadable.saturating_add(1);
                    }
                    state.cursor = item.cursor;
                }
                if remaining < 4 {
                    account.generate_one_time_keys(PREKEY_BATCH);
                }
                for letter in state
                    .outbox
                    .iter()
                    .filter(|l| blocked.contains(&l.value.to))
                {
                    let id = digest(letter)?;
                    if let Some(m) = state.messages.iter_mut().find(|m| m.id == id) {
                        m.status = "unavailable".into();
                    }
                }
                state.outbox.retain(|l| !blocked.contains(&l.value.to));
                if let Some(letter) = state.outbox.first() {
                    let id = digest(letter)?;
                    // Delivery can fail independently of a successful receive. Commit the new
                    // inbound ratchet and leave the original packet queued for an exact retry.
                    let delivery = self
                        .social_value(SocialOp::Deliver {
                            letter: Box::new(letter.clone()),
                        })
                        .await;
                    if let Some(accepted) = delivery.ok().and_then(|r| r["accepted"].as_bool()) {
                        if let Some(m) = state.messages.iter_mut().find(|m| m.id == id) {
                            m.status = if accepted { "sent" } else { "unavailable" }.into();
                        }
                        state.outbox.remove(0);
                        state.delivery_paused = false;
                    } else {
                        state.delivery_paused = true;
                    }
                }
            }
            LocalAction::Prepare {
                peer,
                label,
                name,
                body,
            } => {
                valid_peer(&peer, owner)?;
                let content = Content { name, body };
                content.validate()?;
                ensure!(
                    label.encode_utf16().count() <= 80 && state.outbox.len() < 16,
                    "message queue or label too long"
                );
                if !state.threads.contains_key(&peer) {
                    ensure!(state.threads.len() < 256, "conversation capacity reached");
                    let mut response = self
                        .social_value(SocialOp::Claim { peer: peer.clone() })
                        .await?;
                    let bundle: Signed<Prekey> = serde_json::from_value(response["bundle"].take())?;
                    bundle.check(&peer, &home)?;
                    let session = account.create_outbound_session(
                        SessionConfig::version_1(),
                        bundle.value.identity,
                        bundle.value.once,
                    )?;
                    let sid = session.session_id();
                    state.threads.insert(
                        peer.clone(),
                        Thread {
                            identity: bundle.value.identity,
                            active: sid.clone(),
                            sessions: BTreeMap::from([(sid, session.pickle())]),
                        },
                    );
                }
                let thread = state
                    .threads
                    .get_mut(&peer)
                    .context("missing conversation")?;
                let mut session = Session::from_pickle(
                    thread
                        .sessions
                        .remove(&thread.active)
                        .context("missing session")?,
                );
                let packet = Signed::sign(
                    record,
                    &self.author,
                    Letter {
                        to: peer.clone(),
                        identity: account.curve25519_key(),
                        recipient_key: thread.identity,
                        session: session.session_id(),
                        ciphertext: session.encrypt(serde_json::to_vec(&content)?)?,
                    },
                )?;
                thread
                    .sessions
                    .insert(thread.active.clone(), session.pickle());
                state.append(Message {
                    id: digest(&packet)?,
                    peer,
                    label,
                    body: content.body,
                    outgoing: true,
                    age: 0,
                    status: "queued".into(),
                });
                state.outbox.push(packet);
            }
        }
        state.account = account.pickle();
        Ok(json!({"state":state}))
    }
}
