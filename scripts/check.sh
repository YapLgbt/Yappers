#!/bin/bash
# Fast maintainer checks. Native simulator/federation tests run separately.
set -euo pipefail
cd "$(dirname "$0")/.."

YAPPERS_CARGO="${CARGO_HOME:-$HOME/.cargo}/bin/cargo"
if [ ! -x "$YAPPERS_CARGO" ]; then YAPPERS_CARGO=$(command -v cargo); fi
if [ -x /usr/libexec/java_home ]; then
  export JAVA_HOME=$(/usr/libexec/java_home -v 21)
fi

python3 -m unittest discover -s scripts/tests

"$YAPPERS_CARGO" fmt --all -- --check
"$YAPPERS_CARGO" clippy --workspace --all-targets --features yappers-netkit/server --locked -- -D warnings
"$YAPPERS_CARGO" test --workspace --features yappers-netkit/server --locked
RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D warnings" "$YAPPERS_CARGO" doc --workspace --features yappers-netkit/server --no-deps --locked
./gradlew :composeApp:jvmTest
