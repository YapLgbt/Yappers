//! Cut power to real Iroh home processes; keep device ratchets alive across the transition.
#![cfg(unix)]
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use yappers_netkit::{
    client::{Client, ClientConfig},
    model::*,
    portability::AccountWallet,
    takeover::FullHomeStandby,
};

struct Processes(Vec<Child>, PathBuf);
impl Drop for Processes {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.1);
    }
}
fn json_file(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    fs::write(path, serde_json::to_vec(value)?)?;
    Ok(())
}
fn key(secret: &str) -> String {
    hex::encode(signing_key(secret).unwrap().verifying_key().as_bytes())
}
impl Processes {
    async fn launch(&mut self, dir: &Path, extra: &[&str]) -> Result<(usize, Value)> {
        let addr = dir.join("ha-address.json");
        if addr.exists() {
            fs::remove_file(&addr)?;
        }
        let child = Command::new(env!("CARGO_BIN_EXE_yappers-server"))
            .current_dir(dir)
            .args(["ha-run", "--config", "config.json", "--grant", "grant.json"])
            .env("YAP_REPORT_INBOX", dir.join("reports.sqlite"))
            .args(extra)
            .stdout(Stdio::null())
            .spawn()?;
        self.0.push(child);
        let index = self.0.len() - 1;
        let address = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                if addr.exists() {
                    return anyhow::Ok(serde_json::from_slice::<Value>(&fs::read(&addr)?)?);
                }
                ensure!(
                    self.0[index].try_wait()?.is_none(),
                    "HA process exited before readiness"
                );
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        })
        .await??;
        Ok((index, address))
    }
    fn crash(&mut self, index: usize) -> Result<()> {
        self.0[index].kill()?;
        self.0[index].wait()?;
        Ok(())
    }
}
async fn call(c: &Client, command: Value) -> Result<Value> {
    c.command(serde_json::from_value(command)?).await
}
async fn direct(c: &Client, state: &Value, action: Value) -> Result<Value> {
    Ok(call(c, json!({"type":"direct","state":state,"action":action})).await?["state"].take())
}
async fn device(
    join: &JoinConfig,
    grant: Option<FullHomeStandby>,
    wallet: &AccountWallet,
) -> Result<Client> {
    Client::open(ClientConfig {
        full_home_standby: grant,
        join: join.clone(),
        author_secret: wallet.device_secret.clone(),
        next_sequence: wallet.next_sequence,
        account_chain: wallet.chain.clone(),
    })
    .await
}
fn note(body: &str) -> Action {
    Action::Post {
        display_name: "Neighbor".into(),
        anonymous: false,
        body: body.into(),
        topic: "Chatter".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn encrypted_mail_and_private_policy_survive_crash_partition_and_failback() -> Result<()> {
    let base = std::env::temp_dir().join(format!("yap-ha-{}", new_secret()));
    let p = base.join("dedicated");
    let s = base.join("fly");
    fs::create_dir_all(&p)?;
    fs::create_dir(&s)?;
    let mut processes = Processes(vec![], base);
    let owner_secret = new_secret();
    let backup_secret = new_secret();
    let home = key(&owner_secret);
    let scope = format!("{home}/public");
    let token = new_secret();
    let standby_id = key(&backup_secret);
    let signer = signing_key(&owner_secret)?;
    let mut grant = FullHomeStandby::issue(
        &signer,
        serde_json::from_value(json!({"id":standby_id,"addrs":[]}))?,
    )?;
    for d in [&p, &s] {
        json_file(
            &d.join("config.json"),
            &json!({"privateKey":owner_secret,"communities":{&scope:&token},"databasePath":d.join("stage.db")}),
        )?;
        json_file(&d.join("grant.json"), &grant)?;
    }
    json_file(
        &s.join("node.json"),
        &json!({"privateKey":backup_secret,"communities":{&scope:&token},"databasePath":s.join("unused.db")}),
    )?;
    let fly_args = ["--standby-config", "node.json", "--ledger", "authority.db"];
    let (fly, backup) = processes.launch(&s, &fly_args).await?;
    grant.standby = serde_json::from_value(backup)?;
    json_file(&p.join("grant.json"), &grant)?;
    // A fresh test home starts with a genuine initialized SQLite store.
    yappers_netkit::store::Store::open(&p.join("stage.db"))?;
    let seed = || {
        Command::new(env!("CARGO_BIN_EXE_yappers-server"))
            .current_dir(&p)
            .args([
                "ha-bootstrap",
                "--config",
                "config.json",
                "--grant",
                "grant.json",
            ])
            .status()
    };
    ensure!(seed()?.success(), "initial seed failed");
    ensure!(!seed()?.success(), "second seed must be denied");
    let (primary, primary_addr) = processes.launch(&p, &[]).await?;
    let mut join: JoinConfig = serde_json::from_value(
        json!({"protocolVersion":4,"community":scope,"accessToken":token,"networkMode":"local","serverAddr":primary_addr}),
    )?;
    let alice = AccountWallet::create(new_secret(), home.clone())?;
    let bob = AccountWallet::create(new_secret(), home.clone())?;
    let a = device(&join, Some(grant.clone()), &alice).await?;
    let b = device(&join, Some(grant.clone()), &bob).await?;
    let aid = alice.validate()?.payload.account.clone();
    let bid = bob.validate()?.payload.account.clone();
    a.feed().await?;
    b.feed().await?;
    let report_marker = "Primary-only report evidence";
    let draft = call(&b, json!({"type":"reportDraft"})).await?;
    let private_receipt = draft["receipt"]
        .as_str()
        .context("missing report receipt")?;
    let report = call(
        &b,
        json!({"type":"submitReport", "receipt":private_receipt,
        "evidence":{"subject":"account","category":"other","text":report_marker}}),
    )
    .await?;
    assert_eq!(report["status"], "received");
    assert!(!s.join("reports.sqlite").exists());
    let init = json!({"kind":"initialize"});
    let sync = json!({"kind":"sync"});
    let mut sa = direct(&a, &Value::Null, init.clone()).await?;
    let mut sb = direct(&b, &Value::Null, init).await?;
    sa = direct(&a, &sa, sync.clone()).await?;
    sb = direct(&b, &sb, sync.clone()).await?;
    let send = |peer: &str, body: &str| json!({"kind":"prepare","peer":peer,"label":"Friend","name":"Neighbor","body":body});
    sa = direct(&a, &sa, send(&bid, "before crash")).await?;
    let replay = sa.clone();
    sa = direct(&a, &sa, sync.clone()).await?;
    let prefs = json!({"type":"social","op":{"kind":"setPrivacy","settings":{"hideAnonymous":true,"feedDays":0,"dmDays":0,"approvedOnly":false}}});
    call(&b, prefs).await?;
    let before = a.prepare(note("Before crash"))?.event;
    a.submit(before.clone()).await?;
    processes.crash(primary)?;
    let standby_status = Request::HomeOperation {
        request_id: new_secret(),
        call: Box::new(Request::ReportStatus {
            receipt: private_receipt.into(),
        }),
    };
    assert!(
        yappers_netkit::transport::rpc(&b.endpoint, grant.standby.clone(), &standby_status)
            .await
            .is_err()
    );
    // No clean shutdown or coordinated flush. Mailbox state must already be durable.
    sb = direct(&b, &sb, sync.clone()).await?;
    assert_eq!(sb["messages"][0]["body"], "before crash");
    let status = call(&b, json!({"type":"social","op":{"kind":"privacyStatus"}})).await?;
    assert_eq!(status["settings"]["hideAnonymous"], true);
    direct(&a, &replay, sync.clone()).await?;
    sb = direct(&b, &sb, sync.clone()).await?;
    assert_eq!(sb["messages"].as_array().unwrap().len(), 1);
    sb = direct(&b, &sb, send(&aid, "reply from standby")).await?;
    sb = direct(&b, &sb, sync.clone()).await?;
    let after = a.prepare(note("During takeover"))?.event;
    a.submit(after.clone()).await?;
    let accepted = a.submit(after.clone()).await?;
    assert!(
        matches!(accepted,Response::Accepted{envelope:Some(e),..} if e.validate().is_ok() && e.event.payload.home==home)
    );
    call(
        &b,
        json!({"type":"social","op":{"kind":"block","target":{"account":aid}}}),
    )
    .await?;
    let Response::Feed { posts, .. } = b.feed().await? else {
        panic!("private projection missing")
    };
    assert!(posts.is_empty());
    // The standby commits the owner delete while the primary is offline.
    let deletion = a
        .prepare(Action::Delete {
            target: before.id.clone(),
        })?
        .event;
    a.submit(deletion).await?;
    let receipt = a.submit(before.clone()).await?;
    assert!(
        matches!(receipt,Response::Accepted{envelope:Some(e),..} if e.deletion.is_some() && e.validate().is_ok())
    );
    let ledger_bytes = fs::read(s.join("authority.db"))?;
    for marker in [
        "Before crash",
        before.payload.nonce.as_str(),
        report_marker,
        private_receipt,
        report["case"].as_str().unwrap(),
    ] {
        assert!(
            !ledger_bytes
                .windows(marker.len())
                .any(|w| w == marker.as_bytes())
        );
    }
    // Heal the user-facing primary link but isolate the dedicated control plane. The standby
    // remains available to independent clients; a reachable partitioned worker cannot ack.
    let mut isolated = grant.clone();
    isolated.standby = serde_json::from_value(json!({"id":standby_id,"addrs":[]}))?;
    json_file(&p.join("grant.json"), &isolated)?;
    let (partitioned, address) = processes.launch(&p, &[]).await?;
    join.server_addr = serde_json::from_value(address)?;
    let primary_only = device(&join, None, &alice).await?;
    let never_committed = a
        .prepare(note("A partitioned worker must not publish this"))?
        .event;
    assert!(!matches!(
        tokio::time::timeout(
            Duration::from_secs(9),
            primary_only.submit(never_committed.clone())
        )
        .await,
        Ok(Ok(_))
    ));
    processes.crash(partitioned)?;
    processes.crash(fly)?;
    let (_, address) = processes.launch(&s, &fly_args).await?;
    grant.standby = serde_json::from_value(address)?;
    json_file(&p.join("grant.json"), &grant)?;
    let (_, address) = processes.launch(&p, &[]).await?;
    join.server_addr = serde_json::from_value(address)?;
    // Primary-only clients prove these requests actually returned to the dedicated service.
    let recovered = device(&join, None, &alice).await?;
    let rb = device(&join, None, &bob).await?;
    assert_eq!(
        call(
            &rb,
            json!({"type":"reportStatus","receipt":private_receipt})
        )
        .await?,
        report
    );
    assert!(!s.join("reports.sqlite").exists());
    let Response::Feed { posts, .. } = recovered.feed().await? else {
        panic!("missing feed")
    };
    assert_eq!(posts.len(), 1);
    assert!(!posts.iter().any(|p| p.id == before.id));
    assert!(!posts.iter().any(|p| p.id == never_committed.id));
    let status = call(&rb, json!({"type":"social","op":{"kind":"privacyStatus"}})).await?;
    assert_eq!(status["hasBlocks"], true);
    let blocks = call(&rb, json!({"type":"social","op":{"kind":"blocks"}})).await?;
    let block = blocks["blocks"][0]["id"]
        .as_str()
        .context("missing block")?;
    call(
        &rb,
        json!({"type":"social","op":{"kind":"unblock","id":block}}),
    )
    .await?;
    sa = direct(&recovered, &sa, sync.clone()).await?;
    // Blocking can discard queued mail. Send again on the existing, device-held session.
    sb = direct(&rb, &sb, send(&aid, "after both restarts")).await?;
    direct(&rb, &sb, sync.clone()).await?;
    sa = direct(&recovered, &sa, sync).await?;
    assert!(
        sa["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["body"] == "after both restarts")
    );
    Ok(())
}
