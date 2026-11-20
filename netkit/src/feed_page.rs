// SPDX-License-Identifier: AGPL-3.0-only
//! Select public threads in display order before opening their protected content.
use crate::{messaging::PrivacySettings, model::*, store::Store};
use anyhow::{Result, ensure};
use rusqlite::types::Value;
use std::collections::{BTreeSet, HashSet};

pub(crate) struct FeedPage {
    pub posts: Vec<RemotePost>,
    pub events: Vec<Envelope>,
    pub event_count: usize,
}

struct FeedVisibility<'a> {
    viewer: &'a str,
    blocked: &'a HashSet<String>,
    settings: &'a PrivacySettings,
}

impl FeedVisibility<'_> {
    fn filter_authors(&self, store: &Store, events: &mut Vec<Envelope>) -> Result<()> {
        events.retain(|event| !self.blocked.contains(event.event.owner_id()));
        if self.settings.feed_days > 0 {
            let ages = store.page_ages(events)?;
            events.retain(|event| {
                let author = event.event.owner_id();
                author == self.viewer
                    || ages.get(author).copied().unwrap_or(0) >= self.settings.feed_days
            });
        }
        Ok(())
    }

    fn hides_anonymous(&self, event: &Envelope) -> bool {
        self.settings.hide_anonymous && event.event.owner_id() != self.viewer
    }
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
        let event_count = events.len();
        events.retain(|event| visible(event));
        let mut posts = project_nearby(&events, community, viewer, &self.hidden()?, nearby);
        self.apply_current_bylines(&mut posts, &events)?;
        Ok(FeedPage {
            posts,
            event_count,
            events,
        })
    }

    pub(crate) fn feed_page(
        &self,
        community: &str,
        viewer: &str,
        nearby: Option<&Nearby>,
    ) -> Result<FeedPage> {
        if self.is_native() {
            self.native_feed_page(community, viewer, nearby, None)
        } else {
            self.filtered_feed_page(community, viewer, nearby, |_| true)
        }
    }

    pub(crate) fn personalized_feed_page(
        &self,
        community: &str,
        viewer: &str,
        nearby: Option<&Nearby>,
        blocked: &HashSet<String>,
        settings: &PrivacySettings,
    ) -> Result<FeedPage> {
        if blocked.is_empty() && !settings.hide_anonymous && settings.feed_days == 0 {
            return self.feed_page(community, viewer, nearby);
        }
        let visibility = FeedVisibility {
            viewer,
            blocked,
            settings,
        };
        if self.is_native() {
            return self.native_feed_page(community, viewer, nearby, Some(&visibility));
        }
        // The HA image stores unsealed event actions.
        let ages = if settings.feed_days > 0 {
            self.ages()?
        } else {
            Default::default()
        };
        self.filtered_feed_page(community, viewer, nearby, |event| {
            let owner = event.event.owner_id();
            !blocked.contains(owner)
                && (owner == viewer
                    || !crate::native_content::is_content(&event.event.payload.action)
                    || ((!settings.hide_anonymous || crate::usernames::named(&event.event))
                        && ages.get(owner).copied().unwrap_or(0) >= settings.feed_days))
        })
    }

    fn native_feed_page(
        &self,
        community: &str,
        viewer: &str,
        nearby: Option<&Nearby>,
        visibility: Option<&FeedVisibility<'_>>,
    ) -> Result<FeedPage> {
        ensure!(
            !self.db.is_autocommit()?,
            "feed page requires a pinned snapshot"
        );
        let mut events = self.feed_roots(community, nearby, visibility)?;
        let roots = events
            .iter()
            .map(|e| e.event.id.clone())
            .collect::<Vec<_>>();
        let replies = if let Some(visibility) = visibility {
            crate::feed_replies::latest_where(&self.db, community, &roots, |db, community, ids| {
                let mut statement = db.prepare_cached(&crate::feed_replies::VISIBLE_HEADERS)?;
                let mut headers = Vec::new();
                for raw in statement.query_map(
                    rusqlite::params![serde_json::to_string(ids)?, community],
                    |row| row.get::<_, String>(0),
                )? {
                    headers.push(serde_json::from_str(&raw?)?);
                }
                visibility.filter_authors(self, &mut headers)?;
                let mut visible = HashSet::new();
                let mut routing = db.prepare_cached(
                    "SELECT routing FROM home_openings WHERE target=? AND private=0",
                )?;
                for header in headers {
                    if visibility.hides_anonymous(&header) {
                        let sealed = routing
                            .query_row([&header.event.id], |row| row.get::<_, Vec<u8>>(0))?;
                        if crate::native_content::reply_route(db, &header, &sealed)?.1 {
                            continue;
                        }
                    }
                    visible.insert(header.event.id);
                }
                Ok(visible)
            })?
        } else {
            crate::feed_replies::latest(&self.db, community, &roots)?
        };
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
        let mut votes = self.feed_votes(community, &roots, viewer)?;
        if visibility.is_some_and(|v| !v.blocked.is_empty()) {
            self.remove_blocked_votes(community, &roots, viewer, &mut votes)?;
        }
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

    fn feed_roots(
        &self,
        community: &str,
        nearby: Option<&Nearby>,
        visibility: Option<&FeedVisibility<'_>>,
    ) -> Result<Vec<Envelope>> {
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
            let limit = (FEED_POST_LIMIT - selected.len()).max(32);
            args.push(Value::Integer(limit as i64));
            let mut statement = self.db.prepare(&sql)?;
            let mut batch: Vec<Envelope> = Vec::new();
            for raw in statement.query_map(args, |r| r.get::<_, String>(0))? {
                batch.push(serde_json::from_str(&raw?)?);
            }
            let full = batch.len() == limit;
            if let Some(last) = batch.last() {
                after = Some((last.accepted_at, last.event.id.clone()));
            }
            if let Some(visibility) = visibility {
                visibility.filter_authors(self, &mut batch)?;
            }
            crate::native_content::hydrate_public_page(&self.db, &mut batch)?;
            if let Some(visibility) = visibility {
                batch.retain(|event| {
                    !visibility.hides_anonymous(event) || crate::usernames::named(&event.event)
                });
            }
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
                target: parent.clone(),
                value: 1,
            },
        )?;
        let voter = signing_key(&new_secret())?;
        let blocked_vote = Envelope::accept(
            SignedEvent::create(
                &voter,
                author.clone(),
                "city".into(),
                1,
                Action::Vote {
                    target: parent.clone(),
                    value: 1,
                },
            )?,
            &key,
        )?;
        store.operation(|s| {
            s.accept(&reply)?;
            s.accept(&vote)?;
            s.accept(&blocked_vote)?;
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
            let settings = PrivacySettings {
                hide_anonymous: true,
                ..PrivacySettings::default()
            };
            let blocked = blocked_vote.event.owner_id();
            s.db.execute(
                "INSERT INTO user_blocks VALUES(?,?,?,'Blocked account')",
                [&author, blocked, &new_secret()],
            )?;
            let page = s.personalized_feed_page(
                "city",
                &author,
                None,
                &HashSet::from([blocked.to_owned()]),
                &settings,
            )?;
            assert_eq!(page.posts.len(), FEED_POST_LIMIT);
            let thread = page.posts.iter().find(|post| post.id == parent).unwrap();
            assert_eq!((thread.score, thread.vote), (1, 1));
            assert!(s.existing(last).is_err());
            Ok(())
        })
    }
}
