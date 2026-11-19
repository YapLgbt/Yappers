#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if ! xcodebuild -version >/dev/null 2>&1; then
  echo 'Install full Xcode and select it: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer'
  exit 1
fi
if ! /usr/libexec/java_home -v 21 >/dev/null 2>&1; then
  brew install --cask temurin@21
fi
if ! command -v xcodegen >/dev/null; then brew install xcodegen; fi
NETKIT_RUSTUP="${CARGO_HOME:-$HOME/.cargo}/bin/rustup"
if [ ! -x "$NETKIT_RUSTUP" ]; then
  echo 'Install Rust from https://rustup.rs, then rerun this script.' >&2
  exit 1
fi
"$NETKIT_RUSTUP" show active-toolchain
"$NETKIT_RUSTUP" target add aarch64-apple-ios aarch64-apple-ios-sim
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
./gradlew --version
(cd iosApp && xcodegen generate)
if ! xcrun simctl list runtimes | rg -q '^iOS'; then xcodebuild -downloadPlatform iOS; fi
echo 'Ready. Open iosApp/Yappers.xcodeproj and run the Yappers scheme on an iPhone simulator.'
