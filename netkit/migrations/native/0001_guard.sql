-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE native_guard_head(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    guard_id TEXT NOT NULL, epoch INTEGER NOT NULL, token TEXT NOT NULL
);
-- A committed outbox entry describes precisely the native transaction which
-- advanced head. It contains signed proofs, account chains and routing IDs.
CREATE TABLE native_guard_outbox(epoch INTEGER PRIMARY KEY, operation TEXT NOT NULL);
-- Emptied by the outer commit hook. Native savepoints roll back their own
-- inserts into this table together with the corresponding domain mutations.
CREATE TABLE native_guard_stage(slot INTEGER PRIMARY KEY, intent TEXT NOT NULL);
