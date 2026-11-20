-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS guard.unconfirmed_opening_keys(
    target TEXT NOT NULL, owner TEXT NOT NULL, community TEXT NOT NULL, private INTEGER NOT NULL,
    PRIMARY KEY(target,owner,community,private));
CREATE TABLE IF NOT EXISTS guard.opening_reservations(
    target TEXT NOT NULL, owner TEXT NOT NULL, community TEXT NOT NULL, private INTEGER NOT NULL,
    catalog TEXT NOT NULL, operation TEXT NOT NULL,
    PRIMARY KEY(target,owner,community,private,catalog,operation));
CREATE INDEX IF NOT EXISTS guard.opening_reservations_by_catalog ON opening_reservations(catalog,operation);
CREATE TRIGGER IF NOT EXISTS guard.forget_opening_reservation AFTER DELETE ON opening_keys BEGIN
    DELETE FROM unconfirmed_opening_keys WHERE target=OLD.target AND owner=OLD.owner
        AND community=OLD.community AND private=OLD.private;
    DELETE FROM opening_reservations WHERE target=OLD.target AND owner=OLD.owner
        AND community=OLD.community AND private=OLD.private;
END;
