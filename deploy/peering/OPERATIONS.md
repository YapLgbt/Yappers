# Covenant peering

Homes in the Yap public network automatically admit a Home that proves possession of its endpoint key and presents a valid receipt for the current covenant. The registry exchanges signed receipts with other admitted Homes. The main feed has one scope, `<main Home ID>/public`, with a separate local membership capability on each participating Home. A person reads, publishes, and replies using the invitation from their chosen Home. Public groups continue to issue authority-signed host permits. Private communities use their explicitly configured scopes.

## Join as an independent operator

If you run a Home with Docker Compose, follow the [published image and covenant walkthrough](../../docs/run-home.md#optional-join-the-public-yap-network).

Run these commands as the Home’s service user. Obtain the current covenant, source offer, and bootstrap identity from the network operator’s published discovery information. For yap.lgbt, the public enrollment document contains the bootstrap address. Extract just the address for configuration.

```sh
curl --fail --silent --show-error https://yap.lgbt/.well-known/yap-home.json |
  jq -e '.join.serverAddr' > bootstrap.json
main_home=$(jq -er '.id' bootstrap.json)

yappers-server covenant-show
yappers-server covenant-accept --config HOME_CONFIG --operator 'Operator name' \
  --contact 'Working operator contact' --source-url https://example.org/home/source/REVISION \
  --revision FULL_COMMIT_OR_ARCHIVE_SHA256 --accept-version 2 --output acceptance.json

yappers-server covenant-verify --receipt acceptance.json
yappers-server covenant-enable --config HOME_CONFIG --receipt acceptance.json \
  --main-home "$main_home" --bootstrap bootstrap.json
```

Restart the Home to apply the configuration. Restart the admin panel to refresh its invitation list. The Home writes `join-shared.json` beside its config. Give that invitation to people who should use this Home for the main Yap feed. Current members on another feed of the same Home can import that invitation after delivering pending posts. The invitation is a local membership capability. The public bootstrap address and signed receipt can be exchanged between Homes.

For an updated source revision, sign a fresh acceptance and run `covenant-enable` again with the same main Home ID. The Home keeps its key, feed memberships, and database. The network uses the signed operator and source disclosures for accountability. A signature proves control of the key and agreement to the covenant. Operational checks can verify deletion and stale-replay behavior independently.

## Stop a peer

In Home admin, open **Federation peers**, check the Home ID and scopes, and choose **Stop peering**. The stop applies on the local Home. The service fences new content exchange with that peer and continues exchanging eligible deletion proofs for content it recorded as delivered. Re-submitting a receipt leaves a suspension in place. Previously received content remains subject to the usual moderation and deletion rules.

Preserve `<database filename>.federation.json` with your private operator configuration. It stores the signed directory and local suspensions. Restore it together with the protected feed archive and independent deletion guard. Keep the federation state file mode 0600. Feed backups track deliveries committed by this release and deliveries already tracked by the old covenant policy.

The first release supports up to 1,024 discovered peers and 1,024 local stops per Home. Directory and pull rounds rotate over bounded batches. Operators can use the public feed and group checks below to verify readiness between upgrades.

## Prepare yap.lgbt

1. Build and stage the binary, corresponding source archive, covenant v2, and discovery document using [BUILD.md](BUILD.md). Keep the existing public Home identity and the private config and storage backups.
2. Sign the running revision with the dedicated Home's key. Apply `covenant-enable` to its existing `public-home.json`, setting `--main-home` to its own existing endpoint ID. The canonical public feed and its enrollment capability are already in that configuration. Its current `YAP_PEERING_POLICY` can stay in place for explicitly managed scopes.
3. In staging, connect two disposable Homes using the same flow. Check direct acceptance, discovery of the third Home from a shared seed, posts and replies in both directions, a public group's host permit, and rejection of access to a private community. Suspend one test Home and verify eligible deletions still cross the former peering boundary. Restart and repeat the suspension check.
4. When authorized to release, publish the source archive to GitHub Releases. Install the staged binary and configuration on the dedicated host and verify the existing public enrollment and fresh covenant admission. Publish the website with the new discovery document after the Home is serving v2.
5. To pause new automatic admission, set `autoFederation.covenant.admitNew` to `false` and restart. Existing peers can continue their exchange, including deletion catch-up. Preserve the current database, deletion guard and federation state. Use per-peer suspension when other admissions should continue.
