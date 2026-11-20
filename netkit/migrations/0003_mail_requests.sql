-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS first_seen(owner TEXT PRIMARY KEY, since INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS privacy_settings(owner TEXT PRIMARY KEY, settings TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS dm_approved(owner TEXT NOT NULL, peer TEXT NOT NULL, PRIMARY KEY(owner,peer));
