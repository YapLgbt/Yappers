// SPDX-License-Identifier: AGPL-3.0-only
//! Installation-scoped, opt-in delivery. The provider receives only a generic
//! alert and an opaque route token. Opening it requires the enrolled account/device.
use crate::{
    messaging::mail_cutoff,
    model::{new_secret, now},
    store::Store,
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const PUSH_BATCH: i64 = 16;
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
                ORDER BY m.seq DESC LIMIT 1",params![owner,device,target,mail_cutoff(now())],|r|Ok(json!({
                    "target":target,"through":r.get::<_,i64>(0)?,"created":r.get::<_,i64>(1)?,"last":r.get::<_,String>(2)?,"unread":0
                }))).optional()?;
            if let Some(item) = message {
                return Ok(json!({"category":category,"item":item}));
            }
        }
        Ok(json!({}))
    }
    fn pending_mail_pushes(
        &self,
        owner: &str,
        device: &str,
        category: &str,
        baseline: i64,
        time: i64,
    ) -> Result<Vec<(String, i64)>> {
        let mut statement = self.db.prepare_cached(
            "SELECT m.sender,MAX(m.seq) AS latest
             FROM dm_mail m
             JOIN dm_devices d ON d.owner=m.recipient AND d.device=?2
             LEFT JOIN inbox_reads a ON a.owner=m.recipient AND a.kind='message' AND a.target=m.sender
             LEFT JOIN push_routes r ON r.owner=m.recipient AND r.device=?2 AND r.category=?3 AND r.target=m.sender
             WHERE m.recipient=?1 AND m.seq>?4 AND m.created>=?5
               AND m.seq>COALESCE(r.delivered,0) AND COALESCE(r.retry_at,0)<=?6
               AND ((?3='request' AND m.held=1 AND m.packet IS NOT NULL)
                 OR (?3='message' AND m.held=0 AND m.seq>COALESCE(a.through,0)))
               AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE
                 (b.owner=m.recipient AND b.peer=m.sender) OR (b.owner=m.sender AND b.peer=m.recipient))
             GROUP BY m.sender ORDER BY latest DESC LIMIT ?7",
        )?;
        Ok(statement
            .query_map(
                params![
                    owner,
                    device,
                    category,
                    baseline,
                    mail_cutoff(time),
                    time,
                    PUSH_BATCH
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn pending_reply_pushes(
        &self,
        owner: &str,
        device: &str,
        baseline: i64,
        time: i64,
    ) -> Result<Vec<(String, i64)>> {
        if !self.is_native() {
            let mut statement = self.db.prepare_cached(
                "SELECT n.target,MAX(n.seq) AS latest
                 FROM inbox_visible n
                 LEFT JOIN inbox_reads a ON a.owner=n.owner AND a.kind='reply' AND a.target=n.target
                 LEFT JOIN push_routes r ON r.owner=n.owner AND r.device=?2 AND r.category='reply' AND r.target=n.target
                 WHERE n.owner=?1 AND n.seq>MAX(?3,COALESCE(a.through,0),COALESCE(r.delivered,0))
                   AND COALESCE(r.retry_at,0)<=?4
                 GROUP BY n.target ORDER BY latest DESC LIMIT ?5",
            )?;
            return Ok(statement
                .query_map(params![owner, device, baseline, time, PUSH_BATCH], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?);
        }
        let mut statement = self.db.prepare_cached(
            "SELECT e.community,e.id,MAX(?3,COALESCE(a.through,0),COALESCE(r.delivered,0))
             FROM events e
             LEFT JOIN inbox_reads a ON a.owner=?1 AND a.kind='reply' AND a.target=e.id
             LEFT JOIN push_routes r ON r.owner=?1 AND r.device=?2 AND r.category='reply' AND r.target=e.id
             WHERE COALESCE(json_extract(e.envelope,'$.event.payload.account.payload.account'),e.author)=?1
               AND json_extract(e.envelope,'$.event.payload.action.kind')='erased'
               AND json_extract(e.envelope,'$.event.payload.action.reply')=0
               AND json_extract(e.envelope,'$.deletion') IS NULL
               AND COALESCE(r.retry_at,0)<=?4
               AND NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=e.id)
               AND NOT EXISTS(SELECT 1 FROM inbox_mutes m WHERE m.owner=?1 AND m.target=e.id)",
        )?;
        let mut by_community = BTreeMap::<String, Vec<_>>::new();
        for row in statement.query_map(params![owner, device, baseline, time], |row| {
            Ok((row.get(0)?, (row.get(1)?, row.get(2)?)))
        })? {
            let (community, parent) = row?;
            by_community.entry(community).or_default().push(parent);
        }
        let prefs = crate::privacy::settings(&self.db, owner)?;
        let mut lookup = self.db.prepare_cached(
            "SELECT CASE WHEN ?3 THEN e.envelope ELSE NULL END,
                    COALESCE(json_extract(e.envelope,'$.event.payload.account.payload.account'),e.author)
             FROM events e WHERE e.id=?1 AND CAST(e.community AS TEXT)=?2
               AND json_extract(e.envelope,'$.deletion') IS NULL
               AND NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=e.id)",
        )?;
        let mut actor = self
            .db
            .prepare_cached(crate::notifications::REPLY_ACTOR_VISIBLE)?;
        let mut opening = self
            .db
            .prepare_cached("SELECT routing FROM home_openings WHERE target=? AND private=0")?;
        let mut pending = Vec::new();
        for (community, parents) in by_community {
            pending.extend(crate::feed_replies::pending_arrivals(
                &self.db,
                &community,
                &parents,
                |id| {
                    let found: Option<(Option<String>, String)> = lookup
                        .query_row(params![id, community, prefs.hide_anonymous], |row| {
                            Ok((row.get(0)?, row.get(1)?))
                        })
                        .optional()?;
                    let Some((header, author)) = found else {
                        return Ok(false);
                    };
                    if author == owner
                        || !actor
                            .query_row(params![owner, author, time, prefs.feed_days], |row| {
                                row.get::<_, bool>(0)
                            })?
                    {
                        return Ok(false);
                    }
                    if let Some(header) = header {
                        let header = serde_json::from_str(&header)?;
                        let sealed = opening.query_row([id], |row| row.get::<_, Vec<u8>>(0))?;
                        return Ok(!crate::native_content::reply_route(
                            &self.db, &header, &sealed,
                        )?
                        .1);
                    }
                    Ok(true)
                },
            )?);
        }
        pending.sort_unstable_by_key(|(_, cursor)| std::cmp::Reverse(*cursor));
        pending.truncate(PUSH_BATCH as usize);
        Ok(pending)
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
            for category in ["reply", "message", "request"] {
                if !categories.iter().any(|c| c == category) {
                    continue;
                }
                let start: i64 = tx.query_row(
                    "SELECT baseline FROM push_starts WHERE owner=? AND device=? AND category=?",
                    [&owner, &device, category],
                    |r| r.get(0),
                )?;
                let time = now();
                let pending = if category == "reply" {
                    self.pending_reply_pushes(&owner, &device, start, time)?
                } else {
                    self.pending_mail_pushes(&owner, &device, category, start, time)?
                };
                for (target, seq) in pending {
                    tx.execute("INSERT OR IGNORE INTO push_routes(token,owner,device,category,target) VALUES(?,?,?,?,?)",params![new_secret(),owner,device,category,target])?;
                    let route:Option<String>=tx.query_row("UPDATE push_routes SET leasing=?,retry_at=? WHERE owner=? AND device=? AND category=? AND target=? AND delivered<? AND retry_at<=? RETURNING token",
                        params![seq,time+90,owner,device,category,target,seq,time],|r|r.get(0)).optional()?;
                    if let Some(token) = route {
                        jobs.push(json!({"route":token,"seq":seq,"provider":provider,"subscription":serde_json::from_str::<Value>(&sub)?,"category":category}));
                    }
                    if jobs.len() >= PUSH_BATCH as usize {
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
    use crate::{
        model::{Action, Envelope, SignedEvent, signing_key},
        portability::AccountWallet,
    };

    #[test]
    fn reply_pushes_follow_late_parents_and_recheck_visibility_without_reading_bodies() -> Result<()>
    {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let key = signing_key(&new_secret())?;
        let home = hex::encode(key.verifying_key().as_bytes());
        let wallet = AccountWallet::create(new_secret(), home.clone())?;
        let record = store.register_account(&wallet.chain)?;
        let owner = &record.payload.account;
        let device = record.payload.devices.first().unwrap();
        let author = signing_key(&wallet.device_secret)?;
        let neighbor = signing_key(&new_secret())?;
        let parent = Envelope::accept(
            SignedEvent::create_for_account(
                &author,
                home.clone(),
                "scope".into(),
                1,
                Action::Post {
                    body: "Parent".into(),
                    topic: "Plans".into(),
                    display_name: "Owner".into(),
                    anonymous: false,
                },
                record.clone(),
            )?,
            &key,
        )?;
        let reply = Envelope::accept(
            SignedEvent::create(
                &neighbor,
                home,
                "scope".into(),
                1,
                Action::Reply {
                    parent: parent.event.id.clone(),
                    body: "Reply".into(),
                    display_name: "Anonymous neighbor".into(),
                },
            )?,
            &key,
        )?;
        store.db.execute(
            "INSERT INTO push_devices VALUES(?,?,'fcm','\"sample-token\"','[\"reply\"]')",
            params![owner, device],
        )?;
        store.db.execute(
            "INSERT INTO push_starts VALUES(?,?,'reply',0)",
            params![owner, device],
        )?;
        store.accept(&reply)?;
        assert_eq!(store.claim_push()?, json!([]));
        store.accept(&parent)?;
        store.update_privacy(
            owner,
            &crate::messaging::PrivacySettings {
                hide_anonymous: true,
                ..Default::default()
            },
        )?;
        assert_eq!(store.claim_push()?, json!([]));
        store.db.execute(
            "UPDATE home_openings SET sealed=x'00' WHERE target=?",
            [&reply.event.id],
        )?;
        store.update_privacy(owner, &Default::default())?;
        let jobs = store.claim_push()?;
        let route = jobs[0]["route"].as_str().unwrap();
        assert_eq!(jobs[0]["category"], "reply");
        store.finish_push(&jobs, &json!({route: "ok"}))?;
        assert_eq!(store.claim_push()?, json!([]));
        Ok(())
    }

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
