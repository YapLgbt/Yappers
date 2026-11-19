# Testing Yap

Run these commands from the repository root. The Apple client checks require macOS. For the iPhone tests, boot an iPhone simulator in Xcode. To select a specific device, pass its UDID as the first argument.

| Command | What it checks |
|---|---|
| `./scripts/check.sh` | Python tests for local home management, Rust formatting, Clippy, workspace tests, Rust documentation and Kotlin JVM tests. |
| `./scripts/test-netkit-ios.sh` | Starts two local homes, exercises the native iPhone UI and checks the post, reply and vote at the second home. Use `NETKIT_TEST_SCOPE=groups` for the group scenario. |
| `./scripts/test-desktop.sh` | Exercises the desktop client’s Rust bridge, local storage and federation against a test pair. |
| `./scripts/test-portability.sh` | Checks account recovery and movement between homes, including recovery while the original home is offline. |
| `./scripts/test-web.sh` | Checks browser posting, replies, saved posts and offline delivery. See the [browser test prerequisites](../web-app.md#browser-tests). |

## Live home checks

The workspace suite skips production checks by default. The [public home runbook](../public-home.md#live-v5-cutover) gives the command that creates a disposable post and reply, deletes them, and checks stale replay across the primary, standby and public replica. For the public feed alone, use the read-only [replica check](../public-home.md#automatic-public-feed-failover).

## Limits of local tests

A pair of homes on loopback exercises signatures, authorization and replication. To test handoff between Wi-Fi and cellular or connectivity through a restrictive NAT, use a physical device on those networks. For changes to gestures, the keyboard or accessibility, check the affected screen on hardware with VoiceOver and larger text. Simulator captures and successful builds cannot establish how those interactions feel.

The iPhone harness writes its local home configurations, invitation and test results under `build/`. Its invitations grant access to the test homes. Keep those artifacts local.
