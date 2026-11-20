#![cfg(feature = "server")]
mod support;
use std::{collections::BTreeMap, path::Path};
use yappers_netkit::{
    client::{Client, ClientConfig},
    model::*,
    portability::*,
    server::{PeerConfig, Server, ServerConfig},
    store::Store,
};

fn key(secret: &str) -> String {
    hex::encode(signing_key(secret).unwrap().verifying_key().as_bytes())
}
fn wallet(home: &str) -> AccountWallet {
    AccountWallet::create(new_secret(), home.into()).unwrap()
}
fn action(body: &str) -> Action {
    Action::PostV2 {
        area: None,
        headline: "Portable account".into(),
        body: body.into(),
        topic: "Chatter".into(),
        anonymous: false,
        display_name: "Portable neighbor".into(),
        profile: None,
    }
}
fn accepted(wallet: &AccountWallet, home_secret: &str, sequence: u64) -> Envelope {
    let record = wallet.validate().unwrap();
    Envelope::accept(
        SignedEvent::create_for_account(
            &signing_key(&wallet.device_secret).unwrap(),
            key(home_secret),
            "group".into(),
            sequence,
            action("My authored history"),
            record.clone(),
        )
        .unwrap(),
        &signing_key(home_secret).unwrap(),
    )
    .unwrap()
}
fn config(path: &Path, secret: String, community: &str) -> ServerConfig {
    ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: secret,
        database_path: path.into(),
        communities: BTreeMap::from([(community.into(), new_secret())]),
        peers: vec![],
        network_mode: NetworkMode::Local,
        auto_federation: Default::default(),
    }
}
async fn portable_client(server: &Server, community: &str, wallet: &AccountWallet) -> Client {
    Client::open(ClientConfig {
        full_home_standby: None,
        join: server.join(community, NetworkMode::Local).unwrap(),
        author_secret: wallet.device_secret.clone(),
        next_sequence: wallet.next_sequence,
        account_chain: wallet.chain.clone(),
    })
    .await
    .unwrap()
}

#[test]
fn identity_is_user_owned_and_device_counters_are_not_cloned() {
    let home = key(&new_secret());
    let account = wallet(&home);
    assert_eq!(
        account.validate().unwrap().payload.account,
        key(&account.root_secret)
    );
    assert_ne!(account.device_secret, account.root_secret);
    let second = account.transition(home.clone(), true).unwrap();
    assert_eq!(
        second.validate().unwrap().payload.account,
        account.validate().unwrap().payload.account
    );
    assert_eq!(second.validate().unwrap().payload.devices.len(), 2);
    assert_ne!(second.device_secret, account.device_secret);
    let destination = key(&new_secret());
    let moved = second.transition(destination.clone(), false).unwrap();
    assert_eq!(moved.validate().unwrap().payload.home, destination);
    assert_eq!(moved.validate().unwrap().payload.devices.len(), 1);
    assert!(
        !moved
            .validate()
            .unwrap()
            .payload
            .devices
            .contains(&key(&account.device_secret))
    );
    assert_eq!(moved.next_sequence, 1);
    assert_eq!(moved.chain.len(), 3);
    assert!(account.transition(key(&new_secret()), true).is_err());
}
#[test]
fn recovery_authority_extends_newer_public_state_without_reusing_lost_device() {
    let original = wallet(&key(&new_secret()));
    let moved = original.transition(key(&new_secret()), false).unwrap();
    let recovered = AccountWallet::recover(
        original.root_secret.clone(),
        moved.chain.clone(),
        moved.validate().unwrap().payload.home.clone(),
        false,
    )
    .unwrap();
    assert_eq!(recovered.chain.len(), 3);
    assert_eq!(
        recovered.validate().unwrap().payload.account,
        original.validate().unwrap().payload.account
    );
    assert_ne!(recovered.device_secret, moved.device_secret);
    assert_eq!(recovered.next_sequence, 1);
    assert!(AccountWallet::recover(new_secret(), moved.chain, key(&new_secret()), false).is_err());
}
#[test]
fn owned_history_export_spans_communities_without_exporting_other_authors() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("owned.sqlite")).unwrap();
    let home = new_secret();
    let account = wallet(&key(&home));
    for (sequence, community) in [(1, "one"), (2, "another-private-group")] {
        let event = SignedEvent::create_for_account(
            &signing_key(&account.device_secret).unwrap(),
            key(&home),
            community.into(),
            sequence,
            action("My own content"),
            account.validate().unwrap().clone(),
        )
        .unwrap();
        store
            .accept(&Envelope::accept(event, &signing_key(&home).unwrap()).unwrap())
            .unwrap();
    }
    store
        .accept(&accepted(&wallet(&key(&home)), &home, 1))
        .unwrap();
    let exported = store
        .owned_events(&account.validate().unwrap().payload.account)
        .unwrap();
    assert_eq!(exported.len(), 2);
    assert!(
        exported
            .iter()
            .all(|event| event.event.owner_id() == account.validate().unwrap().payload.account)
    );
}
#[test]
fn forged_forked_and_expired_authorizations_fail_closed() {
    let home = key(&new_secret());
    let account = wallet(&home);
    let moved = account.transition(key(&new_secret()), false).unwrap();
    let mut forged = moved.chain.clone();
    forged[1].payload.devices.insert(key(&new_secret()));
    assert!(validate_chain(&forged).is_err());
    let mut skipped = moved.chain.clone();
    skipped.remove(0);
    assert!(validate_chain(&skipped).is_err());
    let proof = AccountProof::create(&account, "export", &home, "digest").unwrap();
    proof
        .validate(account.validate().unwrap(), "export", &home, "digest")
        .unwrap();
    assert!(
        proof
            .validate(account.validate().unwrap(), "import", &home, "digest")
            .is_err()
    );
    assert!(
        proof
            .validate(
                account.validate().unwrap(),
                "export",
                &key(&new_secret()),
                "digest"
            )
            .is_err()
    );
    assert!(
        proof
            .validate(account.validate().unwrap(), "export", &home, "changed")
            .is_err()
    );
    assert!(
        proof
            .validate(moved.validate().unwrap(), "export", &home, "digest")
            .is_err()
    );
    let mut expired = proof;
    expired.timestamp -= 301;
    assert!(
        expired
            .validate(account.validate().unwrap(), "export", &home, "digest")
            .is_err()
    );
}
#[test]
fn immutable_owned_archives_reject_other_people_tampering_and_duplicates() {
    let home_secret = new_secret();
    let home = key(&home_secret);
    let account = wallet(&home);
    let event = accepted(&account, &home_secret, 1);
    let archive = PersonalArchive::create(
        &account,
        vec![event.clone()],
        serde_json::json!({"profile":{"pronouns":"they/them"}}),
    )
    .unwrap();
    archive.validate().unwrap();
    let mut tampered = archive.clone();
    tampered.payload.local_data = serde_json::json!({"changed":true});
    assert!(tampered.validate().is_err());
    let mut tampered = archive.clone();
    tampered.payload.events[0].accepted_at += 1;
    assert!(tampered.validate().is_err());
    let other = wallet(&home);
    let other_event = accepted(&other, &home_secret, 1);
    assert!(PersonalArchive::create(&account, vec![other_event], serde_json::Value::Null).is_err());
    assert!(
        PersonalArchive::create(
            &account,
            vec![event.clone(), event],
            serde_json::Value::Null
        )
        .is_err()
    );
}
#[test]
fn archive_encryption_wrong_passphrase_and_tamper_detection() {
    let home_secret = new_secret();
    let account = wallet(&key(&home_secret));
    let archive = PersonalArchive::create(
        &account,
        vec![accepted(&account, &home_secret, 1)],
        serde_json::json!({"bio":"My profile"}),
    )
    .unwrap();
    let bundle = RecoveryBundle {
        version: 1,
        wallet: account,
        archive,
    };
    let passphrase = "six violet comets find their own home";
    let encrypted = encrypt_recovery(&bundle, passphrase.into()).unwrap();
    assert!(!String::from_utf8_lossy(&encrypted).contains(&bundle.wallet.root_secret));
    assert!(!String::from_utf8_lossy(&encrypted).contains("My profile"));
    let recovered = decrypt_recovery(&encrypted, passphrase.into()).unwrap();
    assert_eq!(recovered.archive, bundle.archive);
    assert!(decrypt_recovery(&encrypted, "a different private passphrase".into()).is_err());
    let mut damaged = encrypted;
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    assert!(decrypt_recovery(&damaged, passphrase.into()).is_err());
    assert!(encrypt_recovery(&bundle, "short".into()).is_err());
}
#[test]
fn registry_rejects_rollbacks_forks_and_personal_import_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("db.sqlite")).unwrap();
    let home_secret = new_secret();
    let account = wallet(&key(&home_secret));
    let moved = account.transition(key(&new_secret()), false).unwrap();
    store.register_account(&account.chain).unwrap();
    store.register_account(&moved.chain).unwrap();
    assert_eq!(
        store.register_account(&account.chain).unwrap(),
        *moved.validate().unwrap()
    );
    let fork = account.transition(key(&new_secret()), false).unwrap();
    assert!(store.register_account(&fork.chain).is_err());
    let own = accepted(&account, &home_secret, 1);
    let other = accepted(&wallet(&key(&home_secret)), &home_secret, 1);
    assert!(
        store
            .import_personal_events(
                &account.validate().unwrap().payload.account,
                &[own.clone(), other]
            )
            .is_err()
    );
    assert!(
        store
            .owned_events(&account.validate().unwrap().payload.account)
            .unwrap()
            .is_empty()
    );
    store
        .import_personal_events(
            &account.validate().unwrap().payload.account,
            std::slice::from_ref(&own),
        )
        .unwrap();
    store
        .import_personal_events(
            &account.validate().unwrap().payload.account,
            std::slice::from_ref(&own),
        )
        .unwrap();
    assert_eq!(
        store
            .owned_events(&account.validate().unwrap().payload.account)
            .unwrap(),
        vec![own]
    );
    assert!(
        store.events("group").unwrap().is_empty(),
        "private imports must not enter public logs"
    );
}
#[test]
fn legacy_event_serialization_and_legacy_ownership_are_preserved() {
    let root = new_secret();
    let home_secret = new_secret();
    let event = SignedEvent::create(
        &signing_key(&root).unwrap(),
        key(&home_secret),
        "legacy".into(),
        1,
        action("Old signed post"),
    )
    .unwrap();
    assert!(
        !serde_json::to_string(&event)
            .unwrap()
            .contains("\"account\":")
    );
    assert_eq!(event.owner_id(), key(&root));
    let account = AccountWallet::create(root, key(&home_secret)).unwrap();
    let envelope = Envelope::accept(event, &signing_key(&home_secret).unwrap()).unwrap();
    PersonalArchive::create(&account, vec![envelope], serde_json::Value::Null).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_iroh_move_with_old_home_offline_preserves_history_and_revokes_old_devices() {
    let dir = tempfile::tempdir().unwrap();
    let a_config = config(&dir.path().join("a.sqlite"), new_secret(), "home-a/private");
    let b_config = config(&dir.path().join("b.sqlite"), new_secret(), "home-b/private");
    let a = Server::start(a_config).await.unwrap();
    let b = Server::start(b_config.clone()).await.unwrap();
    let root = new_secret();
    let legacy = Client::open(ClientConfig {
        full_home_standby: None,
        join: a.join("home-a/private", NetworkMode::Local).unwrap(),
        author_secret: root.clone(),
        next_sequence: 1,
        account_chain: vec![],
    })
    .await
    .unwrap();
    let old = legacy.prepare(action("Legacy owned post")).unwrap().event;
    legacy.submit(old.clone()).await.unwrap();
    let account = AccountWallet::create(root, a.endpoint.id().to_string()).unwrap();
    let client_a = portable_client(&a, "home-a/private", &account).await;
    client_a.register_account(&account).await.unwrap();
    assert!(
        legacy
            .submit(
                legacy
                    .prepare(action(
                        "Recovery authority cannot keep writing legacy posts"
                    ))
                    .unwrap()
                    .event
            )
            .await
            .is_err()
    );
    let first = client_a
        .prepare(action("Device-signed post"))
        .unwrap()
        .event;
    client_a.submit(first.clone()).await.unwrap();
    let original = client_a.export_owned(&account).await.unwrap();
    assert_eq!(original.len(), 2);
    let archive = PersonalArchive::create(
        &account,
        original.clone(),
        serde_json::json!({"profile":{"pronouns":"they/them"},"bookmarks":[old.id]}),
    )
    .unwrap();
    let encrypted = encrypt_recovery(
        &RecoveryBundle {
            version: 1,
            wallet: account.clone(),
            archive,
        },
        "a private portable recovery passphrase".into(),
    )
    .unwrap();
    client_a.close().await;
    legacy.close().await;
    a.close().await;
    let backup =
        decrypt_recovery(&encrypted, "a private portable recovery passphrase".into()).unwrap();
    let moved = backup
        .wallet
        .transition(b.endpoint.id().to_string(), false)
        .unwrap();
    let client_b = portable_client(&b, "home-b/private", &moved).await;
    client_b
        .import_owned(&moved, &backup.archive.payload.events)
        .await
        .unwrap();
    client_b
        .import_owned(&moved, &backup.archive.payload.events)
        .await
        .unwrap();
    assert_eq!(client_b.export_owned(&moved).await.unwrap(), original);
    let Response::Feed { posts, .. } = client_b.feed().await.unwrap() else {
        panic!("feed")
    };
    assert!(posts.is_empty());
    let new = client_b
        .prepare(action("New home, same account"))
        .unwrap()
        .event;
    assert_eq!(new.owner_id(), old.owner_id());
    assert_ne!(new.payload.author, first.payload.author);
    client_b.submit(new.clone()).await.unwrap();
    let Response::Feed { posts, .. } = client_b.feed().await.unwrap() else {
        panic!("feed")
    };
    assert!(posts[0].mine);
    let mut next = moved
        .transition(b.endpoint.id().to_string(), false)
        .unwrap();
    let next_client = portable_client(&b, "home-b/private", &next).await;
    next_client.register_account(&next).await.unwrap();
    assert!(
        client_b
            .submit(client_b.prepare(action("Revoked device")).unwrap().event)
            .await
            .is_err()
    );
    client_b.submit(new).await.unwrap(); // Lost old receipts remain safely acknowledgeable.
    next_client.close().await;
    client_b.close().await;
    b.close().await;
    let restarted = Server::start(b_config).await.unwrap();
    let client = portable_client(&restarted, "home-b/private", &next).await;
    assert_eq!(
        client.lookup_account(old.owner_id()).await.unwrap(),
        next.chain
    );
    assert_eq!(client.export_owned(&next).await.unwrap().len(), 3);
    let prepared = client.prepare(action("After restart")).unwrap();
    next.next_sequence = prepared.next_sequence;
    client.submit(prepared.event).await.unwrap();
    assert_eq!(client.export_owned(&next).await.unwrap().len(), 4);
    client.close().await;
    restarted.close().await;
}

use serde_json::{Value, json};
async fn command(c: &Client, args: Value) -> anyhow::Result<Value> {
    c.command(serde_json::from_value(args)?).await
}
async fn step(c: &Client, state: &Value, action: Value) -> Value {
    command(c, json!({"type":"direct","state":state,"action":action}))
        .await
        .unwrap()["state"]
        .take()
}

#[tokio::test]
async fn private_mailbox_ratchets_offline_and_enforces_account_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let secret = new_secret();
    let home = key(&secret);
    let server = Server::start(config(&tmp.path().join("home.db"), secret, "test"))
        .await
        .unwrap();
    let alice = wallet(&home);
    let bob = wallet(&home);
    let a = portable_client(&server, "test", &alice).await;
    let b = portable_client(&server, "test", &bob).await;
    let aid = &alice.validate().unwrap().payload.account;
    let bid = &bob.validate().unwrap().payload.account;
    a.feed().await.unwrap();
    b.feed().await.unwrap();
    let named = a.prepare(action("Public hello")).unwrap().event;
    a.submit(named.clone()).await.unwrap();
    let anon = a
        .prepare(Action::Post {
            body: "Another hello".into(),
            topic: "Chatter".into(),
            display_name: "Anonymous neighbor".into(),
            anonymous: true,
        })
        .unwrap()
        .event;
    a.submit(anon.clone()).await.unwrap();
    let contact = |id: &str| json!({"type":"social","op":{"kind":"contact","event":id}});
    assert_eq!(command(&b, contact(&named.id)).await.unwrap()["peer"], *aid);
    assert!(command(&b, contact(&anon.id)).await.is_err());
    let init = json!({"kind":"initialize"});
    let sync = json!({"kind":"sync"});
    let mut sa = step(&a, &Value::Null, init.clone()).await;
    let mut sb = step(&b, &Value::Null, init).await;
    sa = step(&a, &sa, sync.clone()).await;
    sb = step(&b, &sb, sync.clone()).await;
    let send = |peer: &str, name: &str, body: &str| json!({"kind":"prepare","peer":peer,"label":"Friend","name":name,"body":body});
    sa = step(&a, &sa, send(bid, "Alice", "Private greeting")).await;
    assert!(!sa["outbox"].to_string().contains("Private greeting"));
    // Offline preparation keeps the ratchet and all queued packets past sixteen sends.
    let mut offline = sa.clone();
    for _ in 0..16 {
        offline = step(&a, &offline, send(bid, "Alice", "Queued offline")).await;
    }
    assert_eq!(offline["outbox"].as_array().unwrap().len(), 17);
    let before_ack = sa.clone();
    sa = step(&a, &sa, sync.clone()).await;
    step(&a, &before_ack, sync.clone()).await;
    sb = step(&b, &sb, sync.clone()).await;
    assert_eq!(sb["messages"][0]["body"], "Private greeting");
    assert_eq!(sb["messages"].as_array().unwrap().len(), 1);
    sb = step(&b, &sb, send(aid, "Bob", "Private reply")).await;
    sb = step(&b, &sb, sync.clone()).await;
    sa = step(&a, &sa, sync.clone()).await;
    assert_eq!(sa["messages"][1]["body"], "Private reply");
    sa = step(&a, &sa, send(bid, "Alice", "pending before block")).await;
    command(
        &b,
        json!({"type":"social","op":{"kind":"block","target":{"event":anon.id}}}),
    )
    .await
    .unwrap();
    sa = step(&a, &sa, sync.clone()).await;
    assert_eq!(sa["messages"][2]["status"], "unavailable");
    sb = step(&b, &sb, sync.clone()).await;
    assert_eq!(sb["messages"].as_array().unwrap().len(), 2);
    let Response::Feed { posts, .. } = b.feed().await.unwrap() else {
        panic!()
    };
    assert!(posts.is_empty());
    let blocks = command(&b, json!({"type":"social","op":{"kind":"blocks"}}))
        .await
        .unwrap();
    assert!(!blocks.to_string().contains(aid));
    command(
        &b,
        json!({"type":"social","op":{"kind":"unblock","id":blocks["blocks"][0]["id"]}}),
    )
    .await
    .unwrap();
    let Response::Feed { posts, .. } = b.feed().await.unwrap() else {
        panic!()
    };
    assert_eq!(posts.len(), 2);
    a.close().await;
    b.close().await;
    server.close().await;
}

async fn privacy_op(client: &Client, op: Value) -> Value {
    command(client, json!({"type":"social","op":op}))
        .await
        .unwrap()
}

#[tokio::test]
async fn privacy_filters_authors_and_preserves_requests_across_cursor_advances() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("privacy.db");
    let secret = new_secret();
    let home = key(&secret);
    let server = Server::start(config(&path, secret, "test")).await.unwrap();
    let wallets = [wallet(&home), wallet(&home), wallet(&home)];
    let mut clients = Vec::new();
    let mut states = Vec::new();
    let sync = json!({"kind":"sync"});
    for w in &wallets {
        let c = portable_client(&server, "test", w).await;
        c.feed().await.unwrap();
        let initial = step(&c, &Value::Null, json!({"kind":"initialize"})).await;
        states.push(step(&c, &initial, sync.clone()).await);
        clients.push(c);
    }
    let [a, b, c] = clients.as_slice() else {
        unreachable!()
    };
    let aid = wallets[0].validate().unwrap().payload.account.as_str();
    let bid = wallets[1].validate().unwrap().payload.account.as_str();
    let cid = wallets[2].validate().unwrap().payload.account.as_str();
    let named = a.prepare(action("Named post")).unwrap().event;
    a.submit(named.clone()).await.unwrap();
    let anon = a
        .prepare(Action::Reply {
            parent: named.id.clone(),
            body: "Anonymous reply".into(),
            display_name: "Anonymous neighbor".into(),
        })
        .unwrap()
        .event;
    a.submit(anon.clone()).await.unwrap();
    let feed = command(b, json!({"type":"feed"})).await.unwrap();
    assert_eq!(feed["posts"][0]["visibility"]["peer"], aid);
    let reply = &feed["posts"][0]["replies"][0];
    assert_eq!(reply["visibility"]["anonymous"], true);
    assert!(reply["visibility"]["peer"].is_null());
    assert!(
        command(
            b,
            json!({"type":"social","op":{"kind":"contact","event":anon.id}})
        )
        .await
        .is_err()
    );
    let prefs = |dm_days| json!({"kind":"setPrivacy","settings":{"hideAnonymous":true,"feedDays":1,"dmDays":dm_days,"approvedOnly":false}});
    privacy_op(b, prefs(7)).await;
    assert_eq!(
        command(b, json!({"type":"feed"})).await.unwrap()["posts"],
        json!([])
    );
    let db = support::primary(&path).await.unwrap();
    db.execute(
        "UPDATE first_seen SET since=since-86401 WHERE owner=?",
        [aid],
    )
    .await
    .unwrap();
    let feed = command(b, json!({"type":"feed"})).await.unwrap();
    assert_eq!(feed["posts"][0]["visibility"]["age"], 1);
    assert_eq!(feed["posts"][0]["replies"], json!([]));
    let send =
        |body| json!({"kind":"prepare","peer":bid,"name":"Sender","label":"Friend","body":body});
    let queued = step(a, &states[0], send("Held first")).await;
    states[0] = step(a, &queued, sync.clone()).await;
    step(a, &queued, sync.clone()).await;
    states[1] = step(b, &states[1], sync.clone()).await;
    assert_eq!(states[1]["messages"], json!([]));
    assert_eq!(
        privacy_op(b, json!({"kind":"privacyStatus"})).await["requests"],
        json!([aid])
    );
    // Relaxing the age gate must not split a pending conversation across two inboxes.
    privacy_op(b, prefs(0)).await;
    let queued = step(a, &states[0], send("Same pending request")).await;
    states[0] = step(a, &queued, sync.clone()).await;
    states[1] = step(b, &states[1], sync.clone()).await;
    assert_eq!(states[1]["messages"], json!([]));
    privacy_op(b, json!({"kind":"approve","peer":cid})).await;
    let queued = step(c, &states[2], send("Delivered second")).await;
    step(c, &queued, sync.clone()).await;
    states[1] = step(b, &states[1], sync.clone()).await;
    assert_eq!(states[1]["messages"][0]["body"], "Delivered second");
    privacy_op(b, json!({"kind":"approve","peer":aid})).await;
    states[1] = step(b, &states[1], sync.clone()).await;
    assert_eq!(states[1]["messages"][1]["body"], "Held first");
    assert_eq!(states[1]["messages"][2]["body"], "Same pending request");
    assert_eq!(states[1]["messages"].as_array().unwrap().len(), 3);
    // Exercise the device block while the home still permits deliveries from this sender.
    let queued = step(a, &states[0], send("Blocked on the device")).await;
    step(a, &queued, sync.clone()).await;
    states[1] = step(b, &states[1], json!({"kind":"sync","blocked":[aid]})).await;
    states[1] = step(b, &states[1], sync.clone()).await;
    assert_eq!(states[1]["messages"].as_array().unwrap().len(), 3);
    assert_eq!(
        privacy_op(b, json!({"kind":"privacyStatus"})).await["requests"],
        json!([])
    );
    for client in clients {
        client.close().await;
    }
    server.close().await;
}

/// Opt-in deployment check. Uses two fresh accounts and never posts to a public feed.
#[tokio::test]
#[ignore = "requires YAP_SMOKE_ENROLLMENT pointing to a deployed public enrollment document"]
async fn deployed_home_private_exchange() -> anyhow::Result<()> {
    let manifest: Value =
        serde_json::from_reader(std::fs::File::open(std::env::var("YAP_SMOKE_ENROLLMENT")?)?)?;
    let join: JoinConfig = serde_json::from_value(manifest["join"].clone())?;
    let open = async || -> anyhow::Result<(Client, String)> {
        let wallet = wallet(&join.server_addr.id.to_string());
        let id = wallet.validate()?.payload.account.clone();
        let c = Client::open(ClientConfig {
            full_home_standby: None,
            join: join.clone(),
            author_secret: wallet.device_secret,
            next_sequence: wallet.next_sequence,
            account_chain: wallet.chain,
        })
        .await?;
        anyhow::ensure!(
            matches!(c.feed().await?, Response::Feed { direct: true, .. }),
            "home has no messaging capability"
        );
        Ok((c, id))
    };
    let (a, aid) = open().await?;
    let (b, bid) = open().await?;
    let sync = json!({"kind":"sync"});
    let send = |peer: &str, body: &str| json!({"kind":"prepare","peer":peer,"name":"Deployment check","label":"Test contact","body":body});
    let mut sa = step(&a, &Value::Null, json!({"kind":"initialize"})).await;
    let mut sb = step(&b, &Value::Null, json!({"kind":"initialize"})).await;
    sa = step(&a, &sa, sync.clone()).await;
    sb = step(&b, &sb, sync.clone()).await;
    privacy_op(&b,json!({"kind":"setPrivacy","settings":{"hideAnonymous":true,"feedDays":1,"dmDays":1,"approvedOnly":true}})).await;
    sa = step(&a, &sa, send(&bid, "Encrypted delivery check")).await;
    sa = step(&a, &sa, sync.clone()).await;
    sb = step(&b, &sb, sync.clone()).await;
    assert_eq!(sb["messages"], json!([]));
    assert_eq!(
        privacy_op(&b, json!({"kind":"privacyStatus"})).await["requests"],
        json!([aid])
    );
    privacy_op(&b, json!({"kind":"approve","peer":aid})).await;
    sb = step(&b, &sb, sync.clone()).await;
    assert_eq!(sb["messages"][0]["body"], "Encrypted delivery check");
    sb = step(&b, &sb, send(&aid, "Reply received")).await;
    step(&b, &sb, sync.clone()).await;
    sa = step(&a, &sa, sync.clone()).await;
    assert_eq!(sa["messages"][1]["body"], "Reply received");
    command(
        &b,
        json!({"type":"social","op":{"kind":"block","target":{"account":aid}}}),
    )
    .await?;
    sa = step(&a, &sa, send(&bid, "Blocked delivery check")).await;
    sa = step(&a, &sa, sync).await;
    assert_eq!(sa["messages"][2]["status"], "unavailable");
    a.close().await;
    b.close().await;
    Ok(())
}

/// Companion for the iOS UI test. This fixture can only publish to a local demo home.
#[tokio::test]
#[ignore = "started by NETKIT_TEST_SCOPE=messaging scripts/test-netkit-ios.sh"]
async fn ios_messaging_fixture() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(std::env::var("YAP_IOS_FIXTURE")?);
    let join: JoinConfig =
        serde_json::from_reader(std::fs::File::open(directory.join("join-a.json"))?)?;
    anyhow::ensure!(
        matches!(join.network_mode, NetworkMode::Local),
        "requires a local test home"
    );
    let w = wallet(&join.server_addr.id.to_string());
    let client = Client::open(ClientConfig {
        full_home_standby: None,
        join: join.clone(),
        author_secret: w.device_secret,
        next_sequence: 1,
        account_chain: w.chain,
    })
    .await?;
    client.feed().await?;
    let sync = json!({"kind":"sync"});
    let mut state = step(&client, &Value::Null, json!({"kind":"initialize"})).await;
    state = step(&client, &state, sync.clone()).await;
    command(&client,json!({"type":"social","op":{"kind":"setProfile","name":"DM test neighbor","expected":0,"profile":CommunityProfile {
            photo: "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAkElEQVR4nO3RwQ2AIAyFYQZxO28cHMjEERzGqxO4hTYxMVgLLbQciCT/kb7vgJu3o2quA80D5+7v7IFnOqwdgFwnjd8CGX8wrQtUHdAzsQcvoMxIhwFzhgYMjShgwsACA2iM+5wHCpjwUArIDXTlhtGHaRjyPQYknnwdFhgg4ZHT35M8AAXr7BsVIKkDHdB3AeNMX2gQjeJ6AAAAAElFTkSuQmCC".into(),
            ..Default::default()
        }}})).await?;
    let post = client.prepare(Action::PostV2 {
        headline: "Hello from a neighbor".into(),
        area: None,
        profile: None,
        body: "Say hello to the messaging fixture.".into(),
        topic: "Chatter".into(),
        display_name: "DM test neighbor".into(),
        anonymous: false,
    })?;
    client.submit(post.event).await?;
    let requester_wallet = wallet(&join.server_addr.id.to_string());
    let requester = Client::open(ClientConfig {
        full_home_standby: None,
        join,
        author_secret: requester_wallet.device_secret,
        next_sequence: 1,
        account_chain: requester_wallet.chain,
    })
    .await?;
    requester.feed().await?;
    let rq = step(&requester, &Value::Null, json!({"kind":"initialize"})).await;
    let rq = step(&requester, &rq, sync.clone()).await;
    std::fs::write(directory.join("dm-peer-ready"), "")?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(600), async {
        loop {
            state = step(&client, &state, sync.clone()).await;
            if let Some(message) = state["messages"].as_array().unwrap().iter().find(|m| m["body"] == "Hello from iOS") {
                let to = message["peer"].as_str().unwrap().to_owned();
                state = step(&client, &state, json!({"kind":"prepare","peer":to,"label":"iOS","name":"DM test neighbor","body":"Received your encrypted message"})).await;
                state = step(&client, &state, sync.clone()).await;
                assert_eq!(state["messages"][1]["status"], "sent");
                let held = step(&requester, &rq, json!({"kind":"prepare","peer":to,"label":"iOS","name":"New neighbor","body":"Hello after approval"})).await;
                let sent = step(&requester,&held,sync.clone()).await;
                assert_eq!(sent["messages"][0]["status"],"sent");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }).await;
    requester.close().await;
    client.close().await;
    result?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotated_device_can_erase_but_revoked_device_cannot() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let secret = new_secret();
    let id = key(&secret);
    let home = Server::start(config(
        &dir.path().join("home.db"),
        secret.clone(),
        "nearby",
    ))
    .await?;
    let old = wallet(&id);
    let client = portable_client(&home, "nearby", &old).await;
    let post = client.prepare(action("Created before rotation"))?.event;
    client.submit(post.clone()).await?;
    let recovered = client
        .prepare(action("Deleted in recovered history"))?
        .event;
    client.submit(recovered.clone()).await?;
    let next = old.transition(id, false)?;
    let replacement = portable_client(&home, "nearby", &next).await;
    replacement.register_account(&next).await?;
    let delete = Action::Delete {
        target: post.id.clone(),
    };
    assert!(
        client
            .submit(client.prepare(delete.clone())?.event)
            .await
            .is_err()
    );
    replacement
        .submit(replacement.prepare(delete)?.event)
        .await?;
    let accepted_delete = Envelope::accept(
        replacement
            .prepare(Action::Delete {
                target: recovered.id.clone(),
            })?
            .event,
        &signing_key(&secret)?,
    )?;
    replacement.import_owned(&next, &[accepted_delete]).await?;
    let Response::Feed { posts, .. } = replacement.feed().await? else {
        unreachable!()
    };
    assert!(posts.iter().all(|p| p.id != recovered.id));
    let result = replacement.export_owned(&next).await?;
    let retained = result.iter().find(|e| e.event.id == post.id).unwrap();
    retained.validate()?;
    assert_eq!(
        retained.event.payload.account.as_deref(),
        Some(old.validate()?)
    );
    assert_eq!(
        retained
            .deletion
            .as_ref()
            .unwrap()
            .event
            .payload
            .account
            .as_deref(),
        Some(next.validate()?)
    );
    client.close().await;
    replacement.close().await;
    home.close().await;
    Ok(())
}

#[tokio::test]
async fn historical_bylines_follow_account_edits_across_devices_and_homes() -> anyhow::Result<()> {
    use std::collections::BTreeSet;
    let dir = tempfile::tempdir()?;
    let community = "shared";
    let home = Server::start(config(&dir.path().join("home"), new_secret(), community)).await?;
    let mirror = Server::start(config(&dir.path().join("mirror"), new_secret(), community)).await?;
    for (a, b) in [(&home, &mirror), (&mirror, &home)] {
        a.set_peers(vec![PeerConfig {
            address: b.endpoint.addr(),
            communities: BTreeSet::from([community.into()]),
        }])?;
    }
    let id = home.endpoint.id().to_string();
    let first = wallet(&id);
    let author = portable_client(&home, community, &first).await;
    let reader = portable_client(&home, community, &wallet(&id)).await;
    let remote = portable_client(
        &mirror,
        community,
        &wallet(&mirror.endpoint.id().to_string()),
    )
    .await;
    let edit = |expected: u64, name: &str, bio: &str| {
        json!({"type":"social","op":{"kind":"setProfile",
        "expected":expected,"name":name,"profile":CommunityProfile{bio:bio.into(),..Default::default()}}})
    };
    author.feed().await?;
    reader.feed().await?;
    assert_eq!(
        command(&author, edit(0, "First name", "Old bio")).await?["revision"],
        1
    );
    let post = author.prepare(action("Keep this post"))?.event;
    let post_id = post.id.clone();
    author.submit(post).await?;
    for display_name in ["Neighbor", "Anonymous neighbor"] {
        author
            .submit(
                author
                    .prepare(Action::Reply {
                        parent: post_id.clone(),
                        body: display_name.into(),
                        display_name: display_name.into(),
                    })?
                    .event,
            )
            .await?;
    }
    mirror.sync_once().await?;
    let second = first.transition(id, true)?;
    let another_device = portable_client(&home, community, &second).await;
    another_device.feed().await?;
    assert_eq!(
        command(&another_device, edit(1, "Current name", "")).await?["revision"],
        2
    );
    assert_eq!(
        command(&another_device, edit(1, "Current name", "")).await?["revision"],
        2
    );
    assert!(
        command(&author, edit(1, "Stale draft", "Old bio"))
            .await
            .is_err()
    );
    command(&reader, edit(0, "Reader", "Separate account")).await?;
    mirror.sync_once().await?;
    for c in [&reader, &remote] {
        let feed = command(c, json!({"type":"feed"})).await?;
        let row = &feed["posts"][0];
        assert_eq!(row["id"], post_id);
        assert_eq!(row["author"], "Current name");
        assert_eq!(row["profile"]["bio"], "");
        for reply in row["replies"].as_array().unwrap() {
            if reply["body"] == "Anonymous neighbor" {
                assert!(
                    reply["visibility"]["peer"].is_null()
                        && reply["profile"].is_null()
                        && reply["username"].is_null()
                );
            } else {
                assert_eq!(reply["author"], "Current name");
                assert_eq!(reply["profile"]["bio"], "");
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn account_inbox_groups_replies_and_monotonically_reconciles_reads() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let home = Server::start(config(&dir.path().join("home"), new_secret(), "public")).await?;
    let origin = home.endpoint.id().to_string();
    let alice = portable_client(&home, "public", &wallet(&origin)).await;
    let bob = portable_client(&home, "public", &wallet(&origin)).await;
    alice.feed().await?;
    bob.feed().await?;
    let post = alice.prepare(action("First thread"))?.event;
    let target = post.id.clone();
    alice.submit(post).await?;
    let reply = || Action::Reply {
        parent: target.clone(),
        body: "Hello".into(),
        display_name: "Anonymous neighbor".into(),
    };
    alice.submit(alice.prepare(reply())?.event).await?;
    let inbox = json!({"type":"social","op":{"kind":"notificationInbox"}});
    assert_eq!(command(&alice, inbox.clone()).await?["activity"], json!([]));
    let first = bob.prepare(reply())?.event;
    let first_id = first.id.clone();
    bob.submit(first.clone()).await?;
    bob.submit(first).await?;
    let row = command(&alice, inbox.clone()).await?["activity"][0].clone();
    assert_eq!(row["first"], first_id);
    assert_eq!(row["unread"], 1);
    assert!(row.get("actor").is_none());
    assert_eq!(row["byline"], "Anonymous neighbor");
    assert_eq!(row["preview"], "Hello");
    assert_eq!(row["anonymous"], true);
    assert!(row["photo"].is_null());
    assert!(row["created"].as_i64().unwrap() > 0);
    let second = bob.prepare(reply())?.event;
    let second_id = second.id.clone();
    bob.submit(second).await?;
    assert_eq!(
        command(&alice, inbox.clone()).await?["activity"][0]["unread"],
        2
    );
    let read = |through: &Value| json!({"type":"social","op":{"kind":"readNotification","category":"reply","target":target,"through":through}});
    command(&alice, read(&row["through"])).await?;
    let next = command(&alice, inbox.clone()).await?["activity"][0].clone();
    assert_eq!(next["unread"], 1);
    assert_eq!(next["first"], second_id);
    command(&alice, read(&next["through"])).await?;
    command(&alice, read(&row["through"])).await?;
    assert_eq!(
        command(&alice, inbox.clone()).await?["activity"][0]["unread"],
        0
    );
    assert_eq!(command(&bob, inbox.clone()).await?["activity"], json!([]));
    let mute = |muted| json!({"type":"social","op":{"kind":"muteNotifications","target":target,"muted":muted}});
    command(&alice, mute(true)).await?;
    assert_eq!(command(&alice, inbox.clone()).await?["activity"], json!([]));
    command(&alice, mute(false)).await?;
    alice
        .submit(alice.prepare(Action::Delete { target })?.event)
        .await?;
    assert_eq!(command(&alice, inbox).await?["activity"], json!([]));
    Ok(())
}

#[test]
fn recovery_reads_previous_age_release() -> anyhow::Result<()> {
    let bundle = decrypt_recovery(
        include_bytes!("fixtures/recovery-age-0.12.1.age"),
        "public test fixture".into(),
    )?;
    assert_eq!(bundle.wallet.root_secret, "01".repeat(32));
    bundle.validate()
}

#[tokio::test]
async fn existing_registry_accepts_account_1001() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("home");
    let mut store = Store::open(&path)?;
    let db = support::primary(&path).await?;
    db.execute_batch(
        "WITH RECURSIVE seed(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM seed WHERE i<1000)
        INSERT INTO account_states SELECT printf('%064x',i),'[]' FROM seed",
    )
    .await?;
    let account = wallet(&key(&new_secret()));
    store.register_account(&account.chain)?;
    assert_eq!(
        store.account_chain(&account.chain[0].payload.account)?,
        account.chain
    );
    Ok(())
}

#[test]
fn competing_account_updates_accept_one_chain() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("home");
    let mut first = Store::open(&path)?;
    let mut second = Store::open(&path)?;
    let home = key(&new_secret());
    let original = wallet(&home);
    first.register_account(&original.chain)?;
    let a = original.transition(home.clone(), false)?;
    let b = original.transition(home, false)?;
    let ready = std::sync::Barrier::new(2);
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            ready.wait();
            first.register_account(&a.chain)
        });
        ready.wait();
        let right = second.register_account(&b.chain);
        (left.join().unwrap(), right)
    });
    let (accepted, rejected) = match (left, right) {
        (Ok(record), Err(error)) | (Err(error), Ok(record)) => (record, error),
        _ => anyhow::bail!("competing branches must have one winner"),
    };
    assert!(rejected.to_string().contains("conflicting account history"));
    assert_eq!(
        second.account_chain(&accepted.payload.account)?.last(),
        Some(&accepted)
    );
    Ok(())
}
