-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE IF NOT EXISTS schema_version(version INTEGER NOT NULL);
INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM schema_version);
CREATE TABLE IF NOT EXISTS events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
    community TEXT NOT NULL, author TEXT NOT NULL, author_seq INTEGER NOT NULL, envelope TEXT NOT NULL,
    UNIQUE(author, author_seq));
CREATE INDEX IF NOT EXISTS events_community ON events(community, cursor);
CREATE TABLE IF NOT EXISTS peers(id TEXT PRIMARY KEY, generation TEXT NOT NULL, cursor INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS account_states(account TEXT PRIMARY KEY, chain TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS personal_events(account TEXT NOT NULL, event_id TEXT NOT NULL, envelope TEXT NOT NULL, PRIMARY KEY(account,event_id));
CREATE TABLE IF NOT EXISTS hidden(id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS public_projections(community TEXT PRIMARY KEY, digest TEXT NOT NULL, posts TEXT NOT NULL, event_count INTEGER NOT NULL, verified INTEGER NOT NULL);

CREATE TABLE IF NOT EXISTS peer_deliveries(peer TEXT NOT NULL, community TEXT NOT NULL, target TEXT NOT NULL, PRIMARY KEY(peer,community,target));
CREATE TABLE IF NOT EXISTS deletions(
    target TEXT NOT NULL, owner TEXT NOT NULL, community TEXT NOT NULL, private INTEGER NOT NULL,
    proof TEXT NOT NULL, PRIMARY KEY(target, owner, community, private));
CREATE INDEX IF NOT EXISTS owned_event_page ON events(COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author),id);
CREATE INDEX IF NOT EXISTS recent_owner_events ON events(community,COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author),json_extract(envelope,'$.acceptedAt'));
CREATE INDEX IF NOT EXISTS erased_by_community ON events(community,id) WHERE json_extract(envelope,'$.deletion') IS NOT NULL;
CREATE INDEX IF NOT EXISTS live_events ON events(id)
      WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply');
CREATE INDEX IF NOT EXISTS live_personal_events ON personal_events(event_id)
      WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply');

CREATE TABLE IF NOT EXISTS public_groups(id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS group_permits(group_id TEXT NOT NULL, host TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(group_id,host));
CREATE TABLE IF NOT EXISTS group_hosts(group_id TEXT NOT NULL, host TEXT NOT NULL, issued_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, record TEXT NOT NULL, PRIMARY KEY(group_id,host));
CREATE TABLE IF NOT EXISTS hosted_groups(group_id TEXT PRIMARY KEY);

CREATE TABLE IF NOT EXISTS user_blocks(owner TEXT NOT NULL, peer TEXT NOT NULL, handle TEXT UNIQUE NOT NULL, label TEXT NOT NULL, PRIMARY KEY(owner,peer));
CREATE TABLE IF NOT EXISTS dm_devices(owner TEXT PRIMARY KEY, device TEXT NOT NULL, identity TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS dm_prekeys(owner TEXT NOT NULL, once_key TEXT NOT NULL, bundle TEXT, PRIMARY KEY(owner,once_key));
CREATE TABLE IF NOT EXISTS dm_mail(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, sender TEXT NOT NULL, recipient TEXT NOT NULL, packet TEXT, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS dm_claims(owner TEXT NOT NULL, peer TEXT NOT NULL, created INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS dm_claim_owner ON dm_claims(owner,created);
CREATE INDEX IF NOT EXISTS dm_claim_age ON dm_claims(created);
CREATE INDEX IF NOT EXISTS dm_mail_age ON dm_mail(created);
CREATE INDEX IF NOT EXISTS dm_recipient ON dm_mail(recipient,seq);

CREATE TABLE IF NOT EXISTS private_groups(id TEXT PRIMARY KEY, owner TEXT NOT NULL, room TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS private_group_rosters(group_id TEXT NOT NULL, revision INTEGER NOT NULL,
    roster TEXT NOT NULL, PRIMARY KEY(group_id, revision));
CREATE TABLE IF NOT EXISTS private_group_sessions(group_id TEXT NOT NULL, id TEXT NOT NULL,
    packet TEXT NOT NULL, PRIMARY KEY(group_id, id));
CREATE TABLE IF NOT EXISTS private_group_events(cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id TEXT NOT NULL, id TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL,
    session TEXT NOT NULL, ratchet_index INTEGER NOT NULL, parent TEXT, event TEXT NOT NULL,
    UNIQUE(group_id, session, ratchet_index));
CREATE INDEX IF NOT EXISTS private_group_event_page ON private_group_events(group_id, cursor);

CREATE TABLE IF NOT EXISTS public_profiles(
  home TEXT NOT NULL, owner TEXT NOT NULL, revision INTEGER NOT NULL, name TEXT NOT NULL,
  profile TEXT NOT NULL, PRIMARY KEY(home,owner));

CREATE TABLE IF NOT EXISTS username_claims(home TEXT NOT NULL, account TEXT NOT NULL, name TEXT NOT NULL,
    PRIMARY KEY(home,account), UNIQUE(home,name));
CREATE TABLE IF NOT EXISTS username_receipts(event_id TEXT PRIMARY KEY, receipt TEXT NOT NULL);
