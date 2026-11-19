// SPDX-License-Identifier: AGPL-3.0-only
//! Exercise the public client command path and the local operator CLI on a disposable home.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use yappers_netkit::{
    client::{Client, ClientConfig},
    model::*,
    portability::AccountWallet,
    report, transport,
};

const EXE: &str = env!("CARGO_BIN_EXE_yappers-server");
struct Home {
    dir: PathBuf,
    process: Child,
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
impl Home {
    async fn start(peer: Value) -> Result<(Self, JoinConfig)> {
        let dir = std::env::temp_dir().join(format!("yap-report-test-{}", new_secret()));
        fs::create_dir(&dir)?;
        let cfg = dir.join("home.json");
        fs::write(
            &cfg,
            serde_json::to_vec(&json!({
                "privateKey":new_secret(), "databasePath":dir.join("feed.sqlite"),
                "communities":{"test":new_secret()}, "peers":[{"address":peer,"communities":["test"]}]
            }))?,
        )?;
        let process = Command::new(EXE)
            .args(["run", "--config"])
            .arg(cfg)
            .env("YAP_REPORT_INBOX", dir.join("reports.sqlite"))
            .stdout(Stdio::null())
            .spawn()?;
        let mut home = Self { dir, process };
        for _ in 0..250 {
            if let Ok(bytes) = fs::read(home.dir.join("join-0.json")) {
                return Ok((home, serde_json::from_slice(&bytes)?));
            }
            ensure!(home.process.try_wait()?.is_none(), "home exited");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        anyhow::bail!("local home did not start")
    }
    fn operator(&self, args: &[&str]) -> Result<String> {
        let out = Command::new(EXE)
            .args(["reports", "--inbox"])
            .arg(self.dir.join("reports.sqlite"))
            .args(args)
            .output()?;
        ensure!(out.status.success(), "operator command failed");
        Ok(String::from_utf8(out.stdout)?)
    }
}
async fn client(join: &JoinConfig) -> Result<(Client, String)> {
    let wallet = AccountWallet::create(new_secret(), join.server_addr.id.to_string())?;
    let owner = wallet.validate()?.payload.account.clone();
    let c = Client::open(ClientConfig {
        join: join.clone(),
        full_home_standby: None,
        author_secret: wallet.device_secret,
        next_sequence: 1,
        account_chain: wallet.chain,
    })
    .await?;
    c.feed().await?;
    Ok((c, owner))
}
async fn call(c: &Client, cmd: Value) -> Result<Value> {
    c.command(serde_json::from_value(cmd)?).await
}
async fn direct(c: &Client, state: &Value, action: Value) -> Result<Value> {
    Ok(call(c, json!({"type":"direct","state":state,"action":action})).await?["state"].take())
}
#[tokio::test]
async fn private_message_excerpt_to_operator_and_private_outcome() -> Result<()> {
    let peer = transport::bind(NetworkMode::Local, None, false).await?;
    let (home, join) = Home::start(serde_json::to_value(peer.addr())?).await?;
    let (sender, sender_id) = client(&join).await?;
    let (reporter, reporter_id) = client(&join).await?;
    let init = json!({"kind":"initialize"});
    let sync = json!({"kind":"sync"});
    let mut sender_state = direct(&sender, &Value::Null, init.clone()).await?;
    let mut recipient_state = direct(&reporter, &Value::Null, init).await?;
    sender_state = direct(&sender, &sender_state, sync.clone()).await?;
    recipient_state = direct(&reporter, &recipient_state, sync.clone()).await?;
    let unrelated = "TEST-UNRELATED-PRIVATE-CONVERSATION";
    let selected = "TEST-SELECTED-THREAT-EXCERPT";
    for body in [unrelated, selected] {
        sender_state = direct(
            &sender, &sender_state,
            json!({"kind":"prepare","peer":reporter_id,"label":"Neighbor","name":"Sender","body":body}),
        ).await?;
        sender_state = direct(&sender, &sender_state, sync.clone()).await?;
    }
    recipient_state = direct(&reporter, &recipient_state, sync).await?;
    let messages = recipient_state["messages"]
        .as_array()
        .context("missing messages")?;
    ensure!(messages.len() == 2 && messages[1]["body"] == selected);
    let draft = call(&reporter, json!({"type":"reportDraft"})).await?;
    let receipt = draft["receipt"].as_str().unwrap();
    let evidence = json!({"category":"threat","subject":"message","reference":sender_id,"text":messages[1]["body"]});
    let submit = json!({"type":"submitReport","receipt":receipt,"evidence":evidence});
    let result = call(&reporter, submit.clone()).await?;
    ensure!(result["status"] == "received" && call(&reporter, submit.clone()).await? == result);
    let id = result["case"].as_str().unwrap();
    let pending: Value = serde_json::from_str(&home.operator(&["list"])?)?;
    ensure!(pending.as_array().unwrap().len() == 1 && pending[0]["case"] == id);
    let shown = home.operator(&["show", id])?;
    ensure!(serde_json::from_str::<Value>(&shown)?["evidence"] == evidence);
    for omitted in [receipt, unrelated, &reporter_id, &join.access_token] {
        ensure!(!shown.contains(omitted));
    }
    // An admitted federation peer can pull public records while the report remains open.
    let post = sender
        .prepare(Action::Post {
            display_name: "Neighbor".into(),
            anonymous: false,
            body: "Public fixture post".into(),
            topic: "Chatter".into(),
        })?
        .event;
    let post_id = post.id.clone();
    sender.submit(post).await?;
    let page = transport::rpc(
        &peer,
        join.server_addr.clone(),
        &Request::Pull {
            after: 0,
            limit: 100,
            generation: None,
        },
    )
    .await?;
    ensure!(
        matches!(&page, Response::Page { entries, .. } if entries.iter().any(|e| e.envelope.event.id == post_id))
    );
    let page_text = serde_json::to_string(&page)?;
    for private in [selected, unrelated, receipt, id] {
        ensure!(!page_text.contains(private));
    }
    let fake_receipt = report::receipt(&join.server_addr.id.to_string());
    ensure!(
        call(
            &sender,
            json!({"type":"reportStatus","receipt":fake_receipt})
        )
        .await
        .is_err()
    );
    ensure!(
        call(&sender, json!({"type":"reportStatus","receipt":id}))
            .await
            .is_err()
    );
    let report: report::Evidence = serde_json::from_value(evidence)?;
    let denied = transport::rpc(
        &sender.endpoint,
        join.server_addr.clone(),
        &Request::SubmitReport {
            community: join.community.clone(),
            token: new_secret(),
            receipt: fake_receipt,
            evidence: report,
        },
    )
    .await;
    ensure!(denied.is_err());
    ensure!(
        call(&reporter, json!({"type":"social","op":{"kind":"blocks"}})).await?["blocks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    home.operator(&["close", id, "--outcome", "noAction"])?;
    let status = call(&reporter, json!({"type":"reportStatus","receipt":receipt})).await?;
    ensure!(status["status"] == "noAction" && status.get("evidence").is_none());
    ensure!(call(&reporter, submit).await? == status);
    ensure!(serde_json::from_str::<Value>(&home.operator(&["show", id])?)?["evidence"].is_null());
    let post_draft = call(&reporter, json!({"type":"reportDraft"})).await?;
    let post_receipt = &post_draft["receipt"];
    let open = call(
        &reporter,
        json!({"type":"submitReport","receipt":post_receipt,
        "evidence":{"category":"other","subject":"post","reference":post_id,"text":""}}),
    )
    .await?;
    let case = open["case"].as_str().context("missing post case ID")?;
    let disclosed: Value = serde_json::from_str(&home.operator(&["show", case])?)?;
    ensure!(disclosed["evidence"]["reference"] == post_id);
    let result = Command::new(EXE)
        .args(["moderate", "--database"])
        .arg(home.dir.join("feed.sqlite"))
        .args(["--event", &post_id])
        .output()?;
    ensure!(result.status.success(), "home moderation failed");
    ensure!(
        matches!(reporter.feed().await?, Response::Feed { posts,.. } if posts.iter().all(|p| p.id != post_id))
    );
    home.operator(&["close", case, "--outcome", "actionTaken"])?;
    ensure!(
        call(
            &reporter,
            json!({"type":"reportStatus","receipt":post_receipt})
        )
        .await?["status"]
            == "actionTaken"
    );
    for name in [
        "feed.sqlite",
        "feed.sqlite.deletion-guard",
        "reports.sqlite",
    ] {
        let bytes = fs::read(home.dir.join(name))?;
        for private in [selected, unrelated, receipt] {
            ensure!(
                !bytes
                    .windows(private.len())
                    .any(|b| b == private.as_bytes())
            );
        }
    }
    sender.close().await;
    reporter.close().await;
    Ok(())
}
