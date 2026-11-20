// SPDX-License-Identifier: MIT OR Apache-2.0
#![cfg(feature = "server")]
mod support;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use yappers_netkit::{
    JoinConfig, NetworkMode,
    attachments::{ATTACHMENT_CHUNK_BYTES, MAX_ATTACHMENT_BYTES},
    client::{Client, ClientConfig, Command},
    new_secret,
    portability::AccountWallet,
    server::{AutoFederationConfig, Server, ServerConfig},
};

struct Device {
    client: Client,
    wallet: AccountWallet,
    state: Value,
}

impl Device {
    async fn open(join: JoinConfig) -> Self {
        let wallet = AccountWallet::create(new_secret(), join.server_addr.id.to_string()).unwrap();
        let client = Client::open(ClientConfig {
            full_home_standby: None,
            join,
            author_secret: wallet.device_secret.clone(),
            next_sequence: wallet.next_sequence,
            account_chain: wallet.chain.clone(),
        })
        .await
        .unwrap();
        client.register_account(&wallet).await.unwrap();
        Self {
            client,
            wallet,
            state: Value::Null,
        }
    }

    async fn try_step(&mut self, action: Value) -> anyhow::Result<Value> {
        let command: Command = serde_json::from_value(json!({
            "type": "privateGroup", "state": self.state, "action": action
        }))?;
        let mut result = self.client.command(command).await?;
        self.state = result["state"].take();
        Ok(result["result"].take())
    }

    async fn step(&mut self, action: Value) -> Value {
        self.try_step(action).await.unwrap()
    }

    async fn view(&mut self, group: &Value) -> Value {
        self.step(json!({"kind":"open","group":group})).await
    }

    async fn prepare(&mut self, group: &Value, body: &str, parent: Option<&Value>) {
        self.step(json!({"kind":"prepare","group":group,"draft":{
            "author":"Neighbor", "headline":parent.is_none().then_some("Private headline"),
            "body":body, "parent":parent
        }}))
        .await;
    }

    async fn publish(&mut self, group: &Value) -> Value {
        self.step(json!({"kind":"publish","group":group})).await
    }
}

#[tokio::test]
async fn private_attachments_round_trip_at_the_limit() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("home.sqlite");
    let home = Server::start(ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: new_secret(),
        database_path: path.clone(),
        communities: BTreeMap::from([("home".into(), new_secret())]),
        peers: vec![],
        network_mode: NetworkMode::Local,
        auto_federation: AutoFederationConfig::default(),
    })
    .await?;
    let join = home.join("home", NetworkMode::Local)?;
    let mut alice = Device::open(join.clone()).await;
    let mut bob = Device::open(join.clone()).await;
    let outsider = Device::open(join).await;
    let bob_account = bob.wallet.validate()?.payload.account.clone();
    let filename = "confidential-filename-fixture.dat";
    let marker = "confidential-file-contents-fixture";
    let mut file = vec![0; MAX_ATTACHMENT_BYTES];
    for chunk in file.chunks_mut(ATTACHMENT_CHUNK_BYTES) {
        chunk[..marker.len()].copy_from_slice(marker.as_bytes());
    }
    assert!(
        alice
            .client
            .command(Command::AttachmentBegin {
                name: filename.into(),
                size: file.len() + 1,
            })
            .await
            .is_err()
    );
    let descriptor = alice
        .client
        .command(Command::AttachmentBegin {
            name: filename.into(),
            size: file.len(),
        })
        .await?;

    let upload = |part, bytes: &[u8]| Command::AttachmentUpload {
        attachment: serde_json::from_value(descriptor.clone()).unwrap(),
        part,
        bytes: STANDARD.encode(bytes),
    };
    let download = |descriptor: &Value, part| Command::AttachmentDownload {
        attachment: serde_json::from_value(descriptor.clone()).unwrap(),
        part,
    };
    let first = &file[..ATTACHMENT_CHUNK_BYTES];
    alice.client.command(upload(0, first)).await?;
    alice.client.command(upload(0, first)).await?;
    assert!(outsider.client.command(upload(0, first)).await.is_err());
    assert!(bob.client.command(download(&descriptor, 0)).await.is_err());
    for (part, bytes) in file.chunks(ATTACHMENT_CHUNK_BYTES).enumerate().skip(1) {
        alice.client.command(upload(part, bytes)).await?;
    }

    async fn direct(client: &Client, state: Value, action: Value) -> anyhow::Result<Value> {
        Ok(client
            .command(serde_json::from_value(json!({
                "type":"direct", "state":state, "action":action
            }))?)
            .await?["state"]
            .take())
    }
    let initialize = json!({"kind":"initialize"});
    let sync = json!({"kind":"sync"});
    let alice_dm = direct(&alice.client, Value::Null, initialize.clone()).await?;
    let alice_dm = direct(&alice.client, alice_dm, sync.clone()).await?;
    let bob_dm = direct(&bob.client, Value::Null, initialize).await?;
    let bob_dm = direct(&bob.client, bob_dm, sync.clone()).await?;
    let queued = direct(
        &alice.client,
        alice_dm,
        json!({
            "kind":"prepare", "peer":bob_account, "label":"Bob", "name":"Alice", "body":"",
            "attachment":descriptor
        }),
    )
    .await?;
    assert!(!queued["outbox"].to_string().contains(filename));
    direct(&alice.client, queued, sync.clone()).await?;
    let bob_dm = direct(&bob.client, bob_dm, sync).await?;
    let dm_file = &bob_dm["messages"][0]["attachment"];
    assert_eq!(dm_file, &descriptor);

    let group = alice
        .step(json!({"kind":"create","name":"Private group"}))
        .await;
    let invitation = alice.view(&group).await["invitation"].clone();
    bob.step(json!({"kind":"import","invitation":invitation}))
        .await;
    let request = bob.view(&group).await["joinRequest"].clone();
    alice
        .step(json!({"kind":"approve","group":group,"request":request}))
        .await;
    alice
        .step(json!({"kind":"prepare","group":group,"draft":{
            "author":"Alice","headline":"File","body":"","attachment":descriptor
        }}))
        .await;
    alice.publish(&group).await;
    let group_view = bob.view(&group).await;
    let group_file = &group_view["posts"][0]["attachment"];
    assert_eq!(group_file, &descriptor);

    // Only the recipients get the decryption key. Even with the identifier,
    // an authenticated outsider supplying another key cannot decrypt the file.
    let mut wrong_key = descriptor.clone();
    wrong_key["key"] = json!(new_secret());
    assert!(
        outsider
            .client
            .command(download(&wrong_key, 0))
            .await
            .is_err()
    );
    for (part, expected) in file.chunks(ATTACHMENT_CHUNK_BYTES).enumerate() {
        let received = if part == 0 { dm_file } else { group_file };
        let response = bob.client.command(download(received, part)).await?;
        assert_eq!(
            STANDARD.decode(response["bytes"].as_str().unwrap())?,
            expected
        );
    }
    alice.client.close().await;
    bob.client.close().await;
    outsider.client.close().await;
    home.close().await;
    support::assert_no_plaintext(
        &path,
        &[filename, marker, descriptor["key"].as_str().unwrap()],
    )?;
    Ok(())
}

#[tokio::test]
async fn private_groups_enforce_membership_history_and_rotate_pending_posts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home.sqlite");
    let home = Server::start(ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: new_secret(),
        database_path: path.clone(),
        communities: BTreeMap::from([("home".into(), new_secret())]),
        peers: vec![],
        network_mode: NetworkMode::Local,
        auto_federation: AutoFederationConfig::default(),
    })
    .await
    .unwrap();
    let join = home.join("home", NetworkMode::Local).unwrap();
    let mut owner = Device::open(join.clone()).await;
    let mut member = Device::open(join.clone()).await;
    let mut outsider = Device::open(join).await;
    let group = owner
        .step(json!({"kind":"create","name":"Reading group"}))
        .await;
    let invitation = owner.view(&group).await["invitation"].clone();
    let mut forged = serde_json::from_str::<Value>(invitation.as_str().unwrap()).unwrap();
    forged["value"]["name"] = json!("Substituted group");
    assert!(
        outsider
            .try_step(json!({"kind":"import","invitation": forged.to_string()}))
            .await
            .is_err()
    );

    // Historical ciphertext predating the current roster counts toward the old
    // 2,000-event limit. It is outside every current member's visible history.
    let db = support::primary(&path).await.unwrap();
    support::seed_range(
        &db,
        "INSERT INTO private_group_events(group_id,id,revision,session,ratchet_index,event)
         SELECT ?1,printf('%064x',value),0,'historical',value,'{}' FROM generate_series(?2,?3)",
        group.as_str().unwrap(),
        2_000,
    )
    .await
    .unwrap();
    drop(db);

    owner
        .prepare(&group, "pre-join confidential sample", None)
        .await;
    let first = owner.publish(&group).await;
    assert_eq!(
        owner.view(&group).await["posts"][0]["body"],
        "pre-join confidential sample"
    );

    for device in [&mut member, &mut outsider] {
        assert_eq!(
            device
                .step(json!({"kind":"import","invitation":invitation}))
                .await,
            group
        );
        assert_eq!(device.view(&group).await["phase"], "pending");
    }
    let request = member.view(&group).await["joinRequest"].clone();
    assert!(
        outsider
            .try_step(json!({"kind":"approve","group":group,"request":request}))
            .await
            .is_err()
    );

    // Prepare an event while offline from the roster update. A delayed upload
    // must never send the existing ratchet to the home.
    owner
        .prepare(&group, "new member can read this", None)
        .await;
    owner
        .step(json!({"kind":"approve","group":group,"request":request}))
        .await;
    assert!(
        owner
            .try_step(json!({"kind":"publish","group":group}))
            .await
            .is_err()
    );
    assert!(
        member.view(&group).await["posts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    owner.step(json!({"kind":"prepare","group":group})).await;
    let second = owner.publish(&group).await;
    let new_view = member.view(&group).await;
    assert_eq!(new_view["posts"].as_array().unwrap().len(), 1);
    assert_eq!(new_view["posts"][0]["id"], second);
    assert_eq!(new_view["posts"][0]["body"], "new member can read this");
    assert_eq!(outsider.view(&group).await["phase"], "pending");

    member
        .prepare(&group, "confidential reply sample", Some(&second))
        .await;
    let reply = member.publish(&group).await;
    assert_eq!(owner.view(&group).await["posts"][0]["id"], reply);
    assert_eq!(owner.view(&group).await["posts"][0]["parent"], second);

    // Simulate a lost receipt, then retry from the persisted state after another
    // membership change. The durable draft ID must still identify one event.
    owner.prepare(&group, "accepted before removal", None).await;
    let mut pending_state = owner.state.clone();
    let accepted = owner.publish(&group).await;
    let removed = member.client.author_id();
    owner
        .step(json!({"kind":"remove","group":group,"device":removed}))
        .await;
    pending_state["groups"][group.as_str().unwrap()]["tip"] =
        owner.state["groups"][group.as_str().unwrap()]["tip"].clone();
    owner.state = pending_state;
    owner.step(json!({"kind":"prepare","group":group})).await;
    assert_eq!(owner.publish(&group).await, accepted);
    owner
        .prepare(&group, "post-removal confidential sample", None)
        .await;
    let last = owner.publish(&group).await;
    assert_eq!(member.view(&group).await["phase"], "removed");
    assert!(
        member
            .try_step(json!({"kind":"prepare","group":group,"draft":{
                "author":"Neighbor", "headline":"Forbidden", "body":"Removed", "parent":null
            }}))
            .await
            .is_err()
    );

    let posts = owner.view(&group).await["posts"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        posts.iter().map(|p| p["id"].clone()).collect::<Vec<_>>(),
        vec![last, accepted, reply, second, first]
    );
    let db = support::primary(&path).await.unwrap();
    let latest: String = db.query("SELECT packet FROM private_group_sessions WHERE json_extract(packet,'$.value.roster') = (SELECT json_extract(event,'$.value.header.roster') FROM private_group_events ORDER BY cursor DESC LIMIT 1)", ()).await.unwrap().next().await.unwrap().unwrap().get(0).unwrap();
    drop(db);
    let packet: Value = serde_json::from_str(&latest).unwrap();
    assert!(packet["value"]["keys"].get(removed).is_none());
    owner.client.close().await;
    member.client.close().await;
    outsider.client.close().await;
    home.close().await;
    support::assert_no_plaintext(
        &path,
        &[
            "pre-join confidential sample",
            "confidential reply sample",
            "Private headline",
            "post-removal confidential sample",
        ],
    )
    .unwrap();
}

#[tokio::test]
async fn deleting_room_owner_preserves_other_members_encrypted_history() -> anyhow::Result<()> {
    use yappers_netkit::{Response, backup, store::Store};
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("home");
    let cfg = ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: new_secret(),
        database_path: path.clone(),
        communities: BTreeMap::from([("home".into(), new_secret())]),
        peers: vec![],
        network_mode: NetworkMode::Local,
        auto_federation: AutoFederationConfig::default(),
    };
    let home = Server::start(cfg.clone()).await?;
    let join = home.join("home", NetworkMode::Local)?;
    let mut owner = Device::open(join.clone()).await;
    let mut member = Device::open(join).await;
    let solo = owner
        .step(json!({"kind":"create","name":"Owner alone"}))
        .await;
    let shared = owner
        .step(json!({"kind":"create","name":"Shared room"}))
        .await;
    let invite = owner.view(&shared).await["invitation"].clone();
    member
        .step(json!({"kind":"import","invitation":invite}))
        .await;
    let request = member.view(&shared).await["joinRequest"].clone();
    owner
        .step(json!({"kind":"approve","group":shared,"request":request}))
        .await;
    owner.prepare(&shared, "owner post", None).await;
    let original = owner.publish(&shared).await;
    member
        .prepare(&shared, "member reply survives", Some(&original))
        .await;
    let reply = member.publish(&shared).await;
    let archive = dir.path().join("old.backup");
    Store::open(&path)?.backup(&archive)?;
    let intent = owner.client.prepare_account_deletion(&owner.wallet)?;
    let ack = owner.client.account_deletion_step(&intent, vec![]).await?;
    assert!(matches!(ack,Response::AccountDeleted{account} if account==intent.account));
    // The client re-downloads the ciphertext, signed roster and Olm session
    // envelope to decrypt this reply. Saved group state contains no post cache.
    let posts = member.view(&shared).await["posts"].clone();
    assert_eq!(posts.as_array().unwrap().len(), 1);
    assert_eq!(posts[0]["id"], reply);
    assert_eq!(posts[0]["body"], "member reply survives");
    let raw = support::primary(&path).await?;
    assert!(
        raw.query(
            "SELECT 1 FROM private_groups WHERE id=?",
            [solo.as_str().unwrap()]
        )
        .await?
        .next()
        .await?
        .is_none()
    );
    drop(raw);
    owner.client.close().await;
    member.client.close().await;
    home.close().await;

    let restored = dir.path().join("restored");
    backup::restore(
        &path.with_added_extension("deletion-guard"),
        &archive,
        &restored,
    )?;
    let home = Server::start(ServerConfig {
        database_path: restored,
        ..cfg
    })
    .await?;
    member.client = Client::open(ClientConfig {
        full_home_standby: None,
        join: home.join("home", NetworkMode::Local)?,
        author_secret: member.wallet.device_secret.clone(),
        next_sequence: member.wallet.next_sequence,
        account_chain: member.wallet.chain.clone(),
    })
    .await?;
    assert_eq!(member.view(&shared).await["posts"], posts);
    member.client.close().await;
    home.close().await;
    Ok(())
}
