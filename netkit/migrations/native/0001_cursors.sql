-- SPDX-License-Identifier: AGPL-3.0-only
-- A transaction which inserts into an ordered stream must first write its fence.
-- Snapshot isolation detects the shared-row conflict, even for disjoint event IDs.
-- high_water also protects explicit cursor imports and the deletion of tail rows.
CREATE TABLE native_cursor_fences(
    stream TEXT PRIMARY KEY NOT NULL,
    generation INTEGER NOT NULL CHECK(generation IN (0,1)),
    high_water INTEGER NOT NULL CHECK(high_water>=0)
);
INSERT INTO native_cursor_fences(stream,generation,high_water)
VALUES('events',0,0),('dm_mail',0,0),('private_group_events',0,0);

CREATE TRIGGER fence_events_insert BEFORE INSERT ON events BEGIN
    UPDATE native_cursor_fences SET generation=1-generation WHERE stream='events';
END;
CREATE TRIGGER advance_events_cursor AFTER INSERT ON events BEGIN
    SELECT RAISE(ABORT,'missing fence or out-of-order events cursor')
      WHERE NOT EXISTS(SELECT 1 FROM native_cursor_fences WHERE stream='events' AND high_water<NEW.cursor);
    UPDATE native_cursor_fences SET high_water=NEW.cursor WHERE stream='events';
END;
CREATE TRIGGER immutable_events_cursor BEFORE UPDATE ON events
WHEN NEW.cursor<>OLD.cursor BEGIN
    SELECT RAISE(ABORT,'immutable events cursor');
END;

CREATE TRIGGER fence_dm_mail_insert BEFORE INSERT ON dm_mail BEGIN
    UPDATE native_cursor_fences SET generation=1-generation WHERE stream='dm_mail';
END;
CREATE TRIGGER advance_dm_mail_cursor AFTER INSERT ON dm_mail BEGIN
    SELECT RAISE(ABORT,'missing fence or out-of-order dm_mail cursor')
      WHERE NOT EXISTS(SELECT 1 FROM native_cursor_fences WHERE stream='dm_mail' AND high_water<NEW.seq);
    UPDATE native_cursor_fences SET high_water=NEW.seq WHERE stream='dm_mail';
END;
CREATE TRIGGER immutable_dm_mail_cursor BEFORE UPDATE ON dm_mail
WHEN NEW.seq<>OLD.seq BEGIN
    SELECT RAISE(ABORT,'immutable dm_mail cursor');
END;

CREATE TRIGGER fence_private_group_events_insert BEFORE INSERT ON private_group_events BEGIN
    UPDATE native_cursor_fences SET generation=1-generation WHERE stream='private_group_events';
END;
CREATE TRIGGER advance_private_group_events_cursor AFTER INSERT ON private_group_events BEGIN
    SELECT RAISE(ABORT,'missing fence or out-of-order private_group_events cursor')
      WHERE NOT EXISTS(SELECT 1 FROM native_cursor_fences WHERE stream='private_group_events' AND high_water<NEW.cursor);
    UPDATE native_cursor_fences SET high_water=NEW.cursor WHERE stream='private_group_events';
END;
CREATE TRIGGER immutable_private_group_events_cursor BEFORE UPDATE ON private_group_events
WHEN NEW.cursor<>OLD.cursor BEGIN
    SELECT RAISE(ABORT,'immutable private_group_events cursor');
END;

-- Enrolling a push category records a baseline read earlier in this transaction.
-- Force a stale reader to retry if its source stream advanced in the meantime.
CREATE TRIGGER fence_push_baseline BEFORE INSERT ON push_starts BEGIN
    UPDATE native_cursor_fences SET generation=1-generation
     WHERE stream=CASE NEW.category WHEN 'reply' THEN 'events' ELSE 'dm_mail' END;
END;
