# Build the Yap home

From the extracted source directory, run `cargo build --locked --release --bin yappers-server` with Rust 1.93.1, Cargo, and a C compiler. The binary is `target/release/yappers-server`. Cargo.lock names the exact dependency versions and their release checksums. Use `cargo vendor --locked` to obtain their sources for an offline build.

For a dedicated Home release, package the source on Linux and embed its digest in the server binary. Set `release` to the GitHub release tag for the build.

```sh
out=$(mktemp -d)
release=home-v0.1.1
sh deploy/peering/package-source.sh "$out" "$release"
revision=$(cat "$out/revision.txt")
YAP_BUILD_SOURCE_SHA256="$revision" cargo build --locked --release --bin yappers-server
gh release upload "$release" "$out/$revision.tar.gz" --repo YapLgbt/Yappers
cp "$out/peering.json" website/static/.well-known/yap-peering.json
```

Keep earlier content-addressed downloads available on GitHub. Set the dedicated Home's `YAP_SOURCE_BASE_URL` to `https://github.com/YapLgbt/Yappers/releases/download/<release-tag>` and install the matching binary. Publish the website with the discovery document. For the deployed covenant smoke test, set `YAP_EXPECTED_SOURCE_URL` to the full archive URL and `YAP_EXPECTED_SOURCE` to its SHA-256.

Run `yappers-server covenant-show` to read the text signed by an operator. Set `YAP_PEERING_POLICY` to the path of the reviewed policy when starting a home. See `deploy/peering/OPERATIONS.md` for admission and suspension.
