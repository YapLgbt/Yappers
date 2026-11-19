# Development and operations

Run commands from the repository root. The current Mac testing deployment is managed with `./yap`; see [local federation management](../local-federation.md). No remote deployment is performed automatically.

## Toolchain

Install full Xcode and Rust via the official rustup distribution. The repository pins Rust 1.93.1 and installs simulator/device targets through `rust-toolchain.toml` / `scripts/bootstrap.sh`. JDK 21, XcodeGen, the Gradle wrapper, and an iOS simulator are used by the existing app. Build with `--locked` to use the dependency versions in `Cargo.lock`.

```sh
./scripts/bootstrap.sh
cargo test --workspace --features yappers-netkit/server --locked
JAVA_HOME=$(/usr/libexec/java_home -v 21) ./gradlew :composeApp:jvmTest
./scripts/run-ios.sh
```

The first Rust cross-build downloads and compiles Iroh's dependencies. Later app builds reuse the matching static archive. A physical iPhone selects `aarch64-apple-ios`; Apple Silicon Simulator selects `aarch64-apple-ios-sim`.

## Two-instance local development

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- demo --directory netkit-data/demo
```

This runs A and B in one development process with separate stable instance keys, SQLite logs, capabilities, and replication cursors. Both host the same community and explicitly trust each other's endpoint ID for that community. Each startup rewrites private join files with current ephemeral loopback addresses. It uses actual Iroh QUIC, not an HTTP substitute or mocked transport. Stop with Ctrl-C.

Private files:

- `netkit-data/demo/a/config.json`, `b/config.json`: instance secrets and peer policy.
- `a/events.sqlite`, `b/events.sqlite`: durable independent logs.
- `join-a.json`, `join-b.json`: private client configurations, including the capability.

The entire directory is ignored by Git. In the app, open You → Join community, paste one private join file, then Join instance. The local iPhone simulator shares the Mac's loopback network. Local mode does not reach a physical phone or another machine. Use `n0` mode for non-loopback networking.

Use another Rust client to inspect replication:

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- post \
  --join netkit-data/demo/join-a.json --identity netkit-data/demo/client.json \
  --body 'Hello from a Rust client' --topic Chatter --anonymous
cargo run --locked -p yappers-backend --bin yappers-server -- feed \
  --join netkit-data/demo/join-b.json --identity netkit-data/demo/reader.json
```

A CLI identity stores its signing key, next sequence, and pending events in a private JSON file. `post` persists before sending; `retry --join ... --identity ...` retries exact queued IDs. Do not share that file between independent concurrently running clients. If a command errors after acceptance, retry rather than constructing another post.

The demo also creates real public sample groups for New York, London, and worldwide scope. B discovers A through a public bootstrap address and automatically enrolls for the New York group. `join-group-a.json` and `join-group-b.json` are private group invitations; group scopes are not added to the manual private-peer list.

## One independently operated instance

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- init --directory netkit-data/home --network n0
cargo run --locked -p yappers-backend --bin yappers-server -- run --config netkit-data/home/config.json
```

`init` refuses to overwrite a configuration. It creates a stable instance key and an initially unpeered community. `run` writes private `join-0.json` (and subsequent numbered files for additional communities) beside the config. N0 readiness is bounded; startup reports failure instead of writing an unusable relay join. Public relay/discovery availability and metadata exposure are real dependencies of N0 mode.

To federate independent instances, configure the same community string on both; choose a separate private membership capability at each instance. Add the other instance's `EndpointAddr` to each `peers` array, with an explicit `communities` array. Obtain the public address object from the other operator through an authenticated channel; copy only its `serverAddr`, not its membership token. An empty peers list means no explicit private-community federation; opted-in public groups can still auto-federate. Trust must be mutual to exchange logs in both directions. A newly trusted origin can introduce history for its authorized communities; review that policy before adding it.

Configuration shape is `ServerConfig` in `netkit/src/server.rs`: `privateKey`, `databasePath`, `communities` map, `peers` list of `{address, communities}`, and `networkMode` (`local` or `n0`). CLI-created database paths are absolute. For manual config, relative paths resolve from the process working directory. Server config changes currently require restart.

## The yap.lgbt deployment

The dedicated Linux server runs the primary Iroh home. Fly hosts the website, enrollment document and a peer for the primary’s public feed. See [public home operations](../public-home.md) for service names, state paths, enrollment and deployment checks.

## Moderation, backup, and recovery

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- moderate \
  --database /path/to/events.sqlite --event <event-id>
# Add --unhide to reverse the instance-local hide.
```

Back up private configuration separately. For feed databases and HA ledgers, use the [protected backup and restore commands](deletion.md#storage-and-backup).

If a peer serves an invalid page, its cursor intentionally does not advance. Inspect logs, repair the remote source, or remove trust. Do not skip a cursor manually as a convenience; that can hide missing events. A repeated sequence conflict is an identity/restore error, not a reason to re-sign queued content silently.

Publications and registrations continue as the home grows. Each Iroh frame carries at most 1 MiB. Updated clients follow additional pages for large deletion lists, group catalogs, and private membership histories. Upgrade clients before histories grow beyond a single response.

## Native end-to-end validation

`scripts/test-netkit-ios.sh` builds a private two-instance fixture, inserts its join into an ignored test resource, runs the isolated native UI test, and checks that the iPhone-authored post, reply, and vote reached instance B. It restores the skip fixture and stops its own local servers. Normal UI runs use isolated storage and do not need a backend; the federation test is skipped unless the explicit live fixture is installed.

## Automatic public-group federation

Start a first operator with public groups enabled:

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- init --directory netkit-data/first --network n0 --auto-federate
cargo run --locked -p yappers-backend --bin yappers-server -- run --config netkit-data/first/config.json
```

`run` writes `address.json`, a public EndpointAddr suitable for sharing as a bootstrap address. Share only that public address, not `join-0.json` or config.json. Obtain it through an authenticated operator channel so the endpoint ID is the intended seed. On the second operator, copy that address JSON to `bootstrap.json` and initialize:

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- init --directory netkit-data/second --network n0 --auto-federate --bootstrap bootstrap.json
cargo run --locked -p yappers-backend --bin yappers-server -- run --config netkit-data/second/config.json
```

For existing configurations, add `autoFederation: {"enabled":true,"seeds":[<public EndpointAddr object>],"blockedHosts":[]}` and restart. Seeds and blocklists are bounded. This does not modify private `peers` or share their capabilities. Bootstrap discovery needs a reachable seed; it cannot discover a disconnected network from nothing. Public relay/address lookup is Iroh's N0 transport; the group catalog is application-level signed gossip, not a newly deployed DHT or central index.

In the iOS app, connect to a home once, then use Groups to choose a city or worldwide scope, search, create, join, and reopen groups. The home enrolls with the group authority automatically; there is no peer-pair editing. A saved group keeps its home-specific invitation in Keychain. Pending uploads block switching to another feed. A joined group's home continues hosting/replicating even when its phone sleeps.

Equivalent CLI operations:

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- groups --join netkit-data/first/join-0.json --identity netkit-data/first/client.json
cargo run --locked -p yappers-backend --bin yappers-server -- create-group --join netkit-data/first/join-0.json --identity netkit-data/first/client.json --name 'City readers' --description 'Books and new friends around the city.' --city 'New York' --region NY --country US --output netkit-data/first/readers.join.json
# Omit city/country/region for a worldwide group. Copy the public group ID from its summary.
cargo run --locked -p yappers-backend --bin yappers-server -- join-group --join netkit-data/second/join-0.json --identity netkit-data/second/client.json --group '<public group ID>' --output netkit-data/second/readers.join.json
```

These commands print public summaries and write private join results only to the requested output file. Use that join file with `post`, `feed`, and `retry`. The same author identity reserves sequences across all its groups; do not clone it into concurrently writing independent clients.

Keep authority keys, additive catalog/permit/address/hosting tables, and event logs together in backups. Catalog metadata gossips, but a home needs its own saved hosting choices and signing key to resume its role. Group descriptors are immutable; metadata edits/revocations and home unhosting/member management are not implemented. Same normalized name/locality with unchanged details is idempotent on one home; another owner can legitimately have a different same-named group.

## Full home takeover

A full-service standby can operate the enrolled logical home when the dedicated server is unreachable. The home owner signs the standby's Iroh identity. Treat this grant as permission to process account records, private moderation state, signed prekeys and encrypted mail. Device recovery secrets and Olm ratchets stay on clients. The original public projection peer can continue to serve clients that use the public-read protocol.

The Fly standby holds the single durable commit ledger on its persistent volume. For each operation, the executing node gets the current revision and a short one-use permit from that authority. A successful response follows a single EXTRA-synchronous SQLite transaction that records the new revision, changed database image and retry receipt. Restarts discard outstanding permits. After a primary crash, authenticated clients retry on Fly with the same operation ID and the same logical home, preserving account proofs and signed receipts. While using Fly, a client retries the dedicated route after 20 seconds. If both private routes are unavailable, a client enrolled for full takeover retains its last authenticated feed.

### Provision the two authorized machines

1. Create an independent transport identity for the HA service on Fly in a private directory. Keep the existing public Iroh peer running separately. Obtain the new identity's public endpoint address and use only its public address file when signing the grant.
2. On the dedicated home, run `yappers-server ha-authorize --config PRIMARY_CONFIG --standby STANDBY_ADDRESS_JSON --output GRANT_JSON`. The grant contains the logical home ID, physical standby address and an Ed25519 signature. Iroh authenticates the signed standby ID even when its network address changes.
3. Provision the logical home config privately to the authorized standby, with its own staging database path. Both machines must have the same signing key, community credentials, peer scope policy and automatic-federation admission settings. Each machine needs a private work directory, separate from the original public peer. Keep the Fly ledger and configuration on one persistent volume. Give only one authority process access to that volume and transport identity.
4. Start Fly with `yappers-server ha-run --config LOGICAL_HOME_CONFIG --grant GRANT_JSON --standby-config STANDBY_TRANSPORT_CONFIG --ledger LEDGER`. Its private service becomes usable once the ledger is seeded.
5. Stop the original standalone dedicated home, confirm its old supervisor cannot restart it, and take a consistent backup. On that machine run `yappers-server ha-bootstrap --config PRIMARY_CONFIG --grant GRANT_JSON`. The command transfers the committed database, verifies the authority acknowledged the seed, and retires the source database and backup keys. It refuses to replace a different seed. Create a new protected HA backup after the transfer.
6. Start the dedicated service using `yappers-server ha-run --config PRIMARY_CONFIG --grant GRANT_JSON`. Publish the grant under the separate top-level `fullHomeStandby` key of the HTTPS enrollment document. Keep the existing `join.serverAddr` pinned to the original logical identity. Old clients require the dedicated physical endpoint to reach private services.

### Operations and recovery

- Use `yappers-server ha-moderate --ledger LEDGER --grant GRANT_JSON --event EVENT_ID` on Fly to hide a post, or append `--unhide`. The update participates in the live ledger's revision fencing.
- Use the [HA export and guarded restore procedure](deletion.md#storage-and-backup) for a storage move or cutover. The exported file is an encrypted backup. HA workers stage the fenced image in memory.
- A dedicated-to-Fly partition prevents the dedicated node from acknowledging reads or writes. A client with an independent connection to Fly can continue there. An unavailable Fly authority blocks both workers from serving fresh private data. Loss of that authority volume requires recovery of its durable state. Starting a second authority from an old backup breaks fencing.
- Each worker serializes staging operations. Permits expire after eight seconds on the authority's monotonic clock. Foreground calls get a five-second first-route probe. Background federation skips busy workers and gives discovery, each peer page and peer metadata independent operations with three-second work deadlines. Each verified page commits its cursor and records together. During takeover, an independent foreign peer that only trusts the dedicated Iroh identity waits for the dedicated route before further outbound federation pulls or foreign host enrollment. The local authorized home continues serving its committed records.
- The standby owns the durable SQLite image. Changed commits transfer a full image. Both workers default to a 64 MiB image budget. Set `YAP_HA_SNAPSHOT_MIB` on both workers and the HA operator commands after measuring memory, bandwidth and latency at the proposed size. An in-place budget increase preserves the ledger, receipts, guard and home identity. Size the standby first. Keep the old authority fenced throughout any volume move.
- Fly monitors three private HTTP checks in addition to HTTPS enrollment. The replica on port 8081 returns 503 from `/ready` until every configured public projection is verified within 120 seconds. The standby on port 8082 returns 503 until it can read a seeded ledger head matching its current guard. Its separate `/capacity` check goes unhealthy at 80% of the configured image budget. These machine checks keep the enrollment site routable during worker recovery. Configure paging for each check and volume free space, and monitor a read from an independent Iroh client.
- Before increasing traffic, exercise peak simultaneous reads, writes and deletion propagation on a representative copy. Record p95/p99 latency, peak worker RSS, committed image size, network transfer time and free volume space. Verify a protected restore with the independently maintained current deletion guard on an isolated machine and record elapsed recovery time.
- Run `cargo test -p yappers-backend --test full_takeover --locked` for the disposable two-process crash and partition drill. The test kills both roles in succession, verifies ciphertext delivery and deduplication, and checks private policy and recovery. To exercise both production endpoints deliberately, download the public HTTPS enrollment to a local file and run `YAP_SMOKE_ENROLLMENT=manifest.json cargo test -p yappers-netkit --features server --lib deployed_full_home_private_exchange --locked -- --ignored --nocapture`. This opt-in check creates two disposable accounts, two encrypted test messages and one private block.

### Public home profile cutover

Protocol 6 starts with fresh public application state. Fly uses `/data/v6` and the dedicated service uses `/var/lib/yap-iroh-replica/v6`. Keep the logical home signing key, granted standby transport identity, grant, peer policy and published source archives. Rotate the public join token in the primary, standby, peer and HTTPS enrollment together. Set the manifest join protocol to 6. Start the new authority with an empty ledger and bootstrap it from a fresh dedicated database before starting the primary supervisor. Export a protected backup from the new ledger after verifying both roles.

The previous volume paths contain the retired generation. A rollback pairs the previous service binaries and enrollment with those paths. Keep a single running primary for each logical identity. The new signing domain prevents a queued event from the retired generation being accepted after re-enrollment.

## Inbox delivery

The home stores read positions, thread mutes, device subscriptions and pending delivery leases inside its committed SQLite image. The public feed replica never registers devices. Both HA members use the same Web Push key and run the delivery worker with the providers configured for that logical home. The worker sends a generic alert and a random route. Opening the route authenticates the account and installation and checks current visibility.

For a standalone home, install Node and run `npm ci --omit=dev --prefix deploy/push`. Set `YAP_PUSH_WORKER` to the absolute path of `deploy/push/send.cjs`. Set `YAP_PUSH_PROVIDERS` to a comma-separated list of the configured providers. Restart the service after changing its environment.

| Provider | Configuration |
| --- | --- |
| Web Push (`web`) | Set `YAP_WEB_PUSH_KEY` to the public VAPID key and `YAP_WEB_PUSH_PRIVATE_FILE` to the owner-readable private-key file. The Fly standby reads these from the volume at `$YAP_SERVICE_STATE/push/{public,private}`. Keep the same key across releases. |
| Android (`fcm`) | Build with `YAP_FIREBASE_ANDROID_CONFIG` pointing to the Firebase Android configuration for `app.yappers.android`. Set `YAP_FCM_CREDENTIALS` on the home to a service-account file authorized for FCM v1 in that project. |
| Apple (`apns`) | Enable Push Notifications for the signing team's `app.yappers.prototype` App ID and provisioning profile. Set `YAP_APPLE_TEAM_ID`, `YAP_APNS_KEY_ID`, `YAP_APNS_KEY_FILE` and `YAP_APNS_TOPIC` on the home. Set `YAP_APNS_SANDBOX=1` for development builds. TestFlight and distribution builds use production APNs. |

An installation opts in through **Inbox → Device notifications**. The three categories are replies, messages and message requests. On iOS, Web Push is available to the installed Home Screen web app. Test native distribution builds by closing Yap, sending a reply from a second account, opening the OS alert and checking the thread and badge. Repeat with an accepted message and a held request on the registered messaging installation.
