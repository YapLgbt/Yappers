# Public home and nearby feeds

Fresh iPhone and Mac profiles discover the public home at `https://yap.lgbt/.well-known/yap-home.json`. During DNS setup the clients can use the same service at `https://yap-lgbt-home.fly.dev/.well-known/yap-home.json`. A saved home, group or explicit local-demo choice takes precedence. From the demo, **You → Join yap.lgbt** enrolls again without a private invitation. Discovery failures expose Retry and automatically retry while foregrounded. After enrollment, cached data and the signed outbox support offline use.

## Deployment

The primary Rust home runs on the dedicated Linux server under [yap-iroh-home.service](../deploy/systemd/yap-iroh-home.service). Its pinned Iroh ID is `114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b`. The existing service account, binary directory and state directory retain the original `yap-iroh-replica` name. Preserve the state directory across upgrades. It contains the dedicated home’s signing configuration. The active HA ledger, account state and deletion guard live on Fly.

The v5 dedicated unit uses `/var/lib/yap-iroh-replica/v5/public-home.json` and the existing owner-signed standby grant. The HTTPS enrollment advertises protocol version 5. Publish only the versioned public join for the primary’s own `<endpoint ID>/public` community. The enrollment copy on Fly uses the primary’s relay address for discovery and omits transient IP hints. Refresh it if the relay changes. The public membership capability intentionally allows anyone to join this community.

The Fly app `yap-lgbt-home` serves the website and enrollment over HTTPS and runs a second Iroh peer. Caddy serves the well-known path from `/data/v5/enrollment/home.json`. The peer reads `/data/v5/federation.json` and keeps its own private key and SQLite database. It replicates the new primary’s public feed. Its original community remains available to clients with an existing Fly enrollment. The v5 cutover used fresh storage for both communities. A second Fly process can serve the dedicated community under its explicit owner-signed standby grant.

On a replacement Fly volume, provision its private peer configuration and the dedicated home’s **public enrollment document** before starting the app. Deploy from the repository root with `fly deploy --ha=false`. Verify the HTTPS enrollment ID, perform a Netkit feed read, and check the peers’ SQLite replication checkpoints after changing either deployment.

## Outside peering

The production home and public replica use `deploy/peering/yap.json`. Outside admission is closed. Both HA roles must use the same reviewed policy. See the [operator procedure](../deploy/peering/OPERATIONS.md) for reviewing an independent home's signed acceptance, granting community access and suspending new deliveries. The public [peering covenant](https://yap.lgbt/peering/) and [source offer](https://yap.lgbt/source/) are available to anyone.

## Live v5 cutover

On 2026-11-18 the dedicated home, Fly standby, public replica and web app switched to protocol v5. The existing home ID, peer ID, signed standby grant and public community capability were preserved. The previous deployment’s databases are offline. The live Fly ledger is `/data/v5/ha/ledger.sqlite`. Historical backups use `ha-export` and depend on its current `.deletion-guard`. Scheduled Fly volume snapshots were disabled and the provider listed no existing snapshots at cutover.

The release check published a post through the dedicated home and a reply through the standby, observed both on the public peer, then deleted both. Each home route returned the original signed header and deletion proof on replay. The live browser and public replica confirmed both deletions. A protected backup taken before deletion was restored in RAM using a consistent copy of the current guard. Both restored records were erased, and neither opening key remained in the live guard.

To repeat the check, save the live HTTPS enrollment and run `YAP_SMOKE_ENROLLMENT=PATH cargo test --locked -p yappers-netkit --features server --test live_deletion -- --ignored --nocapture`. It creates a disposable account and publishes and deletes two public items.

## Owner-authorized full takeover

The dedicated server is the preferred route to yap.lgbt. Its owner signs the `fullHomeStandby` grant in the public enrollment document. Clients verify the grant against the enrolled home’s key before sending authenticated requests to the standby. The account keeps the same home identity when routes change. Either worker can accept posts, return a personalized feed, update privacy settings and handle encrypted mail. On homes that enable safety reporting, the primary alone receives reports and serves case status. [Report inbox setup and retention](../deploy/peering/REPORTS.md) If both are unreachable, the client can display its saved feed.

Fly supervises the public replica and the private standby as separate processes. The standby uses its own transport identity and starts when provisioning creates `/data/v5/ha/active`. Its configuration and the shared commit ledger live under `/data/v5/ha`. On the dedicated host, install the [systemd drop-in](../deploy/systemd/yap-iroh-home-ha.conf) after the one-time bootstrap with the standalone home stopped. See [HA provisioning, failure modes and recovery](netkit/operations.md#full-home-takeover).

## Automatic public-feed failover

Publish the Fly peer’s public Iroh address in the optional top-level `publicFeedReplica` field alongside `version` and `join` in the HTTPS enrollment document. Existing clients can still decode the unchanged join. The updated iPhone and Mac clients refresh discovery for their saved public home and pin its existing endpoint and community. They cache the backup route for offline discovery. A newly published, different primary identity cannot replace a previously enrolled home through this refresh.

Deploy the updated Rust binary on both hosts. In Fly’s `/data/v5/federation.json` set `publicReplicaFeeds` to an array containing the dedicated home’s exact `<endpoint ID>/public` scope. The primary must already explicitly trust Fly for that scope. Restart the Fly peer after changing its configuration. Once an authenticated log pull completes, Fly requests the primary’s public feed projection. It honors the primary’s moderation, adds its own local hides, and persists the last snapshot and verification time. Conditional refreshes run at most once per 15 seconds after successful synchronization.

Without a full-service standby grant, a client probes its primary first, then reads the public backup snapshot using only the public community ID. An application-level denial, such as a revoked membership capability, is returned directly. The backup receives no account chain, signing keys, membership token, private block list, or location in the fallback request. The client applies device privacy filters. When blocks exist or the client has yet to confirm an existing account’s privacy settings, it retains its personalized saved feed. This keeps anonymous blocks effective during failover.

Posts, replies and votes retain their original signed IDs in the device outbox. On the next successful primary sync they can be submitted and acknowledged, including after an app restart. Clients using only the public projection wait for their original home for encrypted mail, group enrollment, account changes and moderation. Clients enrolled for full takeover use the signed private standby for those operations. If neither authorized route is available, they show their saved personalized feed. A backup can serve the latest confirmed public snapshot until the primary returns; its verification time tells the client how stale that snapshot is.

Verify outage and recovery with `cargo test --workspace --locked public_feed_failover`. To verify the deployed public replica directly, download the HTTPS enrollment manifest, set `YAP_SMOKE_ENROLLMENT` to its local path and run `cargo test --workspace --locked deployed_public_feed_replica -- --ignored --nocapture`. This check reads the backup over Iroh and leaves production traffic on the primary.

## Cloudflare

In the active Cloudflare zone for yap.lgbt, set the apex A and AAAA records to the addresses reported by `fly ips list -a yap-lgbt-home`. Use name `@`, DNS only, and Auto TTL. Check the certificate with `fly certs check yap.lgbt -a yap-lgbt-home`. HTTPS discovery is ready when both the certificate is issued and the well-known path returns the public home document. Keep other DNS records, such as email records, intact.

## Location behavior

All cities works immediately without location permission. Select a town from Apple Maps, or request one approximate fix from Core Location. The app saves the chosen center and radius locally. Use current location again to move the center. The app rounds coordinates to hundredths of a degree before reverse geocoding, persistence or federation; the visible label and rounded coordinates accompany new home-feed posts, including anonymous posts. All cities omits location on new posts. A city selection describes a posting area and does not establish residence.

The home applies great-circle distance before its 200-post and response-size limits. The client also filters cached and pending posts locally. Nearby queries can span city and federation boundaries among posts replicated to the home. Unlocated and legacy posts appear in All cities. Saved posts and public group feeds are independent of the nearby filter. Upgrade participating homes before sending location-bearing posts; older binaries reject the new signed field.

## Review reports on yap.lgbt

On the dedicated primary, run these commands in a private terminal. The queue lists case IDs. Opening a case shows the evidence the reporter chose to share.

```sh
sudo -u yap-iroh-replica yap-reports list
sudo -u yap-iroh-replica yap-reports show CASE_ID
sudo -u yap-iroh-replica yap-reports close CASE_ID --outcome noAction
```

Use `actionTaken` for a case where you took moderation action. Closing clears the evidence and updates the reporter’s private receipt. The inbox is on a dedicated LUKS volume mounted at `/var/lib/yap-report-inbox`, with an unlock key sealed to the primary’s TPM. Check `systemctl status yap-report-storage` if intake is unavailable after a restart. Hardware or Secure Boot changes can prevent unlocking. The Fly standby serves the feed and encrypted DMs during an outage. Report intake and lookup require the primary.
