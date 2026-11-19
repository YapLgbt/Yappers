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
            let response = serde_json::from_slice(&response).context("invalid response")?;
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
