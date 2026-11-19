#![cfg(feature = "server")]
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use yappers_netkit::{
    client::{Client, ClientConfig},
    model::*,
    server::{PeerConfig, Server, ServerConfig},
    store::Store,
    transport,
};
fn config(path: &Path, secret: String, community: &str) -> ServerConfig {
    ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: secret,
        database_path: path.to_owned(),
        communities: BTreeMap::from([(
            community.to_owned(),
            "test-capability-32-bytes-minimum-value".into(),
        )]),
        peers: vec![],
        network_mode: NetworkMode::Local,
        auto_federation: Default::default(),
    }
}
async fn client(server: &Server, community: &str, author: String, next: u64) -> Client {
    Client::open(ClientConfig {
        full_home_standby: None,
        account_chain: vec![],
        join: server.join(community, NetworkMode::Local).unwrap(),
        author_secret: author,
        next_sequence: next,
    })
    .await
    .unwrap()
}
fn post(body: &str) -> Action {
    Action::Post {
        body: body.into(),
        topic: "Chatter".into(),
        anonymous: true,
        display_name: "Anonymous neighbor".into(),
    }
}
async fn posts(client: &Client) -> Vec<RemotePost> {
    match client.feed().await.unwrap() {
        Response::Feed { posts, .. } => posts,
        _ => panic!("not a feed"),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_iroh_two_home_federation_roundtrip_restart_and_authority() {
    let dir = tempfile::tempdir().unwrap();
    let community = "integration/community";
    let a_config = config(&dir.path().join("a.sqlite"), new_secret(), community);
    let b_config = config(&dir.path().join("b.sqlite"), new_secret(), community);
    let a = Server::start(a_config.clone()).await.unwrap();
    let b = Server::start(b_config.clone()).await.unwrap();
    let scopes = BTreeSet::from([community.to_owned()]);
    a.set_peers(vec![PeerConfig {
        address: b.endpoint.addr(),
        communities: scopes.clone(),
    }])
    .unwrap();
    b.set_peers(vec![PeerConfig {
        address: a.endpoint.addr(),
        communities: scopes.clone(),
    }])
    .unwrap();
    let author_a = new_secret();
    let author_b = new_secret();
    let ca = client(&a, community, author_a.clone(), 1).await;
    let cb = client(&b, community, author_b, 1).await;
    let created = ca
        .prepare(Action::PostV2 {
            area: Some(PostArea {
                label: "Boston, MA".into(),
                lat100: 4236,
                lon100: -7106,
            }),
            body: "A real Iroh federation test".into(),
            headline: "Across two homes".into(),
            topic: "Chatter".into(),
            anonymous: false,
            display_name: "Tender comet".into(),
            profile: None,
        })
        .unwrap()
        .event;
    ca.submit(created.clone()).await.unwrap();
    ca.submit(created.clone()).await.unwrap();
    assert!(posts(&cb).await.is_empty());
    b.sync_once().await.unwrap();
    let seen = posts(&cb).await;
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].id, created.id);
    assert_eq!(seen[0].headline, "Across two homes");
    for (lat, lon, count) in [(4237, -7111, 1), (4071, -7401, 0)] {
        let response = cb
            .command(yappers_netkit::client::Command::NearbyFeed {
                nearby: Nearby {
                    center: PostArea {
                        label: "Query".into(),
                        lat100: lat,
                        lon100: lon,
                    },
                    radius_miles: 10,
                },
            })
            .await
            .unwrap();
        assert_eq!(response["posts"].as_array().unwrap().len(), count);
    }
    assert!(seen[0].profile.is_none());
    cb.submit(
        cb.prepare(Action::Reply {
            parent: created.id.clone(),
            body: "From instance B".into(),
            display_name: "B neighbor".into(),
        })
        .unwrap()
        .event,
    )
    .await
    .unwrap();
    cb.submit(
        cb.prepare(Action::Vote {
            target: created.id.clone(),
            value: 1,
        })
        .unwrap()
        .event,
    )
    .await
    .unwrap();
    a.sync_once().await.unwrap();
    b.sync_once().await.unwrap();
    a.sync_once().await.unwrap();
    let pa = posts(&ca).await;
    let pb = posts(&cb).await;
    assert_eq!(pa[0].score, 1);
    assert_eq!(pa[0].replies.len(), 1);
    assert_eq!(pa[0].body, pb[0].body);
    // Local moderation does not silently propagate to another operator.
    b.moderate(&created.id, true).unwrap();
    assert!(posts(&cb).await.is_empty());
    assert_eq!(posts(&ca).await.len(), 1);
    b.moderate(&created.id, false).unwrap();
    // An author cannot delete somebody else's post.
    let bad = cb
        .prepare(Action::Delete {
            target: created.id.clone(),
        })
        .unwrap()
        .event;
    assert!(cb.submit(bad).await.is_err());
    // A random endpoint with a valid member token still cannot read federation logs.
    assert!(
        transport::rpc(
            &ca.endpoint,
            b.endpoint.addr(),
            &Request::Pull {
                after: 0,
                generation: None,
                limit: 100
            }
        )
        .await
        .is_err()
    );
    cb.close().await;
    b.close().await;
    let restarted = Server::start(b_config.clone()).await.unwrap();
    restarted
        .set_peers(vec![PeerConfig {
            address: a.endpoint.addr(),
            communities: scopes.clone(),
        }])
        .unwrap();
    a.set_peers(vec![PeerConfig {
        address: restarted.endpoint.addr(),
        communities: scopes,
    }])
    .unwrap();
    let cb2 = client(&restarted, community, new_secret(), 1).await;
    assert_eq!(posts(&cb2).await[0].replies.len(), 1);
    ca.submit(ca.prepare(post("After a replica restart")).unwrap().event)
        .await
        .unwrap();
    restarted.sync_once().await.unwrap();
    assert_eq!(posts(&cb2).await.len(), 2);
    cb2.close().await;
    restarted.close().await;
    ca.submit(
        ca.prepare(Action::Delete {
            target: created.id.clone(),
        })
        .unwrap()
        .event,
    )
    .await
    .unwrap();
    let returning = Server::start(b_config).await.unwrap();
    let fresh_path = dir.path().join("fresh.sqlite");
    let fresh = Server::start(config(&fresh_path, new_secret(), community))
        .await
        .unwrap();
    let peer = |home: &Server| PeerConfig {
        address: home.endpoint.addr(),
        communities: BTreeSet::from([community.to_string()]),
    };
    a.set_peers(vec![peer(&returning), peer(&fresh)]).unwrap();
    returning.set_peers(vec![peer(&a)]).unwrap();
    fresh.set_peers(vec![peer(&a), peer(&returning)]).unwrap();
    returning.sync_peer(&peer(&a)).await.unwrap();
    fresh.sync_peer(&peer(&a)).await.unwrap();
    ca.close().await;
    a.close().await;
    returning.close().await;
    fresh.close().await;
    for path in [dir.path().join("b.sqlite"), fresh_path] {
        let record = Store::open(&path)
            .unwrap()
            .existing(&created.id)
            .unwrap()
            .unwrap();
        record.validate().unwrap();
        assert!(record.deletion.is_some());
        let raw = std::fs::read(&path).unwrap();
        for needle in [
            "A real Iroh federation test",
            "Boston, MA",
            &created.payload.nonce,
        ] {
            assert!(!raw.windows(needle.len()).any(|w| w == needle.as_bytes()));
        }
    }
}
#[test]
fn store_import_and_cursor_are_atomic_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.sqlite");
    let home = signing_key(&new_secret()).unwrap();
    let author = signing_key(&new_secret()).unwrap();
    let home_id = hex::encode(home.verifying_key().as_bytes());
    let one = Envelope::accept(
        SignedEvent::create(&author, home_id.clone(), "test".into(), 1, post("one")).unwrap(),
        &home,
    )
    .unwrap();
    let conflicting = Envelope::accept(
        SignedEvent::create(&author, home_id, "test".into(), 1, post("sequence reused")).unwrap(),
        &home,
    )
    .unwrap();
    let mut store = Store::open(&path).unwrap();
    let page = vec![
        LogEntry {
            username: None,
            cursor: 1,
            envelope: one.clone(),
        },
        LogEntry {
            username: None,
            cursor: 2,
            envelope: conflicting,
        },
    ];
    assert!(store.import_page("peer", "generation", 2, &page).is_err());
    assert_eq!(store.cursor("peer").unwrap(), (None, 0));
    assert!(store.events("test").unwrap().is_empty());
    let valid = vec![LogEntry {
        username: None,
        cursor: 1,
        envelope: one.clone(),
    }];
    store.import_page("peer", "generation", 1, &valid).unwrap();
    store.import_page("peer", "generation", 1, &valid).unwrap();
    assert_eq!(store.events("test").unwrap().len(), 1);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.cursor("peer").unwrap(),
        (Some("generation".into()), 1)
    );
    assert_eq!(store.events("test").unwrap()[0], one);
}
fn read_ffi(ptr: *mut std::ffi::c_char) -> serde_json::Value {
    unsafe {
        let value = serde_json::from_str(std::ffi::CStr::from_ptr(ptr).to_str().unwrap()).unwrap();
        yappers_netkit::ffi::yappers_netkit_string_free(ptr);
        value
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ffi_real_client_uses_same_transport_and_frees_every_result() {
    let dir = tempfile::tempdir().unwrap();
    let community = "ffi/test";
    let server = Server::start(config(
        &dir.path().join("server.sqlite"),
        new_secret(),
        community,
    ))
    .await
    .unwrap();
    let config=serde_json::json!({"join":server.join(community,NetworkMode::Local).unwrap(),"authorSecret":new_secret(),"nextSequence":1}).to_string();
    tokio::task::spawn_blocking(move || unsafe {
        use std::ffi::CString;
        use yappers_netkit::ffi::*;
        let config = CString::new(config).unwrap();
        let opened = read_ffi(yappers_netkit_open(config.as_ptr()));
        assert_eq!(opened["ok"], true);
        let handle = opened["data"]["handle"].as_u64().unwrap();
        let command = CString::new(
            serde_json::json!({"type":"prepare","action":post("From the C ABI")}).to_string(),
        )
        .unwrap();
        let prepared = read_ffi(yappers_netkit_request(handle, command.as_ptr()));
        assert_eq!(prepared["ok"], true);
        let command = CString::new(
            serde_json::json!({"type":"submit","event":prepared["data"]["event"]}).to_string(),
        )
        .unwrap();
        assert_eq!(
            read_ffi(yappers_netkit_request(handle, command.as_ptr()))["ok"],
            true
        );
        let feed = CString::new(r#"{"type":"feed"}"#).unwrap();
        let expired = yappers_netkit_begin_refresh();
        let active = yappers_netkit_begin_refresh();
        yappers_netkit_end_refresh(expired);
        let refused = read_ffi(yappers_netkit_refresh_request(
            expired,
            handle,
            feed.as_ptr(),
        ));
        assert_eq!(refused["error"], "refresh expired");
        let feed_result = read_ffi(yappers_netkit_refresh_request(
            active,
            handle,
            feed.as_ptr(),
        ));
        yappers_netkit_end_refresh(active);
        assert_eq!(
            read_ffi(yappers_netkit_request(handle, feed.as_ptr()))["ok"],
            true
        );
        assert_eq!(feed_result["data"]["posts"][0]["body"], "From the C ABI");
        assert_eq!(read_ffi(yappers_netkit_close(handle))["ok"], true);
        assert_eq!(
            read_ffi(yappers_netkit_request(handle, feed.as_ptr()))["ok"],
            false
        );
    })
    .await
    .unwrap();
    server.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expiring_background_refresh_interrupts_a_pending_rpc() {
    use yappers_netkit::ffi::*;
    let endpoint = transport::bind(NetworkMode::Local, None, true)
        .await
        .unwrap();
    let join = JoinConfig {
        protocol_version: yappers_netkit::PROTOCOL_VERSION,
        server_addr: endpoint.addr(),
        community: "background-test".into(),
        access_token: new_secret(),
        network_mode: NetworkMode::Local,
    };
    let scope = yappers_netkit_begin_refresh();
    let request = tokio::task::spawn_blocking(move || unsafe {
        let config = std::ffi::CString::new(
            serde_json::json!({"join":join,"authorSecret":new_secret(),"nextSequence":1})
                .to_string(),
        )
        .unwrap();
        let handle = read_ffi(yappers_netkit_open(config.as_ptr()))["data"]["handle"]
            .as_u64()
            .unwrap();
        let result = read_ffi(yappers_netkit_refresh_request(
            scope,
            handle,
            c"{\"type\":\"feed\"}".as_ptr(),
        ));
        read_ffi(yappers_netkit_close(handle));
        result
    });
    let connection = endpoint.accept().await.unwrap().await.unwrap();
    let (_reply, mut incoming) = connection.accept_bi().await.unwrap();
    incoming
        .read_to_end(yappers_netkit::MAX_FRAME)
        .await
        .unwrap();
    yappers_netkit_end_refresh(scope);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["error"], "refresh expired");
    endpoint.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trusted_intermediary_does_not_grant_transitive_origin_trust_or_skip_a_bad_page() {
    let dir = tempfile::tempdir().unwrap();
    let community = "trust/test";
    let a_config = config(&dir.path().join("a.sqlite"), new_secret(), community);
    let a = Server::start(a_config.clone()).await.unwrap();
    let b_config = config(&dir.path().join("b.sqlite"), new_secret(), community);
    let b = Server::start(b_config.clone()).await.unwrap();
    let c_config = config(&dir.path().join("c.sqlite"), new_secret(), community);
    let c = Server::start(c_config.clone()).await.unwrap();
    let scope = BTreeSet::from([community.to_owned()]);
    let a_peer = PeerConfig {
        address: a.endpoint.addr(),
        communities: scope.clone(),
    };
    a.set_peers(vec![PeerConfig {
        address: b.endpoint.addr(),
        communities: scope.clone(),
    }])
    .unwrap();
    b.set_peers(vec![a_peer.clone()]).unwrap();
    let ca = client(&a, community, new_secret(), 1).await;
    ca.submit(
        ca.prepare(post("Valid A event in the same page"))
            .unwrap()
            .event,
    )
    .await
    .unwrap();
    // Model a compromised intermediary's local database, carrying a genuinely signed but untrusted origin.
    let key_c = signing_key(&c_config.private_key).unwrap();
    let author = signing_key(&new_secret()).unwrap();
    let foreign = Envelope::accept(
        SignedEvent::create(
            &author,
            c.endpoint.id().to_string(),
            community.into(),
            1,
            post("Unknown C origin"),
        )
        .unwrap(),
        &key_c,
    )
    .unwrap();
    Store::open(&a_config.database_path)
        .unwrap()
        .accept(&foreign)
        .unwrap();
    assert!(b.sync_peer(&a_peer).await.is_err());
    let stored = Store::open(&b_config.database_path).unwrap();
    assert!(stored.events(community).unwrap().is_empty());
    assert_eq!(
        stored.cursor(&a.endpoint.id().to_string()).unwrap(),
        (None, 0)
    );
    drop(stored);
    // An explicit operator trust decision permits that origin's retained history on the next retry.
    b.set_peers(vec![
        a_peer.clone(),
        PeerConfig {
            address: c.endpoint.addr(),
            communities: scope,
        },
    ])
    .unwrap();
    b.sync_peer(&a_peer).await.unwrap();
    let cb = client(&b, community, new_secret(), 1).await;
    assert_eq!(posts(&cb).await.len(), 2);
    ca.close().await;
    cb.close().await;
    a.close().await;
    b.close().await;
    c.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_log_generation_prevents_skipping_overlapping_cursors_after_restore() {
    let dir = tempfile::tempdir().unwrap();
    let community = "restore/test";
    let mut a_config = config(&dir.path().join("a.sqlite"), new_secret(), community);
    let a = Server::start(a_config.clone()).await.unwrap();
    let b = Server::start(config(
        &dir.path().join("b.sqlite"),
        new_secret(),
        community,
    ))
    .await
    .unwrap();
    let scope = BTreeSet::from([community.to_owned()]);
    a.set_peers(vec![PeerConfig {
        address: b.endpoint.addr(),
        communities: scope.clone(),
    }])
    .unwrap();
    b.set_peers(vec![PeerConfig {
        address: a.endpoint.addr(),
        communities: scope.clone(),
    }])
    .unwrap();
    let author = new_secret();
    let ca = client(&a, community, author.clone(), 1).await;
    ca.submit(ca.prepare(post("Before the home restore")).unwrap().event)
        .await
        .unwrap();
    b.sync_once().await.unwrap();
    ca.close().await;
    a.close().await;
    // Same stable home key, a restored/empty database, and overlapping local sequence numbers.
    a_config.database_path = dir.path().join("a-restored.sqlite");
    let restored = Server::start(a_config).await.unwrap();
    restored
        .set_peers(vec![PeerConfig {
            address: b.endpoint.addr(),
            communities: scope.clone(),
        }])
        .unwrap();
    b.set_peers(vec![PeerConfig {
        address: restored.endpoint.addr(),
        communities: scope,
    }])
    .unwrap();
    let ca2 = client(&restored, community, author, 2).await;
    ca2.submit(ca2.prepare(post("After the home restore")).unwrap().event)
        .await
        .unwrap();
    b.sync_once().await.unwrap();
    let cb = client(&b, community, new_secret(), 1).await;
    assert_eq!(posts(&cb).await.len(), 2);
    restored.sync_once().await.unwrap();
    assert_eq!(posts(&ca2).await.len(), 2);
    ca2.close().await;
    cb.close().await;
    restored.close().await;
    b.close().await;
}
