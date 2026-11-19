//! Only the configured home transport may retrieve a private ledger image.
#![cfg(feature = "server")]
use anyhow::{Result, ensure};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use yappers_netkit::{
    ALPN, PROTOCOL_VERSION,
    model::*,
    server::{HaOptions, HaRole, Server, ServerConfig},
    store::Store,
    takeover::FullHomeStandby,
    transport,
};

async fn query(endpoint: &Endpoint, address: EndpointAddr, packet: &[u8]) -> Result<Vec<u8>> {
    let c = endpoint.connect(address, ALPN).await?;
    let (mut send, mut recv) = c.open_bi().await?;
    send.write_all(packet).await?;
    send.finish()?;
    let reply = recv.read_to_end(1024 * 1024).await?;
    c.close(0u32.into(), b"done");
    Ok(reply)
}

#[tokio::test]
async fn identical_valid_control_packet_is_private_to_the_authorized_transport() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let owner = signing_key(&new_secret())?;
    let secret = hex::encode(owner.to_bytes());
    let home = hex::encode(owner.verifying_key().as_bytes());
    let node = SecretKey::generate();
    let scope = format!("{home}/public");
    let token = new_secret();
    let communities = BTreeMap::from([(scope.clone(), token.clone())]);
    let config = |name: &str| -> Result<ServerConfig> {
        Ok(serde_json::from_value(
            json!({"privateKey":secret,"communities":communities,"databasePath":tmp.path().join(name)}),
        )?)
    };
    let mut grant = FullHomeStandby::issue(&owner, EndpointAddr::new(node.public()))?;
    let fly = Server::start_ha(
        config("standby")?,
        HaOptions {
            grant: grant.clone(),
            role: HaRole::Standby {
                transport_key: hex::encode(node.to_bytes()),
                ledger: tmp.path().join("ledger"),
            },
        },
    )
    .await?;
    grant.standby = fly.public_address();
    let primary_cfg = config("dedicated")?;
    Store::open(&primary_cfg.database_path)?;
    Server::bootstrap_ha(primary_cfg, grant.clone()).await?;
    let policy = blake3::keyed_hash(
        &owner.to_bytes(),
        &serde_json::to_vec(&(
            PROTOCOL_VERSION,
            &communities,
            BTreeMap::<String, BTreeSet<String>>::new(),
            BTreeSet::<String>::new(),
            false,
            BTreeSet::<String>::new(),
            Option::<Value>::None,
        ))?,
    )
    .to_hex()
    .to_string();
    let op = json!({"remote":home,"id":new_secret(),"digest":new_secret()});
    let encode = |command: Value| -> Result<Vec<u8>> {
        let header = serde_json::to_vec(&json!([policy, command]))?;
        let mut packet = vec![0x81];
        packet.extend_from_slice(&(header.len() as u32).to_be_bytes());
        packet.extend_from_slice(&header);
        Ok(packet)
    };
    let packet = encode(json!({"Open":{"op":op,"known":null}}))?;
    let primary = transport::bind(
        NetworkMode::Local,
        Some(SecretKey::from_bytes(&owner.to_bytes())),
        false,
    )
    .await?;
    let valid = query(&primary, grant.standby.clone(), &packet).await?;
    ensure!(
        valid.windows(16).any(|b| b == b"SQLite format 3\0"),
        "positive control failed to retrieve the authenticated snapshot"
    );
    let outsider = transport::bind(NetworkMode::Local, None, false).await?;
    let denied = tokio::time::timeout(
        Duration::from_secs(2),
        query(&outsider, grant.standby.clone(), &packet),
    )
    .await;
    assert!(!matches!(denied,Ok(Ok(ref data)) if !data.is_empty()));
    let length = u32::from_be_bytes(valid[..4].try_into()?) as usize;
    let granted: Value = serde_json::from_slice(&valid[4..4 + length])?;
    let permit = granted["Ready"]["permit"]
        .as_str()
        .expect("missing fenced permit");
    // Drop the authenticated primary after it obtained its snapshot, before a commit.
    primary.close().await;
    let response = transport::rpc(
        &outsider,
        grant.standby.clone(),
        &Request::Feed {
            token,
            community: scope,
            author: String::new(),
            nearby: None,
        },
    )
    .await?;
    assert!(matches!(response, Response::Feed { .. }));
    // The same authorized physical identity returns with its original pending candidate.
    let recovered = transport::bind(
        NetworkMode::Local,
        Some(SecretKey::from_bytes(&owner.to_bytes())),
        false,
    )
    .await?;
    let stale = encode(json!({"Commit":{"op":op,"permit":permit,"result":"stale"}}))?;
    let reply = query(&recovered, grant.standby, &stale).await?;
    let header: Value = serde_json::from_slice(&reply[4..])?;
    assert_eq!(header, "Refused");
    outsider.close().await;
    recovered.close().await;
    fly.close().await;
    Ok(())
}
