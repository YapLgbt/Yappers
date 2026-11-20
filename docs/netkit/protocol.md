# Netkit protocol

## Current transport

App clients offer `yappers/netkit/7` and `yappers/netkit/6` over Iroh. With version 7, a client reuses its authenticated connection to a Home and opens one bidirectional stream per RPC. Independent requests can overlap. Each request and response still ends at stream EOF, and Home checks each request’s permissions. Account registration finishes before the dependent signed-in feed request. Version 6 serves one request per connection and remains available to older clients and Homes.

A Home admits up to 256 established multiplexed connections and up to eight simultaneous streams per connection. Handshakes and active requests share a 64-slot work limit. Requests have individual 20-second deadlines. After 20 seconds without active streams the Home releases the connection, and the client reconnects on demand. The invitation’s JSON protocol version stays at 6.

New posts, replies, votes and deletions use event version 4. A home verifies the device signature, checks the account grant when present, and signs an acceptance receipt. After deletion, a receiver can verify the retained header and deletion proof without the original plaintext or salt. [Event format, signing domains and deletion proofs](deletion.md#what-a-peer-verifies)

The [Rust wire types](../../netkit/src/model.rs) define the request and response fields. [Group records](groups.md#canonical-signed-records) and [portable account records](../ownership.md) have separate signatures and version numbers.

Feed responses fit the newest complete conversations inside a 1 MiB frame. When the deletion history needs another page, the client follows `feedWithMoreDeletions` with authorized `deletionsAfter` requests before it displays or caches the feed. Public group catalogs and private roster histories also support continuation pages. Upgrade clients before a home needs any of these larger responses. Older clients reject an unfamiliar paged response.

Account registration, shared event history and groups created on a home have no fixed lifetime count. Operators can monitor the HA image budget and increase it as the database grows. [Worker health and capacity](operations.md)

## Historical RPC v4 and signed events v1/v2/v3

The records below document earlier releases. Transport v4 negotiated `yappers/netkit/4`. Its clients exchanged original version-1 events, version-2 headline posts and version-3 portable-account events.

### Framing and resource bounds

One request and one response are UTF-8 JSON documents on a single QUIC bidirectional stream. The sender finishes its half-stream; EOF terminates the document. Both sides cap a document at 1 MiB. Connections and streams are bounded by a 20-second request deadline. The server handles one request per connection, caps concurrent connections at 64, and refuses excess arrivals. JSON is not logged with credentials.

Configuration supports at most 32 communities and 16 peers. Pull pages scan at most 100 log rows. A sync cycle handles at most 20 pages per peer. The prototype database caps distinct events at 10,000. A home applies 120 accepted events per author/community per rolling minute. These limits are executable defaults, not a production abuse-prevention system.

### Event format

`SignedEvent` has `id`, `payload`, and `signature`. Payload fields, in this canonical serialization order, are:

`version`, `home`, `community`, `author`, `sequence`, `createdAt`, `nonce`, `action`.

- `home`: the designated instance's Ed25519/Iroh public key, 64 lowercase hexadecimal characters.
- `author`: the separate author's Ed25519 public key in hex.
- `sequence`: positive integer no greater than signed 64-bit maximum; reserved monotonically by the author installation across operations.
- `createdAt`: Unix seconds.
- `nonce`: random 32 bytes encoded as 64 hex characters.
- Community: nonempty ASCII identifier, at most 160 bytes.

Rust's struct serializer defines canonical bytes for each event version: compact UTF-8 JSON with the declared field order and serde's string escaping. Signing and canonicalization happen only in Rust. Other languages retain the returned event as an opaque JSON object; object key order in persisted JSON need not survive because Rust reserializes the typed payload before verification. Unknown event/payload/action fields are rejected. This is a closed, versioned schema, not generic JSON canonicalization. Any incompatible field, canonicalization, or ordering change requires a new application version and ALPN.

`id = lowercase_hex(BLAKE3("yappers:event:v1\0" || canonical_payload_bytes))`, where `\0` denotes a single NUL byte, not two literal characters. The author signature is Ed25519 over `"yappers:author:v1\0" || UTF8(id)` and is encoded as 128 hex characters. Verification uses strict Ed25519 verification.

Actions use a `kind` discriminator:

| Kind | Additional fields | Meaning |
|---|---|---|
| `postV2` | `body`, `topic`, `anonymous`, `displayName`, `headline`, optional `profile` | Headline post with an optional signed author snapshot (event v2) |
| `post` | `body`, `topic`, `anonymous`, `displayName` | Create a text post |
| `reply` | `parent`, `body`, `displayName` | Reply to a post in the same community |
| `vote` | `target`, `value` | Set own vote to -1, 0, or +1 |
| `delete` | `target` | Tombstone own post/reply |

Bodies contain 1–500 UTF-16 code units after application trimming; this matches Kotlin's length convention. Display names are nonempty and at most 30 UTF-16 code units. NUL and control characters other than newline/tab are rejected. Topics are `Chatter`, `Friends`, `Missed connections`, or `Plans`. Anonymous posts must encode `displayName` as `Anonymous neighbor`, so the chosen alias is not accidentally sent as part of the post. The author public key remains visible.

A home accepts client timestamps from 30 days before acceptance through five minutes after acceptance. The author sequence is unique across accepted events for that author in a replica. A duplicate event ID with the same event is acknowledged; a reused sequence with a different ID is rejected rather than resolved silently.

### Home envelope

An envelope contains `event`, `acceptedAt`, and `homeSignature`. Its signature covers:

`"yappers:home:v1\0" || compact_JSON([event.id, event.payload.home, acceptedAt])`.

The signing key must match the event's designated home. Home acceptance time must be positive and no more than five minutes ahead of the receiving instance's clock. Import preserves the original acceptance time and signature. Conflicting envelopes for the same event ID are rejected. This detects equivocation locally; it is not a Byzantine consensus protocol.

### RPC messages

Requests use the `type` discriminator:

- `feed`: `token`, `community`, `author`. Returns `feed` with `posts` and `eventCount`. `author` selects the viewer's own vote/mine presentation; it is not a secret and does not grant authority.
- `submit`: `token`, `event`. Returns `accepted` with `eventId` only after durable acceptance.
- `pull`: `after`, optional `generation`, `limit` (1–100). Restricted to trusted transport endpoint identities. Returns `page` with `generation`, `entries`, `next`, and `more`.

Additional v2 requests:

- `directory`: `snapshot` containing public signed groups, host permits, and address announcements. Records are validated as a complete batch before commit. Returns the same public shape. No member capability is needed, and no private capability is returned.
- `enrollHost`: `group`. The group authority uses the connecting Iroh endpoint ID, not a caller-supplied host key. For open groups it returns an owner-signed `hostPermit`; blocked hosts and capacity excesses are rejected.
- `groups`: member `token`, `community`. Returns bounded group summaries from this home's verified catalog.
- `createGroup`: member `token`, `community`, `details` (name, description, category, nullable locality). Creates an owner-signed public descriptor and hosts it. Same normalized name/locality on the same home with unchanged description/category returns the existing group, so retrying unchanged details after a lost receipt does not create duplicates. Conflicting details fail; descriptors are immutable in this version.
- `joinGroup`: member `token`, `community`, `group`. The home obtains a host permit from the authority when necessary, persists hosting, and returns `groupJoined` with the summary and a private capability pinned to this same home and group. Existing permits do not require the authority to be online. A new enrollment does.

Catalog limits are 64 descriptors, 128 permits/addresses, 16 providers per group, and 24 hosted groups per home. Discovery rotates over at most four destinations per sync; automatic event replication rotates over at most eight discovered peers in addition to explicit peers. Individual RPC deadlines remain 20 seconds; a whole sync cycle may take longer when several peers are unavailable. Address records expire after at most one hour and renew around halfway through their lifetime or on address change. Stable host permits do not expire; local blocklists provide prospective endpoint denial. Global permit revocation is not implemented.

An entry has `cursor` and `envelope`. The serving instance scans its global ingestion log but only returns communities authorized for the caller; `next` advances over filtered rows too. If the caller's generation differs, the server scans from zero. On receipt, entry cursors must strictly increase beyond the effective starting cursor and not exceed `next`. `more=true` requires progress. Entries and cursor persist atomically.

An `error` response has a machine-oriented `code` and human-readable `message`. Current server rejection uses `rejected`, and an oversized projection uses `capacity`. Do not branch durable application logic on error prose. The client preserves unacknowledged events on every error; a user must explicitly discard an unrecoverable pending operation. Versioned structured error categories and automatic permanent-rejection handling remain follow-on work.

### Projection and bounded history

The event set is authoritative; the feed is a bounded view of it. V1 returns the latest 200 posts and up to 200 replies per post. If that projection exceeds the wire cap, the server returns an explicit capacity error; it does not truncate bytes or falsely acknowledge a read. Event-log replication remains paginated and complete up to configured database capacity. Client feed/thread pagination, log compaction, full history export, and snapshot checkpoints are required before large communities are supported.

Deletion requires target ownership. A deletion arriving before its target takes effect once that target arrives. A vote arriving before its post is retained and materializes when the post arrives. The home rejects new replies/votes whose post is not currently known; a queued client can retry after federation catches up.

Local bookmarks, hidden posts, pending status, and numeric UI IDs are never federated. Immutable string event IDs are used on the wire. Kotlin allocates and persists local numeric IDs instead of truncating hashes into collision-prone UI identifiers.

### Headline/profile events (v2)

The introduction of `postV2` added a separate signing domain for headline posts and optional profile snapshots. Anonymous posts omitted profiles. That release used transport v3. Current clients retain the action name inside a version-4 event. [Current post fields](../profiles-and-posts.md#wire-compatibility)

### Portable-account additions

Version-3 events carry a root-signed account authorization and use the `yap:author:v3` / `yap:event:v3` NUL-terminated signing domains. Legacy v1/v2 serialization omits the new optional account field, preserving original bytes. V4 adds signed-account lookup/registration/announcement and authenticated private authored-history export/import. Accepted receipts include their exact signed envelope; feeds can include verified current account state. See [the complete ownership and recovery contract](../ownership.md) for canonical records, authorization, paging, replay, archive privacy and compatibility limits.
