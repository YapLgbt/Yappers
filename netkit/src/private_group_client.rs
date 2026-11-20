// SPDX-License-Identifier: MIT OR Apache-2.0
//! Device-only group key state. Persist Prepare's returned state before calling Publish.
use crate::{
    client::Client,
    model::{Request, Response, new_secret, now},
    portability::{AccountProof, digest},
    private_groups::*,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use vodozemac::{
    megolm::{self, GroupSession, GroupSessionPickle, InboundGroupSession},
    olm::{self, Account, AccountPickle, OlmMessage},
};

const GROUP_CONFIG: megolm::SessionConfig = megolm::SessionConfig::version_1();
const OLM_CONFIG: olm::SessionConfig = olm::SessionConfig::version_1();

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vault {
    home: String,
    device: String,
    groups: BTreeMap<String, SavedGroup>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedGroup {
    room: Signed<Room>,
    device: Device,
    olm: AccountPickle,
    initial: Option<Signed<Roster>>,
    tip: Option<(u32, String)>,
    outbound: Option<Outbound>,
    pending: Option<Pending>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outbound {
    roster: String,
    created: i64,
    pickle: GroupSessionPickle,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    draft: Draft,
    event: Signed<Event>,
    session: Option<Signed<Session>>,
}

impl SavedGroup {
    fn new(room: Signed<Room>, account: String, signing: String) -> Result<Self> {
        let mut olm = Account::new();
        olm.generate_fallback_key();
        let fallback = olm
            .fallback_key()
            .into_values()
            .next()
            .context("group prekey unavailable")?;
        olm.mark_keys_as_published();
        Ok(Self {
            room,
            device: Device {
                account,
                signing,
                identity: olm.curve25519_key(),
                fallback,
            },
            olm: olm.pickle(),
            initial: None,
            tip: None,
            outbound: None,
            pending: None,
        })
    }

    fn account(&self) -> Result<Account> {
        Ok(Account::from_pickle(serde_json::from_value(
            serde_json::to_value(&self.olm)?,
        )?))
    }

    fn roster_check(&mut self, group: &str, history: &[Signed<Roster>]) -> Result<()> {
        let mut prior = None;
        for roster in history {
            roster.check(group, &self.room.value.owner, prior)?;
            prior = Some(roster);
        }
        let latest = history
            .last()
            .context("this device is not an active private group member")?;
        if let Some((revision, known)) = &self.tip {
            ensure!(
                *revision > 0
                    && history
                        .get((*revision - 1) as usize)
                        .map(digest)
                        .transpose()?
                        .as_ref()
                        == Some(known),
                "private group membership history was rolled back or forked"
            );
        }
        ensure!(
            latest
                .value
                .members
                .get(&self.device.signing)
                .is_some_and(|m| m.device == self.device),
            "private group keys do not match this device"
        );
        self.tip = Some((latest.value.revision, digest(latest)?));
        Ok(())
    }

    fn make_session(
        &self,
        sender: &Client,
        roster: &Signed<Roster>,
        session: &GroupSession,
    ) -> Result<Signed<Session>> {
        let account = self.account()?;
        let roster_id = digest(roster)?;
        let mut keys = BTreeMap::new();
        for (id, member) in &roster.value.members {
            let mut olm = account.create_outbound_session(
                OLM_CONFIG,
                member.device.identity,
                member.device.fallback,
            )?;
            let key = SessionKeyPacket {
                group: roster.value.group.clone(),
                roster: roster_id.clone(),
                sender: self.device.signing.clone(),
                recipient: id.clone(),
                session: session.session_id(),
                key: session.session_key().to_base64(),
            };
            keys.insert(id.clone(), olm.encrypt(serde_json::to_vec(&key)?)?);
        }
        let packet = Signed::new(
            Session {
                group: roster.value.group.clone(),
                roster: roster_id,
                sender: self.device.signing.clone(),
                id: session.session_id(),
                keys,
            },
            &sender.author,
        )?;
        packet.check(roster)?;
        Ok(packet)
    }

    fn prepare(
        &mut self,
        client: &Client,
        roster: &Signed<Roster>,
        draft: Option<Draft>,
    ) -> Result<()> {
        let roster_id = digest(roster)?;
        let (draft, nonce) = match (self.pending.take(), draft) {
            (Some(pending), None) if pending.event.value.header.roster == roster_id => {
                self.pending = Some(pending);
                return Ok(());
            }
            (Some(pending), None) => (pending.draft, pending.event.value.header.nonce),
            (None, Some(draft)) => (draft, new_secret()),
            (None, None) => return Ok(()),
            (Some(_), Some(_)) => bail!("send or discard the pending private post first"),
        };
        draft.check()?;
        let time = now();
        let retained = self.outbound.take().filter(|outbound| {
            outbound.roster == roster_id && time.saturating_sub(outbound.created) < SESSION_SECONDS
        });
        let (session, created) = match retained {
            Some(outbound) => (
                Some(GroupSession::from_pickle(outbound.pickle)),
                outbound.created,
            ),
            None => (None, time),
        };
        let (mut ratchet, packet, created) = match session {
            Some(session) if session.message_index() < SESSION_MESSAGES => (session, None, created),
            _ => {
                let session = GroupSession::new(GROUP_CONFIG);
                let packet = self.make_session(client, roster, &session)?;
                (session, Some(packet), time)
            }
        };
        let header = EventHeader {
            group: roster.value.group.clone(),
            roster: roster_id.clone(),
            revision: roster.value.revision,
            sender: self.device.signing.clone(),
            session: ratchet.session_id(),
            nonce,
            parent: draft.parent.clone(),
        };
        let ciphertext = ratchet
            .encrypt(serde_json::to_vec(&(&header, &draft))?)
            .to_base64();
        let event = Signed::new(Event { header, ciphertext }, &client.author)?;
        event.check(roster)?;
        self.outbound = Some(Outbound {
            roster: roster_id,
            created,
            pickle: ratchet.pickle(),
        });
        self.pending = Some(Pending {
            draft,
            event,
            session: packet,
        });
        Ok(())
    }

    fn decrypt_page(
        &self,
        roster_history: &[Signed<Roster>],
        entries: Vec<Delivery>,
        before: Option<u64>,
    ) -> Result<Vec<Value>> {
        let current = roster_history.last().context("missing private roster")?;
        let since = current.value.members[&self.device.signing].since;
        let mut account = self.account()?;
        let mut sessions = BTreeMap::<(String, String), InboundGroupSession>::new();
        let mut seen = BTreeSet::new();
        let mut last = before.unwrap_or(u64::MAX);
        let mut posts = Vec::new();
        for delivery in entries {
            ensure!(
                delivery.cursor < last && delivery.cursor > 0,
                "invalid private group order"
            );
            last = delivery.cursor;
            let header = &delivery.event.value.header;
            ensure!(
                header.revision >= since,
                "post predates your group membership"
            );
            let roster = roster_history
                .get((header.revision - 1) as usize)
                .context("missing event roster")?;
            let message = delivery.event.check(roster)?;
            let member = roster
                .value
                .members
                .get(&self.device.signing)
                .context("post is outside your history")?;
            ensure!(
                member.device == self.device,
                "group post targets an earlier device"
            );
            let id = header.id()?;
            ensure!(
                seen.insert((header.session.clone(), message.message_index())),
                "repeated private group message"
            );
            let key = (header.sender.clone(), header.session.clone());
            let session = match sessions.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let OlmMessage::PreKey(packet) = delivery.key else {
                        bail!("invalid private group key packet");
                    };
                    let sender = &roster.value.members[&header.sender].device;
                    ensure!(
                        packet.one_time_key() == self.device.fallback,
                        "private group recipient mismatch"
                    );
                    let inbound =
                        account.create_inbound_session(OLM_CONFIG, sender.identity, &packet)?;
                    let share: SessionKeyPacket = serde_json::from_slice(&inbound.plaintext)?;
                    ensure!(
                        share.group == header.group
                            && share.roster == header.roster
                            && share.sender == header.sender
                            && share.recipient == self.device.signing
                            && share.session == header.session,
                        "private group session context mismatch"
                    );
                    let key = megolm::SessionKey::from_base64(&share.key)?;
                    let session = InboundGroupSession::new(&key, GROUP_CONFIG);
                    ensure!(
                        session.session_id() == header.session,
                        "private group session ID mismatch"
                    );
                    entry.insert(session)
                }
            };
            let clear = session.decrypt(&message)?;
            let (context, draft): (EventHeader, Draft) = serde_json::from_slice(&clear.plaintext)?;
            draft.check()?;
            ensure!(
                context == *header && draft.parent == header.parent,
                "private group event context mismatch"
            );
            posts.push(json!({
                "id": id, "author": draft.author, "headline": draft.headline, "body": draft.body,
                "attachment": draft.attachment,
                "parent": draft.parent, "mine": header.sender == self.device.signing,
            }));
        }
        Ok(posts)
    }
}

impl Client {
    async fn private_rpc(&self, op: Operation) -> Result<Outcome> {
        let chain = self.account_chain()?;
        let record = chain.last().context("sign in to use private groups")?;
        let home = self.join.server_addr.id.to_string();
        let digest = digest(&(self.endpoint.id().to_string(), &self.join.community, &op))?;
        let proof = AccountProof::signed(record, &self.author, "private-group-v1", &home, &digest)?;
        let response = self
            .home_rpc(&Request::PrivateGroup {
                token: self.join.access_token.clone(),
                community: self.join.community.clone(),
                account: record.payload.account.clone(),
                proof,
                op: Box::new(op),
            })
            .await?;
        let Response::PrivateGroup { result } = response else {
            bail!("unexpected private group response");
        };
        Ok(*result)
    }

    async fn private_history(
        &self,
        id: &str,
        group: &mut SavedGroup,
    ) -> Result<Vec<Signed<Roster>>> {
        if let Some(roster) = &group.initial {
            let result = self
                .private_rpc(Operation::Create {
                    room: group.room.clone(),
                    roster: roster.clone(),
                })
                .await?;
            ensure!(
                matches!(result, Outcome::Saved),
                "private group was not saved"
            );
            group.initial = None;
        }
        let mut history = Vec::new();
        let mut operation = Operation::Roster {
            group: id.to_owned(),
        };
        loop {
            match self.private_rpc(operation).await? {
                Outcome::Roster { history: last } => {
                    if last.is_empty() {
                        return Ok(Vec::new());
                    }
                    history.extend(last);
                    break;
                }
                Outcome::RosterPage { history: page } => {
                    let next = page.last().context("empty roster page")?.value.revision;
                    ensure!(
                        next as usize == history.len() + page.len(),
                        "invalid roster cursor"
                    );
                    history.extend(page);
                    operation = Operation::RosterAfter {
                        group: id.to_owned(),
                        after: next,
                    };
                }
                _ => bail!("expected group membership history"),
            }
        }
        if !history.is_empty() {
            group.roster_check(id, &history)?;
        }
        Ok(history)
    }

    pub(crate) async fn private_group(
        &self,
        state: Option<Value>,
        action: LocalAction,
    ) -> Result<Value> {
        let home = self.join.server_addr.id.to_string();
        let device = self.author_id();
        let chain = self.account_chain()?;
        let owner = &chain
            .last()
            .context("sign in to use private groups")?
            .payload
            .account;
        let mut vault: Vault = match state {
            Some(state) => serde_json::from_value(state)?,
            None => Vault {
                home: home.clone(),
                device: device.clone(),
                groups: BTreeMap::new(),
            },
        };
        ensure!(
            vault.home == home && vault.device == device,
            "wrong private group device"
        );
        let mut result = Value::Null;
        match action {
            LocalAction::List => {
                result = json!(
                    vault
                        .groups
                        .iter()
                        .map(|(id, group)| { json!({"id": id, "name": group.room.value.name}) })
                        .collect::<Vec<_>>()
                );
            }
            LocalAction::Create { name } => {
                let room = Signed::new(
                    Room {
                        home: home.clone(),
                        owner: device.clone(),
                        nonce: new_secret(),
                        name: name.trim().to_owned(),
                    },
                    &self.author,
                )?;
                let id = room.check(&home)?;
                let mut group = SavedGroup::new(room, owner.clone(), device.clone())?;
                let roster = Signed::new(
                    Roster {
                        group: id.clone(),
                        revision: 1,
                        previous: None,
                        members: BTreeMap::from([(
                            device,
                            Member {
                                device: group.device.clone(),
                                since: 1,
                            },
                        )]),
                    },
                    &self.author,
                )?;
                group.tip = Some((1, digest(&roster)?));
                group.initial = Some(roster);
                vault.groups.insert(id.clone(), group);
                result = json!(id);
            }
            LocalAction::Import { invitation } => {
                ensure!(
                    invitation.len() <= 4096,
                    "private group invitation is too large"
                );
                let room: Signed<Room> = serde_json::from_str(invitation.trim())
                    .context("invalid private group invitation")?;
                let id = room.check(&home)?;
                if !vault.groups.contains_key(&id) {
                    vault.groups.insert(
                        id.clone(),
                        SavedGroup::new(room, owner.clone(), device.clone())?,
                    );
                }
                result = json!(id);
            }
            LocalAction::Open { group, before } => {
                let saved = vault
                    .groups
                    .get_mut(&group)
                    .context("open a saved private group")?;
                let history = self.private_history(&group, saved).await?;
                let (posts, next) = match history.last() {
                    Some(roster) => {
                        let Outcome::Page {
                            entries,
                            before: next,
                        } = self
                            .private_rpc(Operation::Read {
                                group,
                                roster: digest(roster)?,
                                before,
                            })
                            .await?
                        else {
                            bail!("expected private group posts");
                        };
                        ensure!(
                            next.is_none() || next == entries.last().map(|entry| entry.cursor),
                            "invalid private group cursor"
                        );
                        (saved.decrypt_page(&history, entries, before)?, next)
                    }
                    None => (Vec::new(), None),
                };
                let owner = saved.room.value.owner == device;
                let phase = if !history.is_empty() {
                    "joined"
                } else if saved.tip.is_some() {
                    "removed"
                } else {
                    "pending"
                };
                let request = if history.is_empty() && !owner {
                    let request = JoinRequest {
                        group: digest(&saved.room)?,
                        device: saved.device.clone(),
                    };
                    Some(serde_json::to_string(&Signed::new(request, &self.author)?)?)
                } else {
                    None
                };
                result = json!({
                    "name": saved.room.value.name,
                    "owner": owner,
                    "phase": phase,
                    "members": history.last().map(|entry| &entry.value.members),
                    "posts": posts,
                    "before": next,
                    "pending": saved.pending.is_some(),
                    "joinRequest": request,
                    "invitation": if owner { Some(serde_json::to_string(&saved.room)?) } else { None },
                });
            }
            LocalAction::Approve { group, request } => {
                ensure!(
                    request.len() <= 4096,
                    "private group join request is too large"
                );
                let request: Signed<JoinRequest> = serde_json::from_str(request.trim())
                    .context("invalid private group join request")?;
                request.check(&group)?;
                self.change_private_group(&mut vault, &group, Some(request), None)
                    .await?;
            }
            LocalAction::Remove { group, device } => {
                hash_id(&device)?;
                self.change_private_group(&mut vault, &group, None, Some(device))
                    .await?;
            }
            LocalAction::Prepare { group, draft } => {
                let saved = vault
                    .groups
                    .get_mut(&group)
                    .context("open a saved private group")?;
                if draft.is_some() || saved.pending.is_some() {
                    let history = self.private_history(&group, saved).await?;
                    let roster = history
                        .last()
                        .context("this device is not an active private group member")?;
                    saved.prepare(self, roster, draft)?;
                }
                result = json!(saved.pending.is_some());
            }
            LocalAction::Publish { group } => {
                let saved = vault
                    .groups
                    .get_mut(&group)
                    .context("open a saved private group")?;
                if saved.pending.is_some() {
                    let history = self.private_history(&group, saved).await?;
                    let latest = history
                        .last()
                        .context("this device is not an active private group member")?;
                    let pending = saved
                        .pending
                        .take()
                        .context("pending private post unavailable")?;
                    ensure!(
                        pending.event.value.header.roster == digest(latest)?,
                        "private group membership changed; retry the pending post"
                    );
                    let expected = pending.event.value.header.id()?;
                    let outcome = self
                        .private_rpc(Operation::Publish {
                            event: pending.event,
                            session: pending.session,
                        })
                        .await?;
                    let Outcome::Accepted { id, roster } = outcome else {
                        bail!("invalid private group post receipt");
                    };
                    ensure!(id == expected, "invalid private group post receipt");
                    if roster != digest(latest)? {
                        // The old epoch already accepted the draft. The replacement
                        // session was never registered, so its ratchet cannot be reused.
                        saved.outbound = None;
                    }
                    result = json!(expected);
                }
            }
            LocalAction::Discard { group } => {
                let saved = vault
                    .groups
                    .get_mut(&group)
                    .context("open a saved private group")?;
                saved.pending = None;
                // The first packet of this ratchet may never have reached the home.
                saved.outbound = None;
            }
        }
        Ok(json!({"state": vault, "result": result}))
    }

    async fn change_private_group(
        &self,
        vault: &mut Vault,
        group: &str,
        request: Option<Signed<JoinRequest>>,
        remove: Option<String>,
    ) -> Result<()> {
        let saved = vault
            .groups
            .get_mut(group)
            .context("open a saved private group")?;
        let owner = &saved.room.value.owner;
        ensure!(
            owner == &vault.device,
            "only the group owner can change membership"
        );
        let owner = owner.clone();
        let history = self.private_history(group, saved).await?;
        let previous = history.last().context("private group unavailable")?;
        let mut members = previous.value.members.clone();
        let revision = previous
            .value
            .revision
            .checked_add(1)
            .context("roster revision overflow")?;
        if let Some(request) = &request {
            let device = &request.value.device;
            if let Some(already) = members.get(&device.signing) {
                ensure!(
                    already.device == *device,
                    "existing member device keys changed"
                );
                return Ok(());
            }
            members.insert(
                device.signing.clone(),
                Member {
                    device: device.clone(),
                    since: revision,
                },
            );
        } else if let Some(remove) = remove {
            ensure!(remove != owner, "the owner must remain in the group");
            if members.remove(&remove).is_none() {
                return Ok(());
            }
        }
        let roster = Signed::new(
            Roster {
                group: group.to_owned(),
                revision,
                previous: Some(digest(previous)?),
                members,
            },
            &self.author,
        )?;
        roster.check(group, &owner, Some(previous))?;
        let hash = digest(&roster)?;
        let outcome = self
            .private_rpc(Operation::Change { roster, request })
            .await?;
        ensure!(
            matches!(outcome, Outcome::Saved),
            "private group membership was not saved"
        );
        saved.tip = Some((revision, hash));
        Ok(())
    }
}
