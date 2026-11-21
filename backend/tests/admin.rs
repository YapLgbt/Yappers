// SPDX-License-Identifier: AGPL-3.0-only
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use yappers_netkit::{model::*, report, transport};

const EXE: &str = env!("CARGO_BIN_EXE_yappers-server");

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn cli(args: &[&str], config: &Path) -> Result<String> {
    let result = Command::new(EXE)
        .args(args)
        .arg("--config")
        .arg(config)
        .output()?;
    ensure!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

struct Browser<'a> {
    address: &'a str,
    token: Option<String>,
}

impl Browser<'_> {
    fn request(
        &self,
        path: &str,
        body: Option<Value>,
        host: &str,
        origin: &str,
    ) -> Result<(u16, Value)> {
        let mut connection = TcpStream::connect(self.address)?;
        connection.set_read_timeout(Some(Duration::from_secs(15)))?;
        let body_text = body.as_ref().map(Value::to_string).unwrap_or_default();
        let method = if body.is_some() { "POST" } else { "GET" };
        write!(
            connection,
            "{method} /api/{path} HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
            body_text.len()
        )?;
        if let Some(token) = &self.token {
            write!(connection, "Authorization: Bearer {token}\r\n")?;
        }
        write!(connection, "\r\n{body_text}")?;
        let mut response = String::new();
        connection.read_to_string(&mut response)?;
        let (headers, body) = response
            .split_once("\r\n\r\n")
            .context("missing HTTP headers")?;
        let status = headers
            .split_whitespace()
            .nth(1)
            .context("missing HTTP status")?
            .parse()?;
        ensure!(headers.contains("cache-control: no-store"));
        Ok((
            status,
            if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(body)?
            },
        ))
    }

    fn call(&self, path: &str, body: Option<Value>, expected: u16) -> Result<Value> {
        let (status, value) = self.request(
            path,
            body,
            self.address,
            &format!("http://{}", self.address),
        )?;
        ensure!(
            status == expected,
            "{path}: expected status {expected}, got {status}"
        );
        Ok(value)
    }
}

#[tokio::test]
async fn loopback_admin_requires_a_session_and_reuses_home_operations() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("config.json");
    let inbox = directory.path().join("reports.sqlite");
    let assets = directory.path().join("assets");
    fs::create_dir(&assets)?;
    fs::write(assets.join("index.html"), "Fixture admin build")?;
    ensure!(
        Command::new(EXE)
            .args(["init", "--directory"])
            .arg(directory.path())
            .output()?
            .status
            .success()
    );
    let peer_id = hex::encode(signing_key(&new_secret())?.verifying_key().as_bytes());
    let mut settings: Value = serde_json::from_slice(&fs::read(&config)?)?;
    let scope = settings["communities"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    settings["peers"] = json!([{"address":{"id":peer_id,"addrs":[]},"communities":[scope]}]);
    fs::write(&config, serde_json::to_vec(&settings)?)?;
    let mut home = Process(
        Command::new(EXE)
            .args(["run", "--config"])
            .arg(&config)
            .env("YAP_REPORT_INBOX", &inbox)
            .stdout(Stdio::null())
            .spawn()?,
    );
    let invite = directory.path().join("join-0.json");
    for _ in 0..250 {
        if invite.exists() {
            break;
        }
        ensure!(home.0.try_wait()?.is_none(), "Home exited");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let join: JoinConfig = serde_json::from_slice(&fs::read(invite)?)?;
    let mut admin = Process(
        Command::new(EXE)
            .args(["admin", "serve", "--port", "0", "--report-inbox"])
            .arg(&inbox)
            .arg("--assets")
            .arg(&assets)
            .arg("--config")
            .arg(&config)
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let mut address = String::new();
    BufReader::new(admin.0.stdout.take().unwrap()).read_line(&mut address)?;
    let address = address
        .trim()
        .strip_prefix("Home admin listening on http://")
        .context("admin startup message")?;
    ensure!(address.starts_with("127.0.0.1:"));
    let mut browser = Browser {
        address,
        token: None,
    };

    browser.call("home", None, 401)?;
    browser.call("peers", None, 401)?;
    browser.call("moderation", None, 401)?;
    browser.call("invitations/0", None, 401)?;
    let code = cli(&["admin", "code"], &config)?;
    let code_path = directory.path().join("admin-login.json");
    ensure!(code_path.metadata()?.permissions().mode() & 0o777 == 0o600);
    ensure!(!fs::read_to_string(&code_path)?.contains(&code));
    ensure!(
        browser
            .request(
                "login",
                Some(json!({"code":code})),
                "foreign.invalid",
                "http://foreign.invalid"
            )?
            .0
            == 421
    );
    ensure!(
        browser
            .request(
                "login",
                Some(json!({"code":code})),
                address,
                "http://foreign.invalid"
            )?
            .0
            == 403
    );
    let login = browser.call("login", Some(json!({"code": code})), 200)?;
    browser.token = Some(login["token"].as_str().context("missing token")?.to_owned());
    browser.call("login", Some(json!({"code": code})), 401)?;

    let status = browser.call("home", None, 200)?;
    ensure!(status["storageOnline"] == true && status["id"] == join.server_addr.id.to_string());
    ensure!(!status.to_string().contains(&join.access_token));
    let peers = browser.call("peers", None, 200)?;
    ensure!(peers[0]["id"] == peer_id && peers[0]["status"] == "active");
    let stop = format!("peers/{peer_id}/stop");
    ensure!(
        browser
            .request(&stop, Some(json!({})), address, "http://foreign.invalid")?
            .0
            == 403
    );
    browser.call(&stop, Some(json!({})), 204)?;
    ensure!(browser.call("peers", None, 200)?[0]["status"] == "suspended");
    ensure!(browser.call("invitations/0", None, 200)? == serde_json::to_value(&join)?);
    let pending_id = new_secret();
    let change = json!({"event": pending_id, "hide": true});
    ensure!(
        browser
            .request(
                "moderation",
                Some(change.clone()),
                address,
                "http://foreign.invalid"
            )?
            .0
            == 403
    );
    browser.call("moderation", Some(change), 204)?;
    ensure!(browser.call("moderation", None, 200)?["ids"] == json!([pending_id]));
    ensure!(
        browser.call(&format!("moderation/{pending_id}"), None, 200)?
            == json!({"hidden": true, "content": null})
    );
    browser.call("moderation/hidden/bad", None, 400)?;
    browser.call("moderation/not-an-id", None, 400)?;
    let saved = browser.call("backups", Some(json!({})), 201)?;
    let archive = directory
        .path()
        .join(saved["file"].as_str().context("missing backup filename")?);
    ensure!(
        archive.metadata()?.len() > 0 && archive.metadata()?.permissions().mode() & 0o777 == 0o600
    );

    let peer = transport::bind(NetworkMode::Local, None, false).await?;
    let author = signing_key(&new_secret())?;
    let rpc =
        async |request: Request| transport::rpc(&peer, join.server_addr.clone(), &request).await;
    let publish = async |seq, action| -> Result<String> {
        let event = SignedEvent::create(
            &author,
            join.server_addr.id.to_string(),
            join.community.clone(),
            seq,
            action,
        )?;
        match rpc(Request::Submit {
            token: join.access_token.clone(),
            event,
            account_chain: vec![],
        })
        .await?
        {
            Response::Accepted { event_id, .. } => Ok(event_id),
            response => anyhow::bail!("unexpected publish response: {response:?}"),
        }
    };
    let published = publish(
        1,
        Action::PostV2 {
            body: "The actual post on this Home".into(),
            topic: "Plans".into(),
            anonymous: true,
            display_name: "Anonymous neighbor".into(),
            headline: "Local post".into(),
            profile: None,
            area: None,
        },
    )
    .await?;
    let event_path = format!("moderation/{published}");
    let lookup = browser.call(&event_path, None, 200)?;
    ensure!(
        lookup["hidden"] == false && lookup["content"]["body"] == "The actual post on this Home"
    );
    ensure!(lookup["content"]["kind"] == "post" && lookup["content"]["headline"] == "Local post");
    let report_event = async |subject| -> Result<String> {
        let submitted = rpc(Request::SubmitReport {
            token: join.access_token.clone(),
            community: join.community.clone(),
            receipt: report::receipt(&join.server_addr.id.to_string()),
            evidence: report::Evidence {
                category: report::Category::Other,
                subject,
                reference: Some(published.clone()),
                text: "Private test evidence".into(),
            },
        })
        .await?;
        match submitted {
            Response::ReportResult { result } => {
                Ok(result["case"].as_str().context("missing case ID")?.into())
            }
            response => anyhow::bail!("unexpected report result: {response:?}"),
        }
    };
    let wrong = report_event(report::Subject::Reply).await?;
    browser.call(&format!("reports/{wrong}/hide"), Some(json!({})), 409)?;
    ensure!(browser.call(&event_path, None, 200)?["hidden"] == false);
    let case = report_event(report::Subject::Post).await?;
    ensure!(
        browser
            .call("reports", None, 200)?
            .as_array()
            .unwrap()
            .len()
            == 2
    );
    browser.call(&format!("reports/{case}/hide"), Some(json!({})), 204)?;
    ensure!(browser.call(&event_path, None, 200)?["hidden"] == true);
    let visible = async || -> Result<bool> {
        match rpc(Request::Feed {
            token: join.access_token.clone(),
            community: join.community.clone(),
            author: hex::encode(author.verifying_key().as_bytes()),
            nearby: None,
        })
        .await?
        {
            Response::Feed { posts, .. } => Ok(posts.iter().any(|post| post.id == published)),
            response => anyhow::bail!("unexpected feed response: {response:?}"),
        }
    };
    ensure!(!visible().await?);
    ensure!(
        browser.call(&format!("reports/{case}"), None, 200)?["evidence"]["text"]
            == "Private test evidence"
    );
    browser.call(
        &format!("reports/{case}/close"),
        Some(json!({"outcome":"actionTaken"})),
        204,
    )?;
    ensure!(browser.call(&format!("reports/{case}"), None, 200)?["evidence"].is_null());
    browser.call(&format!("reports/{case}/hide"), Some(json!({})), 409)?;
    browser.call(
        "moderation",
        Some(json!({"event": published, "hide": false})),
        204,
    )?;
    ensure!(visible().await?);
    ensure!(browser.call("moderation", None, 200)?["ids"] == json!([pending_id]));
    publish(
        2,
        Action::Delete {
            target: published.clone(),
        },
    )
    .await?;
    ensure!(browser.call(&event_path, None, 200)?["content"].is_null());

    // Deleting the row used as the page cursor must not skip the next hidden event.
    let mut ids = vec![pending_id];
    for _ in 0..21 {
        let id = new_secret();
        browser.call("moderation", Some(json!({"event": id, "hide": true})), 204)?;
        ids.push(id);
    }
    ids.sort();
    let first = browser.call("moderation", None, 200)?;
    ensure!(first["ids"] == json!(ids[..20]));
    let cursor = first["next"].as_str().context("missing next cursor")?;
    browser.call(
        "moderation",
        Some(json!({"event": cursor, "hide": false})),
        204,
    )?;
    ensure!(
        browser.call(&format!("moderation/hidden/{cursor}"), None, 200)?
            == json!({"ids": ids[20..], "next": null})
    );

    home.0.kill()?;
    home.0.wait()?;
    ensure!(browser.call("home", None, 200)?["storageOnline"] == false);
    browser.call("backups", Some(json!({})), 409)?;
    browser.call("logout", Some(json!({})), 204)?;
    browser.call("home", None, 401)?;
    peer.close().await;
    Ok(())
}
