#![cfg(feature = "server")]
use anyhow::{Context, Result, ensure};
use iroh::{Endpoint, endpoint::Connection};
use std::{collections::BTreeMap, time::Duration};
use tokio::time::timeout;
use yappers_netkit::{
    ALPN, MAX_FRAME, MULTIPLEX_ALPN, PROTOCOL_VERSION,
    client::{Client, ClientConfig},
    model::{JoinConfig, NetworkMode, Request, Response, new_secret},
    server::{Server, ServerConfig},
    transport,
};

const COMMUNITY: &str = "test/multiplex";
const TOKEN: &str = "a-capability-long-enough-for-a-test-home";
const DEADLINE: Duration = Duration::from_secs(8);

fn feed(token: &str) -> Request {
    Request::Feed {
        token: token.into(),
        community: COMMUNITY.into(),
        author: String::new(),
        nearby: None,
    }
}

async fn call(connection: &Connection, token: &str) -> Result<Response> {
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(&serde_json::to_vec(&feed(token))?).await?;
    send.finish()?;
    Ok(serde_json::from_slice(&recv.read_to_end(MAX_FRAME).await?)?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn home_serves_an_authorized_request_while_another_stream_is_stalled() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let home = Server::start(ServerConfig {
        database_path: dir.path().join("home"),
        private_key: new_secret(),
        communities: BTreeMap::from([(COMMUNITY.into(), TOKEN.into())]),
        peers: vec![],
        public_replica_feeds: Default::default(),
        network_mode: NetworkMode::Local,
        auto_federation: Default::default(),
    })
    .await?;
    let endpoint = transport::bind(NetworkMode::Local, None, false).await?;
    let connection = endpoint
        .connect(home.endpoint.addr(), MULTIPLEX_ALPN)
        .await?;
    let (mut stalled, _pending_response) = connection.open_bi().await?;
    stalled.write_all(b"{").await?;
    let authorized = timeout(DEADLINE, call(&connection, TOKEN)).await??;
    ensure!(matches!(authorized, Response::Feed { .. }));
    ensure!(matches!(
        call(&connection, "wrong capability").await?,
        Response::Error { .. }
    ));
    stalled.reset(0u32.into())?;
    ensure!(matches!(
        timeout(DEADLINE, call(&connection, TOKEN)).await??,
        Response::Feed { .. }
    ));
    // A peer holding a live connection must not prevent the Home shutting down.
    timeout(DEADLINE, home.close()).await?;
    endpoint.close().await;
    Ok(())
}

async fn client_for(home: &Endpoint) -> Result<Client> {
    Client::open(ClientConfig {
        join: JoinConfig {
            protocol_version: PROTOCOL_VERSION,
            server_addr: home.addr(),
            community: COMMUNITY.into(),
            access_token: TOKEN.into(),
            network_mode: NetworkMode::Local,
        },
        full_home_standby: None,
        account_chain: vec![],
        author_secret: new_secret(),
        next_sequence: 1,
    })
    .await
}

async fn read_feed(connection: &Connection) -> Result<iroh::endpoint::SendStream> {
    let (send, mut recv) = connection.accept_bi().await?;
    ensure!(matches!(
        serde_json::from_slice::<Request>(&recv.read_to_end(MAX_FRAME).await?)?,
        Request::Feed { .. }
    ));
    Ok(send)
}

async fn respond(mut send: iroh::endpoint::SendStream) -> Result<()> {
    send.write_all(br#"{"type":"feed","posts":[],"eventCount":0}"#)
        .await?;
    send.finish()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_calls_and_a_later_call_share_one_negotiated_connection() -> Result<()> {
    let home = transport::bind(NetworkMode::Local, None, true).await?;
    let client = client_for(&home).await?;
    let peer = async {
        let connection = home.accept().await.context("endpoint closed")?.await?;
        ensure!(connection.alpn() == MULTIPLEX_ALPN);
        // Neither call receives a response until both arrive on this connection.
        let mut replies = Vec::new();
        for _ in 0..2 {
            let (send, mut recv) = connection.accept_bi().await?;
            let request = serde_json::from_slice(&recv.read_to_end(MAX_FRAME).await?)?;
            let response = match request {
                Request::Feed { .. } => br#"{"type":"feed","posts":[],"eventCount":0}"#.as_slice(),
                Request::Groups { .. } => br#"{"type":"groups","groups":[]}"#.as_slice(),
                _ => anyhow::bail!("unexpected request"),
            };
            replies.push((send, response));
        }
        for (mut send, reply) in replies.into_iter().rev() {
            send.write_all(reply).await?;
            send.finish()?;
        }
        respond(read_feed(&connection).await?).await?;
        connection.closed().await;
        anyhow::Ok(())
    };
    let calls = async {
        let (a, b) = tokio::try_join!(client.feed(), client.groups())?;
        ensure!(matches!(a, Response::Feed { .. }) && matches!(b, Response::Groups { .. }));
        client.feed().await?;
        client.close().await;
        Ok(())
    };
    timeout(DEADLINE, async { tokio::try_join!(peer, calls) }).await??;
    home.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_client_talks_to_single_request_legacy_home() -> Result<()> {
    let home = transport::bind(NetworkMode::Local, None, false).await?;
    home.set_alpns(vec![ALPN.to_vec()]);
    let client = client_for(&home).await?;
    let peer = async {
        for _ in 0..2 {
            let connection = home.accept().await.context("endpoint closed")?.await?;
            ensure!(connection.alpn() == ALPN);
            respond(read_feed(&connection).await?).await?;
            connection.closed().await;
        }
        anyhow::Ok(())
    };
    let calls = async {
        client.feed().await?;
        client.feed().await?;
        client.close().await;
        Ok(())
    };
    timeout(DEADLINE, async { tokio::try_join!(peer, calls) }).await??;
    home.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disconnect_after_submit_does_not_replay_it_and_next_call_reconnects() -> Result<()> {
    let home = transport::bind(NetworkMode::Local, None, true).await?;
    let client = client_for(&home).await?;
    let event = client
        .prepare(yappers_netkit::Action::Post {
            body: "single submission".into(),
            topic: "Plans".into(),
            display_name: "Neighbor".into(),
            anonymous: false,
        })?
        .event;
    let peer = async {
        let connection = home.accept().await.context("endpoint closed")?.await?;
        ensure!(connection.alpn() == MULTIPLEX_ALPN);
        let (_send, mut recv) = connection.accept_bi().await?;
        ensure!(matches!(
            serde_json::from_slice::<Request>(&recv.read_to_end(MAX_FRAME).await?)?,
            Request::Submit { .. }
        ));
        // A Home could already have committed the event before losing this reply.
        connection.close(0u32.into(), b"lost response");
        let replacement = home.accept().await.context("endpoint closed")?.await?;
        respond(read_feed(&replacement).await?).await?;
        replacement.closed().await;
        anyhow::Ok(())
    };
    let calls = async {
        ensure!(client.submit(event).await.is_err());
        client.feed().await?;
        client.close().await;
        Ok(())
    };
    timeout(DEADLINE, async { tokio::try_join!(peer, calls) }).await??;
    home.close().await;
    Ok(())
}
