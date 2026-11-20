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

## Home database upgrades

A standalone Home uses native Turso MVCC, with one process owning the database and multiple connections serving requests. Federation pulls up to four peers concurrently. A page and its checkpoint commit together. Writes that allocate a feed, mailbox or group cursor coordinate on the corresponding stream so a peer can safely resume after its last cursor. Independent writes use separate transactions and retry from a fresh snapshot on a write conflict.

The public feed selects up to 200 visible threads from an index ordered by acceptance time and event ID. Vote totals and community event counts update in the same transaction as the events. A Home rebuilds its in-memory reply routing index on demand after a restart. Feed reads open the selected posts and replies against the current deletion guard. Nearby searches may examine additional threads to find matches. Signed-in feeds with blocks or content privacy filters use the full projection to preserve their visibility rules and filtered event counts.

The first startup on a mainline SQLite catalog applies the legacy steps in `netkit/src/migrations.rs` and imports the rows into the native primary. The import preserves existing cursors and their high-water marks. The old catalog becomes the locator after the native transaction commits. Allow space for both databases until the upgrade finishes. Run the upgraded binary for subsequent starts.

Append native schema changes to the ordered list in `netkit/src/native_primary.rs` and add SQL under `netkit/migrations/native/`. The schema changes and native version advance in the same transaction. Guard migrations live in `netkit/migrations/guard/`. Keep released steps intact and test each new step against a populated database at the preceding version. Backup and restore continue to use the protected archive format. [Storage and backup](deletion.md#storage-and-backup)

## Two-instance local development

```sh
cargo run --locked -p yappers-backend --bin yappers-server -- demo --directory netkit-data/demo
```

This runs A and B in one development process with separate stable instance keys, native databases, capabilities, and replication cursors. Both host the same community and explicitly trust each other's endpoint ID for that community. Each startup rewrites private join files with current ephemeral loopback addresses. It uses actual Iroh QUIC, not an HTTP substitute or mocked transport. Stop with Ctrl-C.

Private files:

- `netkit-data/demo/a/config.json`, `b/config.json`: instance secrets and peer policy.
- `a/events.sqlite` and `b/events.sqlite` locate the two Homes' independent Turso databases.
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

The dedicated Linux server runs the primary Iroh home. Fly hosts the website, enrollment document and PWA. See [public home operations](../public-home.md) for service names, state paths, enrollment and deployment checks.

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

## Inbox delivery

The dedicated Home stores read positions, thread mutes, device subscriptions and pending delivery leases in its Turso database. It runs the delivery worker, which sends a generic alert and a random route. Opening the route authenticates the account and installation and checks current visibility. The public feed replica never registers devices.

For a standalone home, install Node and run `npm ci --omit=dev --prefix deploy/push`. Set `YAP_PUSH_WORKER` to the absolute path of `deploy/push/send.cjs`. Set `YAP_PUSH_PROVIDERS` to a comma-separated list of the configured providers. Restart the service after changing its environment.

| Provider | Configuration |
| --- | --- |
| Web Push (`web`) | Set `YAP_WEB_PUSH_KEY` to the public VAPID key and `YAP_WEB_PUSH_PRIVATE_FILE` to the owner-readable private-key file. Keep the same key across releases. |
| Android (`fcm`) | Build with `YAP_FIREBASE_ANDROID_CONFIG` pointing to the Firebase Android configuration for `app.yappers.android`. Set `YAP_FCM_CREDENTIALS` on the home to a service-account file authorized for FCM v1 in that project. |
| Apple (`apns`) | Enable Push Notifications for the signing team's `app.yappers.prototype` App ID and provisioning profile. Set `YAP_APPLE_TEAM_ID`, `YAP_APNS_KEY_ID`, `YAP_APNS_KEY_FILE` and `YAP_APNS_TOPIC` on the home. Set `YAP_APNS_SANDBOX=1` for development builds. TestFlight and distribution builds use production APNs. |

An installation opts in through **Inbox → Device notifications**. The three categories are replies, messages and message requests. On iOS, Web Push is available to the installed Home Screen web app. Test native distribution builds by closing Yap, sending a reply from a second account, opening the OS alert and checking the thread and badge. Repeat with an accepted message and a held request on the registered messaging installation.
