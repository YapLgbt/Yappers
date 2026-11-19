# Build the Yap home

From the extracted source directory, run `cargo build --locked --release --bin yappers-server` with Rust 1.93.1, Cargo, and a C compiler. The binary is `target/release/yappers-server`. Cargo.lock names the exact dependency versions and their release checksums. Use `cargo vendor --locked` to obtain their sources for an offline build.

The release Dockerfile contains the actual Linux build flags and package inputs. The website assets are published separately from the home program. To reproduce the home build stage with Docker, use `docker build --target build --file deploy/fly/Dockerfile .`. The archive includes the service startup and peering policy configuration for Yap-operated homes. Operators supply private identity keys and memberships separately.

Run `yappers-server covenant-show` to read the text signed by an operator. Set `YAP_PEERING_POLICY` to the path of the reviewed policy when starting a home, including each HA role. See `deploy/peering/OPERATIONS.md` for admission and suspension.
