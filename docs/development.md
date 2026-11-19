# Developing Yap

Start with [the root README](../README.md) to install the pinned toolchains and run the app. [Netkit's guide](netkit/README.md) explains the federation protocol and operating model. This document describes where to make changes and which invariants to preserve.

## Code map

| Area | Responsibility |
|---|---|
| `composeApp/.../commonMain/.../App.kt` | Compose screens and UI actions; chooses demo or live state |
| `FeedRepository.kt` | Fictional local demo and common feed models/filtering |
| `Netkit.kt` | Public storage/engine contracts and immutable observable state |
| `NetkitController.kt` | Connection ownership, serialized mutations, durable outbox, foreground sync |
| `Groups.kt` / `GroupsScreen.kt` | Public group models, local filtering, discovery/create/join UI |
| `NetkitCache.kt` | Versioned cache schema, stable local ID allocation, pure home projection/pending overlays |
| `composeApp/.../iosMain/.../NativeNetkitEngine.kt` | Worker-dispatched C calls, JSON result ownership, cancellation cleanup |
| `MainViewController.kt` | iOS lifecycle, Dynamic Type, demo UserDefaults adapter |
| `iosApp/iosApp/AppleNetkitStorage.swift` | Keychain and protected atomic file implementation |
| `netkit/src/model.rs` | Closed wire schema, canonical signing bytes, validation, convergent projection |
| `netkit/src/client.rs` | Local event preparation and separate authenticated submission/read RPCs |
| `netkit/src/ffi.rs` | Owned C strings, opaque handles, Rust runtime, panic/error boundary |
| `netkit/src/transport.rs` | Iroh endpoint presets and bounded request/response framing |
| `netkit/src/server.rs` | Membership admission, scoped peer/origin trust, replication orchestration |
| `netkit/src/groups.rs` | Canonical signed group descriptors, host permits, expiring announcements |
| `netkit/src/directory.rs` | Durable bounded public catalog and hosting records |
| `netkit/src/auto_federation.rs` | Server child module: discovery, authority enrollment, dynamic public scopes |
| `netkit/src/store.rs` | SQLite transactions, immutable envelopes, moderation, replication checkpoints |
| `backend/src/main.rs` | Operator/development-client CLI and private configuration files |

The abbreviated Kotlin paths above start at `composeApp/src/commonMain/kotlin/app/yappers/` or `composeApp/src/iosMain/kotlin/app/yappers/`.

## Client state and persistence

Keep the controller's mutex around mutable connection/cache/outbox operations. Cache transformations in `NetkitCache.kt` are pure: they neither persist nor send. `commit` writes the protected file before publishing matching in-memory state. Initial cache loading is synchronous during controller construction; subsequent writes/credential operations use a worker dispatcher. Corrupt live state fails closed because falling back to sample data could erase signed pending operations.

Local numeric IDs identify Compose items. They are not timestamps or truncated event hashes. **New** preserves the repository/home projection order, including after a refresh allocates IDs for newly observed remote posts. **Popular** sorts by score, retaining source order for ties. Server-side chronological ordering belongs to `model.rs`.

A new operation follows this order:

1. Rust prepares an immutable signed event without sending it.
2. Keychain reserves the next author sequence.
3. The protected file stores the exact signed event and optimistic cache.
4. UI completion acknowledges local enqueue.
5. Sync submits that same event, then removes it only after a matching receipt.

A failed write can consume a sequence; gaps are safe. A timeout can follow remote acceptance; retry the same event ID. Never re-sign an operation merely because its receipt was lost. Keep the current v1 JSON field names and missing-value sentinel compatible when reorganizing Kotlin classes. The loader accepts both the original single-cache v1 shape and the current archive wrapper; the next successful write upgrades the former without losing its outbox.

Connection installation and durable demo switching use noncancellable critical sections. Shutdown also finishes native close before forgetting a handle. Do not replace native `open` with an ordinary cancellable dispatcher call: if cancellation wins after allocation, its returned handle still needs explicit cleanup. The process-lifetime native worker scope exists for that cleanup, independently of screen lifetimes.

## Rust trust and transaction boundaries

`model` checks cryptographic/structural invariants. `server` checks membership, designated home, action relationships, peer scopes, and independent origin trust. `store` enforces durable uniqueness and transactions. Calling the public store directly does **not** authorize a publication or origin. In particular, `import_page` callers must validate scope and page ordering before committing.

Validate peer configuration before binding/spawning long-lived server work. Do not hold a SQLite guard across a network await. Import an entire validated page and its cursor together; a bad page must not advance the checkpoint. A serving generation or authorized-scope change resets scanning to zero, while immutable IDs deduplicate already retained history. Sync return counts describe examined entries, including duplicates; they are not a count of newly created events.

Named limits live with the layer that enforces them. The shared frame/deadline/page bounds are in `netkit/src/lib.rs`, server admission/cycle bounds in `server.rs`, event capacity in `store.rs`, and mobile outbox/batch limits in `NetkitController.kt`. Changing limits is a capacity decision, not a substitute for pagination or abuse policy.

## Build and ABI changes

Compose dependencies use explicit coordinates at their existing resolved versions: runtime/foundation 1.12.2 and Material 3 1.9.0. Material 3 has an independent release version; do not copy the Compose plugin version onto it. Coroutines is declared directly because application code uses it. `kotlin.mpp.enableCInteropCommonization=true` shares the custom Netkit C declarations across device/simulator source sets; see [Kotlin's native library sharing guidance](https://kotlinlang.org/docs/whatsnew1530.html#ability-to-use-custom-cinterop-libraries-in-shared-native-code). Verify commonized declarations with `./gradlew :composeApp:commonizeCInterop` after changing the header or target definitions.

`iosApp/project.yml` is authoritative. Run `(cd iosApp && xcodegen generate)` after changing project settings or adding Swift files. The generated Xcode project is tracked; `.generated/` and build products are not. Preserve the generated Kotlin framework checksum source: it makes Xcode relink a changed static Kotlin/Rust archive in the same build.

Keep `netkit.h` and exported Rust C functions synchronized. Inputs are borrowed NUL-terminated UTF-8 for the call duration; outputs are owned and freed once with `yappers_netkit_string_free`. Kotlin copies JSON before freeing it. No Rust layouts or exceptions cross the ABI. Adding commands requires changes to Rust dispatch, Kotlin integration, executable tests, and [the ABI guide](netkit/ios-ffi.md).

Changing signed payload field order, serde attributes, canonical string encoding, or incompatible wire fields requires a new application protocol version/ALPN and conformance tests. A source cleanup must preserve signatures, event IDs, existing databases, and persisted outboxes. For an Iroh upgrade, update the exact Cargo dependency and `IROH_VERSION`, regenerate Cargo.lock deliberately, revisit the documented libc/mio compatibility pair, and run both native target builds. Do not remove compatibility pins just to make dependency resolution float.

Simulator signing uses ad hoc identity plus simulator-only Keychain entitlements. Device builds use actual Apple provisioning. Do not copy simulator entitlements into device configuration or replace Keychain with UserDefaults to bypass a test-signing failure.

## Verification

```sh
./scripts/check.sh
./scripts/test-netkit-ios.sh
```

The first command checks Rust formatting/lints, Rust tests, Rust API docs with warnings denied, and Kotlin JVM tests. It selects JDK 21 on macOS; elsewhere provide a working JDK 21 environment. The native command creates its own private two-home fixture, runs both demo and live UI flows, and independently checks federation at home B. It restores the skip resource and stops its own servers afterward. Boot an iPhone simulator before starting the test. If several are booted, pass the desired UDID as the first argument.

For a device compilation check without provisioning:

```sh
JAVA_HOME=$(/usr/libexec/java_home -v 21) xcodebuild \
  -project iosApp/Yappers.xcodeproj -scheme Yappers -configuration Debug \
  -destination 'generic/platform=iOS' -derivedDataPath build/ios \
  CODE_SIGNING_ALLOWED=NO build
```

For platform-specific, recovery and federation checks, see [Testing Yap](netkit/validation.md).

## Repository hygiene

Keep private join files, signing keys, SQLite databases, fixture logs, generated bridge files, and build products out of source control. `netkit-data/`, `build/`, and `iosApp/.generated/` are ignored. Use CLI-created private files for development; never place real capabilities in sample configuration or screenshots.

## Public groups and automatic scopes

New public groups are owner-signed immutable descriptors with separate signature domains for provider permits and expiring addresses. Keep the private-community allowlist path intact; catalog discovery is not authorization. A provider's authority-signed permit must independently match the group and origin. A self-signed host address never grants private or public scope on its own.

Checkpoints now bind both serving generation and a hash of the authorized community set and operator blocklist. Expanding a peer's scope must rescan from zero: the old global cursor may already have skipped rows for the newly joined group. The serving log remains global and advances over filtered rows. Page imports, generation, scope, and cursor commit together. This is covered by the late-worldwide-join regression test.

`groups-v1` is a separate Keychain schema, not a feed-cache migration. It holds saved home-specific group joins, selected city, and the retained private home feed. Saved groups keep their original home; opening them can enable live mode from the demo. New group creation/joining requires a connected home and an empty old-target outbox. Reopening the same queued group is safe and supported. Group creation is semantically idempotent per home/name/locality with unchanged details, but is not a signed content outbox operation.

`NETKIT_TEST_SCOPE=groups ./scripts/test-netkit-ios.sh` runs just the group UI/federation scenario while iterating. The default runs the full native suite. `NETKIT_CAPTURE_UI=1 ./scripts/test-netkit-ios.sh` also captures the Groups screen in light/dark/large text and restores the simulator's original settings on exit. The test-only `--show-groups` launch argument opens discovery directly for native visual QA; it does not reset storage or bypass membership. See [groups](netkit/groups.md) for canonical records, consent, quotas, and remaining production work.

When changing replication, preserve the [page and cursor behavior for denied origins](netkit/architecture.md#handling-denied-origins).

## Profiles and headline posts

See [the profile/post guide](profiles-and-posts.md) for the photo boundary, local persistence, anonymous omission, historical profile snapshots and the signed post format. Use the system photo picker; do not add unrestricted photo-library access. Read focus/keyboard composition locals inside modal content, since a sheet owns a different focus root from the feed.

## Public brand and compatibility

The public name is **Yap**, the app wordmark is **yap**, and the owned domain is
**yap.lgbt**. Shared UI strings live in `AppBrand.kt`; native app labels and packaging
metadata must match. See [public home operations](public-home.md) for DNS and deployment.

Existing `app.yappers.*` bundle/package IDs, Keychain service identifiers, defaults keys,
data directories, Rust crate names, ABI symbols, ALPN and signed-message domains remain
unchanged. They are compatibility identifiers, not public product copy. Renaming them
requires an explicit migration, especially for installed identities and signed history.
Xcode project/target/scheme and the Kotlin framework remain `Yappers`; the installed iOS
product is `Yap.app`, and the desktop package is `Yap.app`. The management entry point
is `./yap`; `./yappers` remains an alias.
