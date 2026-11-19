#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
ndk=$1
out=$2
case "$(uname -s)" in Darwin) host=darwin-x86_64;; Linux) host=linux-x86_64;; *) echo "Use macOS or Linux to build Android Netkit" >&2; exit 1;; esac
bin="$ndk/toolchains/llvm/prebuilt/$host/bin"
for target in aarch64-linux-android x86_64-linux-android; do
    case "$target" in aarch64*) abi=arm64-v8a;; x86_64*) abi=x86_64;; esac
    cc="$bin/${target}28-clang"
    name=${target//-/_}
    upper=$(echo "$name" | tr '[:lower:]' '[:upper:]')
    env "CARGO_TARGET_${upper}_LINKER=$cc" "CC_${name}=$cc" "CXX_${name}=$cc++" "AR_${name}=$bin/llvm-ar" \
        "CARGO_TARGET_${upper}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384" \
        cargo build --locked --release --target "$target" -p yappers-netkit --lib
    mkdir -p "$out/$abi"
    "$bin/llvm-strip" --strip-unneeded -o "$out/$abi/libyappers_netkit.so" "build/rust/$target/release/libyappers_netkit.so"
    "$cc" -O2 -shared -fPIC -Wl,-z,max-page-size=16384 -Wl,-z,defs -IcomposeApp/src/nativeInterop/cinterop \
        androidApp/src/main/cpp/bridge.c -L"$out/$abi" -lyappers_netkit -o "$out/$abi/libyap_android.so"
done
