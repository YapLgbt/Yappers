// SPDX-License-Identifier: AGPL-3.0-only
//! A home owns one native connection per worker, or a single HA staging connection.
use super::{PeerConfig, State, Store};
use anyhow::{Context, Result};
use std::{
    ops::Deref,
    sync::{Arc, Condvar, Mutex, RwLock},
};
use tokio::sync::Semaphore;

const NATIVE_WORKERS: usize = 4;

pub(super) struct CatalogOp<'a> {
    pub(super) state: &'a State,
    pub(super) store: &'a mut Store,
    pub(super) peers: &'a [PeerConfig],
    pub(super) disclosure: Disclosure,
}

#[derive(PartialEq)]
pub(super) enum Disclosure {
    Shared,
    Exclusive,
}

#[derive(Debug)]
pub(super) struct NeedsExclusive;
impl std::fmt::Display for NeedsExclusive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation changes disclosure policy")
    }
}
impl std::error::Error for NeedsExclusive {}

impl Disclosure {
    pub(super) fn restrict_if(&self, changed: bool) -> Result<()> {
        if changed && *self == Self::Shared {
            return Err(NeedsExclusive.into());
        }
        Ok(())
    }
}

// Transactions hold an operation lease through commit. Transport holds only
// the epoch's read guard for each synchronous poll, never across network waits.
#[derive(Default)]
pub(crate) struct DisclosureFence {
    operations: RwLock<()>,
    pub(super) epoch: RwLock<u64>,
}

impl DisclosureFence {
    pub(super) fn shared<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<(T, u64)> {
        let _lease = self.operations.read().unwrap();
        let value = work()?;
        Ok((value, *self.epoch.read().unwrap()))
    }

    pub(super) fn exclusive<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<(T, u64)> {
        let _lease = self.operations.write().unwrap();
        let result = work();
        // Finalization may fail after a durable commit. Invalidate old replies
        // before acknowledging the change, including in that case.
        let mut epoch = self.epoch.write().unwrap();
        *epoch += 1;
        result.map(|value| (value, *epoch))
    }

    pub(crate) fn restrict<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<T> {
        self.exclusive(work).map(|(value, _)| value)
    }
}

impl Deref for CatalogOp<'_> {
    type Target = State;

    fn deref(&self) -> &State {
        self.state
    }
}

pub(super) struct StoreWorkers {
    stores: Vec<Mutex<Store>>,
    idle: Mutex<Vec<usize>>,
    ready: Condvar,
    pub(super) slots: Arc<Semaphore>,
}

impl StoreWorkers {
    pub(super) async fn drain(&self) {
        let _all = self.slots.acquire_many(self.stores.len() as u32).await;
    }

    pub(super) fn new(store: Store, standalone: bool) -> Result<Self> {
        let count = if standalone { NATIVE_WORKERS } else { 1 };
        let mut stores = Vec::with_capacity(count);
        for _ in 1..count {
            stores.push(Mutex::new(store.fork()?));
        }
        stores.push(Mutex::new(store));
        Ok(Self {
            stores,
            idle: Mutex::new((0..count).collect()),
            ready: Condvar::new(),
            slots: Arc::new(Semaphore::new(count)),
        })
    }

    // Both the synchronous operator API and admitted Tokio blocking jobs check out
    // from the same pool. Returning an index never needs an async task to be polled.
    pub(super) fn checkout<T>(&self, work: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        let mut idle = self
            .ready
            .wait_while(self.idle.lock().unwrap(), |idle| idle.is_empty())
            .unwrap();
        let lease = Lease {
            workers: self,
            index: idle.pop().unwrap(),
        };
        drop(idle);
        let mut store = self.stores[lease.index]
            .lock()
            .ok()
            .context("store worker panicked")?;
        work(&mut store)
    }
}

struct Lease<'a> {
    workers: &'a StoreWorkers,
    index: usize,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.workers.idle.lock().unwrap().push(self.index);
        self.workers.ready.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        messaging::{PrivacySettings, SocialOp},
        model::*,
        portability::{AccountProof, AccountWallet, digest},
        server::{Server, federation_tests::config},
        transport,
    };
    use std::time::Duration;
    use tokio::{sync::oneshot, time::timeout};

    #[tokio::test]
    async fn native_checkouts_overlap_and_cancelled_jobs_keep_their_slot() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cfg = config(dir.path().join("catalog"));
        let server = Server::start(cfg.clone()).await?;
        let mut held = Vec::new();
        for _ in 0..NATIVE_WORKERS {
            let state = server.state.clone();
            let (entered, running) = oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let job = tokio::spawn(async move {
                let mut entered = Some(entered);
                state
                    .blocking(move |op| {
                        assert!(op.store.is_native());
                        op.store.hidden()?;
                        if let Some(entered) = entered.take() {
                            entered.send(()).unwrap();
                            wait.recv_timeout(Duration::from_secs(10))?;
                        }
                        Ok(())
                    })
                    .await
            });
            timeout(Duration::from_secs(5), running).await??;
            held.push((job, release));
        }
        let (cancelled, release) = held.pop().unwrap();
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        let mut next = Box::pin(server.state.blocking(|op| {
            op.store.db.execute(
                "INSERT INTO public_projections VALUES('next','','[]',0,0)",
                [],
            )?;
            Ok(())
        }));
        assert!(timeout(Duration::from_millis(50), &mut next).await.is_err());
        release.send(())?;
        timeout(Duration::from_secs(5), next).await??;
        let (last, release_last) = held.pop().unwrap();
        for (job, release) in held {
            release.send(())?;
            job.await??;
        }
        // The write completed while the other snapshots were still checked out.
        assert!(server.state.operation(|op| Ok(op.store.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM public_projections WHERE community='next')",
            [],
            |r| r.get::<_, bool>(0),
        )?))?);
        last.abort();
        assert!(last.await.unwrap_err().is_cancelled());
        let mut shutdown = Box::pin(server.close());
        assert!(
            timeout(Duration::from_millis(100), &mut shutdown)
                .await
                .is_err()
        );
        release_last.send(())?;
        timeout(Duration::from_secs(5), shutdown).await?;
        Server::start(cfg).await?.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn an_occ_conflict_replays_the_whole_catalog_operation() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let server = Server::start(config(dir.path().join("home"))).await?;
        let state = &server.state;
        state.operation(|op| {
            op.store.db.execute(
                "INSERT INTO public_projections VALUES('test','','[]',0,0)",
                [],
            )?;
            Ok(())
        })?;
        let (entered, snapshot) = oneshot::channel();
        let (resume, changed) = std::sync::mpsc::channel();
        let mut entered = Some(entered);
        let mut attempts = 0;
        let first = state.blocking(move |op| {
            attempts += 1;
            let n: i64 = op.store.db.query_row(
                "SELECT event_count FROM public_projections WHERE community='test'",
                [],
                |r| r.get(0),
            )?;
            if let Some(entered) = entered.take() {
                entered.send(()).unwrap();
                changed.recv_timeout(Duration::from_secs(5))?;
            }
            op.store.db.execute(
                "UPDATE public_projections SET event_count=? WHERE community='test'",
                [n + 1],
            )?;
            Ok(attempts)
        });
        let second = async {
            snapshot.await?;
            state.blocking(|op| {
                op.store.db.execute("UPDATE public_projections SET event_count=event_count+1 WHERE community='test'", [])?;
                Ok(())
            }).await?;
            resume.send(())?;
            anyhow::Ok(())
        };
        let (attempts, ()) = tokio::try_join!(first, second)?;
        assert!(attempts >= 2);
        let n: i64 = state.operation(|op| {
            Ok(op.store.db.query_row(
                "SELECT event_count FROM public_projections WHERE community='test'",
                [],
                |r| r.get(0),
            )?)
        })?;
        assert_eq!(n, 2);
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn signed_feeds_and_privacy_writes_use_native_workers() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut cfg = config(dir.path().join("home"));
        cfg.auto_federation.enabled = true;
        let server = Server::start(cfg).await?;
        let state = &server.state;
        let client = transport::bind(NetworkMode::Local, None, false).await?;
        let home = server.endpoint.id().to_string();
        let wallet = AccountWallet::create(new_secret(), home.clone())?;
        let group = server
            .create_group(crate::groups::NewGroup {
                name: "Readers".into(),
                description: "Reader tests".into(),
                category: "Chatter".into(),
                locality: None,
            })?
            .id;
        let token = state.group_token(&group);
        let author = signing_key(&new_secret())?;
        let event = SignedEvent::create(
            &author,
            home.clone(),
            group.clone(),
            1,
            Action::Post {
                body: "Original opening".into(),
                topic: "Chatter".into(),
                anonymous: true,
                display_name: "Anonymous neighbor".into(),
            },
        )?;
        let envelope = Envelope::accept(event, &state.key)?;
        state.operation(|op| {
            op.store.accept(&envelope)?;
            op.store.register_account(&wallet.chain)?;
            Ok(())
        })?;
        let public = |token: &str| Request::Feed {
            community: group.clone(),
            token: token.into(),
            author: String::new(),
            nearby: None,
        };
        let social = |op: SocialOp| -> Result<Request> {
            Ok(Request::Social {
                community: group.clone(),
                token: token.clone(),
                account: wallet.validate()?.payload.account.clone(),
                proof: AccountProof::create(
                    &wallet,
                    "social-v1",
                    &home,
                    &digest(&(client.id().to_string(), &group, &op))?,
                )?,
                op: Box::new(op),
            })
        };
        let rpc = |call: Request| {
            let endpoint = &client;
            let address = server.public_address();
            async move {
                timeout(
                    Duration::from_secs(5),
                    transport::rpc(endpoint, address, &call),
                )
                .await?
            }
        };
        let occupied = state
            .workers
            .slots
            .acquire_many((NATIVE_WORKERS - 1) as u32)
            .await?;
        for (request, direct, privacy) in [
            (public(&token), false, false),
            (social(SocialOp::Feed { nearby: None })?, true, false),
            (social(SocialOp::PrivacyFeed { nearby: None })?, true, true),
        ] {
            let Response::Feed {
                posts,
                direct: d,
                privacy: p,
                ..
            } = rpc(request).await?
            else {
                anyhow::bail!("expected a feed");
            };
            assert_eq!((d, p), (direct, privacy));
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0].body, "Original opening");
        }
        assert!(
            rpc(public("wrong"))
                .await
                .unwrap_err()
                .to_string()
                .contains("capability")
        );
        let (old_feed, old_stamp) = state
            .dispatch_stamped(
                &client.id().to_string(),
                social(SocialOp::PrivacyFeed { nearby: None })?,
            )
            .await?;
        let settings = PrivacySettings {
            hide_anonymous: true,
            ..Default::default()
        };
        rpc(social(SocialOp::SetPrivacy { settings })?).await?;
        drop(occupied);
        assert_ne!(old_stamp, *state.disclosure.epoch.read().unwrap());
        // Try to publish the feed that was already computed when the privacy RPC committed.
        let delayed = transport::bind(NetworkMode::Local, None, true).await?;
        let send_old = async {
            let conn = delayed.accept().await.context("listener closed")?.await?;
            let (mut send, mut recv) = conn.accept_bi().await?;
            recv.read_to_end(crate::MAX_FRAME).await?;
            state
                .send_response(
                    &mut send,
                    &transport::encode_response(&old_feed)?,
                    Some(old_stamp),
                )
                .await?;
            conn.closed().await;
            anyhow::Ok(())
        };
        let request = public(&token);
        let (result, sent) =
            tokio::join!(transport::rpc(&client, delayed.addr(), &request), send_old);
        sent?;
        assert!(result.unwrap_err().to_string().contains("expired"));
        delayed.close().await;
        assert!(
            matches!(rpc(social(SocialOp::PrivacyFeed { nearby: None })?).await?, Response::Feed { posts, .. } if posts.is_empty())
        );
        // Hold an operation that has authorized the old device and written to an
        // unrelated row. A concurrent revocation must wait for its commit.
        let (authorized, running) = oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let before_revoke = {
            let state = state.clone();
            let remote = client.id().to_string();
            let call = social(SocialOp::Approve { peer: home.clone() })?;
            tokio::spawn(async move {
                let mut authorized = Some(authorized);
                state
                    .blocking(move |op| {
                        op.handle(&remote, call.clone())?;
                        if let Some(authorized) = authorized.take() {
                            authorized.send(()).unwrap();
                            wait.recv_timeout(Duration::from_secs(5))?;
                        }
                        Ok(())
                    })
                    .await
            })
        };
        timeout(Duration::from_secs(5), running).await??;
        let rotated = wallet.transition(home.clone(), false)?;
        let mut revoke = Box::pin(rpc(Request::AnnounceAccount {
            chain: rotated.chain,
        }));
        assert!(
            timeout(Duration::from_millis(100), &mut revoke)
                .await
                .is_err()
        );
        release.send(())?;
        before_revoke.await??;
        revoke.await?;
        let rejected = rpc(social(SocialOp::Feed { nearby: None })?)
            .await
            .unwrap_err();
        assert!(rejected.to_string().contains("device revoked"));
        let stamp = *state.disclosure.epoch.read().unwrap();
        let path = dir.path().join("home");
        let id = envelope.event.id.clone();
        tokio::task::spawn_blocking(move || Store::moderate_existing(&path, &id, true)).await??;
        assert_ne!(stamp, *state.disclosure.epoch.read().unwrap());
        assert!(
            matches!(rpc(public(&token)).await?, Response::Feed { posts, .. } if posts.is_empty())
        );
        server.moderate(&envelope.event.id, false)?;
        let (visible, stamp) = state
            .dispatch_stamped(&client.id().to_string(), public(&token))
            .await?;
        assert!(matches!(visible, Response::Feed { posts, .. } if posts.len() == 1));
        let delete = SignedEvent::create(
            &author,
            home.clone(),
            group.clone(),
            2,
            Action::Delete {
                target: envelope.event.id.clone(),
            },
        )?;
        rpc(Request::Submit {
            token: token.clone(),
            event: delete,
            account_chain: Vec::new(),
        })
        .await?;
        assert_ne!(stamp, *state.disclosure.epoch.read().unwrap());
        assert!(
            matches!(rpc(public(&token)).await?, Response::Feed { posts, deleted, .. }
            if posts.is_empty() && deleted.contains(&envelope.event.id))
        );
        client.close().await;
        server.close().await;
        Ok(())
    }
}
