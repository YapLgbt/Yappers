-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS guard.native_watermark(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    epoch INTEGER NOT NULL, token TEXT NOT NULL,
    mac_key BLOB NOT NULL CHECK(length(mac_key)=32)
);
-- Legacy/HA writers also own this guard. Each committed epoch advance fences
-- any prior native branch. A native finalizer sets its authenticated token in
-- the same transaction after updating state.
CREATE TRIGGER IF NOT EXISTS guard.native_legacy_epoch AFTER UPDATE OF epoch ON state
WHEN NEW.epoch > OLD.epoch
BEGIN
    UPDATE native_watermark SET epoch=NEW.epoch, token=lower(hex(randomblob(32)))
    WHERE singleton=NEW.singleton;
END;
