// SPDX-License-Identifier: AGPL-3.0-only
use crate::store::Store;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

// json_each.value has no declared affinity. The text cast lets Turso seek
// the entire composite key instead of scanning all votes in the community.
const BATCH_VOTES: &str = "SELECT t.value,coalesce(s.score,0),coalesce(v.value,0)
    FROM json_each(?1) t
    LEFT JOIN feed_vote_scores s ON s.community=?2 AND s.target=CAST(t.value AS TEXT)
    LEFT JOIN feed_owner_votes v ON v.community=?2 AND v.target=CAST(t.value AS TEXT) AND v.owner=?3";

impl Store {
    pub(crate) fn feed_event_count(&self, community: &str) -> Result<usize> {
        Ok(self
            .db
            .query_row(
                "SELECT event_count FROM feed_event_counts WHERE community=?",
                [community],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    /// One indexed score and one account lookup per candidate in the feed.
    pub(crate) fn feed_votes(
        &self,
        community: &str,
        targets: &[String],
        viewer: &str,
    ) -> Result<HashMap<String, (i64, i8)>> {
        let mut read = self.db.prepare_cached(BATCH_VOTES)?;
        let votes = read
            .query_map(
                params![serde_json::to_string(targets)?, community, viewer],
                |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))),
            )?
            .collect::<rusqlite::Result<_>>()?;
        Ok(votes)
    }

    pub(crate) fn remove_blocked_votes(
        &self,
        community: &str,
        targets: &[String],
        viewer: &str,
        votes: &mut HashMap<String, (i64, i8)>,
    ) -> Result<()> {
        let mut read = self.db.prepare_cached(
            "SELECT t.value,SUM(v.value) FROM json_each(?1) t
             CROSS JOIN user_blocks b ON b.owner=?3
             CROSS JOIN feed_owner_votes v
               ON v.community=?2 AND v.target=CAST(t.value AS TEXT) AND v.owner=b.peer
             GROUP BY t.value",
        )?;
        let mut rows = read.query(params![serde_json::to_string(targets)?, community, viewer])?;
        while let Some(row) = rows.next()? {
            let target: String = row.get(0)?;
            if let Some((score, _)) = votes.get_mut(&target) {
                *score -= row.get::<_, i64>(1)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/turso/feed_votes.rs"]
mod tests;
