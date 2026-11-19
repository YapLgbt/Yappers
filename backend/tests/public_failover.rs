//! A real process suspension leaves the pinned endpoint and its port unchanged on recovery.
#![cfg(unix)]
use anyhow::{Result, ensure};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use yappers_netkit::{
    client::{Client, ClientConfig},
    model::*,
    server::{PeerConfig, Server},
    transport,
};

struct Fixture {
    directory: PathBuf,
    primary: Option<Child>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.primary {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}
impl Fixture {
    fn signal(&self, signal: &str) -> Result<()> {
        ensure!(
            Command::new("kill")
                .arg(signal)
                .arg(self.primary.as_ref().unwrap().id().to_string())
                .status()?
                .success(),
            "signal failed"
        );
        Ok(())
    }
}
fn endpoint(key: &str) -> String {
    hex::encode(signing_key(key).unwrap().verifying_key().as_bytes())
}
fn post(body: &str) -> Action {
    Action::Post {
        display_name: "Anonymous neighbor".into(),
        anonymous: true,
        body: body.into(),
        topic: "Chatter".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn public_feed_failover_and_failback_preserve_primary_identity() -> Result<()> {
    let mut f = Fixture {
        directory: std::env::temp_dir().join(format!("yap-failover-{}", new_secret())),
        primary: None,
    };
    fs::create_dir(&f.directory)?;
    let primary_key = new_secret();
    let replica_key = new_secret();
    let owner = endpoint(&primary_key);
    let scope = format!("{owner}/public");
    let replica_config = serde_json::from_value(json!({
        "privateKey":replica_key,"databasePath":f.directory.join("replica.sqlite"),
        "communities":{&scope:new_secret()},"publicReplicaFeeds":[&scope],
        "peers":[{"address":{"id":owner,"addrs":[]},"communities":[&scope]}]
    }))?;
    let replica = Server::start(replica_config).await?;
    let primary_config = json!({
        "privateKey":primary_key,"databasePath":f.directory.join("primary.sqlite"),
        "communities":{&scope:new_secret()},
        "peers":[{"address":replica.public_address(),"communities":[&scope]}]
    });
    let config_path = f.directory.join("config.json");
    fs::write(&config_path, serde_json::to_vec(&primary_config)?)?;
    f.primary = Some(
        Command::new(env!("CARGO_BIN_EXE_yappers-server"))
            .arg("run")
            .arg("--config")
            .arg(config_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let join_path = f.directory.join("join-0.json");
    tokio::time::timeout(Duration::from_secs(10), async {
        while !join_path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    let join: JoinConfig = serde_json::from_slice(&fs::read(join_path)?)?;
    replica.set_peers(vec![PeerConfig {
        address: join.server_addr.clone(),
        communities: BTreeSet::from([scope.clone()]),
    }])?;
    let open = async |join| {
        Client::open(ClientConfig {
            full_home_standby: None,
            join,
            author_secret: new_secret(),
            next_sequence: 1,
            account_chain: vec![],
        })
        .await
    };
    let client = open(join.clone()).await?;
    let backup = replica.public_address();
    let public_read = || Request::PublicReplicaFeed {
        community: scope.clone(),
    };
    // No publication until this peer actually has a projection approved by the primary.
    assert!(
        transport::rpc(&client.endpoint, backup.clone(), &public_read())
            .await
            .is_err()
    );
    let visible = client.prepare(post("Visible")).unwrap().event;
    client.submit(visible.clone()).await?;
    let hidden = client
        .prepare(post("Hidden by primary moderation"))
        .unwrap()
        .event;
    client.submit(hidden.clone()).await?;
    ensure!(
        Command::new(env!("CARGO_BIN_EXE_yappers-server"))
            .arg("moderate")
            .arg("--database")
            .arg(f.directory.join("primary.sqlite"))
            .arg("--event")
            .arg(&hidden.id)
            .status()?
            .success()
    );
    replica.sync_once().await?;
    assert!(matches!(
        client.feed_with_failover(backup.clone()).await?,
        Response::Feed { .. }
    ));
    assert!(
        transport::rpc(
            &client.endpoint,
            backup.clone(),
            &Request::PublicReplicaFeed {
                community: "another/private".into()
            }
        )
        .await
        .is_err()
    );
    let mut rejected = join.clone();
    rejected.access_token = new_secret();
    let stranger = open(rejected).await?;
    assert!(
        stranger.feed_with_failover(backup.clone()).await.is_err(),
        "rejection must never trigger fallback"
    );
    stranger.close().await;

    f.signal("-STOP")?;
    let response = client.feed_with_failover(backup.clone()).await?;
    let Response::PublicReplicaFeed { posts, as_of, .. } = response else {
        panic!("expected read failover")
    };
    assert!(as_of > 0);
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].id, visible.id);
    assert!(!posts[0].mine);
    assert!(posts[0].visibility.as_ref().unwrap().peer.is_none());
    let queued = client
        .prepare(post("Queued for the original primary"))?
        .event;
    assert_eq!(queued.payload.home, owner);

    f.signal("-CONT")?;
    client.submit(queued.clone()).await?;
    let Response::Feed { posts, .. } = client.feed_with_failover(backup).await? else {
        panic!("expected automatic failback")
    };
    assert!(posts.iter().any(|p| p.id == queued.id));
    client.close().await;
    replica.close().await;
    Ok(())
}

/// Read-only production check. The only input is the public HTTPS enrollment document.
#[tokio::test]
#[ignore = "requires YAP_SMOKE_ENROLLMENT with publicFeedReplica"]
async fn deployed_public_feed_replica() -> Result<()> {
    let manifest: serde_json::Value =
        serde_json::from_reader(fs::File::open(std::env::var("YAP_SMOKE_ENROLLMENT")?)?)?;
    let join: JoinConfig = serde_json::from_value(manifest["join"].clone())?;
    let replica = serde_json::from_value(manifest["publicFeedReplica"].clone())?;
    ensure!(join.community == format!("{}/public", join.server_addr.id));
    let client = Client::open(ClientConfig {
        full_home_standby: None,
        join,
        author_secret: new_secret(),
        next_sequence: 1,
        account_chain: vec![],
    })
    .await?;
    let response = transport::rpc(
        &client.endpoint,
        replica,
        &Request::PublicReplicaFeed {
            community: client.join.community.clone(),
        },
    )
    .await?;
    let Response::PublicReplicaFeed { posts, as_of, .. } = response else {
        anyhow::bail!("unexpected replica response")
    };
    ensure!(
        (0..=120).contains(&(now() - as_of)),
        "replica snapshot is stale"
    );
    ensure!(
        posts
            .iter()
            .all(|p| !p.mine && p.vote == 0 && p.replies.iter().all(|r| !r.mine))
    );
    println!(
        "Primary-confirmed snapshot served by replica, {} public posts, {} seconds old",
        posts.len(),
        now() - as_of
    );
    client.close().await;
    Ok(())
}
