-- SPDX-License-Identifier: AGPL-3.0-only
CREATE TABLE events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
    community TEXT NOT NULL, author TEXT NOT NULL, author_seq INTEGER NOT NULL, envelope TEXT NOT NULL
    CHECK(json_extract(envelope,'$.event.payload.action.kind') NOT IN ('post','postV2','reply')),
    UNIQUE(author, author_seq));
CREATE INDEX events_community ON events(community, cursor);
CREATE TABLE peers(id TEXT PRIMARY KEY, generation TEXT NOT NULL, cursor INTEGER NOT NULL, scope TEXT NOT NULL DEFAULT '');
CREATE TABLE account_states(account TEXT PRIMARY KEY, chain TEXT NOT NULL);
CREATE TABLE personal_events(account TEXT NOT NULL, event_id TEXT NOT NULL, envelope TEXT NOT NULL
    CHECK(json_extract(envelope,'$.event.payload.action.kind') NOT IN ('post','postV2','reply')), PRIMARY KEY(account,event_id));
CREATE TABLE hidden(id TEXT PRIMARY KEY);
CREATE TABLE public_projections(community TEXT PRIMARY KEY, digest TEXT NOT NULL, posts TEXT NOT NULL, event_count INTEGER NOT NULL, verified INTEGER NOT NULL);

CREATE TABLE peer_deliveries(peer TEXT NOT NULL, community TEXT NOT NULL, target TEXT NOT NULL, PRIMARY KEY(peer,community,target));
CREATE TABLE deletions(
    target TEXT NOT NULL, owner TEXT NOT NULL, community TEXT NOT NULL, private INTEGER NOT NULL,
    proof TEXT NOT NULL, PRIMARY KEY(target, owner, community, private));
CREATE INDEX owned_event_page ON events(COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author),id);
CREATE INDEX recent_owner_events ON events(community,COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author),json_extract(envelope,'$.acceptedAt'));
CREATE INDEX erased_by_community ON events(community,id) WHERE json_extract(envelope,'$.deletion') IS NOT NULL;
CREATE INDEX live_events ON events(id)
      WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply');
CREATE INDEX live_personal_events ON personal_events(event_id)
      WHERE json_extract(envelope,'$.event.payload.action.kind') IN ('post','postV2','reply');

CREATE TABLE public_groups(id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE group_permits(group_id TEXT NOT NULL, host TEXT NOT NULL, record TEXT NOT NULL, PRIMARY KEY(group_id,host));
CREATE TABLE group_hosts(group_id TEXT NOT NULL, host TEXT NOT NULL, issued_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, record TEXT NOT NULL, PRIMARY KEY(group_id,host));
CREATE TABLE hosted_groups(group_id TEXT PRIMARY KEY);

CREATE TABLE user_blocks(owner TEXT NOT NULL, peer TEXT NOT NULL, handle TEXT UNIQUE NOT NULL, label TEXT NOT NULL, PRIMARY KEY(owner,peer));
CREATE TABLE dm_devices(owner TEXT PRIMARY KEY, device TEXT NOT NULL, identity TEXT NOT NULL);
CREATE TABLE dm_prekeys(owner TEXT NOT NULL, once_key TEXT NOT NULL, bundle TEXT, PRIMARY KEY(owner,once_key));
CREATE TABLE dm_mail(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, sender TEXT NOT NULL, recipient TEXT NOT NULL, packet TEXT, created INTEGER NOT NULL, held INTEGER NOT NULL DEFAULT 0);
CREATE TABLE dm_claims(owner TEXT NOT NULL, peer TEXT NOT NULL, created INTEGER NOT NULL);
CREATE INDEX dm_claim_owner ON dm_claims(owner,created);
CREATE INDEX dm_claim_age ON dm_claims(created);
CREATE INDEX dm_mail_age ON dm_mail(created);
CREATE INDEX dm_recipient ON dm_mail(recipient,seq);

CREATE TABLE private_groups(id TEXT PRIMARY KEY, owner TEXT NOT NULL, room TEXT NOT NULL);
CREATE TABLE private_group_rosters(group_id TEXT NOT NULL, revision INTEGER NOT NULL,
    roster TEXT NOT NULL, PRIMARY KEY(group_id, revision));
CREATE TABLE private_group_sessions(group_id TEXT NOT NULL, id TEXT NOT NULL,
    packet TEXT NOT NULL, PRIMARY KEY(group_id, id));
CREATE TABLE private_group_events(cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id TEXT NOT NULL, id TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL,
    session TEXT NOT NULL, ratchet_index INTEGER NOT NULL, parent TEXT, event TEXT NOT NULL,
    UNIQUE(group_id, session, ratchet_index));
CREATE INDEX private_group_event_page ON private_group_events(group_id, cursor);

CREATE TABLE public_profiles(
  home TEXT NOT NULL, owner TEXT NOT NULL, revision INTEGER NOT NULL, name TEXT NOT NULL,
  profile TEXT NOT NULL, PRIMARY KEY(home,owner));

CREATE TABLE username_claims(home TEXT NOT NULL, account TEXT NOT NULL, name TEXT NOT NULL,
    PRIMARY KEY(home,account), UNIQUE(home,name));
CREATE TABLE username_receipts(event_id TEXT PRIMARY KEY, receipt TEXT NOT NULL);

CREATE TABLE first_seen(owner TEXT PRIMARY KEY, since INTEGER NOT NULL);
CREATE TABLE privacy_settings(owner TEXT PRIMARY KEY, settings TEXT NOT NULL);
CREATE TABLE dm_approved(owner TEXT NOT NULL, peer TEXT NOT NULL, PRIMARY KEY(owner,peer));

CREATE TABLE push_devices(owner TEXT,device TEXT,provider TEXT NOT NULL,subscription TEXT NOT NULL UNIQUE,
    categories TEXT NOT NULL,PRIMARY KEY(owner,device));
CREATE TABLE push_starts(owner TEXT,device TEXT,category TEXT,baseline INTEGER NOT NULL,
    PRIMARY KEY(owner,device,category));
CREATE TABLE push_routes(token TEXT PRIMARY KEY,owner TEXT,device TEXT,category TEXT,target TEXT,
    delivered INTEGER NOT NULL DEFAULT 0,leasing INTEGER NOT NULL DEFAULT 0,retry_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE(owner,device,category,target));

CREATE TABLE inbox_reads(
    owner TEXT, kind TEXT, target TEXT, through INTEGER NOT NULL,
    PRIMARY KEY(owner,kind,target));
CREATE TABLE inbox_mutes(
    owner TEXT, target TEXT, PRIMARY KEY(owner,target));
CREATE TABLE inbox_arrivals(event TEXT PRIMARY KEY, received INTEGER NOT NULL);

CREATE INDEX events_reply_parent
    ON events(json_extract(envelope,'$.event.payload.action.parent'));
CREATE INDEX events_account_owner
    ON events(COALESCE(json_extract(envelope,'$.event.payload.account.payload.account'),author));

-- Durable receipt for the cutover from the configured catalog path.
CREATE TABLE home_import_complete(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1), id TEXT NOT NULL);
