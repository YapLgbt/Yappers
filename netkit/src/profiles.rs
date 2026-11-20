// SPDX-License-Identifier: AGPL-3.0-only
//! Current home-authorized public profiles. Profile revisions are independent of post IDs.
use crate::{CommunityProfile, store::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        if crate::account_store::deleted_registration(&self.db, owner)?.is_some() {
            return Ok(());
        }
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

    pub(crate) fn current_profiles<'a>(
        &self,
        owners: &[(&'a str, &'a str)],
    ) -> Result<HashMap<(&'a str, &'a str), ProfileRecord>> {
        let mut profiles = HashMap::new();
        for chunk in owners.chunks(crate::MAX_PAGE_ENTRIES as usize) {
            // Drive primary-key lookups from the requested (home, owner) pairs.
            // The array index lets the result reuse the feed's existing key strings.
            let mut stmt = self.db.prepare_cached(
                "SELECT k.key, p.revision, p.name, p.profile
                 FROM json_each(?) k CROSS JOIN public_profiles p
                 WHERE p.home=json_extract(k.value,'$[0]') AND p.owner=json_extract(k.value,'$[1]')",
            )?;
            let mut rows = stmt.query([serde_json::to_string(chunk)?])?;
            while let Some(row) = rows.next()? {
                let key = *chunk
                    .get(row.get::<_, usize>(0)?)
                    .context("invalid profile index")?;
                profiles.insert(
                    key,
                    ProfileRecord {
                        revision: row.get(1)?,
                        name: row.get(2)?,
                        profile: serde_json::from_str(&row.get::<_, String>(3)?)?,
                    },
                );
            }
        }
        Ok(profiles)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_profiles_match_exact_home_and_owner_across_batches() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("home"))?;
        let owners: Vec<_> = (0..=crate::MAX_PAGE_ENTRIES)
            .map(|i| i.to_string())
            .collect();
        let mut requested: Vec<_> = owners.iter().map(|id| ("first", id.as_str())).collect();
        let last = ("second", "0");
        requested.push(last);
        store.set_profile("first", "0", 0, "First name".into(), Default::default())?;
        store.set_profile(last.0, last.1, 0, "Last name".into(), Default::default())?;
        // Both halves of this pair occur on the page, but the pair itself does not.
        store.db.execute(
            "INSERT INTO public_profiles VALUES('second','1',1,'Unrelated','invalid')",
            [],
        )?;
        let profiles = store.current_profiles(&requested)?;
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[&("first", "0")].name, "First name");
        assert_eq!(profiles[&last].name, "Last name");
        Ok(())
    }
}
