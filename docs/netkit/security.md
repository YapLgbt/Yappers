# Security, privacy, and moderation

## What is authenticated

Iroh authenticates endpoint public keys and encrypts transport with QUIC/TLS. The join configuration pins the home endpoint key; a direct address or relay change does not change that identity. A valid community capability grants read/write membership, while every mutation must also carry a strict Ed25519 author signature. The home signs acceptance envelopes. Private federation uses the connecting endpoint's authenticated identity plus explicit community scopes. Opt-in public groups use owner-signed host permits for those scopes; self-signed discovery advertisements alone grant nothing.

These layers have different purposes. A member token does not grant peer replication access. A peer cannot change an author's signed body, invent another home attestation, or add an unknown origin through transitive forwarding. It can withhold data, delay replication, deny service, or admit abusive signed identities into a community for which it is trusted. This is a trust federation, not a trustless consensus or censorship-resistant ledger.

## Threat handling implemented

| Threat | Implemented control |
|---|---|
| Modified event or signature | Canonical event hash, strict Ed25519 verification, closed schema |
| Alternate casing creates duplicate voting identities | Public keys/signatures require canonical lowercase hex |
| Retry after lost receipt | Immutable event ID; identical resubmission is idempotent |
| Author sequence reuse/equivocation | Unique `(author, sequence)` database constraint |
| Forged home / changed acceptance | Home signature bound to ID, home key, and acceptance time |
| Unauthorized federation scrape | Explicit endpoint scopes or verified group host permit; a client token is insufficient |
| Community bleed | Per-peer community scopes on serving and importing |
| Unknown origin forwarded by a trusted intermediary | Origin must independently have explicit trust or that group authority's valid host permit |
| Invalid page skips future events | Validate complete page; imports + cursor in one transaction |
| Restart / offline restore reuses low log cursors | Fresh serving generation and idempotent full rescan |
| Oversized / stalled stream | 1 MiB cap, 20-second request deadline, 64 connection slots |
| Unbounded prototype growth | 10,000 event cap, bounded pages/cycles, author rate limit |
| UI-thread networking or FFI allocation leak | Worker-dispatched ABI, copied/freed owned results |
| Credential leakage through local preferences | Keychain for identity/capabilities; protected atomic state file |
| Silent loss of unreadable outbox | Fail closed; no automatic cache replacement |
| Pending events routed after instance switch | Home/community binding and switch guard |

## Privacy semantics

“Anonymous” means the chosen display name is omitted from that post. The signed author key remains visible and linkable. A home knows which publications arrived over a connection. Federated operators receive community content and signed identifiers. Direct peers and relay/address-lookup infrastructure may observe network metadata; Iroh encryption does not make IP addresses or social relationships anonymous. On supported native clients, a person can share a posting area. The app rounds coordinates before storage or federation. [Location behavior](../public-home.md#location-behavior)

The default development mode binds loopback sockets only, disables public relay/discovery presets, and never publishes join capabilities. `n0` mode opts into Iroh's public relay and discovery preset for real networking. Operators must review that metadata/service dependency and may later supply self-hosted relay/discovery configuration.

The private join JSON is a bearer capability. Treat it like a password: do not commit it, paste it into logs, or publish it as an endpoint address. Server configuration includes private instance key and capabilities. CLI-created files are mode 0600; SQLite is pre-created with mode 0600. Access-control and backup permissions still matter on shared hosts. Rotation currently requires updating the instance's capability and distributing new private joins; signed author identity and peer trust are separate.

## Moderation and deletion

Hiding a post changes the viewer’s feed. A separate report action sends the approved excerpt and optional subject ID directly to a staffed primary home. The reporter can check the outcome with a private receipt. Reports stay out of federation, feed backups and standby replication. Operators can hide or unhide an event on their own home. [Report handling and retention](../../deploy/peering/REPORTS.md)

On v5 homes, an owner-signed deletion erases the content opening and preserves verifiable authorship, acceptance and deletion evidence. Participating peers apply it on sync. Managed backups use separately revocable keys. See [deletion, verification and backup limits](deletion.md).

## Operational limits

Yap’s public home runs protocol v5 and accepts portable accounts. Operators remain responsible for handling abuse reports, protecting shared invitations, monitoring capacity and testing restores with the current deletion guard. [Public home runbook](../public-home.md)

The example capability is shared by a community, so possession allows reading and submitting under newly generated author identities. The per-author rate limit is not a Sybil defense. Trusted homes can lie about acceptance time within validation tolerances or withhold events. There is no global clock or cryptographic completeness proof for a materialized feed. Full end-user signature/inclusion verification and richer membership authorization are future protocol work.

No security review beyond source checks and executable negative tests is implied. Physical-device network transitions, IPv6-only cellular paths, public relay fallback, malicious-traffic load, and VoiceOver require additional validation.

## Public group discovery and enrollment

Group descriptors and their city labels are public. Choosing/searching a city filters the downloaded catalog locally; it does not send GPS coordinates or prove where someone lives. Joining/posting can reveal an interest in a city. Home operators and federation peers receive public group content, linkable author identifiers, and permitted-host metadata. A host lease means a recent signed advertisement, not a verified member count or guaranteed uptime.

The catalog seed is not a global trust root. Group descriptors bind a unique ID to their owner's key. That owner admits provider homes using a separate signature domain; announcements bind a permitted host key to expiring endpoint addresses. Imports validate all dependent records before committing, enforce cardinality/size bounds, and retain the newest announcement for each group/home. Outgoing work rotates so a fixed first page of peers does not permanently starve other groups.

On a self-hosted home with automatic federation enabled, members can create public groups. The creator’s home is the group authority. A configured peering policy also requires operator approval for the requesting home and community. Yap’s public home currently declines outside enrollment. A signed permit identifies the home authorized to host a group. It cannot verify the operator’s behavior or the group’s city label.

Permits are stable to validate retained origin history after address expiry or authority outage. There is no authority-wide signed revocation protocol yet. An operator's `blockedHosts` denies that origin prospectively and prevents contact/discovery with that endpoint, without erasing retained history or revoking all certificates it previously signed. Already admitted providers may continue serving that group's history. Losing the group authority key prevents future host enrollment; secure backup/recovery is an operator responsibility.

Private group capabilities, home credentials, and author secrets never enter directory records. Hosting a discovered group is an explicit member action on a home whose operator enabled this feature. Discovery alone does not automatically host every catalog group or expose a private community. Local leaving/unhosting, private/invitation-only group creation, group-wide bans and a public exhaustive index remain follow-on work.

## Portable account ownership

New portable-account events bind the root account, authorized device and chosen home. Account
state is revisioned and root-signed; known forks cannot overwrite a stored history. A move
uses a new writing key and does not require permission from the old home. Signed updates
only enforce revocation once a home learns them; offline/malicious operators are not globally
controlled. Root-signed state includes linkable account/device/home identifiers, including on
anonymous-display posts. Recovery keys remain client-side; encrypted backups contain them.

Clients retain exact signed acceptance receipts before clearing uploads. Private personal
imports never grant public event-origin trust, group admission or moderation exemptions.
Owned-history reads require an active-device proof and a home membership capability; only
one's own authored events are returned, across that home's communities. See [ownership and
recovery](../ownership.md) for bounds, encryption, pending-draft handling and remaining limits.
