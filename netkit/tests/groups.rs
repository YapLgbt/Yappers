#![cfg(feature = "server")]
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
