-- SPDX-License-Identifier: AGPL-3.0-only
-- Votes are signed public actions. Project only identifiers, ordering fields and
-- values. Tombstoning a post or reply leaves its event in the community log.
CREATE TABLE feed_event_counts(
    community TEXT PRIMARY KEY NOT NULL, event_count INTEGER NOT NULL
);
INSERT INTO feed_event_counts SELECT community,count(*) FROM events GROUP BY community;
CREATE TRIGGER feed_count_insert AFTER INSERT ON events BEGIN
    INSERT INTO feed_event_counts VALUES(NEW.community,1)
        ON CONFLICT(community) DO UPDATE SET event_count=event_count+1;
END;
CREATE TRIGGER feed_count_delete AFTER DELETE ON events BEGIN
    UPDATE feed_event_counts SET event_count=event_count-1 WHERE community=OLD.community;
    DELETE FROM feed_event_counts WHERE community=OLD.community AND event_count=0;
END;

-- Physical removal of a device winner must find its predecessor without
-- walking the community log. Ordinary appends need only the materialized heads.
CREATE INDEX feed_vote_history_device ON events(
    community,author,json_extract(envelope,'$.event.payload.action.target'),author_seq DESC,id DESC
) WHERE json_extract(envelope,'$.event.payload.action.kind')='vote';

CREATE TABLE feed_device_votes(
    community TEXT NOT NULL,target TEXT NOT NULL,owner TEXT NOT NULL,device TEXT NOT NULL,
    seq INTEGER NOT NULL,id TEXT NOT NULL,accepted INTEGER NOT NULL,value INTEGER NOT NULL,
    PRIMARY KEY(community,target,owner,device)
);
CREATE INDEX feed_device_votes_by_owner ON feed_device_votes(
    community,target,owner,accepted DESC,device DESC,id DESC
);
CREATE TABLE feed_owner_votes(
    community TEXT NOT NULL,target TEXT NOT NULL,owner TEXT NOT NULL,value INTEGER NOT NULL,
    PRIMARY KEY(community,target,owner)
);
CREATE TABLE feed_vote_scores(
    community TEXT NOT NULL,target TEXT NOT NULL,score INTEGER NOT NULL,
    PRIMARY KEY(community,target)
);

-- The first comparison picks a device's greatest (sequence,id). Only those
-- winners participate in the account's (acceptance time,device,id) comparison.
INSERT INTO feed_device_votes
SELECT community,json_extract(envelope,'$.event.payload.action.target'),
    coalesce(json_extract(envelope,'$.event.payload.account.payload.account'),author),
    author,author_seq,id,json_extract(envelope,'$.acceptedAt'),json_extract(envelope,'$.event.payload.action.value')
FROM events WHERE json_extract(envelope,'$.event.payload.action.kind')='vote'
ON CONFLICT(community,target,owner,device) DO UPDATE SET
    seq=excluded.seq,id=excluded.id,accepted=excluded.accepted,value=excluded.value
WHERE (excluded.seq,excluded.id)>(feed_device_votes.seq,feed_device_votes.id);
INSERT INTO feed_owner_votes
SELECT d.community,d.target,d.owner,d.value FROM feed_device_votes d
WHERE d.id=(SELECT v.id FROM feed_device_votes v
    WHERE v.community=d.community AND v.target=d.target AND v.owner=d.owner
    ORDER BY v.accepted DESC,v.device DESC,v.id DESC LIMIT 1);
INSERT INTO feed_vote_scores SELECT community,target,sum(value) FROM feed_owner_votes GROUP BY community,target;

CREATE TRIGGER feed_score_insert AFTER INSERT ON feed_owner_votes BEGIN
    INSERT INTO feed_vote_scores VALUES(NEW.community,NEW.target,NEW.value)
    ON CONFLICT(community,target) DO UPDATE SET score=score+excluded.score;
END;
CREATE TRIGGER feed_score_update AFTER UPDATE OF value ON feed_owner_votes BEGIN
    UPDATE feed_vote_scores SET score=score-OLD.value+NEW.value
    WHERE community=NEW.community AND target=NEW.target;
END;
CREATE TRIGGER feed_score_delete AFTER DELETE ON feed_owner_votes BEGIN
    UPDATE feed_vote_scores SET score=score-OLD.value WHERE community=OLD.community AND target=OLD.target;
    DELETE FROM feed_vote_scores WHERE community=OLD.community AND target=OLD.target AND NOT EXISTS(
        SELECT 1 FROM feed_owner_votes WHERE community=OLD.community AND target=OLD.target
    );
END;

CREATE TRIGGER feed_vote_insert AFTER INSERT ON events
WHEN json_extract(NEW.envelope,'$.event.payload.action.kind')='vote' BEGIN
    INSERT INTO feed_device_votes VALUES(NEW.community,json_extract(NEW.envelope,'$.event.payload.action.target'),
        coalesce(json_extract(NEW.envelope,'$.event.payload.account.payload.account'),NEW.author),
        NEW.author,NEW.author_seq,NEW.id,json_extract(NEW.envelope,'$.acceptedAt'),
        json_extract(NEW.envelope,'$.event.payload.action.value'))
    ON CONFLICT(community,target,owner,device) DO UPDATE SET
        seq=excluded.seq,id=excluded.id,accepted=excluded.accepted,value=excluded.value
    WHERE (excluded.seq,excluded.id)>(feed_device_votes.seq,feed_device_votes.id);

    -- Even a higher device sequence can carry an earlier home acceptance time.
    -- Another device of this account can therefore become the winner.
    INSERT INTO feed_owner_votes SELECT community,target,owner,value FROM feed_device_votes
    WHERE community=NEW.community AND target=json_extract(NEW.envelope,'$.event.payload.action.target')
        AND owner=coalesce(json_extract(NEW.envelope,'$.event.payload.account.payload.account'),NEW.author)
    ORDER BY accepted DESC,device DESC,id DESC LIMIT 1
    ON CONFLICT(community,target,owner) DO UPDATE SET value=excluded.value
    WHERE value<>excluded.value;
END;

CREATE TRIGGER feed_vote_delete AFTER DELETE ON events
WHEN json_extract(OLD.envelope,'$.event.payload.action.kind')='vote' BEGIN
    DELETE FROM feed_device_votes WHERE community=OLD.community
        AND target=json_extract(OLD.envelope,'$.event.payload.action.target')
        AND owner=coalesce(json_extract(OLD.envelope,'$.event.payload.account.payload.account'),OLD.author)
        AND device=OLD.author AND id=OLD.id;
    INSERT INTO feed_device_votes SELECT community,json_extract(envelope,'$.event.payload.action.target'),
        coalesce(json_extract(envelope,'$.event.payload.account.payload.account'),author),author,author_seq,id,
        json_extract(envelope,'$.acceptedAt'),json_extract(envelope,'$.event.payload.action.value')
    FROM events WHERE json_extract(envelope,'$.event.payload.action.kind')='vote'
        AND community=OLD.community AND author=OLD.author
        AND json_extract(envelope,'$.event.payload.action.target')=json_extract(OLD.envelope,'$.event.payload.action.target')
        AND coalesce(json_extract(envelope,'$.event.payload.account.payload.account'),author)=
            coalesce(json_extract(OLD.envelope,'$.event.payload.account.payload.account'),OLD.author)
    ORDER BY author_seq DESC,id DESC LIMIT 1
    ON CONFLICT(community,target,owner,device) DO NOTHING;
    DELETE FROM feed_owner_votes WHERE community=OLD.community
        AND target=json_extract(OLD.envelope,'$.event.payload.action.target')
        AND owner=coalesce(json_extract(OLD.envelope,'$.event.payload.account.payload.account'),OLD.author);
    INSERT INTO feed_owner_votes SELECT community,target,owner,value FROM feed_device_votes
    WHERE community=OLD.community AND target=json_extract(OLD.envelope,'$.event.payload.action.target')
        AND owner=coalesce(json_extract(OLD.envelope,'$.event.payload.account.payload.account'),OLD.author)
    ORDER BY accepted DESC,device DESC,id DESC LIMIT 1;
END;
