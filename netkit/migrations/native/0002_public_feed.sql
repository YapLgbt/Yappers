-- SPDX-License-Identifier: AGPL-3.0-only
-- Native reply openings are sealed. The v1 legacy expression index held NULL
-- for valid native headers and has no native query consumer.
DROP INDEX events_reply_parent;

-- Signed public headers retain the acceptance time and post/reply distinction.
CREATE INDEX feed_posts ON events(
    community, json_extract(envelope,'$.acceptedAt') DESC, id DESC
) WHERE json_extract(envelope,'$.event.payload.action.kind')='erased'
    AND json_extract(envelope,'$.event.payload.action.reply')=0
    AND json_extract(envelope,'$.deletion') IS NULL;

CREATE INDEX feed_replies ON events(community,cursor)
    WHERE json_extract(envelope,'$.event.payload.action.kind')='erased'
    AND json_extract(envelope,'$.event.payload.action.reply')=1
    AND json_extract(envelope,'$.deletion') IS NULL;
