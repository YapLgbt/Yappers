#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
set -eu
out=$1
release=${2:-}
case "$release" in
    *[!a-zA-Z0-9._-]*) printf '%s\n' 'Invalid GitHub release tag.' >&2; exit 2 ;;
esac
mkdir -p "$out"
# Explicit build inputs. Private home configuration, runtime data and signing keys are never inputs.
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -cf "$out/home.tar" \
    Cargo.toml Cargo.lock rust-toolchain.toml LICENSING.md LICENSES \
    backend/Cargo.toml backend/src backend/tests netkit/Cargo.toml netkit/src netkit/tests netkit/migrations netkit/peering \
    website/static/app/fonts/geist-sans.woff2 website/static/app/fonts/OFL.txt \
    website/package.json website/package-lock.json website/vite.admin.config.ts website/tsconfig.json \
    website/src/app.html website/src/admin website/src/lib/ActionButton.svelte website/src/routes/app/skeleton.css website/scripts/ui-notices.mjs \
    compose.yaml deploy/compose deploy/fly deploy/systemd deploy/peering deploy/push/package.json deploy/push/package-lock.json deploy/push/send.cjs fly.toml .dockerignore
gzip -n "$out/home.tar"
digest=$(sha256sum "$out/home.tar.gz" | cut -d' ' -f1)
mv "$out/home.tar.gz" "$out/$digest.tar.gz"
echo "$digest" > "$out/revision.txt"
if [ -n "$release" ]; then
    sed -e "s/@REVISION@/$digest/g" -e "s/@SOURCE_RELEASE@/$release/g" \
        deploy/peering/discovery.json > "$out/peering.json"
fi
