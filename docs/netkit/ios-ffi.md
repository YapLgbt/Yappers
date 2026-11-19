# Rust, Kotlin/Native, and iOS

## Build boundary

`yappers-netkit` produces a Rust `staticlib` for `aarch64-apple-ios-sim` and `aarch64-apple-ios`. Kotlin/Native cinterop reads `composeApp/src/nativeInterop/cinterop/netkit.h`. Gradle builds the matching archive, generates a target-specific `.def`, embeds the static library, and includes its bytes as an input to cinterop. Xcode links the resulting Kotlin framework.

The existing Xcode static-framework integration uses a generated Swift identity containing the Kotlin framework checksum. It forces relinking in the same build when Kotlin or embedded Rust changes. Generated code stays under ignored `.generated/`; it does not rewrite committed Swift source. Build scripts fail on errors, use JDK 21 and the pinned Rust toolchain, and never fetch a floating Iroh version.

Iroh 1.4.1's published lockfile uses `libc` 0.2.189 and `mio` 1.2.3. Those two transitive versions are constrained explicitly here: newer libc 0.2.190/191 removes iOS Darwin definitions still needed by netwatch/netdev, and mio 1.2.5 requires the newer libc. The tested compatible pair preserves the upstream implementation without a vendor patch. Revisit these pins together when upstream mobile compatibility is fixed. This does not downgrade Iroh, rustls, or the application protocol.

## ABI v1

| Function | Input | Output / ownership |
|---|---|---|
| `yappers_netkit_identity` | none | JSON containing a newly generated author secret |
| `yappers_netkit_open` | borrowed UTF-8 NUL-terminated client configuration | JSON handle, author, home, community, Iroh version |
| `yappers_netkit_request` | opaque `uint64_t` handle, borrowed command JSON | owned JSON result |
| `yappers_netkit_close` | handle | owned JSON; idempotent close |
| `yappers_netkit_string_free` | owned result pointer | frees exactly once; NULL allowed |

Every result has either `{"ok":true,"data":...}` or `{"ok":false,"error":"..."}`. No Rust object layout, allocator ownership, exception, Tokio future, or pointer-sized handle is exposed. Handles are monotonically assigned registry IDs with reference-counted clients. A request retains its client while executing; closing a handle cannot cause a use-after-free. Calling a closed/unknown handle returns a structured error. Panics are caught at the ABI boundary under the default unwind build; invalid pointers/double frees are caller contract violations and cannot be made safe by panic catching.

`open` configuration: `join` (the private join configuration), `authorSecret` (32-byte hex key), and positive `nextSequence`. Request commands are `prepare` with an `action`, `submit` with an immutable `event`, `feed`, `groups`, `createGroup` with `details`, and `joinGroup` with a group ID. The additional `social` command manages account blocks and named-contact lookup. The `direct` command takes device-private state and an `initialize`, `prepare` or `sync` action. Persist its returned state before the next call. See [messaging and blocking](messaging.md). The C ABI is version 1. Network requests use transport v5, and new publications use event version 4. `prepare` is local; it does not publish. This split is essential to persist an outbox before any network side effect.

`NativeNetkitEngine` is the only Kotlin C ownership boundary. It copies a result string and frees it in `finally`. All blocking ABI calls run on a worker dispatcher. Rust owns a process-wide Tokio runtime with two workers and one Iroh endpoint per client handle. Request deadlines bound synchronous calls. Cancelling an in-flight submit cannot prove non-delivery; the outbox keeps the event until a matching receipt, so a later retry is safe. The native open wrapper also closes a newly created endpoint if coroutine cancellation wins the resource handoff.

The Kotlin boundary is split into `Netkit.kt` (contracts/state), `NetkitController.kt` (serialized lifecycle and durability), and `NetkitCache.kt` (pure projection and versioned disk models). The source split preserves the existing v1 persisted JSON shapes and signed-event bytes. Stable local numeric IDs identify UI items, while feed chronology remains the home's projection order.

## Private storage and crash ordering

Swift `AppleNetkitStorage` implements a Kotlin-exported `@Throws` interface. Reads return the literal JSON sentinel `"null"` for absence; nonoptional throwing strings distinguish absence from Objective-C NSError failure:

- `identity-v1`: Keychain author secret plus next reserved sequence.
- `connection-v1`: Keychain private join configuration and live/demo preference.
- `groups-v1`: Keychain saved group summaries/home-specific invitations and manually selected city; includes the retained home feed invitation after group navigation.
- `state-v1.json`: protected Application Support file containing feed cache, local numeric ID map, local bookmarks/hides, and immutable signed outbox events.

Keychain items use `AfterFirstUnlockThisDeviceOnly`. The state file uses atomic replacement, first-unlock file protection, and an explicit file synchronization before write success is reported. Its directory is excluded from device backups. Unreadable state fails closed rather than silently overwriting an outbox or minting a replacement identity.

Ordering for a new operation is: Rust prepares → Keychain reserves the next sequence → protected file commits the signed event → UI acknowledges local creation → network attempts delivery → matching receipt → file commits removal. A crash before local acknowledgment may consume a sequence without creating an event. A crash after server acceptance but before local removal replays the same ID and receives the same acceptance. There is no claim of an atomic transaction spanning Keychain, filesystem, and a remote server.

The outbox is capped at 100 events. A sync attempts at most 20 pending events, stops at the first failure, and keeps the remaining events. Switching target communities is blocked while an old-target outbox exists; the app never silently sends it to a new home. Explicit discard is confirmed in the UI. Discard does not delete already accepted remote events. Local bookmarks/hides survive feed refreshes and switching away and back. The protected file stores the active cache plus archived caches keyed by home/community. Switching stages the new Keychain preference, commits the cache, then transfers handle ownership; a file-write failure restores the old preference. A crash across those stores may select the last durable preference and recover its cache on reconnect. Old local demo data remains in its separate repository.

## UI and lifecycle

The You screen offers a private invitation sheet, connection status, manual sync, local-demo switching, and explicit pending-upload discard. The invitation is masked while entered and cleared after use. Live mode identifies community content, not GPS-proven nearby content. Posting and replying acknowledge local enqueue before asynchronous delivery; the feed shows pending state and offline errors. Anonymous mode warns that the home can link posts.

Foreground sync runs approximately every 15 seconds. The Swift app registers `app.yappers.prototype.refresh` at launch and requests the next iOS refresh 15 minutes or more after leaving an enabled home. iOS controls delivery and can defer it for hours. A queued upload also gets a short UIKit background execution request. Both paths reuse one process-wide controller, the existing identity, the saved Nearby filter and the encrypted message store. A cold pass opens the saved home, processes up to three feed uploads and four mailbox exchanges, and closes its transport. Fresh installations and local-demo profiles skip scheduled work. Expiration ends the refresh scope, cancels its Rust requests and leaves durable uploads available for retry. APNs delivery is a separate feature. See [Apple’s background refresh guide](https://developer.apple.com/documentation/uikit/using-background-tasks-to-update-your-app).

Simulator builds use ad hoc signing with simulator-only application/keychain entitlements. Device builds use real provisioning and generated signing entitlements; never reuse the simulator identity as a device signing team.

Test-only launch arguments isolate Keychain service, demo defaults, and network state from production app storage. The optional native federation test fixture is an ignored generated resource; the test script restores a skip stub afterward. Release builds ignore test reset arguments.

## Recovery limits

A saved recovery key lets the owner authorize a fresh writing device for the same account. An encrypted Yap backup and its passphrase can also restore the authored history and profile data it contains. The recovery key alone does not contain the old device’s drafts or outbox. If the last device holding the account authority is lost and the owner has neither the key nor a usable backup, the feed cache cannot restore signing access. [Account backup and recovery](../ownership.md)

## Group navigation

`Groups.kt` owns public directory models and pure city/worldwide/search filtering; `GroupsScreen.kt` provides native discovery and creation. `NetkitController` stores group capabilities separately from feed caches. Saved groups can reopen their stored home from demo mode. Switching to a different group is blocked while old-target uploads exist; reopening the same queued group remains possible. The retained home feed can be reopened without repasting its original invitation. Bookmarks/hides remain keyed by group/home through the existing archive wrapper.

Creation and enrollment are foreground RPCs, not content outbox events. An uncertain group creation can be retried with the same normalized name/locality and unchanged description/category without duplicating that group's descriptor on the home. Group names are not globally unique; IDs bind owner and content. Joining a group may persist hosting on the server even if the phone loses its reply. Retry joining; an existing host permit is reused. There is no claim of an atomic transaction spanning server hosting, Keychain, and active client selection.
