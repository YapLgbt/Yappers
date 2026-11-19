// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded, endpoint-authenticated Iroh request/response streams.
//!
//! Local mode is loopback-only and does not contact public discovery/relays. N0 mode
//! explicitly opts into the public preset. Relays transport packets, not community data
//! storage or federation policy; the application always pins the home/peer endpoint ID.

use crate::{ALPN, MAX_FRAME, RPC_TIMEOUT_SECS, model::*};
use anyhow::{Context, Result, ensure};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use std::time::Duration;

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
        builder.alpns(vec![ALPN.to_vec()])
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

/// A bounded EOF-delimited JSON message on one bidirectional stream per connection.
pub async fn rpc(
    endpoint: &Endpoint,
    address: EndpointAddr,
    request: &Request,
) -> Result<Response> {
    #[cfg(target_arch = "wasm32")]
    let address = {
        let mut target = EndpointAddr::new(address.id);
        for relay in address.relay_urls() {
            target = target.with_relay_url(browser_relay(relay)?);
        }
        target
    };
    let bytes = serde_json::to_vec(request)?;
    ensure!(bytes.len() <= MAX_FRAME, "request too large");
    n0_future::time::timeout(Duration::from_secs(RPC_TIMEOUT_SECS), async {
        let connection = endpoint.connect(address, ALPN).await.context(Unavailable)?;
        let result = async {
            let (mut send, mut recv) = connection.open_bi().await.context(Unavailable)?;
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
        connection.close(0u32.into(), b"request complete");
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
