-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS guard.known_deleted_accounts(account TEXT PRIMARY KEY, record_id TEXT NOT NULL);
