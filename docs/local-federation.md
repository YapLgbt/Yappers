# Manage the federation on this Mac

Run `./yap` from the repository root to see the status of both development homes.

| Command | What it does |
| --- | --- |
| `./yap` | Service state, stored-event count and abbreviated public endpoint IDs |
| `./yap start` | Start both homes; create/build only when missing |
| `./yap stop` | Stop both homes without deleting any keys, invitations or databases |
| `./yap restart` | Restart with existing identities and stored data |
| `./yap update` | Build the current Rust backend and replace the installed binary if changed |
| `./yap doctor` | Check private-file permissions and make authenticated Iroh feed reads against both homes |
| `./yap logs` | Show recent output/errors from both homes, redacting configured signing keys and membership capabilities |
| `./yap logs b --lines 50` | Inspect recent home B logs |
| `./yap invite a --copy` | Copy home A's private invitation to the Mac clipboard |
| `./yap invite b --copy` | Copy home B's private invitation, for pasting on iPhone |
| `./yap status --json` | Machine-readable status for headless scripts |

`up` and `down` remain aliases for `start` and `stop`. Management commands are locked
against concurrent lifecycle/health operations. Nothing resets data automatically.

## Connect the apps

Use **home A for desktop** and **home B for iPhone**. They host the same private testing
community with separate membership capabilities and mutually scoped federation peers.
Both also enable public-group discovery/enrollment with the other home as a bootstrap seed.
Their private home feed is shared; public groups remain separate feeds.

1. Run `./yap invite a --copy`.
2. Open Yap Desktop yourself, then **You → Join community → paste → Join instance**.
3. Run `./yap invite b --copy` and paste into the same screen on iPhone using Universal
   Clipboard, if available. Otherwise transfer the private invitation through a trusted
   channel. `./yap invite b` gives its file path; `--show` deliberately outputs the
   private JSON for controlled piping/transfer. Never put it in a public link or log.
4. Post on one client and use **Sync now** on the other. Foreground automatic sync runs
   approximately every 15 seconds; homes pull federation updates every two seconds.

The manager does not click, launch, reset or edit either app. The app's existing outbox
switch guard remains authoritative; deliver pending uploads before changing communities.
The clipboard retains the invitation until you replace it. Treat that clipboard content
like a password. Endpoint IDs shown by status are public; invitations are not.

`./yap doctor` checks local file permissions and reads each home’s feed over Iroh with its private invitation. It may create or update `health-reader.json`. To check that a post travels between homes, use the [desktop federation test](desktop.md#verification).

## Service behavior and files

The manager installs these user-level launchd services:

- `app.yappers.home-a`
- `app.yappers.home-b`

They start at login and restart after a process failure. `stop` unloads them for the current
session; their installed LaunchAgent definitions remain, so the next login starts them
again. Keep the Mac awake and logged in while testing. This is a development deployment,
not an always-on production service. No firewall or router settings are modified.

State is under `~/Library/Application Support/Yappers Federation/` (owner-only directory):

- `bin/yappers-server`: installed Rust executable, independent of a terminal session.
- `a/` and `b/`: stable private configs/keys, SQLite event logs, current private invitations,
  public addresses, health-reader identities, and stdout/stderr logs.
- LaunchAgent definitions: `~/Library/LaunchAgents/app.yappers.home-{a,b}.plist`.

Network mode is `n0`, enabling Iroh's public relay/address-discovery preset so a physical
phone can reach the Mac; encrypted transport still pins the server endpoint identity.
Relay availability and Mac connectivity matter. The homes are two independent databases
on one machine, so this tests federation but provides no machine-level redundancy.
Invitations keep the same capability across restarts and are regenerated with current
addresses. Use `invite --copy` after a restart to obtain the current file.

Log commands show history, including old startup errors. Use `doctor` for current reachability.
Logs are not rotated automatically in this development setup. Do not delete SQLite/WAL files
or instance keys to repair a connection. See [backup and recovery](netkit/operations.md#moderation-backup-and-recovery).

## Headless verification

```sh
python3 -m unittest discover -s scripts/tests -v
./yap doctor
```

The isolated manager tests exercise private invitation handling, permission-preserving
atomic writes, read-only status, exclusive command locking, log redaction and failure
reporting. Tests mock clipboard and launchd operations; they do not change the user's apps.

## Updating a v5 test pair

Let the clients sync pending uploads before running `./yap update`. Use matching v5 client builds and run `./yap doctor` to verify both homes. For a backup, stop the home and follow the [guarded backup and restore procedure](netkit/deletion.md#storage-and-backup). A pre-v5 store needs a separate cutover plan because old posts lack v5 author signatures. [Account recovery](ownership.md)
