#!/bin/bash
# Build the browser client from the same locked Netkit sources as the Apple apps.
set -euo pipefail
cd "$(dirname "$0")/.."
target=wasm32-unknown-unknown
out=website/static/app/runtime
cli=${WASM_BINDGEN:-wasm-bindgen}
version=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/"/, "", $3); print $3; exit}' Cargo.lock)
if [ "$("$cli" --version)" != "wasm-bindgen $version" ]; then
  echo "Set WASM_BINDGEN to wasm-bindgen $version, matching Cargo.lock." >&2
  exit 1
fi
if [[ "$(uname)" == Darwin && -z "${CC_wasm32_unknown_unknown:-}" ]]; then
  if command -v brew >/dev/null && [[ -x "$(brew --prefix llvm)/bin/clang" ]]; then
    export CC_wasm32_unknown_unknown="$(brew --prefix llvm)/bin/clang"
  fi
fi
# Rust embeds dependency source locations in panic messages in the shipped WASM.
flags=${CARGO_ENCODED_RUSTFLAGS:-}
plain_flags=${RUSTFLAGS:-${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:-}}
if [[ -z "$flags" && "$plain_flags" =~ [^[:space:]] ]]; then
  read -ra options <<< "$plain_flags"
  flags=$(IFS=$'\x1f'; printf '%s' "${options[*]}")
fi
for mapping in "${CARGO_HOME:-$HOME/.cargo}=/cargo" "${RUSTUP_HOME:-$HOME/.rustup}=/rustup" "$PWD=/source"; do
  flags+=${flags:+$'\x1f'}"--remap-path-prefix=$mapping"
done
CARGO_ENCODED_RUSTFLAGS="$flags" cargo build --locked -p yappers-netkit --lib --target "$target" --release
mkdir -p "$out"
"$cli" --target web --no-typescript --out-name yap --out-dir "$out" "build/rust/$target/release/yappers_netkit.wasm"
cp composeApp/src/commonMain/composeResources/files/fabric.glsl website/static/app/fabric.glsl
npm --prefix website ci
npm --prefix website run check
npm --prefix website run build
