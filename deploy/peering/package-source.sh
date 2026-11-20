#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
set -eu
out=$1
mkdir -p "$out"
# Explicit build inputs. Private home configuration, runtime data and signing keys are never inputs.
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -cf "$out/home.tar" \
    Cargo.toml Cargo.lock rust-toolchain.toml LICENSING.md LICENSES \
    backend/Cargo.toml backend/src backend/tests netkit/Cargo.toml netkit/src netkit/tests netkit/migrations netkit/peering \
    deploy/fly deploy/systemd deploy/peering deploy/push/package.json deploy/push/package-lock.json deploy/push/send.cjs fly.toml .dockerignore
gzip -n "$out/home.tar"
digest=$(sha256sum "$out/home.tar.gz" | cut -d' ' -f1)
mv "$out/home.tar.gz" "$out/$digest.tar.gz"
echo "$digest" > "$out/revision.txt"
sed "s/@REVISION@/$digest/g" deploy/peering/source.html > "$out/index.html"
sed "s/@REVISION@/$digest/g" deploy/peering/discovery.json > "$out/peering.json"
