-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS push_devices(owner TEXT,device TEXT,provider TEXT NOT NULL,subscription TEXT NOT NULL UNIQUE,
    categories TEXT NOT NULL,PRIMARY KEY(owner,device));
CREATE TABLE IF NOT EXISTS push_starts(owner TEXT,device TEXT,category TEXT,baseline INTEGER NOT NULL,
    PRIMARY KEY(owner,device,category));
CREATE TABLE IF NOT EXISTS push_routes(token TEXT PRIMARY KEY,owner TEXT,device TEXT,category TEXT,target TEXT,
    delivered INTEGER NOT NULL DEFAULT 0,leasing INTEGER NOT NULL DEFAULT 0,retry_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE(owner,device,category,target));

CREATE TABLE IF NOT EXISTS inbox_reads(
    owner TEXT, kind TEXT, target TEXT, through INTEGER NOT NULL,
    PRIMARY KEY(owner,kind,target));
CREATE TABLE IF NOT EXISTS inbox_mutes(
    owner TEXT, target TEXT, PRIMARY KEY(owner,target));
CREATE TABLE IF NOT EXISTS inbox_arrivals(event TEXT PRIMARY KEY, received INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS inbox_event_arrived AFTER INSERT ON events
WHEN json_extract(NEW.envelope,'$.event.payload.action.kind') IN ('post','postV2','reply')
BEGIN INSERT OR IGNORE INTO inbox_arrivals VALUES(NEW.id,unixepoch()); END;
CREATE INDEX IF NOT EXISTS events_reply_parent
    ON events(json_extract(envelope,'$.event.payload.action.parent'));
CREATE INDEX IF NOT EXISTS events_account_owner
    ON events(COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author));
CREATE VIEW IF NOT EXISTS inbox_replies AS
SELECT p.id AS target, p.community AS scope, r.id AS reply, r.cursor AS seq,
    COALESCE(json_extract(p.envelope,'$.event.payload.account.payload.account'),p.author) AS owner,
    COALESCE(json_extract(r.envelope,'$.event.payload.account.payload.account'),r.author) AS actor,
    json_extract(r.envelope,'$.event.payload.action.displayName')='Anonymous neighbor' AS anonymous,
    COALESCE(json_extract(p.envelope,'$.event.payload.action.headline'),
             substr(json_extract(p.envelope,'$.event.payload.action.body'),1,80)) AS title,
    json_extract(r.envelope,'$.acceptedAt') AS created
FROM events p JOIN events r ON json_extract(r.envelope,'$.event.payload.action.parent')=p.id
    AND r.community=p.community
WHERE json_extract(p.envelope,'$.event.payload.action.kind') IN ('post','postV2')
    AND json_extract(r.envelope,'$.event.payload.action.kind')='reply'
    AND owner<>actor AND NOT EXISTS(SELECT 1 FROM hidden WHERE id IN (r.id,p.id));
CREATE VIEW IF NOT EXISTS inbox_visible AS
SELECT n.* FROM inbox_replies n
LEFT JOIN privacy_settings prefs ON prefs.owner=n.owner
LEFT JOIN first_seen age ON age.owner=n.actor
WHERE NOT EXISTS(SELECT 1 FROM user_blocks b WHERE
    (b.owner=n.owner AND b.peer=n.actor) OR (b.peer=n.owner AND b.owner=n.actor))
AND NOT EXISTS(SELECT 1 FROM inbox_mutes m WHERE m.owner=n.owner AND m.target=n.target)
AND (NOT n.anonymous OR NOT COALESCE(json_extract(prefs.settings,'$.hideAnonymous'),0))
AND (unixepoch()-COALESCE(age.since,unixepoch()))/86400 >= COALESCE(json_extract(prefs.settings,'$.feedDays'),0);
