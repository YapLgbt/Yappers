// SPDX-License-Identifier: AGPL-3.0-only
//! Current home-authorized public profiles. Profile revisions are independent of post IDs.
use crate::{CommunityProfile, store::Store};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS public_profiles(
  home TEXT NOT NULL, owner TEXT NOT NULL, revision INTEGER NOT NULL, name TEXT NOT NULL,
  profile TEXT NOT NULL, PRIMARY KEY(home,owner))";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ProfileRecord {
    pub revision: u64,
    pub name: String,
    pub profile: CommunityProfile,
}

impl ProfileRecord {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.revision > 0
                && self.name.trim() == self.name
                && (2..=30).contains(&self.name.encode_utf16().count())
                && !self.name.chars().any(char::is_control)
                && self.name != "Anonymous neighbor",
            "invalid public profile"
        );
        self.profile.validate()
    }
}
impl Store {
    pub(crate) fn named_authors(
        &self,
        home: &str,
        community: &str,
    ) -> Result<std::collections::BTreeSet<String>> {
        Ok(self
            .events(community)?
            .into_iter()
            .filter(|e| e.event.payload.home == home && crate::usernames::named(&e.event))
            .map(|e| e.event.owner_id().to_owned())
            .collect())
    }
    pub(crate) fn import_profile(
        &self,
        home: &str,
        owner: &str,
        record: ProfileRecord,
    ) -> Result<()> {
        record.validate()?;
        self.db.execute("INSERT INTO public_profiles(home,owner,revision,name,profile) VALUES(?,?,?,?,?)
            ON CONFLICT(home,owner) DO UPDATE SET revision=excluded.revision,name=excluded.name,profile=excluded.profile
            WHERE excluded.revision > public_profiles.revision",
            params![home,owner,record.revision,record.name,serde_json::to_string(&record.profile)?])?;
        Ok(())
    }
    pub(crate) fn current_profile(&self, home: &str, owner: &str) -> Result<Option<ProfileRecord>> {
        let row: Option<(u64, String, String)> = self
            .db
            .query_row(
                "SELECT revision,name,profile FROM public_profiles WHERE home=? AND owner=?",
                params![home, owner],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(revision, name, raw)| {
            Ok(ProfileRecord {
                revision,
                name,
                profile: serde_json::from_str(&raw)?,
            })
        })
        .transpose()
    }

    pub(crate) fn set_profile(
        &self,
        home: &str,
        owner: &str,
        expected: u64,
        name: String,
        profile: CommunityProfile,
    ) -> Result<ProfileRecord> {
        let tx = self.db.unchecked_transaction()?;
        let existing = self.current_profile(home, owner)?;
        if let Some(current) = &existing
            && current.name == name
            && current.profile == profile
        {
            return Ok(current.clone());
        }
        ensure!(
            existing.map_or(0, |p| p.revision) == expected,
            "Profile changed on another device. Refresh, then reopen the editor."
        );
        let revision = expected
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("profile revision exhausted"))?;
        let record = ProfileRecord {
            revision,
            name,
            profile,
        };
        self.import_profile(home, owner, record.clone())?;
        tx.commit()?;
        Ok(record)
    }
}
