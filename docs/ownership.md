# User-owned accounts and portable data

A Yap account has a stable public key. Its owner can authorize a new device or move to another compatible home. The account keeps its identity and signed history across a move.

## What is implemented

- A stable account public key, with separate authorized writing-device keys and counters.
- Root-signed, hash-linked revisions naming the chosen home and active devices.
- Version-4 signed content events carrying account/device/home authorization.
- Encrypted, root-signed recovery archives containing authored history and caller-provided
  local profile/preferences; original event IDs and home signatures remain unchanged.
- Durable local signed receipts before an uploaded event leaves the app/CLI outbox.
- Offline export from those receipts; optional online refresh of older owned history.
- Private, paginated history import at a destination, independent of public-feed admission.
- Fresh device creation on recovery/migration; idempotent import retries preserve the staged
  key and sequence. Known account-state forks are rejected.
- Signed move/revocation announcements to previous homes, without their old membership token.
- Explicit handling of uncertain uploads: backup retains them; migration requires delivery
  or an explicit choice to recover their exact signed bytes as private drafts, never reposting.

The Rust library, C ABI, Kotlin controller and headless operator CLI implement these paths.
The existing app automatically creates/upgrades a portable identity when connecting with an
empty outbox. Under **You → Account recovery → Backup files**, choose **Save a backup file** to export an encrypted archive. **Move to another community** takes an archive, passphrase and destination invitation. **Backed-up posts and replies** shows retained history when available. Recovered drafts appear separately for review.

## Identity and authority

`AccountRecord` is root-signed over a versioned canonical payload:

```
version, account, revision, previous, home, devices
```

`account` is the Ed25519 root public key. `home` is an Iroh endpoint ID. `devices` is a sorted
set of independently generated writing keys. The first revision is 1 with no predecessor;
subsequent revisions link to the hash of the complete previous record. The signature domain
is `yap:account-state:v1\0`. Device enrollment keeps the same home and adds a fresh key;
a move/recovery replaces the active device set by default.

An existing legacy author key becomes the root authority, preserving authorship of old
version-1/2 content. New writing uses a fresh delegated key. Pending legacy uploads are
not rewritten: deliver them and reconnect before automatic upgrade. The recovery authority
is never sent to a home. It is kept in iOS Keychain or the explicitly development-only
owner-readable desktop/CLI credential files. Current recovery wallets include root authority;
this version does not implement device-only administrative restrictions or root-key rotation.

Each writing device has its own sequence. Import never clones an active writer's private key
and counter. The current prototype permits 16 active devices and 64 account revisions; exceeding
these bounds fails explicitly. Chain checkpointing for unlimited lifetime use remains future work.

## Content, receipts, and portability

For a portable account, each new content event carries a signed grant binding the selected home and writing device. The home validates the event, current known
account state, community membership, action target, rate limits and storage bounds. It returns
a complete signed acceptance envelope. The client persists that envelope before removing the
queued event, so subsequent loss/refusal of the home does not remove the user's local evidence.

The server's authenticated owned-history endpoint exports only events whose stable owner is
the requesting account (including legacy root-authored events). A current home membership and
an active-device proof are required. Ownership permits exporting one's own events across that
home's communities; it does not export other members' private conversations. References to
other people's posts remain references. Personal imports are included in later owned exports.

Exports use **age 0.11.5**, with passphrase encryption and authenticated ciphertext. The current
scrypt work factor is fixed at 18, with a matching decryption maximum (approximately 256 MiB
of KDF memory). Use a long unique passphrase; keep it separate from the backup. Desktop and
mobile performance at this bound must be validated before a public recovery UI launch.
The archive also has an account-root signature; event and home signatures are checked individually.

The encrypted recovery contains the root key. Do not publish its plaintext or give it to a
home operator. A portable archive can be decrypted locally without any server. These are
sensitive recovery files, not public profile-sharing files.

A recovery file supports 16 MiB of plaintext and 640 KiB of local metadata, including the profile portrait. Network
imports and exports carry up to 50 events per page within the 1 MiB transport frame.
Local ABI buffers have a separate archive size bound. Personal history
imports commit one complete validated chunk at a time. Repeating an import deduplicates exact
event IDs and resumes; conflicting attestations and partial invalid chunks fail closed.

## Moving homes

1. Save an encrypted export, including local profile/preferences. Refresh older server-only
   history first when available; `local-receipts` coverage does not claim an exhaustive history.
2. Obtain membership at the destination home. Migration does not bypass its admission policy.
3. Decrypt and verify locally. Compare the destination's known signed history with the backup.
   Recovery authority may extend newer compatible public state with a fresh device; forks fail.
4. Persist the new device/key/revision locally **before** any registration/import request.
5. Register the new signed home choice and upload authored history in idempotent pages.
6. Restore the profile and preferences for the destination feed. The backup retains group
   references and preferences for other feeds. Rejoin groups through the new home.
7. Deliver the root-signed update to former homes. Kotlin persists an announcement queue and
   retries during sync; the CLI provides `account notify`. Retain the backup until verified.

The old home need not be online and does not approve the move. A migration cannot recover
bytes that exist only on an unavailable server and were never backed up or locally retained.
Content cached before signed-receipt retention was introduced needs an online refresh to obtain
its original acceptance envelopes. A home can withhold history; there is no completeness proof.

Homes enforce the latest state they know. An offline home cannot immediately learn a move or
revocation, and a malicious operator can refuse to honor it. Announcements only advance
already-known accounts, preventing unauthenticated allocation of arbitrary account records.
There is no global consensus or exhaustive account-home directory: an old backup can conflict
with a newer revision unknown to the chosen destination. Keep current backups and consult the
current home when possible. Recovery cannot silently resolve a genuine root-signed fork.

## Groups and moderation remain independent

Imported history is a **private personal archive**, not a replay into a destination's public
feed. It cannot launder an untrusted origin, grant group membership, bypass blocks, override
moderation, or turn a private community into a public one. Public-group rejoining still uses
signed host permits; private groups may require a new invitation. Existing group IDs stay stable.

Account identity determines one vote per target across devices. Each device's greatest sequence
wins locally; concurrent device candidates resolve by signed home acceptance time and stable
key/event-ID ties. Author deletion permissions and `mine` projection use stable account ownership.
This is deterministic eventual convergence, not a global clock or immediate cross-home consensus.

Group authorities are currently founding-home keys. Moving an account does **not** transfer
control of a group authority. Portable user/group administrator roles, authority delegation,
global host-permit revocation and group ownership recovery require a separate protocol change.
Copies already received by another home cannot be recalled; v5 homes retain the signed creation header and deletion proof after erasing the post or reply. See [deletion and backup](netkit/deletion.md). Data portability does not promise universal
erasure or control over another person's replies.

## Headless CLI workflow

All files below are private; use a directory with mode 0700 and `umask 077`. Passphrases are
read from a mode-0600 file, never from a command argument or environment variable. Use real
private invitation paths produced by `./yap invite a` / `./yap invite b`.

```sh
SERVER=build/rust/debug/yappers-server
$SERVER account init --wallet /private/source.wallet.json --join /private/home-a.join.json
$SERVER account post --wallet /private/source.wallet.json --join /private/home-a.join.json \
  --headline 'Hello from my account' --body 'My words, my choice of home.'
$SERVER account export --wallet /private/source.wallet.json --join /private/home-a.join.json \
  --output /private/recovery.age --passphrase-file /private/passphrase \
  --local-data /private/profile.json --offline
$SERVER account import --wallet /private/destination.wallet.json --join /private/home-b.join.json \
  --input /private/recovery.age --passphrase-file /private/passphrase
$SERVER account history --wallet /private/destination.wallet.json --join /private/home-b.join.json
```

Remove `--offline` to refresh older authored events from the home before encrypting. `show`
prints public identity/home/revision only. `retry` delivers exact queued events. `enroll` adds
a second device on the same home without copying its writer key/counter. `init --legacy-identity`
can upgrade a legacy **CLI** identity whose pending outbox is empty; never extract an app's
Keychain secrets to use it. Import rejects replacing a different existing wallet/account.
Use `--pending-as-drafts` only when choosing to preserve uncertain uploads for manual review;
they are written to a private pending-drafts file, never automatically published.

After an old home returns, deliver the signed move using its public address file:

```sh
$SERVER account notify --wallet /private/destination.wallet.json --join /private/home-b.join.json \
  --address /private/home-a.address.json
```

CLI wallets have a process-lifetime OS file lock. Exact signed acceptances are saved atomically
in a companion `*.history.json` before acknowledgement. Imported client metadata is saved in
`*.local-data.json`. Account-enrollment outputs also contain recovery authority: protect them
as carefully as the original wallet. Files use owner-only mode, fsync and atomic replacement.

## Kotlin / C ABI integration

`NetkitController.exportRecovery(passphrase, localData, refreshFromHome = false)` uses locally
retained receipts by default. It includes current/archived bookmark and hide references, joined
group summaries, selected locality, caller-provided profile/local data, and uncertain queued
bytes. It returns encrypted ciphertext encoded as hex for the owned JSON ABI.

`importRecovery(ciphertext, passphrase, joinText, alias, pendingAsDrafts = false)` verifies locally,
rejects replacing another account, checks known state, creates/persists a fresh device, imports
private history, restores matching preference references and queues the old-home update. A
retry reuses the staged device and sequence. The return value contains caller-local profile
metadata and explicitly recovered pending drafts, for the host UI to apply after success.
Keep the encrypted input until the whole transfer completes. Failures retain the staged state
and report that the same import can be retried.

`yappers_netkit_portability` handles offline cryptography. `accountLookup`, `accountRegister`,
`accountExport`, `accountImport` and `accountNotify` are local client ABI commands that construct
scoped server requests; recovery secrets never enter those server request schemas. Results
follow the existing copy/free ownership contract and execute off the UI thread.

## Stable Yap-run hosting

Yap operates the [default public home](public-home.md). People can join a compatible home directly or move their account using its recovery key. The local pair started by `./yap start` runs on the developer’s Mac.

## Compatibility and verification

Homes and clients communicate over `yappers/netkit/5`. An older invitation can still identify the home. A connection requires a server that speaks v5. The [v5 deletion guide](netkit/deletion.md) describes the signed event format and the limits of upgrading populated stores.

```sh
scripts/check.sh
scripts/test-portability.sh
scripts/test-desktop.sh
```

The isolated Rust/Iroh and CLI checks exercise source-home outage, original signed history,
private-feed isolation, replay, restart recovery, legacy identity, wrong passphrases, tampering,
forged account/device records, stale/forked state and revoked writers. The Kotlin real-engine
fixture checks offline receipts, local profile data, import retry, previous-home notice and
explicit pending-draft recovery.

Recovery file encryption uses [age](https://docs.rs/age/0.11.5/age/).
