// SPDX-License-Identifier: AGPL-3.0-only
//! Installation-scoped, opt-in delivery. The provider receives only a generic
//! alert and an opaque route token. Opening it requires the enrolled account/device.
use crate::{
    model::{new_secret, now},
    store::Store,
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
pub(crate) fn capabilities() -> Value {
    let enabled = std::env::var("YAP_PUSH_PROVIDERS").unwrap_or_default();
    let has = |name: &str| enabled.split(',').any(|s| s == name);
    json!({"webKey":std::env::var("YAP_WEB_PUSH_KEY").ok().filter(|_|has("web")),"android":has("fcm"),"ios":has("apns")})
}
fn validate_subscription(provider: &str, sub: &Value) -> Result<()> {
    ensure!(
        serde_json::to_vec(sub)?.len() < 4096,
        "invalid push subscription"
    );
    let caps = capabilities();
    match provider {
        "web" => {
            ensure!(caps["webKey"].is_string(), "web push unavailable");
            let url = url::Url::parse(sub["endpoint"].as_str().unwrap_or(""))?;
            let host = url.host_str().unwrap_or("");
            ensure!(
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
                    && url.fragment().is_none()
                    && (host == "fcm.googleapis.com"
                        || host == "updates.push.services.mozilla.com"
                        || host.ends_with(".push.services.mozilla.com")
                        || host == "web.push.apple.com"
                        || host.ends_with(".push.apple.com")
                        || host.ends_with(".notify.windows.com")),
                "unsupported browser push service"
            );
            for (key, min, max) in [("auth", 22, 24), ("p256dh", 87, 88)] {
                let raw = sub["keys"][key].as_str().unwrap_or("");
                ensure!(
                    (min..=max).contains(&raw.len())
                        && raw
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-_=".contains(&c)),
                    "invalid browser push key"
                );
            }
        }
        "apns" | "fcm" => {
            let token = sub.as_str().unwrap_or("");
            ensure!(
                if provider == "apns" {
                    caps["ios"] == true && token.len() == 64 && hex::decode(token).is_ok()
                } else {
                    caps["android"] == true
                        && (32..=2048).contains(&token.len())
                        && token.bytes().all(|c| c.is_ascii_graphic())
                },
                "invalid device push registration"
            );
        }
        _ => anyhow::bail!("unknown push provider"),
    }
    Ok(())
}
impl Store {
    pub(crate) fn set_push(
        &self,
        owner: &str,
        device: &str,
        provider: &str,
        subscription: Value,
        categories: Vec<String>,
    ) -> Result<Value> {
        ensure!(
            categories.len() <= 3
                && categories
                    .iter()
                    .all(|c| matches!(c.as_str(), "reply" | "message" | "request")),
            "invalid notification categories"
        );
        let tx = self.db.unchecked_transaction()?;
        for kind in ["reply", "message", "request"] {
            if !categories.iter().any(|c| c == kind) {
                tx.execute(
                    "DELETE FROM push_starts WHERE owner=? AND device=? AND category=?",
                    [owner, device, kind],
                )?;
                tx.execute(
                    "DELETE FROM push_routes WHERE owner=? AND device=? AND category=?",
                    [owner, device, kind],
                )?;
            }
        }
        if categories.is_empty() {
            tx.execute(
                "DELETE FROM push_devices WHERE owner=? AND device=?",
                [owner, device],
            )?;
            tx.execute(
                "DELETE FROM push_routes WHERE owner=? AND device=?",
                [owner, device],
            )?;
        } else {
            validate_subscription(provider, &subscription)?;
            let baseline: (i64,i64)=tx.query_row("SELECT COALESCE(MAX(cursor),0),(SELECT COALESCE(MAX(seq),0) FROM dm_mail) FROM events",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            tx.execute("INSERT INTO push_devices VALUES(?,?,?,?,?) ON CONFLICT(owner,device) DO UPDATE SET
                provider=excluded.provider,subscription=excluded.subscription,categories=excluded.categories",params![
                owner,device,provider,serde_json::to_string(&subscription)?,serde_json::to_string(&categories)?])?;
            for category in &categories {
                tx.execute(
                    "INSERT OR IGNORE INTO push_starts VALUES(?,?,?,?)",
                    params![
                        owner,
                        device,
                        category,
                        if category == "reply" {
                            baseline.0
                        } else {
                            baseline.1
                        }
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(json!({"categories":categories}))
    }
    pub(crate) fn push_categories(&self, owner: &str, device: &str) -> Result<Value> {
        let raw: Option<String> = self
            .db
            .query_row(
                "SELECT categories FROM push_devices WHERE owner=? AND device=?",
                [owner, device],
                |r| r.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(raw.as_deref().unwrap_or("[]"))?)
    }
    pub(crate) fn push_route(&self, owner: &str, device: &str, token: &str) -> Result<Value> {
        let pair: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT category,target FROM push_routes WHERE owner=? AND device=? AND token=?",
                params![owner, device, token],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((category, target)) = pair else {
            return Ok(json!({}));
        };
        let inbox = self.notification_inbox(owner, device)?;
        let items = match category.as_str() {
            "reply" => "activity",
            "request" => "requests",
            _ => "messages",
        };
        let item = inbox[items]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["target"] == target));
        if let Some(item) = item {
            return Ok(json!({"category":category,"item":item}));
        }
        // A read receipt removes a message from the unread list. Its delivered
        // notification can still open the conversation on the owning installation.
        if category == "message"
            && !crate::privacy::needs_request(
                &self.db,
                owner,
                &target,
                &crate::privacy::settings(&self.db, owner)?,
            )?
        {
            let message=self.db.query_row("SELECT m.seq,m.created,m.id FROM dm_mail m JOIN dm_devices d ON d.owner=m.recipient
                WHERE m.recipient=?1 AND d.device=?2 AND m.sender=?3 AND m.held=0 AND m.created>=?4
                AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE (b.owner=?1 AND b.peer=?3) OR (b.owner=?3 AND b.peer=?1))
                ORDER BY m.seq DESC LIMIT 1",params![owner,device,target,now()-30*86400],|r|Ok(json!({
                    "target":target,"through":r.get::<_,i64>(0)?,"created":r.get::<_,i64>(1)?,"last":r.get::<_,String>(2)?,"unread":0
                }))).optional()?;
            if let Some(item) = message {
                return Ok(json!({"category":category,"item":item}));
            }
        }
        Ok(json!({}))
    }
    pub(crate) fn claim_push(&self) -> Result<Value> {
        let tx = self.db.unchecked_transaction()?;
        let devices = tx
            .prepare("SELECT owner,device,provider,subscription,categories FROM push_devices")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut jobs = Vec::new();
        for (owner, device, provider, sub, cats) in devices {
            if !self
                .account_chain(&owner)?
                .last()
                .is_some_and(|r| r.payload.devices.contains(&device))
            {
                continue;
            }
            let categories: Vec<String> = serde_json::from_str(&cats)?;
            let inbox = self.notification_inbox(&owner, &device)?;
            for (category, field) in [
                ("reply", "activity"),
                ("message", "messages"),
                ("request", "requests"),
            ] {
                if !categories.iter().any(|c| c == category) {
                    continue;
                }
                let start: i64 = tx.query_row(
                    "SELECT baseline FROM push_starts WHERE owner=? AND device=? AND category=?",
                    [&owner, &device, category],
                    |r| r.get(0),
                )?;
                for row in inbox[field].as_array().into_iter().flatten() {
                    let seq = row["through"].as_i64().unwrap_or(0);
                    if seq <= start || row["unread"] == 0 {
                        continue;
                    }
                    let target = row["target"].as_str().unwrap();
                    tx.execute("INSERT OR IGNORE INTO push_routes(token,owner,device,category,target) VALUES(?,?,?,?,?)",params![new_secret(),owner,device,category,target])?;
                    let route:Option<String>=tx.query_row("UPDATE push_routes SET leasing=?,retry_at=? WHERE owner=? AND device=? AND category=? AND target=? AND delivered<? AND retry_at<=? RETURNING token",
                        params![seq,now()+90,owner,device,category,target,seq,now()],|r|r.get(0)).optional()?;
                    if let Some(token) = route {
                        jobs.push(json!({"route":token,"seq":seq,"provider":provider,"subscription":serde_json::from_str::<Value>(&sub)?,"category":category}));
                    }
                    if jobs.len() >= 16 {
                        tx.commit()?;
                        return Ok(json!(jobs));
                    }
                }
            }
        }
        tx.commit()?;
        Ok(json!(jobs))
    }
    pub(crate) fn finish_push(&self, jobs: &Value, outcomes: &Value) -> Result<Value> {
        let tx = self.db.unchecked_transaction()?;
        for job in jobs.as_array().into_iter().flatten() {
            let route = job["route"].as_str().unwrap();
            let seq = job["seq"].as_i64().unwrap();
            match outcomes[route].as_str() {
                Some("gone") => {
                    tx.execute("DELETE FROM push_devices WHERE (owner,device) IN (SELECT owner,device FROM push_routes WHERE token=? AND leasing=?) AND subscription=?",
                        params![route,seq,serde_json::to_string(&job["subscription"])?])?;
                    tx.execute("DELETE FROM push_starts WHERE NOT EXISTS(SELECT 1 FROM push_devices d WHERE d.owner=push_starts.owner AND d.device=push_starts.device)",[])?;
                    tx.execute("DELETE FROM push_routes WHERE NOT EXISTS(SELECT 1 FROM push_devices d WHERE d.owner=push_routes.owner AND d.device=push_routes.device)",[])?;
                }
                Some("ok") => {
                    tx.execute("UPDATE push_routes SET delivered=MAX(delivered,?),leasing=0,retry_at=0 WHERE token=? AND leasing=?",params![seq,route,seq])?;
                }
                _ => {} // Ambiguous delivery uses the same route/collapse identifier on retry.
            }
        }
        tx.commit()?;
        Ok(json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::signing_key, portability::AccountWallet};
    #[test]
    fn delivery_is_fenced_scoped_and_rechecked_after_policy_changes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let home = hex::encode(signing_key(&new_secret())?.verifying_key().as_bytes());
        let wallet = AccountWallet::create(new_secret(), home)?;
        let record = store.register_account(&wallet.chain)?;
        let owner = &record.payload.account;
        let device = record.payload.devices.first().unwrap();
        let sender = new_secret();
        store.db.execute(
            "INSERT INTO dm_devices VALUES(?,?,?)",
            [owner, device, &new_secret()],
        )?;
        store.db.execute(
            "INSERT INTO push_devices VALUES(?,?,'fcm',?,?)",
            params![
                owner,
                device,
                "\"sample-token\"",
                "[\"message\",\"request\"]"
            ],
        )?;
        for category in ["message", "request"] {
            store.db.execute(
                "INSERT INTO push_starts VALUES(?,?,?,0)",
                params![owner, device, category],
            )?;
        }
        store.db.execute("INSERT INTO dm_mail(id,sender,recipient,packet,created,held) VALUES(?,?,?,'ciphertext',?,1)",params![new_secret(),sender,owner,now()])?;
        assert_eq!(
            store.notification_inbox(owner, &new_secret())?["requests"],
            json!([])
        );
        let jobs = store.claim_push()?;
        let route = jobs[0]["route"].as_str().unwrap();
        assert_eq!(jobs[0]["category"], "request");
        assert_eq!(store.claim_push()?, json!([]));
        assert_eq!(
            store.push_route(owner, device, route)?["item"]["target"],
            sender
        );
        assert_eq!(store.push_route(&sender, device, route)?, json!({}));
        assert_eq!(store.push_route(owner, &new_secret(), route)?, json!({}));
        store.finish_push(&jobs, &json!({route:"retry"}))?;
        store.db.execute("UPDATE push_routes SET retry_at=0", [])?;
        assert_eq!(store.claim_push()?, jobs);
        store.finish_push(&jobs, &json!({route:"ok"}))?;
        assert_eq!(store.claim_push()?, json!([]));
        crate::privacy::approve(&store.db, owner, &sender)?;
        assert_eq!(store.push_route(owner, device, route)?, json!({}));
        let messages = store.claim_push()?;
        assert_eq!(messages[0]["category"], "message");
        let message = messages[0]["route"].as_str().unwrap();
        store.read_notification(
            owner,
            device,
            "message",
            &sender,
            messages[0]["seq"].as_i64().unwrap(),
        )?;
        assert_eq!(
            store.push_route(owner, device, message)?["item"]["unread"],
            0
        );
        store.db.execute(
            "INSERT INTO user_blocks VALUES(?,?,?,'Blocked account')",
            params![owner, sender, new_secret()],
        )?;
        assert_eq!(store.push_route(owner, device, message)?, json!({}));
        store.db.execute("UPDATE push_routes SET retry_at=0", [])?;
        assert_eq!(store.claim_push()?, json!([]));
        store.db.execute(
            "UPDATE push_devices SET subscription='\"replacement-token\"'",
            [],
        )?;
        store.finish_push(&messages, &json!({message:"gone"}))?;
        assert_eq!(
            store.push_categories(owner, device)?,
            json!(["message", "request"])
        );
        store.set_push(owner, device, "fcm", Value::Null, vec![])?;
        assert_eq!(store.push_categories(owner, device)?, json!([]));
        Ok(())
    }
}
