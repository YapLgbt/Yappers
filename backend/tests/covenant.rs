// SPDX-License-Identifier: AGPL-3.0-only
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use yappers_netkit::{
    model::*,
    peering::Acceptance,
    server::{Server, ServerConfig},
    store::Store,
    transport,
};
const EXE: &str = env!("CARGO_BIN_EXE_yappers-server");

struct Fixture {
    dir: PathBuf,
    process: Option<Child>,
}
impl Fixture {
    fn stop(&mut self) {
        if let Some(mut p) = self.process.take() {
            let _ = p.kill();
            let _ = p.wait();
        }
    }
    async fn start(&mut self, policy: &Value) -> Result<JoinConfig> {
        self.stop();
        let config = self.dir.join("home.json");
        let p = self.dir.join("policy.json");
        fs::write(&p, serde_json::to_vec(policy)?)?;
        let join = self.dir.join("join-0.json");
        if join.exists() {
            fs::remove_file(&join)?;
        }
        self.process = Some(
            Command::new(EXE)
                .args(["run", "--config"])
                .arg(config)
                .env("YAP_PEERING_POLICY", p)
                .stdout(Stdio::null())
                .spawn()?,
        );
        for _ in 0..250 {
            if let Ok(bytes) = fs::read(&join) {
                return Ok(serde_json::from_slice(&bytes)?);
            }
            ensure!(
                self.process.as_mut().unwrap().try_wait()?.is_none(),
                "home exited"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        anyhow::bail!("home failed to start")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn config(key: &str, path: &Path) -> Value {
    json!({"privateKey":key,"databasePath":path,"communities":{"a":new_secret(),"b":new_secret()},"autoFederation":{"enabled":true}})
}
#[tokio::test]
async fn scoped_admission_suspension_and_tamper_rejection() -> Result<()> {
    let mut f = Fixture {
        dir: std::env::temp_dir().join(format!("yap-covenant-{}", new_secret())),
        process: None,
    };
    fs::create_dir(&f.dir)?;
    let owner = signing_key(&new_secret())?;
    let home = hex::encode(owner.verifying_key().as_bytes());
    let peer_key = new_secret();
    let peer = Server::start(serde_json::from_value::<ServerConfig>(config(
        &peer_key,
        &f.dir.join("observer.db"),
    ))?)
    .await?;
    let db = f.dir.join("home.db");
    let mut cfg = config(&hex::encode(owner.to_bytes()), &db);
    cfg["peers"] = json!([{"address":peer.endpoint.addr(),"communities":["a","b"]}]);
    fs::write(f.dir.join("home.json"), serde_json::to_vec(&cfg)?)?;
    let sign = |sequence, community: &str, action| -> Result<Envelope> {
        Envelope::accept(
            SignedEvent::create(&owner, home.clone(), community.into(), sequence, action)?,
            &owner,
        )
    };
    let post = |s| Action::Post {
        body: s,
        anonymous: true,
        topic: "Plans".into(),
        display_name: "Anonymous neighbor".into(),
    };
    let seen = sign(1, "a", post("shared once".into()))?;
    let hidden = sign(2, "b", post("outside agreed scope".into()))?;
    let unopened = sign(3, "a", post("never delivered".into()))?;
    {
        let mut store = Store::open(&db)?;
        store.accept(&seen)?;
        store.accept(&hidden)?;
        store.backup(&f.dir.join("before.yap"))?;
    }
    let receipt = Acceptance::sign(
        &signing_key(&peer_key)?,
        "Example home".into(),
        "ops@example.org".into(),
        "https://example.org/source/release.tar.gz".into(),
        "a".repeat(64),
    )?;
    let mut forged = serde_json::to_value(&receipt)?;
    forged["statement"]["operator"] = json!("Another operator");
    assert!(
        serde_json::from_value::<Acceptance>(forged)?
            .validate()
            .is_err()
    );
    let mut policy = json!({"managedHomes":[],"peers":[]});
    let pull = Request::Pull {
        after: 0,
        limit: 100,
        generation: None,
    };
    let read =
        async |join: JoinConfig| transport::rpc(&peer.endpoint, join.server_addr, &pull).await;
    let join = f.start(&policy).await?;
    assert!(
        read(join)
            .await
            .unwrap_err()
            .to_string()
            .contains("covenant")
    );
    policy["peers"] = json!([{"acceptance":receipt,"communities":["a"],"status":"active"}]);
    let join = f.start(&policy).await?;
    let Response::Page { entries, .. } = read(join).await? else {
        anyhow::bail!("missing page")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].envelope, seen);
    f.stop();
    let remove = sign(
        4,
        "a",
        Action::Delete {
            target: seen.event.id.clone(),
        },
    )?;
    {
        let mut store = Store::open(&db)?;
        store.accept(&unopened)?;
        store.accept(&remove)?;
        store.accept(&sign(
            5,
            "a",
            Action::Delete {
                target: unopened.event.id.clone(),
            },
        )?)?;
    }
    let restored = f.dir.join("restored.db");
    yappers_netkit::backup::restore(
        &db.with_added_extension("deletion-guard"),
        &f.dir.join("before.yap"),
        &restored,
    )?;
    cfg["databasePath"] = json!(restored);
    fs::write(f.dir.join("home.json"), serde_json::to_vec(&cfg)?)?;
    policy["peers"][0]["status"] = json!("suspended");
    let join = f.start(&policy).await?;
    let Response::Page { entries, .. } = read(join.clone()).await? else {
        anyhow::bail!("missing deletion page")
    };
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| entry.envelope == remove && entry.username.is_none())
    );
    let error = transport::rpc(
        &peer.endpoint,
        join.server_addr,
        &Request::EnrollHost { group: "a".into() },
    )
    .await
    .unwrap_err();
    ensure!(
        error.to_string().contains("blocked"),
        "unexpected denial: {error}"
    );
    peer.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "read-only check of production admissions and exact source on all three physical routes"]
async fn deployed_covenant() -> Result<()> {
    let m: Value = serde_json::from_slice(&fs::read(std::env::var("YAP_SMOKE_ENROLLMENT")?)?)?;
    let join: JoinConfig = serde_json::from_value(m["join"].clone())?;
    let revision = std::env::var("YAP_EXPECTED_SOURCE")?;
    let observer = transport::bind(join.network_mode, None, false).await?;
    for (role, addr) in [
        ("home", join.server_addr),
        (
            "standby",
            serde_json::from_value(m["fullHomeStandby"]["standby"].clone())?,
        ),
        (
            "replica",
            serde_json::from_value(m["publicFeedReplica"].clone())?,
        ),
    ] {
        let Response::ServerSource {
            source_url,
            revision: running,
        } = transport::rpc(&observer, addr.clone(), &Request::ServerSource).await?
        else {
            anyhow::bail!("missing source offer")
        };
        ensure!(
            running == revision
                && source_url == format!("https://yap.lgbt/source/{revision}.tar.gz"),
            "source mismatch on {role}"
        );
        for request in [
            Request::Pull {
                after: 0,
                limit: 1,
                generation: None,
            },
            Request::EnrollHost {
                group: join.community.clone(),
            },
        ] {
            let denied = transport::rpc(&observer, addr.clone(), &request)
                .await
                .err()
                .context("unapproved peer gained access")?;
            let msg = denied.to_string();
            ensure!(
                msg.contains("untrusted federation endpoint")
                    || msg.contains("covenant")
                    || msg.contains("host enrollment disabled or blocked"),
                "unexpected error from {role}: {msg}"
            );
        }
        println!("{role} verified source and refused external federation and enrollment");
    }
    observer.close().await;
    Ok(())
}
