// SPDX-License-Identifier: AGPL-3.0-only
use super::{State, ha};
use crate::{ALPN, MAX_FRAME, MULTIPLEX_ALPN, RPC_TIMEOUT_SECS, model::*, transport};
use anyhow::{Context, Result, bail};
use iroh::endpoint::{Connection, Incoming, RecvStream, SendStream};
use n0_future::{FuturesUnordered, StreamExt};
use std::{
    future::{Future, ready},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{AcquireError, OwnedSemaphorePermit, Semaphore},
    time::{Instant, sleep, timeout_at},
};

pub(super) const MAX_WORK: usize = 64;
pub(super) const MAX_MUX_SESSIONS: usize = 256;
const STREAMS_PER_CONNECTION: usize = 8;
const RPC_TIMEOUT: Duration = Duration::from_secs(RPC_TIMEOUT_SECS);

pub(super) async fn serve(
    state: Arc<State>,
    incoming: Incoming,
    handshake: OwnedSemaphorePermit,
    sessions: Arc<Semaphore>,
) -> Result<()> {
    let deadline = Instant::now() + RPC_TIMEOUT;
    let connection = timeout_at(deadline, incoming).await??;
    let remote = connection.remote_id().to_string();
    match connection.alpn() {
        ALPN => {
            // The legacy client closes its connection after reading the response.
            let stream = timeout_at(deadline, connection.accept_bi()).await??;
            let _permit =
                serve_stream(&state, &remote, ready(Ok(handshake)), stream, deadline).await?;
            timeout_at(deadline, connection.closed()).await?;
            Ok(())
        }
        MULTIPLEX_ALPN => {
            let _session = sessions.try_acquire_owned()?;
            let slots = handshake.semaphore().clone();
            drop(handshake);
            serve_multiplex(&state, &remote, &slots, &connection).await
        }
        _ => bail!("unexpected Home protocol"),
    }
}

async fn serve_multiplex(
    state: &Arc<State>,
    remote: &str,
    slots: &Arc<Semaphore>,
    connection: &Connection,
) -> Result<()> {
    connection.set_max_concurrent_bi_streams((STREAMS_PER_CONNECTION as u32).into());
    // The connection task owns these futures. Cancelling it drops all stream
    // handlers before shutdown starts draining the shared worker permits.
    let mut active = FuturesUnordered::new();
    loop {
        tokio::select! {
            stream = connection.accept_bi(), if active.len() < STREAMS_PER_CONNECTION => {
                let Ok(stream) = stream else { return Ok(()); };
                let admission = slots.clone().acquire_owned();
                active.push(serve_stream(state, remote, admission, stream, Instant::now() + RPC_TIMEOUT));
            }
            Some(_) = active.next(), if !active.is_empty() => {}
            _ = sleep(RPC_TIMEOUT), if active.is_empty() => return Ok(()),
            _ = connection.closed() => return Ok(()),
        }
    }
}

async fn serve_stream(
    state: &Arc<State>,
    remote: &str,
    admission: impl Future<Output = Result<OwnedSemaphorePermit, AcquireError>>,
    (mut send, mut recv): (SendStream, RecvStream),
    deadline: Instant,
) -> Result<OwnedSemaphorePermit> {
    let result = timeout_at(deadline, async {
        // Idle connections consume no stream capacity. Time spent queued for
        // admission counts towards this stream's own request deadline.
        let permit = admission.await?;
        let mut first = [0u8; 1];
        recv.read_exact(&mut first).await?;
        if first[0] == ha::CONTROL {
            let ha = state.ha.as_ref().context("HA control disabled")?;
            let bytes = ha.control(remote, &mut recv).await?;
            send.write_all(&bytes).await?;
            send.finish()?;
            return Ok(permit);
        }

        let mut bytes = first.to_vec();
        bytes.extend_from_slice(&recv.read_to_end(MAX_FRAME - 1).await?);
        let (response, stamp) = dispatch(state, remote, &bytes).await;
        // A timeout can drop the JoinHandle while encoding is still running.
        let (permit, mut bytes) = tokio::task::spawn_blocking(move || {
            let bytes = transport::encode_response(&response)?;
            anyhow::Ok((permit, bytes))
        })
        .await??;
        if bytes.len() > MAX_FRAME {
            bytes = serde_json::to_vec(&Response::Error {
                code: "capacity".into(),
                message: "feed exceeds v1 response capacity".into(),
            })?;
        }
        state.send_response(&mut send, &bytes, stamp).await?;
        Ok(permit)
    })
    .await
    .map_err(anyhow::Error::from)
    .and_then(|r| r);
    if result.is_err() {
        let _ = send.reset(0u32.into());
        let _ = recv.stop(0u32.into());
    }
    result
}

async fn dispatch(state: &Arc<State>, remote: &str, bytes: &[u8]) -> (Response, Option<u64>) {
    let result = match serde_json::from_slice(bytes).context("invalid request") {
        Ok(request) => {
            if let Some(ha) = &state.ha {
                let (id, call) = match request {
                    Request::HomeOperation { request_id, call } => (request_id, *call),
                    other => (new_secret(), other),
                };
                let response = ha
                    .request(state, remote, id, call)
                    .await
                    .unwrap_or_else(|_| Response::Error {
                        code: "ha_unavailable".into(),
                        message: "Home commit authority unavailable; retry this operation".into(),
                    });
                return (response, None);
            }
            state.dispatch_stamped(remote, request).await
        }
        Err(error) => Err(error),
    };
    match result {
        Ok((response, epoch)) => (response, Some(epoch)),
        Err(error) => (
            Response::Error {
                code: "rejected".into(),
                message: error.to_string(),
            },
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{Server, federation_tests::config};
    use tokio::time::timeout;

    #[tokio::test]
    async fn idle_session_returns_handshake_slot_and_rpc_waits_for_capacity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cfg = config(dir.path().join("home"));
        let request = Request::Feed {
            community: "test".into(),
            token: cfg.communities["test"].clone(),
            author: String::new(),
            nearby: None,
        };
        let server = Server::start(cfg).await?;
        let listener = transport::bind(NetworkMode::Local, None, true).await?;
        let work = Arc::new(Semaphore::new(1));
        let sessions = Arc::new(Semaphore::new(1));
        let handshake = work.clone().try_acquire_owned()?;
        let handler = async {
            let incoming = listener.accept().await.context("listener closed")?;
            serve(server.state.clone(), incoming, handshake, sessions.clone()).await
        };
        let peer = async {
            let conn = server
                .endpoint
                .connect(listener.addr(), MULTIPLEX_ALPN)
                .await?;
            // The connection is still live. Consume the only work slot again.
            let busy = work.acquire().await?;
            assert_eq!(sessions.available_permits(), 0);
            let (mut send, mut recv) = conn.open_bi().await?;
            send.write_all(&serde_json::to_vec(&request)?).await?;
            send.finish()?;
            let mut response = Box::pin(recv.read_to_end(MAX_FRAME));
            assert!(
                timeout(Duration::from_millis(100), &mut response)
                    .await
                    .is_err()
            );
            drop(busy);
            assert!(matches!(
                serde_json::from_slice(&response.await?)?,
                Response::Feed { .. }
            ));
            conn.close(0u32.into(), b"done");
            anyhow::Ok(())
        };
        timeout(Duration::from_secs(8), async {
            tokio::try_join!(handler, peer)
        })
        .await??;
        assert_eq!(work.available_permits(), 1);
        assert_eq!(sessions.available_permits(), 1);
        listener.close().await;
        server.close().await;
        Ok(())
    }
}
