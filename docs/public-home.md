# Public home and nearby feeds

Fresh iPhone and Mac profiles discover the public home at `https://yap.lgbt/.well-known/yap-home.json`. During DNS setup the clients can use the same service at `https://yap-lgbt-home.fly.dev/.well-known/yap-home.json`. A saved home, group or explicit local-demo choice takes precedence. From the demo, **You → Join yap.lgbt** enrolls again without a private invitation. Discovery failures expose Retry and automatically retry while foregrounded. After enrollment, cached data and the signed outbox support offline use.

## Deployment

The dedicated Linux host runs the Home and loopback admin from the same release image using [Docker Compose](../deploy/compose/dedicated.yaml) and the [systemd Compose unit](../deploy/systemd/yap-dedicated-compose.service). Both containers use the existing `yap-iroh-replica` UID and GID. The private configuration and live database remain under `/var/lib/yap-iroh-replica/standalone`. The Home uses the existing host push worker and keys. Both containers use the encrypted report inbox. Use the [standalone backup procedure](netkit/deletion.md#storage-and-backup) for recovery.

The dedicated Compose files live in `/etc/yap/compose`. Its root-only `.env` selects the locally available image, service UID and GID, and matching GitHub source download base. Pull an authorized registry image or load the public release's Docker archive before switching versions. Take a protected backup, then restart `yap-dedicated-compose.service`. Compose waits for the Home to regenerate its enrollment before starting the admin. Keep the existing data paths and verify the public feed, source offer, federation policy and report inbox after the restart. The [previous native service](../deploy/systemd/yap-iroh-home.service) can be used for rollback when its binary is compatible with the current data. Stop the Compose unit before starting the native services.

Fly runs Caddy and serves the compiled website, PWA and public enrollment. GitHub Releases hosts the server source archives. To publish enrollment, copy only the public `version` and `join` from the dedicated service’s generated `enrollment.json` into `website/static/.well-known/yap-home.json`. Keep relay addresses for public discovery and omit transient IP hints. Deploy the site from the repository root with `fly deploy --ha=false`. Verify a fresh client can load the PWA and read the dedicated feed.

The dedicated Home loads the explicit scopes in `deploy/peering/yap.json`. See the [covenant operator procedure](../deploy/peering/OPERATIONS.md) to prepare and activate automatic admission.

## Cloudflare

In the active Cloudflare zone for yap.lgbt, set the apex A and AAAA records to the addresses reported by `fly ips list -a yap-lgbt-home`. Use name `@`, DNS only, and Auto TTL. Check the certificate with `fly certs check yap.lgbt -a yap-lgbt-home`. HTTPS discovery is ready when both the certificate is issued and the well-known path returns the public home document. Keep other DNS records, such as email records, intact.

## Location behavior

All cities works immediately without location permission. Select a town from Apple Maps, or request one approximate fix from Core Location. The app saves the chosen center and radius locally. Use current location again to move the center. The app rounds coordinates to hundredths of a degree before reverse geocoding, persistence or federation; the visible label and rounded coordinates accompany new home-feed posts, including anonymous posts. All cities omits location on new posts. A city selection describes a posting area and does not establish residence.

The home applies great-circle distance before its 200-post and response-size limits. The client also filters cached and pending posts locally. Nearby queries can span city and federation boundaries among posts replicated to the home. Unlocated and legacy posts appear in All cities. Saved posts and public group feeds are independent of the nearby filter. Upgrade participating homes before sending location-bearing posts; older binaries reject the new signed field.

## Review reports on yap.lgbt

On the dedicated primary, run these commands in a private terminal. The queue lists case IDs. Opening a case shows the evidence the reporter chose to share.

```sh
cd /etc/yap/compose
sudo docker compose exec home yappers-server reports --inbox /var/lib/yap-report-inbox/private/reports.sqlite list
sudo docker compose exec home yappers-server reports --inbox /var/lib/yap-report-inbox/private/reports.sqlite show CASE_ID
sudo docker compose exec home yappers-server reports --inbox /var/lib/yap-report-inbox/private/reports.sqlite close CASE_ID --outcome noAction
```

Use `actionTaken` for a case where you took moderation action. Closing clears the evidence and updates the reporter’s private receipt. The inbox is on a dedicated LUKS volume mounted at `/var/lib/yap-report-inbox`, with an unlock key sealed to the primary’s TPM. Check `systemctl status yap-report-storage` if intake is unavailable after a restart. Hardware or Secure Boot changes can prevent unlocking.
