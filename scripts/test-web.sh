#!/bin/bash
# Run the browser end-to-end checks against a newly created Iroh home.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked -p yappers-backend -p yappers-netkit
mkdir -p build
fixture=$(mktemp -d "$PWD/build/web-qa.XXXXXX")
server_pids=()
bootstrap_pid=''
cleanup() {
  for pid in "$bootstrap_pid" "${server_pids[@]:-}"; do
    if [[ -n "$pid" ]]; then kill -INT "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
  done
}
trap cleanup EXIT

await_manifest() {
  local manifest=$1 pid=$2
  for _ in {1..300}; do
    [[ -s "$manifest" ]] && return 0
    kill -0 "$pid" || return 1
    sleep .1
  done
  [[ -s "$manifest" ]]
}

start_named_home() {
  local directory=$1 manifest=$2 domain=$3 log=$4 reports=$5 home_id
  build/rust/debug/yappers-server public-home --directory "$directory" --manifest "$manifest" >"$log" 2>&1 &
  bootstrap_pid=$!
  await_manifest "$manifest" "$bootstrap_pid"
  kill -INT "$bootstrap_pid"
  wait "$bootstrap_pid"
  bootstrap_pid=''
  mv "$manifest" "$directory/bootstrap.json"
  home_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["join"]["serverAddr"]["id"])' "$directory/bootstrap.json")
  YAP_REPORT_INBOX="$reports" YAP_USERNAME_DOMAINS="$home_id=$domain" build/rust/debug/yappers-server public-home \
    --directory "$directory" --manifest "$manifest" >>"$log" 2>&1 &
  local pid=$!
  server_pids+=("$pid")
  await_manifest "$manifest" "$pid"
}

start_named_home "$fixture/home" "$fixture/enrollment.json" neighborhood.test "$fixture/home.log" "$fixture/reports.sqlite"
if [[ "${YAP_QA_PARITY_ONLY:-}" == 1 && "${YAP_BROWSER:-chrome}" == chrome ]]; then
  start_named_home "$fixture/destination" "$fixture/destination.json" destination.test "$fixture/destination.log" "$fixture/destination-reports.sqlite"
  export YAP_QA_DESTINATION="$fixture/destination.json"
fi
YAP_QA_REPORTS=1 node website/qa/feed.cjs "$fixture/enrollment.json"
