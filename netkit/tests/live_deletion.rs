//! Opt-in release check. All three roles are addressed by their physical Iroh IDs.
use anyhow::{Context, Result, ensure};
use iroh::{Endpoint, EndpointAddr};
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};
use yappers_netkit::{model::*, portability::AccountWallet, takeover::FullHomeStandby, transport};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Enrollment {
    join: JoinConfig,
    public_feed_replica: EndpointAddr,
    full_home_standby: FullHomeStandby,
}

async fn submit(
    endpoint: &Endpoint,
    addr: &EndpointAddr,
    join: &JoinConfig,
    wallet: &AccountWallet,
    event: &SignedEvent,
) -> Result<Envelope> {
    let response = transport::rpc(
        endpoint,
        addr.clone(),
        &Request::Submit {
            token: join.access_token.clone(),
            event: event.clone(),
            account_chain: wallet.chain.clone(),
        },
    )
    .await?;
    let Response::Accepted {
        event_id,
        envelope: Some(receipt),
        ..
    } = response
    else {
        anyhow::bail!("missing home receipt")
    };
    receipt.validate()?;
    ensure!(
        event.id == event_id
            && receipt.event.id == event.id
            && receipt.event.signature == event.signature
    );
    Ok(*receipt)
}

async fn replica_sees(
    endpoint: &Endpoint,
    m: &Enrollment,
    post: &SignedEvent,
    reply: &SignedEvent,
    deleted: bool,
) -> Result<()> {
    let request = Request::PublicReplicaFeed {
        community: m.join.community.clone(),
    };
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            if let Ok(Response::PublicReplicaFeed {
                posts,
                deleted: ids,
                ..
            }) = transport::rpc(endpoint, m.public_feed_replica.clone(), &request).await
            {
                let entry = posts.iter().find(|p| p.id == post.id);
                let ready = if deleted {
                    entry.is_none() && ids.contains(&post.id) && ids.contains(&reply.id)
                } else {
                    entry.is_some_and(|p| p.replies.iter().any(|r| r.id == reply.id))
                };
                if ready {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
    .await
    .context("public federation peer did not converge")?;
    Ok(())
}

#[tokio::test]
#[ignore = "publishes and deletes a disposable public post and reply on YAP_SMOKE_ENROLLMENT"]
async fn deployed_deletion_end_to_end() -> Result<()> {
    let m: Enrollment =
        serde_json::from_reader(std::fs::File::open(std::env::var("YAP_SMOKE_ENROLLMENT")?)?)?;
    m.join.validate()?;
    let home = m.join.server_addr.id.to_string();
    m.full_home_standby.validate(&home)?;
    ensure!(m.join.protocol_version == 5 && m.join.community == format!("{home}/public"));
    let primary = &m.join.server_addr;
    let standby = &m.full_home_standby.standby;
    ensure!(m.public_feed_replica.id != primary.id && m.public_feed_replica.id != standby.id);
    let endpoint = transport::bind(m.join.network_mode, None, false).await?;
    let wallet = AccountWallet::create(new_secret(), home.clone())?;
    let make = |seq, action| {
        SignedEvent::create_for_account(
            &signing_key(&wallet.device_secret)?,
            home.clone(),
            m.join.community.clone(),
            seq,
            action,
            wallet.validate()?.clone(),
        )
    };
    let tag = new_secret();
    let post = make(
        1,
        Action::Post {
            body: format!("Yap deletion release check {}", &tag[..12]),
            topic: "Plans".into(),
            anonymous: true,
            display_name: "Anonymous neighbor".into(),
        },
    )?;
    let reply = make(
        2,
        Action::Reply {
            parent: post.id.clone(),
            body: format!("Yap reply release check {}", &tag[..12]),
            display_name: "Anonymous neighbor".into(),
        },
    )?;
    let remove_reply = make(
        3,
        Action::Delete {
            target: reply.id.clone(),
        },
    )?;
    let remove_post = make(
        4,
        Action::Delete {
            target: post.id.clone(),
        },
    )?;
    let live_post = submit(&endpoint, primary, &m.join, &wallet, &post).await?;
    let live_check = async {
        let live_reply = submit(&endpoint, standby, &m.join, &wallet, &reply).await?;
        replica_sees(&endpoint, &m, &post, &reply, false).await?;
        println!(
            "Live creation and replication verified. Post {} reply {}",
            post.id, reply.id
        );
        if let Some(path) = std::env::var_os("YAP_SMOKE_CHECKPOINT") {
            let dir = PathBuf::from(path);
            std::fs::write(
                dir.join("verified-live-ids.json"),
                serde_json::to_vec(&[&post.id, &reply.id])?,
            )?;
            tokio::time::timeout(Duration::from_secs(150), async {
                while !dir.join("backup-complete").exists() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            })
            .await
            .context("waiting for operator backup check")?;
        }
        anyhow::Ok(live_reply)
    }
    .await;
    // Always attempt both removals, even if verification or backup capture failed.
    let r = submit(&endpoint, standby, &m.join, &wallet, &remove_reply).await;
    let p = submit(&endpoint, primary, &m.join, &wallet, &remove_post).await;
    let live_reply = live_check?;
    let deletes = [p?, r?];
    for route in [primary, standby] {
        for ((event, live), proof) in [(&post, &live_post), (&reply, &live_reply)]
            .into_iter()
            .zip(&deletes)
        {
            let retained = submit(&endpoint, route, &m.join, &wallet, event).await?;
            ensure!(retained.same_acceptance(live)? && retained.deletion.as_deref() == Some(proof));
            ensure!(
                retained.event.payload.nonce.is_empty()
                    && matches!(retained.event.payload.action, Action::Erased { .. })
            );
        }
        let feed = transport::rpc(
            &endpoint,
            route.clone(),
            &Request::Feed {
                token: m.join.access_token.clone(),
                community: m.join.community.clone(),
                author: String::new(),
                nearby: None,
            },
        )
        .await?;
        let Response::Feed { posts, deleted, .. } = feed else {
            anyhow::bail!("unexpected home feed")
        };
        ensure!(
            posts.iter().all(|p| p.id != post.id)
                && deleted.contains(&post.id)
                && deleted.contains(&reply.id)
        );
    }
    replica_sees(&endpoint, &m, &post, &reply, true).await?;
    println!(
        "Both physical home routes verified the original signatures and deletion proofs after stale replay. The public replica confirmed both deletions."
    );
    endpoint.close().await;
    Ok(())
}
