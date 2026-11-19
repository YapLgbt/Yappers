# Review an outside home

Yap currently accepts no outside peers. The production policy names only the dedicated home and the independently keyed Yap-operated public replica. The standby uses the dedicated home's logical identity and its existing owner-signed delegation.

When admission opens, an authorized operator reads `yappers-server covenant-show` and executes the following on their own home. The signing key stays in the private home configuration.

```sh
yappers-server covenant-accept --config HOME_CONFIG --operator 'Operator name' \
  --contact 'Working operator contact' --source-url https://example.org/home/source/REVISION \
  --revision FULL_COMMIT_OR_ARCHIVE_SHA256 --accept-version 1 --output acceptance.json
yappers-server covenant-verify --receipt acceptance.json
```

Confirm the contact reaches the named operator and that they can speak for the organization. Check the source URL without signing in, review its license, match its revision, review the disclosed providers and backup practices, and run a disposable deletion and stale-replay check in a test community. Retain the acceptance and review result. A valid signature proves control of the home key. Yap grants access only after reviewing the request.

Add a reviewed entry to the `peers` array in the local policy with `acceptance` containing the complete signed JSON, `communities` listing the reviewed scopes, and `status` set to `active`. The home's existing peer configuration or group permit must authorize those scopes too. Keep all HA workers on the same policy. Restart workers to apply changes, then verify their health and federation reads. A missing or invalid configured file prevents startup. Self-hosted deployments can enable this admission system through the same environment variable. Keep unrelated operators out of `managedHomes`.

For suspension, keep the peer entry and its existing community grants, change its status to `suspended`, deploy the policy to every worker and restart. Fresh pulls can then carry signed deletions for content previously delivered to that peer. The home withholds new payloads and moderated feed projections. Keep the deletion route available while the operator catches up. Review the source and repeat the deletion check before reinstatement. An offline peer cannot confirm a deletion until it reconnects.

Public posts remain readable by users of the public feed. The covenant governs operator federation and does not prevent someone copying public content or keeping a private snapshot.
