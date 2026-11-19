#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
NETKIT_TARGET="${1:-aarch64-apple-ios-sim}"
case "$NETKIT_TARGET" in aarch64-apple-ios|aarch64-apple-ios-sim) ;; *) echo 'Unsupported iOS target' >&2; exit 1;; esac
NETKIT_CARGO="${CARGO_HOME:-$HOME/.cargo}/bin/cargo"
if [ ! -x "$NETKIT_CARGO" ]; then NETKIT_CARGO=$(command -v cargo); fi
export IPHONEOS_DEPLOYMENT_TARGET=18.5
"$NETKIT_CARGO" build --locked -p yappers-netkit --lib --release --target "$NETKIT_TARGET"
