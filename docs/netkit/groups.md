# Local and worldwide groups with automatic federation

A group has one signed, globally unique identity, public descriptive metadata, and an optional coarse locality (city, region, two-letter country). No locality means worldwide. Matching a city is a discovery filter, not proof of residence or a geofence. People may browse another city without sending coordinates. Names and cities are descriptive claims, not official verification or unique namespaces.

Each group is also a distinct event community. Posts, replies, votes, bookmarks, and pending uploads stay within that group's feed. The app browses a public catalog, filters by a manually chosen city or worldwide scope, creates groups, joins through its existing home, and remembers joined groups for switching. Pending uploads must be delivered or explicitly discarded before switching feeds.

## Automatic federation

1. Operators enable automatic federation and configure one or more public Iroh bootstrap addresses. Private communities keep their existing explicit peer policy.
2. Nodes exchange bounded public directories over authenticated Iroh. Directories contain owner-signed immutable group descriptors, owner-signed host permits, and host-signed expiring address announcements. No client capability or author secret is published.
3. A member creates or joins a group through their home. The home finds the group's authority by its cryptographic owner ID and discovered address. For these public/open groups, it automatically asks that authority for a host permit over a connection proving its own endpoint identity.
4. The authority signs permission for that home to originate and replicate this group. A self-signed advertisement without the authority's permit grants no trust. No manual peer-pair editing is needed.
5. The home persists the permit and hosting choice, announces its current address, discovers other permitted hosts, and exchanges the group's event log using the existing atomic cursor/deduplication protocol.
6. Address leases expire and renew; stable host permits remain verifiable for retained historical events. Operators can block endpoint identities locally. Existing permitted homes continue exchanging data if the creator is offline; new provider enrollment needs the group authority to be reachable.

Catalog discovery and group admission are separate. The group authority signs host permits. When an operator configures a peering policy, the home also checks the peer’s identity and approved communities. Outside homes cannot enroll with Yap’s public home while admission is closed. Operators can block a host locally. Group authorities currently have no network-wide permit revocation mechanism. [Peering policy](../../deploy/peering/OPERATIONS.md)

## Version and storage boundaries

Group discovery, enrollment and replication use transport v5 (`yappers/netkit/5`). New group posts and replies use the same event version 4 and deletion proofs as other communities. The directory records below have their own signing formats. [Event commitments and deletion](deletion.md)

Homes store the catalog, host permits, addresses and local hosting choices in SQLite. A change to a peer’s authorized communities triggers a rescan so the home can retrieve older posts in a newly joined group. iPhone stores group invitations, the home-feed invitation and the selected city in Keychain.

The catalog holds up to 64 groups and 128 host permits and announcements each. Each group supports 16 providers, and a home can host 24 groups. A catalog reflects the homes reachable through its configured seeds.

## Testing

The [group tests](../../netkit/tests/groups.rs) exercise discovery, signed permits, local blocklists and replication between homes. Run `NETKIT_TEST_SCOPE=groups ./scripts/test-netkit-ios.sh <simulator-udid>` to create and join a group from the iPhone simulator and check replication.

## Canonical signed records

All records use closed serde schemas and compact UTF-8 JSON generated/verified in Rust. Do not reorder fields without a versioned signing-contract change.

- `SignedGroup` has `id`, `payload`, and `signature`. Payload order is `version`, `owner`, `nonce`, `createdAt`, `details`, `federation`. Version is 1 and federation is `open`. Details order is `name`, `description`, `category`, `locality`; locality is null or `{city,region,country}`. Names are 1–60 UTF-16 units, descriptions 1–300 (newlines/tabs allowed), city/region at most 60, and country exactly two uppercase ASCII letters. These are syntactic labels, not a country/gazetteer verification service.
- Group ID is `owner + "/groups/" + lowercase_hex(BLAKE3(compact_JSON(payload)))`. Its signature covers `"yappers:group:v1\0" || compact_JSON(payload)`. Here `\0` denotes one NUL byte. Owner is the authority/home public key, not a user account name.
- `HostPermit` has `group`, `host`, `signature`. The group authority signs `"yappers:group-host:v1\0" || compact_JSON([group,host])`. Keys and signatures require canonical lowercase hex. This stable certificate authorizes the host for the named group, including retained history; it has no expiry or global revocation mechanism in this version.
- `HostAnnouncement` has `payload` and `signature`. Payload order is `group`, `address`, `issuedAt`, `expiresAt`. The host signs `"yappers:host-address:v1\0" || compact_JSON(payload)`. The address ID must equal the permitted host; the encoded address is capped at 4096 bytes. Times use Unix seconds, with five-minute future-issue tolerance and at most one-hour validity. A restarting host advances its own issue time monotonically when replacing a same-second stale address.
- `DirectorySnapshot` carries `groups`, `permits`, and `hosts`. The complete dependency set is validated before its atomic catalog transaction. Unknown groups, forged certificates, expired/tampered host records, and capacity excesses reject the batch. Newer issue time wins per group/host; stale advertisements cannot roll back an address. Cached groups/permits survive process restarts; expired addresses do not confer current availability.

The app receives authenticated home-projected summaries, like its feed. It does not independently verify every catalog signature or an inclusion/completeness proof. The server performs those checks. Group homes are recently advertised providers, not verified online users or member totals. Bootstrap deployment, Internet relay/NAT behavior, and real-device network transitions require operational validation beyond the local tests.

See [how a home handles denied origins](architecture.md#handling-denied-origins) during a group sync.

## End-to-end encrypted private groups

Open **Groups → Private groups**. The owner creates a group and shares its invitation with someone on the same home. The recipient opens it, copies their signed join request, and sends it directly to the owner. The owner approves the device. Approval starts that device's history, and additional or replacement devices join on their own. Owners remove devices from the group's member list.

Netkit pins the owner's device signing key through the invitation. Each membership change extends the owner's signed roster with a new revision. Every sending device uses a vodozemac Megolm session for that revision and sends its key in an Olm envelope for each current device. A new revision requires fresh sessions, as do the session age and message limits. Before upload the client checks the current roster again. If membership changed, retrying re-encrypts the saved draft with the current recipient set and preserves its publication ID.

Homes store encrypted titles, post and reply bodies, and author display labels. They can see group names, device rosters, event senders, reply links, timing, and sizes. Home databases and snapshots store ciphertext and wrapped keys. Access to a new roster revision still depends on the home delivering the owner's signed update. Clients reject rollbacks below a revision they've already seen.

Group keys live on the approved device. The original owner device manages membership, and recovering an account requires joining again on the replacement device. The per-group Olm fallback keys allow an approved device to receive session keys while offline. Compromise of its local keys can expose previously delivered envelopes addressed to that device. A removed device may retain content and keys it already received.

Creation, approval, and posting require a home connection. If delivery fails after preparation, the device keeps the draft and offers **Retry pending post**. A group supports up to 16 approved devices and 2,000 posts and replies. The roster holds up to 100 signed revisions, after which the group is read-only.
