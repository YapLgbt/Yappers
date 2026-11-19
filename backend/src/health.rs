// SPDX-License-Identifier: AGPL-3.0-only
use anyhow::Result;
use axum::{Json, Router, http::StatusCode, routing::get};
use std::net::SocketAddr;
use yappers_netkit::server::Server;

/// The worker owns the listener. Its death immediately makes its check unreachable.
pub async fn listen(server: &Server) -> Result<()> {
    let Some(addr) = std::env::var_os("YAP_WORKER_HEALTH_ADDR") else {
        return Ok(());
    };
    let address: SocketAddr = addr
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid health address"))?
        .parse()?;
    let probe = server.health_probe();
    let route = move |capacity: bool| {
        let probe = probe.clone();
        async move {
            match tokio::task::spawn_blocking(probe).await {
                Ok(Ok(health)) => {
                    let status = if health.ready && (!capacity || health.has_headroom()) {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    };
                    (status, Json(serde_json::json!(health)))
                }
                _ => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"ready":false})),
                ),
            }
        }
    };
    let capacity = route.clone();
    let app = Router::new()
        .route("/ready", get(move || route(false)))
        .route("/capacity", get(move || capacity(true)));
    let listener = tokio::net::TcpListener::bind(address).await?;
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("worker health listener: {error}");
        }
    });
    Ok(())
}
