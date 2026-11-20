// SPDX-License-Identifier: AGPL-3.0-only
//! Select public threads in display order before opening their protected content.
use crate::{model::*, store::Store};
use anyhow::{Result, ensure};
use rusqlite::types::Value;
use std::collections::BTreeSet;

pub(crate) struct FeedPage {
    pub posts: Vec<RemotePost>,
    pub events: Vec<Envelope>,
    pub event_count: usize,
}

impl Store {
    pub(crate) fn filtered_feed_page(
        &self,
        community: &str,
        viewer: &str,
        nearby: Option<&Nearby>,
        mut visible: impl FnMut(&Envelope) -> bool,
    ) -> Result<FeedPage> {
        let mut events = self.events(community)?;
        events.retain(|event| visible(event));
        let mut posts = project_nearby(&events, community, viewer, &self.hidden()?, nearby);
        self.apply_current_bylines(&mut posts, &events)?;
        Ok(FeedPage {
            posts,
            event_count: events.len(),
            events,
        })
    }

    pub(crate) fn feed_page(
        &self,
        community: &str,
        viewer: &str,
        nearby: Option<&Nearby>,
    ) -> Result<FeedPage> {
        if !self.is_native() {
            return self.filtered_feed_page(community, viewer, nearby, |_| true);
        }
        ensure!(
            !self.db.is_autocommit()?,
            "feed page requires a pinned snapshot"
        );
        let mut events = self.feed_roots(community, nearby)?;
        let roots = events
            .iter()
            .map(|e| e.event.id.clone())
            .collect::<Vec<_>>();
        let replies = crate::feed_replies::latest(&self.db, community, &roots)?;
        let ids: Vec<_> = replies.into_values().flatten().collect();
        for batch in ids.chunks(FEED_POST_LIMIT) {
            let mut statement = self.db.prepare(
                "SELECT e.envelope FROM json_each(?1) ids CROSS JOIN events e
                 WHERE e.id=CAST(ids.value AS TEXT) AND e.community=?2 AND json_extract(e.envelope,'$.deletion') IS NULL
                   AND NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=e.id)",
            )?;
            let rows = statement.query_map(
                rusqlite::params![serde_json::to_string(batch)?, community],
                |r| r.get::<_, String>(0),
            )?;
            let mut replies = Vec::with_capacity(batch.len());
            for raw in rows {
                replies.push(serde_json::from_str(&raw?)?);
            }
            ensure!(replies.len() == batch.len(), "incomplete feed reply page");
            crate::native_content::hydrate_public_page(&self.db, &mut replies)?;
            events.extend(replies);
        }
        // Candidate selection has already excluded moderated and erased entries. Reuse
        // the shared model for thread ordering, ownership and wire representation.
        let mut posts = project(&events, community, viewer, &BTreeSet::new());
        let votes = self.feed_votes(community, &roots, viewer)?;
        for post in &mut posts {
            (post.score, post.vote) = votes.get(&post.id).copied().unwrap_or_default();
        }
        self.apply_current_bylines(&mut posts, &events)?;
        Ok(FeedPage {
            posts,
            events,
            event_count: self.feed_event_count(community)?,
        })
    }

    fn feed_roots(&self, community: &str, nearby: Option<&Nearby>) -> Result<Vec<Envelope>> {
        const SELECT: &str = "SELECT envelope FROM events e WHERE community=?
            AND json_extract(envelope,'$.event.payload.action.kind')='erased'
            AND json_extract(envelope,'$.event.payload.action.reply')=0
            AND json_extract(envelope,'$.deletion') IS NULL
            AND NOT EXISTS(SELECT 1 FROM hidden h WHERE h.id=e.id)";
        const ORDER: &str = " ORDER BY json_extract(envelope,'$.acceptedAt') DESC,id DESC LIMIT ?";
        let mut selected = Vec::new();
        let mut after = None;
        loop {
            let mut sql = SELECT.to_owned();
            let mut args = vec![Value::Text(community.to_owned())];
            if let Some((accepted_at, id)) = after.take() {
                // Give Turso an explicit range on the leading time index column.
                sql.push_str(" AND json_extract(envelope,'$.acceptedAt')<=? AND (json_extract(envelope,'$.acceptedAt')<? OR id<?)");
                args.extend([
                    Value::Integer(accepted_at),
                    Value::Integer(accepted_at),
                    Value::Text(id),
                ]);
            }
            sql.push_str(ORDER);
            args.push(Value::Integer(FEED_POST_LIMIT as i64));
            let mut statement = self.db.prepare(&sql)?;
            let mut batch: Vec<Envelope> = Vec::new();
            for raw in statement.query_map(args, |r| r.get::<_, String>(0))? {
                batch.push(serde_json::from_str(&raw?)?);
            }
            let full = batch.len() == FEED_POST_LIMIT;
            if let Some(last) = batch.last() {
                after = Some((last.accepted_at, last.event.id.clone()));
            }
            crate::native_content::hydrate_public_page(&self.db, &mut batch)?;
            if let Some(filter) = nearby {
                batch.retain(|event| match &event.event.payload.action {
                    Action::PostV2 {
                        area: Some(area), ..
                    } => filter.contains(area),
                    _ => false,
                });
            }
            selected.extend(batch.into_iter().take(FEED_POST_LIMIT - selected.len()));
            if selected.len() == FEED_POST_LIMIT || !full {
                return Ok(selected);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_page_matches_projection_and_does_not_open_older_bodies() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("home"))?;
        let key = signing_key(&new_secret())?;
        let author = hex::encode(key.verifying_key().as_bytes());
        let place = PostArea {
            label: "Here".into(),
            lat100: 100,
            lon100: 200,
        };
        let make = |seq, action| {
            Envelope::accept(
                SignedEvent::create(&key, author.clone(), "city".into(), seq, action)?,
                &key,
            )
        };
        let mut posts = Vec::new();
        for seq in 1..=(FEED_POST_LIMIT + 3) as u64 {
            posts.push(make(
                seq,
                Action::PostV2 {
                    anonymous: false,
                    display_name: "Neighbor".into(),
                    headline: "A thread".into(),
                    body: seq.to_string(),
                    topic: "Plans".into(),
                    profile: None,
                    area: (seq % 2 == 0).then(|| place.clone()),
                },
            )?);
        }
        posts.sort_by(|a, b| (b.accepted_at, &b.event.id).cmp(&(a.accepted_at, &a.event.id)));
        let parent = posts[1].event.id.clone();
        let reply = make(
            1000,
            Action::Reply {
                parent: parent.clone(),
                body: "An early reply".into(),
                display_name: "Neighbor".into(),
            },
        )?;
        let vote = make(
            1001,
            Action::Vote {
                target: parent,
                value: 1,
            },
        )?;
        store.operation(|s| {
            s.accept(&reply)?;
            s.accept(&vote)?;
            for post in &posts {
                s.accept(post)?;
            }
            s.moderate(&posts[0].event.id, true)?;
            Ok(())
        })?;
        let nearby = Nearby {
            center: place,
            radius_miles: 1,
        };
        store.operation(|s| {
            for filter in [None, Some(&nearby)] {
                let page = s.feed_page("city", &author, filter)?;
                let expected = s.filtered_feed_page("city", &author, filter, |_| true)?;
                assert_eq!(page.event_count, expected.event_count);
                assert_eq!(
                    serde_json::to_value(page.posts)?,
                    serde_json::to_value(expected.posts)?
                );
            }
            Ok(())
        })?;
        // A warm refresh must never touch an opening beyond the requested window.
        let last = &posts.last().unwrap().event.id;
        store.operation(|s| {
            s.db.execute(
                "UPDATE home_openings SET sealed=x'00' WHERE target=?",
                [last],
            )?;
            Ok(())
        })?;
        store.operation(|s| {
            assert_eq!(s.feed_page("city", "", None)?.posts.len(), FEED_POST_LIMIT);
            assert!(s.existing(last).is_err());
            Ok(())
        })
    }
}
