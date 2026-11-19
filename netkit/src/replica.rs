// SPDX-License-Identifier: AGPL-3.0-only
//! The primary supplies the moderated public projection directly over authenticated Iroh.
//! Per-account blocks, proofs, location queries and mailbox contents never enter this cache.
use super::*;
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

fn fingerprint(posts: &[RemotePost], count: usize) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(&(posts, count))?)
        .to_hex()
        .to_string())
}

impl State {
    pub(super) fn public_projection(
        &self,
        remote: &str,
        community: &str,
        known: Option<&str>,
    ) -> Result<Response> {
        ensure!(
            community == format!("{}/public", self.logical_addr().id)
                && self.peer(remote)?.communities.contains(community),
            "public projection not authorized"
        );
        let store = self.store()?;
        let events = store.events(community)?;
        let ages = store.ages()?;
        let mut posts = project(&events, community, "", &store.hidden()?);
        store.apply_current_bylines(&mut posts, &events)?;
        let metadata: HashMap<_, _> = events.iter().map(|e| (e.event.id.as_str(), e)).collect();
        for p in &mut posts {
            p.visibility = metadata
                .get(p.id.as_str())
                .map(|e| crate::privacy::visibility(e, &ages));
            for r in &mut p.replies {
                r.visibility = metadata
                    .get(r.id.as_str())
                    .map(|e| crate::privacy::visibility(e, &ages));
            }
        }
        let deleted = store.erased_ids(community)?;
        let overhead = serde_json::to_vec(&deleted)?.len() + 1024;
        while serde_json::to_vec(&posts)?.len() + overhead > MAX_FRAME {
            posts.pop();
        }
        if known == Some(fingerprint(&posts, events.len())?.as_str()) {
            return Ok(Response::ProjectionUnchanged);
        }
        if self.peering.as_ref().is_some_and(|p| p.external(remote)) {
            store.remember_deliveries(
                remote,
                posts.iter().flat_map(|p| {
                    std::iter::once((community, p.id.as_str()))
                        .chain(p.replies.iter().map(|r| (community, r.id.as_str())))
                }),
            )?;
        }
        Ok(Response::PublicReplicaFeed {
            deleted,
            posts,
            event_count: events.len(),
            as_of: now(),
        })
    }

    pub(super) fn replica_feed(&self, community: &str) -> Result<Response> {
        ensure!(
            self.public_replica_feeds.contains(community),
            "public replica unavailable"
        );
        let owner = community
            .strip_suffix("/public")
            .context("invalid public replica scope")?;
        ensure!(
            self.peer(owner)?.communities.contains(community),
            "public replica unavailable"
        );
        let store = self.store()?;
        let (raw, count, verified): (String, usize, i64) = store
            .db
            .query_row(
                "SELECT posts,event_count,verified FROM public_projections WHERE community=?",
                [community],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .context("waiting for the first primary projection")?;
        let mut posts: Vec<RemotePost> = serde_json::from_str(&raw)?;
        let hidden = store.hidden()?;
        posts.retain(|p| !hidden.contains(&p.id));
        for p in &mut posts {
            p.replies.retain(|r| !hidden.contains(&r.id));
        }
        Ok(Response::PublicReplicaFeed {
            deleted: store.erased_ids(community)?,
            posts,
            event_count: count,
            as_of: verified,
        })
    }
}

impl Server {
    pub(super) async fn refresh_public_projection(&self, peer: &PeerConfig) -> Result<()> {
        let community = format!("{}/public", peer.address.id);
        if !self.state.public_replica_feeds.contains(&community)
            || !peer.communities.contains(&community)
        {
            return Ok(());
        }
        let cached = self
            .state
            .store()?
            .db
            .query_row(
                "SELECT digest,verified FROM public_projections WHERE community=?",
                [&community],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        let timestamp = now();
        if cached
            .as_ref()
            .is_some_and(|(_, verified)| (0..15).contains(&(timestamp - verified)))
        {
            return Ok(());
        }
        let response = transport::rpc(
            &self.endpoint,
            peer.address.clone(),
            &Request::PullPublicProjection {
                community: community.clone(),
                digest: cached.as_ref().map(|(hash, _)| hash.clone()),
            },
        )
        .await?;
        let store = self.state.store()?;
        match response {
            Response::ProjectionUnchanged => {
                ensure!(cached.is_some(), "missing prior primary projection");
                store.db.execute(
                    "UPDATE public_projections SET verified=? WHERE community=?",
                    params![timestamp, community],
                )?;
            }
            Response::PublicReplicaFeed {
                mut posts,
                event_count,
                ..
            } => {
                // The projection can arrive ahead of its signed creation or after a
                // deletion. Only cache content backed by a locally verified live event.
                let live = store.db.prepare("SELECT id FROM events WHERE community=? AND json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply')")?
                    .query_map([&community],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<BTreeSet<_>>>()?;
                posts.retain(|p| live.contains(&p.id));
                for post in &mut posts {
                    post.replies.retain(|r| live.contains(&r.id));
                }
                let digest = fingerprint(&posts, event_count)?;
                store.db.execute("INSERT INTO public_projections(community,digest,posts,event_count,verified) VALUES(?,?,?,?,?)
                   ON CONFLICT(community) DO UPDATE SET digest=excluded.digest, posts=excluded.posts, event_count=excluded.event_count, verified=excluded.verified",
                   params![community,digest,serde_json::to_string(&posts)?,event_count,timestamp])?;
            }
            _ => anyhow::bail!("invalid primary projection"),
        }
        Ok(())
    }
}
