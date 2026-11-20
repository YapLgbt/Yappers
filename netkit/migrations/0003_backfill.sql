-- SPDX-License-Identifier: AGPL-3.0-only
ALTER TABLE dm_mail ADD COLUMN held INTEGER NOT NULL DEFAULT 0;
INSERT OR IGNORE INTO first_seen SELECT account, unixepoch() FROM account_states;
INSERT OR IGNORE INTO first_seen SELECT COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author),unixepoch() FROM events;
INSERT OR IGNORE INTO dm_approved SELECT DISTINCT sender, recipient FROM dm_mail;
