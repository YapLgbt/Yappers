-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS guard.state(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1), id TEXT NOT NULL,
    epoch INTEGER NOT NULL, key BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS guard.known_deliveries(
    peer TEXT, community TEXT, target TEXT, PRIMARY KEY(peer,community,target));
CREATE TABLE IF NOT EXISTS guard.current_accounts(account TEXT PRIMARY KEY, chain TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS guard.opening_keys(
    target TEXT, owner TEXT, community TEXT, private INTEGER, key BLOB,
    PRIMARY KEY(target,owner,community,private));
CREATE TABLE IF NOT EXISTS guard.known_deletions(
    target TEXT, owner TEXT, community TEXT, private INTEGER, proof TEXT,
    PRIMARY KEY(target,owner,community,private));
