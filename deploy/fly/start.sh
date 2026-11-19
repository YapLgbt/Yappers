#!/bin/bash
set -euo pipefail
umask 077

state=${YAP_SERVICE_STATE:-/data}
test -s "$state/enrollment/home.json"
test -s "$state/federation.json"

# Keep content-addressed offers available across upgrades on the persistent volume.
revision=$(cat /srv/yap-site/source/revision.txt)
[[ "$revision" =~ ^[0-9a-f]{64}$ ]]
archive="$state/source-releases/$revision.tar.gz"
install -d -m 0755 "$state/source-releases"
if [[ ! -f "$archive" ]]; then
    install -m 0644 "/srv/yap-site/source/$revision.tar.gz" "$archive.new"
    mv "$archive.new" "$archive"
fi
echo "$revision  $archive" | sha256sum --check --status

# Keep HTTPS and the original public peer available while a network service reconnects.
supervise() {
    local child=''
    trap 'if [[ -n "$child" ]]; then kill -TERM "$child" 2>/dev/null || true; fi; exit 0' TERM INT
    while true; do
        "$@" &
        child=$!
        wait "$child" || true
        child=''
        sleep 2
    done
}
serve_standby() {
    # Provisioning writes the activation marker last, after staging the private files.
    while [[ ! -f "$state/ha/active" ]]; do sleep 2; done
    if [[ -s "$state/push/public" && -s "$state/push/private" ]]; then
        export YAP_PUSH_PROVIDERS="${YAP_PUSH_PROVIDERS:-web}"
        export YAP_WEB_PUSH_KEY="$(cat "$state/push/public")"
        export YAP_WEB_PUSH_PRIVATE_FILE="$state/push/private"
        export YAP_PUSH_WORKER=/opt/yap-push/send.cjs
    fi
    export YAP_WORKER_HEALTH_ADDR=0.0.0.0:8082
    exec yappers-server ha-run --config "$state/ha/home.json" --grant "$state/ha/grant.json" \
        --standby-config "$state/ha/transport/config.json" --ledger "$state/ha/ledger.sqlite"
}

caddy run --config /etc/caddy/Caddyfile --adapter caddyfile &
web_pid=$!
YAP_WORKER_HEALTH_ADDR=0.0.0.0:8081 supervise yappers-server run --config "$state/federation.json" &
peer_pid=$!
supervise serve_standby &
ha_pid=$!
stop() {
    kill -TERM "$web_pid" "$peer_pid" "$ha_pid" 2>/dev/null || true
    wait || true
}
trap stop EXIT
trap 'exit 0' TERM INT
wait "$web_pid"
exit 1
