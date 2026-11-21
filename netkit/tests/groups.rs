#![cfg(feature = "server")]
mod support;
use std::{collections::BTreeMap, path::Path};
use yappers_netkit::{
    client::{Client, ClientConfig},
    groups::*,
    model::*,
    server::{AutoFederationConfig, Server, ServerConfig},
    store::Store,
    transport,
};

fn config(path: &Path) -> ServerConfig {
    ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: new_secret(),
        database_path: path.into(),
        communities: BTreeMap::from([("private-home".into(), new_secret())]),
        peers: vec![],
        network_mode: NetworkMode::Local,
        auto_federation: AutoFederationConfig {
            covenant: None,
            enabled: true,
            ..Default::default()
        },
    }
}
fn details(name: &str, local: bool) -> NewGroup {
    NewGroup {
        name: name.into(),
        description: "A text-first community for meeting neighbors.".into(),
        category: "Friends".into(),
        locality: local.then(|| Locality {
            city: "New York".into(),
            region: "NY".into(),
            country: "US".into(),
        }),
    }
}
async fn client(join: JoinConfig) -> Client {
    Client::open(ClientConfig {
        full_home_standby: None,
        account_chain: vec![],
        join,
        author_secret: new_secret(),
        next_sequence: 1,
    })
    .await
    .unwrap()
}
fn post(body: &str) -> Action {
    Action::Post {
        body: body.into(),
        topic: "Friends".into(),
        anonymous: true,
        display_name: "Anonymous neighbor".into(),
    }
}
async fn posts(client: &Client) -> Vec<RemotePost> {
    let Response::Feed { posts, .. } = client.feed().await.unwrap() else {
        panic!("not a feed")
    };
    posts
}

#[test]
fn public_records_require_group_authority_and_live_host_signatures_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("catalog.sqlite")).unwrap();
    let authority = signing_key(&new_secret()).unwrap();
    let host = signing_key(&new_secret()).unwrap();
    let group = SignedGroup::create(&authority, details("Neighborhood readers", true)).unwrap();
    let host_id = hex::encode(host.verifying_key().as_bytes());
    let permit = HostPermit::create(&authority, &group, host_id.clone()).unwrap();
    let address = iroh::EndpointAddr::new(host_id.parse().unwrap());
    let announcement = HostAnnouncement::create(&host, group.id.clone(), address).unwrap();
    let mut tampered = group.clone();
    tampered.payload.details.name = "Forged name".into();
    assert!(tampered.validate().is_err());
    assert!(HostPermit::create(&host, &group, host_id.clone()).is_err());
    let mut forged = permit.clone();
    forged.signature = "00".repeat(64);
    assert!(
        store
            .import_directory(&DirectorySnapshot {
                groups: vec![group.clone()],
                permits: vec![forged],
                hosts: vec![]
            })
            .is_err()
    );
    assert!(store.directory().unwrap().groups.is_empty());
    let mut expired = announcement.clone();
    expired.payload.expires_at = now() - 1;
    assert!(
        expired
            .validate(&permit)
            .unwrap_err()
            .to_string()
            .contains("expired")
    );
    store
        .import_directory(&DirectorySnapshot {
            groups: vec![group.clone()],
            permits: vec![permit],
            hosts: vec![announcement],
        })
        .unwrap();
    assert_eq!(store.directory().unwrap().groups.len(), 1);
    assert_eq!(
        store.group_summaries().unwrap()[0]
            .locality
            .as_ref()
            .unwrap()
            .city,
        "New York"
    );
    assert!(
        !serde_json::to_string(&store.directory().unwrap())
            .unwrap()
            .contains("accessToken")
    );
}

#[test]
fn legacy_checkpoint_migrates_without_reusing_a_scope_less_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy.execute_batch("CREATE TABLE peers(id TEXT PRIMARY KEY,generation TEXT NOT NULL,cursor INTEGER NOT NULL);
        INSERT INTO peers VALUES('peer','legacy-generation',42);").unwrap();
    drop(legacy);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        store.cursor("peer").unwrap(),
        (Some("legacy-generation".into()), 42)
    );
    assert_eq!(
        store.scoped_cursor("peer", "new-scope-policy").unwrap(),
        (None, 0)
    );
    store
        .import_scoped_page("peer", "new-generation", 0, &[], "new-scope-policy")
        .unwrap();
    assert_eq!(
        store.scoped_cursor("peer", "new-scope-policy").unwrap(),
        (Some("new-generation".into()), 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_homes_discover_enroll_replicate_and_restart_without_manual_peers() {
    let dir = tempfile::tempdir().unwrap();
    let a_config = config(&dir.path().join("a.sqlite"));
    let a = Server::start(a_config.clone()).await.unwrap();
    let mut b_config = config(&dir.path().join("b.sqlite"));
    b_config.auto_federation.seeds = vec![a.public_address()];
    let b = Server::start(b_config.clone()).await.unwrap();
    assert!(a_config.peers.is_empty() && b_config.peers.is_empty());
    let group = a
        .create_group(details("Neighborhood readers", true))
        .unwrap();
    let same = a
        .create_group(details("Neighborhood readers", true))
        .unwrap();
    assert_eq!(
        group.id, same.id,
        "a lost create receipt must not duplicate an unchanged group"
    );
    let worldwide = a
        .create_group(details("Readers everywhere", false))
        .unwrap();
    assert_ne!(worldwide.id, group.id);
    b.sync_once().await.unwrap();
    assert_eq!(b.groups().unwrap().len(), 2);
    let cb = client(b.join_group(&group.id).await.unwrap()).await;
    b.sync_once().await.unwrap();
    let ca = client(a.join_group(&group.id).await.unwrap()).await;
    let event = ca
        .prepare(post("Across homes, in one city group"))
        .unwrap()
        .event;
    ca.submit(event.clone()).await.unwrap();
    let private = client(a.join("private-home", NetworkMode::Local).unwrap()).await;
    private
        .submit(
            private
                .prepare(post(
                    "Private home content must not enter the catalog or group",
                ))
                .unwrap()
                .event,
        )
        .await
        .unwrap();
    b.sync_once().await.unwrap();
    assert_eq!(posts(&cb).await.len(), 1);
    assert_eq!(posts(&cb).await[0].id, event.id);
    assert!(
        transport::rpc(
            &cb.endpoint,
            a.public_address(),
            &Request::Pull {
                after: 0,
                generation: None,
                limit: 100
            }
        )
        .await
        .is_err(),
        "client capability is not a host permit"
    );

    // B's old cursor has already skipped unjoined worldwide rows. Enrollment expands
    // its scope, so history must be replayed even without a serving-generation change.
    let global_a = client(a.join_group(&worldwide.id).await.unwrap()).await;
    let historical = global_a
        .prepare(post("Worldwide history before B joined"))
        .unwrap()
        .event;
    global_a.submit(historical.clone()).await.unwrap();
    b.sync_once().await.unwrap();
    let global_b = client(b.join_group(&worldwide.id).await.unwrap()).await;
    b.sync_once().await.unwrap();
    assert_eq!(posts(&global_b).await[0].id, historical.id);
    global_a.close().await;
    global_b.close().await;

    let mut c_config = config(&dir.path().join("c.sqlite"));
    c_config.auto_federation.seeds = vec![b.public_address()];
    let c = Server::start(c_config.clone()).await.unwrap();
    c.sync_once().await.unwrap();
    let cc = client(c.join_group(&group.id).await.unwrap()).await;
    c.sync_once().await.unwrap();
    assert_eq!(posts(&cc).await[0].id, event.id);
    cc.submit(
        cc.prepare(Action::Reply {
            parent: event.id.clone(),
            body: "Found through a different bootstrap home".into(),
            display_name: "C neighbor".into(),
        })
        .unwrap()
        .event,
    )
    .await
    .unwrap();
    cc.submit(
        cc.prepare(Action::Vote {
            target: event.id.clone(),
            value: 1,
        })
        .unwrap()
        .event,
    )
    .await
    .unwrap();
    c.sync_once().await.unwrap();
    b.sync_once().await.unwrap();
    assert_eq!(posts(&cb).await[0].score, 1);
    assert_eq!(posts(&cb).await[0].replies.len(), 1);
    cc.close().await;
    c.close().await;

    let c = Server::start(c_config.clone()).await.unwrap();
    c.sync_once().await.unwrap();
    let cc = client(c.join_group(&group.id).await.unwrap()).await;
    assert_eq!(posts(&cc).await[0].score, 1);
    b.sync_once().await.unwrap();
    cc.close().await;
    c.close().await;

    c_config
        .auto_federation
        .blocked_hosts
        .insert(a.endpoint.id().to_string());
    let c = Server::start(c_config.clone()).await.unwrap();
    ca.submit(
        ca.prepare(post("A new event after C blocked home A"))
            .unwrap()
            .event,
    )
    .await
    .unwrap();
    // C publishes its new address before B attempts the restarted endpoint.
    c.sync_once().await.unwrap();
    b.sync_once().await.unwrap();
    let welcome = cb
        .prepare(post("B remains welcome while C blocks A"))
        .unwrap()
        .event;
    cb.submit(welcome.clone()).await.unwrap();
    c.sync_once().await.unwrap();
    let cc = client(c.join("private-home", NetworkMode::Local).unwrap()).await;
    // Block is prospective; it neither imports the new origin nor erases old accepted data.
    let stored = Store::open(&dir.path().join("c.sqlite")).unwrap();
    assert_eq!(
        stored
            .events(&group.id)
            .unwrap()
            .iter()
            .filter(|e| matches!(e.event.payload.action, Action::Post { .. }))
            .count(),
        2
    );
    assert!(
        stored
            .events(&group.id)
            .unwrap()
            .iter()
            .any(|entry| entry.event.id == welcome.id)
    );
    assert!(
        c.join_group(&worldwide.id).await.is_err(),
        "blocked authority cannot admit a new group host"
    );
    cc.close().await;
    c.close().await;
    c_config.auto_federation.blocked_hosts.clear();
    let c = Server::start(c_config).await.unwrap();
    c.sync_once().await.unwrap();
    let stored = Store::open(&dir.path().join("c.sqlite")).unwrap();
    assert_eq!(
        stored
            .events(&group.id)
            .unwrap()
            .iter()
            .filter(|e| matches!(e.event.payload.action, Action::Post { .. }))
            .count(),
        3,
        "unblocking changes the policy checkpoint and recovers previously skipped history"
    );
    c.close().await;
    private.close().await;
    ca.close().await;
    cb.close().await;
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn home_can_create_and_browse_more_than_64_public_groups() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let cfg = config(&dir.path().join("home"));
    let store = Store::open(&cfg.database_path)?;
    let db = support::primary(cfg.database_path.as_ref()).await?;
    let key = signing_key(&cfg.private_key)?;
    // Seed existing, genuinely signed descriptors. Only the final group needs a live announcement.
    for index in 0..64 {
        let group = SignedGroup::create(&key, details(&format!("Group {index}"), false))?;
        db.execute(
            "INSERT INTO public_groups VALUES(?,?)",
            (&*group.id, serde_json::to_string(&group)?),
        )
        .await?;
        store.host_group(&group.id)?;
    }
    drop(db);
    drop(store);
    let server = Server::start(cfg).await?;
    server.create_group(details("Group 65", false))?;
    let reader = client(server.join("private-home", NetworkMode::Local)?).await;
    let Response::Groups { groups } = reader.groups().await? else {
        panic!("expected group list")
    };
    assert_eq!(groups.len(), 65);
    assert!(groups.iter().all(|group| group.hosted));
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..9 {
        let Response::Directory { snapshot } = transport::rpc(
            &reader.endpoint,
            server.public_address(),
            &Request::Directory {
                snapshot: DirectorySnapshot::default(),
            },
        )
        .await?
        else {
            panic!("expected directory")
        };
        assert!(snapshot.groups.len() <= MAX_GROUPS);
        seen.extend(snapshot.groups.into_iter().map(|g| g.id));
    }
    assert_eq!(seen.len(), 65);
    reader.close().await;
    server.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn covenant_admits_homes_to_shared_feed_and_groups_and_preserves_deletions()
-> anyhow::Result<()> {
    use yappers_netkit::{
        peering::Acceptance,
        server::{CovenantFederation, CovenantPeer},
    };

    let dir = tempfile::tempdir()?;
    let owner_key = signing_key(&new_secret())?;
    let main_home = hex::encode(owner_key.verifying_key().as_bytes());
    let main = format!("{main_home}/public");
    let setup = |name: &str, secret: String| -> anyhow::Result<ServerConfig> {
        let mut settings = config(&dir.path().join(format!("{name}.sqlite")));
        let acceptance = Acceptance::sign(
            &signing_key(&secret)?,
            name.into(),
            "test@example.org".into(),
            "https://example.org/source/release.tar.gz".into(),
            "a".repeat(64),
        )?;
        settings.private_key = secret;
        settings.communities.insert(main.clone(), new_secret());
        settings.auto_federation.covenant = Some(CovenantFederation {
            main_home: main_home.clone(),
            acceptance,
            admit_new: true,
        });
        Ok(settings)
    };
    let a_config = setup("Yap", hex::encode(owner_key.to_bytes()))?;
    let a = Server::start(a_config.clone()).await?;
    let mut b_config = setup("Second operator", new_secret())?;
    b_config.auto_federation.seeds = vec![a.public_address()];
    let b = Server::start(b_config.clone()).await?;
    let mut c_config = setup("Third operator", new_secret())?;
    c_config.auto_federation.seeds = vec![b.public_address()];
    let c = Server::start(c_config.clone()).await?;
    let b_id = b.endpoint.id().to_string();
    let c_id = c.endpoint.id().to_string();

    let receipt = b_config
        .auto_federation
        .covenant
        .as_ref()
        .unwrap()
        .acceptance
        .clone();
    let outsider = transport::bind(NetworkMode::Local, None, false).await?;
    let spoof = Request::CovenantPeers {
        network: main_home.clone(),
        peer: CovenantPeer {
            acceptance: receipt.clone(),
            local_addresses: vec![],
        },
        after: None,
    };
    assert!(
        transport::rpc(&outsider, a.public_address(), &spoof)
            .await
            .is_err()
    );
    assert!(
        transport::rpc(
            &outsider,
            a.public_address(),
            &Request::Pull {
                after: 0,
                generation: None,
                limit: 100
            }
        )
        .await
        .is_err()
    );
    outsider.close().await;

    b.sync_once().await?;
    c.sync_once().await?;
    b.sync_once().await?;
    b.sync_once().await?;
    let peers = Store::live_peers(&b_config.database_path)?;
    assert!(
        peers
            .iter()
            .any(|peer| peer.id == main_home && peer.status == "active")
    );
    assert!(
        peers
            .iter()
            .any(|peer| peer.id == c_id && peer.status == "active")
    );
    assert!(
        peers
            .iter()
            .all(|peer| !peer.communities.contains("private-home"))
    );

    let ca = client(a.join(&main, NetworkMode::Local)?).await;
    let wallet = yappers_netkit::portability::AccountWallet::create(new_secret(), b_id.clone())?;
    let cb = Client::open(ClientConfig {
        full_home_standby: None,
        join: b.join(&main, NetworkMode::Local)?,
        author_secret: wallet.device_secret,
        next_sequence: 1,
        account_chain: wallet.chain,
    })
    .await?;
    let cc = client(c.join(&main, NetworkMode::Local)?).await;
    let first = cb.prepare(post("Published from the second Home"))?.event;
    cb.submit(first.clone()).await?;
    a.sync_once().await?;
    c.sync_once().await?;
    assert_eq!(posts(&ca).await[0].id, first.id);
    assert_eq!(posts(&cc).await[0].id, first.id);
    ca.submit(
        ca.prepare(Action::Reply {
            parent: first.id.clone(),
            body: "Main feed reply".into(),
            display_name: "Yap neighbor".into(),
        })?
        .event,
    )
    .await?;
    b.sync_once().await?;
    assert_eq!(posts(&cb).await[0].replies.len(), 1);

    let hidden = client(a.join("private-home", NetworkMode::Local)?).await;
    let private_post = hidden.prepare(post("Only on this Home"))?.event;
    hidden.submit(private_post.clone()).await?;
    hidden.close().await;
    b.sync_once().await?;
    assert!(
        !Store::open(&b_config.database_path)?
            .events("private-home")?
            .iter()
            .any(|entry| entry.event.id == private_post.id)
    );

    let group = a.create_group(details("Covenant readers", false))?;
    b.sync_once().await?;
    let group_b = client(b.join_group(&group.id).await?).await;
    let group_a = client(a.join_group(&group.id).await?).await;
    let group_post = group_b.prepare(post("Same group across Homes"))?.event;
    group_b.submit(group_post.clone()).await?;
    a.sync_once().await?;
    assert_eq!(posts(&group_a).await[0].id, group_post.id);
    group_a.close().await;
    group_b.close().await;

    let delivered = ca.prepare(post("Delivered before suspension"))?.event;
    ca.submit(delivered.clone()).await?;
    b.sync_once().await?;
    assert!(posts(&cb).await.iter().any(|post| post.id == delivered.id));
    Store::stop_live_peering(&a_config.database_path, &b_id)?;
    assert!(
        Store::live_peers(&a_config.database_path)?
            .iter()
            .any(|peer| peer.id == b_id && peer.status == "suspended")
    );
    assert!(
        transport::rpc(&b.endpoint, a.public_address(), &spoof)
            .await
            .is_err()
    );
    let after = cb.prepare(post("Published after suspension"))?.event;
    cb.submit(after.clone()).await?;
    c.sync_once().await?;
    let _ = a.sync_once().await;
    assert!(!posts(&ca).await.iter().any(|post| post.id == after.id));
    ca.submit(
        ca.prepare(Action::Delete {
            target: delivered.id.clone(),
        })?
        .event,
    )
    .await?;
    cb.submit(
        cb.prepare(Action::Delete {
            target: first.id.clone(),
        })?
        .event,
    )
    .await?;
    let _ = a.sync_once().await;
    let _ = b.sync_once().await;
    assert!(!posts(&ca).await.iter().any(|post| post.id == first.id));
    assert!(!posts(&cb).await.iter().any(|post| post.id == delivered.id));

    ca.close().await;
    cb.close().await;
    cc.close().await;
    a.close().await;
    let mut a_config = a_config;
    a_config
        .auto_federation
        .covenant
        .as_mut()
        .unwrap()
        .admit_new = false;
    let a = Server::start(a_config.clone()).await?;
    assert!(
        Store::live_peers(&a_config.database_path)?
            .iter()
            .any(|peer| peer.id == b_id && peer.status == "suspended")
    );
    assert!(
        transport::rpc(&b.endpoint, a.public_address(), &spoof)
            .await
            .is_err()
    );
    let current = Request::CovenantPeers {
        network: main_home.clone(),
        peer: CovenantPeer {
            acceptance: c_config.auto_federation.covenant.unwrap().acceptance,
            local_addresses: vec![],
        },
        after: None,
    };
    assert!(
        transport::rpc(&c.endpoint, a.public_address(), &current)
            .await
            .is_ok()
    );
    let fresh_key = signing_key(&new_secret())?;
    let fresh = setup("New operator", hex::encode(fresh_key.to_bytes()))?;
    let d = Server::start(fresh.clone()).await?;
    let new = Request::CovenantPeers {
        network: main_home.clone(),
        peer: CovenantPeer {
            acceptance: fresh.auto_federation.covenant.unwrap().acceptance,
            local_addresses: vec![],
        },
        after: None,
    };
    assert!(
        transport::rpc(&d.endpoint, a.public_address(), &new)
            .await
            .is_err()
    );
    d.close().await;
    a.close().await;
    b.close().await;
    c.close().await;
    Ok(())
}
