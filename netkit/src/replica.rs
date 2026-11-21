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

impl CatalogOp<'_> {
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
        let store = &*self.store;
        let crate::feed_page::FeedPage {
            mut posts,
            events,
            event_count,
        } = store.feed_page(community, "", None)?;
        let ages = store.page_ages(&events)?;
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
        let mut deleted = store.erased_ids(community)?;
        let more = fit_feed_page(&mut posts, &mut deleted, 0)?;
        if known == Some(fingerprint(&posts, event_count)?.as_str()) {
            return Ok(Response::ProjectionUnchanged);
        }
        store.remember_deliveries(
            remote,
            posts.iter().flat_map(|p| {
                std::iter::once((community, p.id.as_str()))
                    .chain(p.replies.iter().map(|r| (community, r.id.as_str())))
            }),
        )?;
        Ok(Response::PublicReplicaFeed {
            deleted,
            posts,
            event_count,
            as_of: now(),
        }
        .with_deletion_pages(more))
    }

    pub(super) fn authorize_replica(&self, community: &str) -> Result<()> {
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
        Ok(())
    }

    pub(super) fn replica_feed(&self, community: &str) -> Result<Response> {
        self.authorize_replica(community)?;
        let store = &*self.store;
        let (raw, count, verified): (String, usize, i64) = store
            .db
            .query_row(
                "SELECT posts,event_count,verified FROM public_projections WHERE community=?",
                [community],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .context("waiting for the first primary projection")?;
        let mut posts = crate::native_projection::decode(store, community, &raw)?;
        let hidden = store.hidden()?;
        posts.retain(|p| !hidden.contains(&p.id));
        for p in &mut posts {
            p.replies.retain(|r| !hidden.contains(&r.id));
        }
        let mut deleted = store.erased_ids(community)?;
        let more = fit_feed_page(&mut posts, &mut deleted, 0)?;
        Ok(Response::PublicReplicaFeed {
            deleted,
            posts,
            event_count: count,
            as_of: verified,
        }
        .with_deletion_pages(more))
    }
    fn projection_version(&self, community: &str) -> Result<Option<(String, i64)>> {
        Ok(self
            .store
            .db
            .query_row(
                "SELECT digest,verified FROM public_projections WHERE community=?",
                [community],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    fn cache_projection(
        &mut self,
        community: &str,
        requested: &Option<(String, i64)>,
        response: &Response,
        timestamp: i64,
    ) -> Result<()> {
        self.authorize_replica(community)?;
        // A cancelled refresh can still have a worker in flight. A later refresh
        // may already have committed a different answer while this RPC was out.
        if &self.projection_version(community)? != requested {
            return Ok(());
        }
        let store = &*self.store;
        match response {
            Response::ProjectionUnchanged => {
                ensure!(requested.is_some(), "missing prior primary projection");
                store.db.execute(
                    "UPDATE public_projections SET verified=? WHERE community=?",
                    params![timestamp, community],
                )?;
            }
            Response::PublicReplicaFeed {
                posts, event_count, ..
            } => {
                // The projection can arrive ahead of its signed creation or after a
                // deletion. Only cache content backed by a locally verified live event.
                // A committed detached header identifies a live body until a deletion
                // proof is attached. Check only events in this bounded projection.
                let mut lookup = store.db.prepare_cached(
                    "SELECT EXISTS(SELECT 1 FROM events WHERE id=?1 AND community=?2
                       AND json_extract(envelope,'$.deletion') IS NULL
                       AND json_extract(envelope,'$.event.payload.action.kind')
                           IN ('post','postV2','reply','erased'))",
                )?;
                let mut retained = Vec::with_capacity(posts.len());
                for post in posts {
                    if !lookup.query_row([&post.id, community], |r| r.get::<_, bool>(0))? {
                        continue;
                    }
                    let mut post = post.clone();
                    let mut replies = Vec::with_capacity(post.replies.len());
                    for reply in post.replies {
                        if lookup.query_row([&reply.id, community], |r| r.get::<_, bool>(0))? {
                            replies.push(reply);
                        }
                    }
                    post.replies = replies;
                    retained.push(post);
                }
                let posts = retained;
                let digest = fingerprint(&posts, *event_count)?;
                // Replacing a moderated projection may withdraw a previously visible
                // post, reply, or byline. Expire replies built from the previous page.
                self.disclosure
                    .restrict_if(requested.as_ref().is_some_and(|(old, _)| old != &digest))?;
                store.db.execute("INSERT INTO public_projections(community,digest,posts,event_count,verified) VALUES(?,?,?,?,?)
                   ON CONFLICT(community) DO UPDATE SET digest=excluded.digest, posts=excluded.posts, event_count=excluded.event_count, verified=excluded.verified",
                   params![community,digest,crate::native_projection::encode(store,community,&posts)?,event_count,timestamp])?;
            }
            _ => anyhow::bail!("invalid primary projection"),
        }
        Ok(())
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
        let scope = community.clone();
        let cached = self
            .state
            .blocking(move |state| state.projection_version(&scope))
            .await?;
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
        self.state
            .blocking(move |state| {
                state.cache_projection(&community, &cached, &response, timestamp)
            })
            .await
    }
}
