# Desktop live-testing app (macOS)

Yap Desktop shares the Kotlin feed, groups, profile editors, post composer, comments,
cache/outbox controller, and Rust/Iroh engine with iOS. This is a macOS development app,
not a published or notarized release. Fresh profiles can join the public home at yap.lgbt. Choose the local demo to try the app offline.

## Run in IntelliJ IDEA

Open the directory containing `settings.gradle.kts`, import the Gradle project and select JDK 21 as the project SDK. Run **Yap Desktop** to build the shared UI and Rust library. **Yap Tests** runs the Kotlin JVM tests.

## Run or package

Prerequisites are the repository's JDK 21, Rust toolchain and Xcode command-line tools.
No additional global runtime is needed after packaging: the app image includes its JVM.

```sh
scripts/run-desktop.sh
# Independent identity/store for a second client:
scripts/run-desktop.sh --data-dir "$PWD/build/desktop-person-two"

# Build a standalone local .app (including Rust dylib and Java runtime):
JAVA_HOME=$(/usr/libexec/java_home -v 21) ./gradlew :composeApp:createDistributable
open composeApp/build/compose/binaries/main/app/Yap.app
```

The launcher automatically builds the host Rust `cdylib` and embeds it in JVM resources.
The JNA adapter extracts a private temporary copy, calls the existing versioned JSON C ABI
on `Dispatchers.IO`, copies each returned UTF-8 response, and frees it exactly once.
No HTTP proxy, alternate protocol, or separate networking implementation is involved.
The current packaging task supports the host macOS architecture; Windows/Linux binaries
and signing/notarization are not configured.

At window widths of at least 900dp, destinations live in a lavender sidebar. Smaller
windows use the compact Compose bottom selector shared with Android. The reading column is capped
at 760dp; modal editors retain their fixed Save/Post/Cancel actions and scrollable fields.
Desktop photo selection accepts bounded JPEG/PNG files and generates the same small
square JPEG profile payload used by iOS.

Yap polls roughly every 15 seconds while its window is focused and every two minutes while unfocused. Closing the window leaves Yap running. Click its Dock icon to reopen the window, or use **Quit Yap** to stop syncing. Sleep suspends work until the Mac wakes.

## Test against two real homes

Run this in a terminal and keep it running:

```sh
mkdir -p build
build/rust/debug/yappers-server demo --directory "$PWD/build/desktop-live-homes"
```

If the backend executable is missing, run `cargo build --locked -p yappers-backend` first.
The demo creates home A and home B, private invitations, public group discovery, and
federation between the homes. `join-a.json` and `join-b.json` are private read/write
capabilities: do not publish them or include them in screenshots.

1. Open **You → Join community** on desktop. Paste the contents of `join-a.json`.
2. Join the same home on the iPhone, or use `join-b.json` to exercise the other home.
3. Give each client a distinct community name. Create a named post, comment and vote;
   use **Sync now** on the other client. Automatic sync runs every 15 seconds while open.
4. Exercise **Groups**: choose a city, discover Local or Worldwide groups, join and post.
   Public groups federate through the configured homes, as on iOS.
5. Quit/reopen desktop to check persisted profile, saved posts and connection restore.
   Disconnect a home to test queued posts, then reconnect and sync.

The fixture is local to this Mac; it must remain running for the phone to connect.
Iroh can use its usual direct/relay transport. See [operations](netkit/operations.md)
for durable home deployments and [groups](netkit/groups.md) for automatic federation.
A local app install does not provision a public backend.

## Identity, storage and lifecycle

The default store is `~/Library/Application Support/Yappers Desktop/`. Desktop and iOS
never share an identity, outbox, or demo data. `--data-dir PATH` creates another independent
desktop client; opening the same directory twice is rejected by a lifetime file lock,
preventing duplicate signing-sequence allocation. A packaged app can also receive it:

```sh
open -n ~/Applications/Yap.app --args --data-dir "$PWD/build/desktop-person-two"
```

Desktop credentials are development-only plaintext files, restricted to the current user
(directory 0700; state and secret files 0600). They are **not encrypted or in Keychain**;
use disposable testing identities. Do not share or check in a data directory. Writes use
fsynced temporary files, atomic replacement and directory fsync before UI acknowledgement.
Corrupt network state still fails closed through the shared controller. Quitting closes
the Rust client and flushes its resources before releasing the identity lock. iOS continues
to use its existing protected files and Keychain.

## Verification

```sh
scripts/test-desktop.sh
```

This starts an isolated two-home fixture, runs shared Kotlin tests, desktop UI tests,
C-ABI/storage tests, and confirms the desktop post, comment and vote arrive at independent
home B. Screenshots are written under `composeApp/build/desktop-qa/`. Fixtures are stored
under `build/netkit-desktop-fixture.*` for inspection; their processes stop automatically.
The script never resets or reads the normal desktop/iPhone profile stores.

Compose packaging follows the [official native distribution guide](https://kotlinlang.org/docs/multiplatform/compose-native-distribution.html);
the adapter follows JNA's [pointer/string ownership API](https://github.com/java-native-access/jna/blob/master/src/com/sun/jna/Pointer.java).
