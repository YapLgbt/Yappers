# Build the Yap home

From the extracted source directory, run `cargo build --locked --release --bin yappers-server` with Rust 1.93.1, Cargo, and a C compiler. The binary is `target/release/yappers-server`. Cargo.lock names the exact dependency versions and their release checksums. Use `cargo vendor --locked` to obtain their sources for an offline build.

For a dedicated Home release, package the source on Linux and embed its digest in the server binary:

```sh
out=$(mktemp -d)
sh deploy/peering/package-source.sh "$out"
revision=$(cat "$out/revision.txt")
YAP_BUILD_SOURCE_SHA256="$revision" cargo build --locked --release --bin yappers-server
cp "$out/$revision.tar.gz" "$out/index.html" website/static/source/
cp "$out/peering.json" website/static/.well-known/yap-peering.json
```

Run the copy steps in the website checkout. Keep earlier content-addressed downloads available across website releases. Install the matching binary on the dedicated host and publish the static site with the source offer.

Run `yappers-server covenant-show` to read the text signed by an operator. Set `YAP_PEERING_POLICY` to the path of the reviewed policy when starting a home. See `deploy/peering/OPERATIONS.md` for admission and suspension.
