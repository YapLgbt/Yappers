// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded, endpoint-authenticated Iroh request/response streams.
//!
//! Local mode is loopback-only and does not contact public discovery/relays. N0 mode
//! explicitly opts into the public preset. Relays transport packets, not community data
//! storage or federation policy; the application always pins the home/peer endpoint ID.

pub use crate::MULTIPLEX_ALPN;
use crate::{ALPN, MAX_FRAME, RPC_TIMEOUT_SECS, model::*};
use anyhow::{Context, Result, ensure};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey,
    endpoint::{ConnectError, ConnectOptions, Connection, presets},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::OnceCell;

// Rust's DNS resolver accepts a trailing root dot. WebKit validates the WebSocket
// certificate against the literal hostname, so use the ordinary DNS spelling for TLS.
#[cfg(target_arch = "wasm32")]
fn browser_relay(relay: &iroh::RelayUrl) -> Result<iroh::RelayUrl> {
    let mut url = (**relay).clone();
    if let Some(host) = url
        .host_str()
        .and_then(|host| host.strip_suffix('.'))
        .map(str::to_owned)
    {
        url.set_host(Some(&host))?;
    }
    Ok(url.into())
}

pub async fn bind(mode: NetworkMode, secret: Option<SecretKey>, server: bool) -> Result<Endpoint> {
    let builder = match mode {
        NetworkMode::Local => Endpoint::builder(presets::Minimal),
        NetworkMode::N0 => Endpoint::builder(presets::N0),
    };
    #[cfg(target_arch = "wasm32")]
    ensure!(
        matches!(mode, NetworkMode::N0) && !server,
        "browser clients require a relay-enabled home"
    );
    #[cfg(target_arch = "wasm32")]
    let builder = {
        let urls = iroh::RelayMode::Default.relay_map().urls::<Vec<_>>();
        let relays = urls.iter().map(browser_relay).collect::<Result<Vec<_>>>()?;
        builder.relay_mode(iroh::RelayMode::custom(relays))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let builder = if matches!(mode, NetworkMode::Local) {
        builder
            .clear_ip_transports()
            .bind_addr("127.0.0.1:0")?
            .bind_addr("[::1]:0")?
    } else {
        builder
    };
    let builder = if let Some(key) = secret {
        builder.secret_key(key)
    } else {
        builder
    };
    let builder = if server {
        builder.alpns(vec![MULTIPLEX_ALPN.to_vec(), ALPN.to_vec()])
    } else {
        builder
    };
    let endpoint = builder.bind().await.context("bind Iroh endpoint")?;
    if server && matches!(mode, NetworkMode::N0) {
        n0_future::time::timeout(Duration::from_secs(RPC_TIMEOUT_SECS), endpoint.online())
            .await
            .context("Iroh relay readiness timed out")?;
    }
    Ok(endpoint)
}

/// Only network failures permit selection of a read-only public feed. A malformed response
/// or an explicit refusal by the pinned home remains authoritative.
#[derive(Debug)]
pub(crate) struct Unavailable;
impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("instance unreachable")
    }
}

enum Negotiated {
    Multiplex(Connection),
    // Exactly one waiter may use the stream on the negotiated v6 connection.
    Legacy(Mutex<Option<Connection>>),
}

type Dial = OnceCell<Result<Negotiated, Arc<ConnectError>>>;
const MAX_POOLED_PEERS: usize = 32;

/// One client's authenticated connections, keyed by remote endpoint identity.
/// Concurrent requests share the handshake and open independent QUIC streams.
pub struct RpcPool {
    endpoint: Endpoint,
    peers: Mutex<HashMap<EndpointId, Arc<Dial>>>,
}

impl RpcPool {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            peers: Mutex::default(),
        }
    }

    pub async fn rpc(&self, address: EndpointAddr, request: &Request) -> Result<Response> {
        rpc_with(Route::Pool(self), address, request).await
    }

    fn dial(&self, id: EndpointId) -> Arc<Dial> {
        let mut peers = self.peers.lock().expect("RPC peers mutex poisoned");
        let reusable = |cell: &Arc<Dial>| match cell.get() {
            None => Arc::strong_count(cell) > 1,
            Some(Ok(Negotiated::Multiplex(c))) => c.close_reason().is_none(),
            _ => false,
        };
        if let Some(cell) = peers.get(&id).filter(|cell| reusable(cell)) {
            return cell.clone();
        }
        // Drop closed or cancelled dials when inserting a peer. Existing
        // waiters still hold their own handle to their shared result.
        peers.retain(|_, cell| reusable(cell));
        if peers.len() >= MAX_POOLED_PEERS {
            // Negotiate both ALPNs for overflow too, but retain no pool-owned
            // handle. Existing handshakes and active streams keep their slots.
            return Arc::default();
        }
        peers.entry(id).or_default().clone()
    }

    async fn connect(&self, address: &EndpointAddr) -> Result<Connection> {
        let dial = self.dial(address.id);
        let result = dial
            .get_or_init(|| async {
                let options = ConnectOptions::new().with_additional_alpns(vec![ALPN.to_vec()]);
                let connecting = self
                    .endpoint
                    .connect_with_opts(address.clone(), MULTIPLEX_ALPN, options)
                    .await
                    .map_err(ConnectError::from)?;
                let connection = connecting.await.map_err(ConnectError::from)?;
                Ok(if connection.alpn() == MULTIPLEX_ALPN {
                    Negotiated::Multiplex(connection)
                } else {
                    Negotiated::Legacy(Mutex::new(Some(connection)))
                })
            })
            .await;
        match result {
            Ok(Negotiated::Multiplex(c)) => Ok(c.clone()),
            Ok(Negotiated::Legacy(first)) => {
                if let Some(c) = first.lock().expect("legacy RPC mutex poisoned").take() {
                    return Ok(c);
                }
                // We shared a handshake to a single-stream Home. Other waiters
                // must each establish a dedicated connection before writing.
                self.endpoint
                    .connect(address.clone(), ALPN)
                    .await
                    .context(Unavailable)
            }
            Err(e) => Err(anyhow::Error::new(e.clone()).context(Unavailable)),
        }
    }
}

#[derive(Clone, Copy)]
enum Route<'a> {
    OneShot(&'a Endpoint),
    Pool(&'a RpcPool),
}

impl Route<'_> {
    async fn connect(self, address: &EndpointAddr) -> Result<Connection> {
        match self {
            Self::OneShot(endpoint) => endpoint
                .connect(address.clone(), ALPN)
                .await
                .context(Unavailable),
            Self::Pool(pool) => pool.connect(address).await,
        }
    }
}

/// A one-shot v6 RPC, including any deletion continuations.
pub async fn rpc(
    endpoint: &Endpoint,
    address: EndpointAddr,
    request: &Request,
) -> Result<Response> {
    rpc_with(Route::OneShot(endpoint), address, request).await
}

async fn rpc_with(route: Route<'_>, address: EndpointAddr, request: &Request) -> Result<Response> {
    #[cfg(target_arch = "wasm32")]
    let address = {
        let mut target = EndpointAddr::new(address.id);
        for relay in address.relay_urls() {
            target = target.with_relay_url(browser_relay(relay)?);
        }
        target
    };
    let response = rpc_once(route, &address, request).await?;
    let Response::FeedWithMoreDeletions { mut feed } = response else {
        return Ok(response);
    };
    let call = match request {
        Request::HomeOperation { call, .. } => call.as_ref(),
        other => other,
    };
    let (community, token) = match call {
        Request::Feed {
            community, token, ..
        }
        | Request::Social {
            community, token, ..
        } => (community, Some(token.clone())),
        Request::PublicReplicaFeed { community }
        | Request::PullPublicProjection { community, .. } => (community, None),
        _ => anyhow::bail!("unexpected paged feed"),
    };
    let (Response::Feed { deleted, .. } | Response::PublicReplicaFeed { deleted, .. }) =
        feed.as_mut()
    else {
        anyhow::bail!("invalid paged feed")
    };
    loop {
        let cursor = deleted.last().context("missing deletion cursor")?.clone();
        let query = Request::DeletionsAfter {
            community: community.clone(),
            token: token.clone(),
            after: cursor.clone(),
        };
        let Response::DeletionPage { ids, more } = rpc_once(route, &address, &query).await? else {
            anyhow::bail!("invalid deletion response");
        };
        ensure!(
            ids.len() <= crate::DELETION_PAGE
                && (!more || !ids.is_empty())
                && ids.first().is_none_or(|id| id > &cursor)
                && ids.windows(2).all(|p| p[0] < p[1])
                && ids
                    .iter()
                    .all(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())),
            "invalid deletion page"
        );
        deleted.extend(ids);
        if !more {
            return Ok(*feed);
        }
    }
}

async fn rpc_once(route: Route<'_>, address: &EndpointAddr, request: &Request) -> Result<Response> {
    let bytes = serde_json::to_vec(request)?;
    ensure!(bytes.len() <= MAX_FRAME, "request too large");
    n0_future::time::timeout(Duration::from_secs(RPC_TIMEOUT_SECS), async {
        let mut connection = route.connect(address).await?;
        let (mut send, mut recv) = match connection.open_bi().await {
            Ok(streams) => streams,
            Err(_)
                if connection.alpn() == MULTIPLEX_ALPN && connection.close_reason().is_some() =>
            {
                // An idle connection can close between checkout and open_bi.
                // No request bytes exist on the wire at this point.
                connection = route.connect(address).await?;
                connection.open_bi().await.context(Unavailable)?
            }
            Err(e) => return Err(anyhow::Error::new(e).context(Unavailable)),
        };
        let result = async {
            send.write_all(&bytes).await.context(Unavailable)?;
            send.finish().context(Unavailable)?;
            let response = recv
                .read_to_end(MAX_FRAME)
                .await
                .map_err(|error| match error {
                    iroh::endpoint::ReadToEndError::Read(_) => {
                        anyhow::Error::new(error).context(Unavailable)
                    }
                    _ => anyhow::Error::new(error),
                })?;
            let response = decode_response(&response).context("invalid response")?;
            if matches!(&response, Response::Error { code, .. } if code == "ha_unavailable") {
                return Err(anyhow::anyhow!(Unavailable));
            }
            require_response(response)
        }
        .await;
        if connection.alpn() == ALPN {
            connection.close(0u32.into(), b"request complete");
        }
        result
    })
    .await
    .context(Unavailable)?
}

/// The app consumes hydrated rows. On the wire each disclosed account contributes
/// its profile once, even when it wrote many of the returned posts and replies.
#[cfg(feature = "server")]
pub(crate) fn encode_response(response: &Response) -> Result<Vec<u8>> {
    if !matches!(
        response,
        Response::Feed { .. } | Response::PublicReplicaFeed { .. }
    ) {
        return Ok(serde_json::to_vec(response)?);
    }
    let mut value = serde_json::to_value(response)?;
    let mut profiles = serde_json::Map::new();
    for post in value["posts"].as_array_mut().unwrap() {
        let mut pack = |row: &mut serde_json::Value| {
            if let Some(peer) = row["visibility"]["peer"].as_str().map(str::to_owned)
                && !row["profile"].is_null()
            {
                let profile = row.as_object_mut().unwrap().remove("profile").unwrap();
                profiles.insert(peer, profile);
            }
        };
        pack(post);
        for reply in post["replies"].as_array_mut().unwrap() {
            pack(reply);
        }
    }
    value["profiles"] = serde_json::Value::Object(profiles);
    Ok(serde_json::to_vec(&value)?)
}

fn decode_response(bytes: &[u8]) -> Result<Response> {
    #[derive(serde::Deserialize)]
    struct Dictionary {
        #[serde(default)]
        profiles: std::collections::HashMap<String, CommunityProfile>,
    }
    let mut response: Response = serde_json::from_slice(bytes)?;
    if let Response::Feed { posts, .. } | Response::PublicReplicaFeed { posts, .. } = &mut response
    {
        let dictionary: Dictionary = serde_json::from_slice(bytes)?;
        let mut budget = MAX_FRAME;
        let mut hydrate = |vis: &Option<AuthorVisibility>,
                           profile: &mut Option<CommunityProfile>|
         -> Result<()> {
            if let Some(peer) = vis
                .as_ref()
                .filter(|v| !v.anonymous)
                .and_then(|v| v.peer.as_ref())
                && let Some(shared) = dictionary.profiles.get(peer)
            {
                budget = budget
                    .checked_sub(serde_json::to_vec(shared)?.len())
                    .context("expanded feed too large")?;
                *profile = Some(shared.clone());
            }
            Ok(())
        };
        for post in posts {
            hydrate(&post.visibility, &mut post.profile)?;
            for reply in &mut post.replies {
                hydrate(&reply.visibility, &mut reply.profile)?;
            }
        }
    }
    Ok(response)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod pool_tests {
    use super::*;

    #[tokio::test]
    async fn pending_peers_share_one_slot_and_overflow_stays_uncached() -> Result<()> {
        let endpoint = bind(NetworkMode::Local, None, false).await?;
        let pool = RpcPool::new(endpoint.clone());
        let mut held = Vec::new();
        for _ in 0..MAX_POOLED_PEERS {
            let id = SecretKey::generate().public();
            held.push((id, pool.dial(id)));
        }

        let extra = SecretKey::generate().public();
        let overflow = pool.dial(extra);
        assert_eq!(pool.peers.lock().unwrap().len(), MAX_POOLED_PEERS);
        assert!(!pool.peers.lock().unwrap().contains_key(&extra));
        for (id, slot) in &held {
            assert!(Arc::ptr_eq(slot, &pool.dial(*id)));
        }

        // Dropping the last caller of a cancelled handshake frees its slot
        // on the next checkout. The overflow caller keeps its own handle.
        held.pop();
        let retained = pool.dial(extra);
        assert!(!Arc::ptr_eq(&overflow, &retained));
        assert!(Arc::ptr_eq(&retained, &pool.dial(extra)));
        assert_eq!(pool.peers.lock().unwrap().len(), MAX_POOLED_PEERS);
        endpoint.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn pooled_feed_rejects_a_nonadvancing_deletion_page() -> Result<()> {
        let server = bind(NetworkMode::Local, None, true).await?;
        let endpoint = bind(NetworkMode::Local, None, false).await?;
        let pool = RpcPool::new(endpoint.clone());
        let cursor = "1".repeat(64);
        let peer = async {
            let conn = server.accept().await.context("endpoint closed")?.await?;
            ensure!(conn.alpn() == MULTIPLEX_ALPN);
            let (mut send, mut recv) = conn.accept_bi().await?;
            let call: Request = serde_json::from_slice(&recv.read_to_end(MAX_FRAME).await?)?;
            ensure!(matches!(call, Request::PublicReplicaFeed { .. }));
            let response = Response::FeedWithMoreDeletions {
                feed: Box::new(Response::PublicReplicaFeed {
                    posts: vec![],
                    deleted: vec![cursor.clone()],
                    event_count: 1,
                    as_of: 1,
                }),
            };
            send.write_all(&serde_json::to_vec(&response)?).await?;
            send.finish()?;
            let (mut send, mut recv) = conn.accept_bi().await?;
            let call: Request = serde_json::from_slice(&recv.read_to_end(MAX_FRAME).await?)?;
            ensure!(
                matches!(call, Request::DeletionsAfter { ref community, token: None, ref after } if community == "public" && after == &cursor)
            );
            send.write_all(&serde_json::to_vec(&Response::DeletionPage {
                ids: vec![cursor],
                more: false,
            })?)
            .await?;
            send.finish()?;
            conn.closed().await;
            anyhow::Ok(())
        };
        let client = async {
            let err = pool
                .rpc(
                    server.addr(),
                    &Request::PublicReplicaFeed {
                        community: "public".into(),
                    },
                )
                .await
                .unwrap_err();
            ensure!(err.to_string() == "invalid deletion page" && !err.is::<Unavailable>());
            endpoint.close().await;
            anyhow::Ok(())
        };
        tokio::time::timeout(Duration::from_secs(8), async {
            tokio::try_join!(peer, client)
        })
        .await??;
        server.close().await;
        Ok(())
    }
}
