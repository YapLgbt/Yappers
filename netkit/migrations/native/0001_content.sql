-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS home_openings(
        target TEXT NOT NULL, private INTEGER NOT NULL, owner TEXT NOT NULL, community TEXT NOT NULL,
        sealed BLOB NOT NULL, routing BLOB,
        PRIMARY KEY(target,private)
    );
