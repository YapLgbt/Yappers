-- SPDX-License-Identifier: AGPL-3.0-only
ALTER TABLE peers ADD COLUMN scope TEXT NOT NULL DEFAULT '';
