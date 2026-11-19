#!/bin/bash
# Real account migration with the old server offline. No app, clipboard or user data access.
set -euo pipefail
cd "$(dirname "$0")/.."
umask 077
"${CARGO_HOME:-$HOME/.cargo}/bin/cargo" build --locked -p yappers-backend
YAP_SERVER="$PWD/build/rust/debug/yappers-server"
mkdir -p build
YAP_FIXTURE=$(mktemp -d "$PWD/build/portability-cli.XXXXXX")
YAP_A_PID=''
YAP_B_PID=''
cleanup() {
  for YAP_PID in "$YAP_A_PID" "$YAP_B_PID"; do
    if [ -n "$YAP_PID" ]; then kill -INT "$YAP_PID" 2>/dev/null || true; wait "$YAP_PID" 2>/dev/null || true; fi
  done
}
trap cleanup EXIT
for YAP_HOME in a b; do
  "$YAP_SERVER" init --directory "$YAP_FIXTURE/$YAP_HOME" >/dev/null
done
"$YAP_SERVER" run --config "$YAP_FIXTURE/a/config.json" > "$YAP_FIXTURE/a.log" 2>&1 &
YAP_A_PID=$!
"$YAP_SERVER" run --config "$YAP_FIXTURE/b/config.json" > "$YAP_FIXTURE/b.log" 2>&1 &
YAP_B_PID=$!
for _ in $(seq 1 100); do
  if [ -f "$YAP_FIXTURE/a/join-0.json" ] && [ -f "$YAP_FIXTURE/b/join-0.json" ]; then break; fi
  sleep .1
done
YAP_SOURCE="$YAP_FIXTURE/source.wallet.json"
YAP_DESTINATION="$YAP_FIXTURE/destination.wallet.json"
YAP_JOIN_A="$YAP_FIXTURE/a/join-0.json"
YAP_JOIN_B="$YAP_FIXTURE/b/join-0.json"
python3 - "$YAP_FIXTURE" <<'PY'
from pathlib import Path
import json,secrets,sys
root=Path(sys.argv[1])
(root/'passphrase').write_text(secrets.token_urlsafe(36))
(root/'profile.json').write_text(json.dumps({'pronouns':'they/them','bio':'My portable profile','bookmarks':['retained-reference']}))
PY
"$YAP_SERVER" account init --wallet "$YAP_SOURCE" --join "$YAP_JOIN_A"
"$YAP_SERVER" account post --wallet "$YAP_SOURCE" --join "$YAP_JOIN_A" --headline 'Before moving home' --body 'My original signed history belongs to me.'
"$YAP_SERVER" account export --wallet "$YAP_SOURCE" --join "$YAP_JOIN_A" --output "$YAP_FIXTURE/recovery.age" --passphrase-file "$YAP_FIXTURE/passphrase" --local-data "$YAP_FIXTURE/profile.json" --offline
kill -INT "$YAP_A_PID"
wait "$YAP_A_PID"
YAP_A_PID=''
"$YAP_SERVER" account import --wallet "$YAP_DESTINATION" --join "$YAP_JOIN_B" --input "$YAP_FIXTURE/recovery.age" --passphrase-file "$YAP_FIXTURE/passphrase"
"$YAP_SERVER" account post --wallet "$YAP_DESTINATION" --join "$YAP_JOIN_B" --headline 'After moving home' --body 'Same account, a new home and device key.'
# Same recovery import resumes safely without resetting the current signing counter.
"$YAP_SERVER" account import --wallet "$YAP_DESTINATION" --join "$YAP_JOIN_B" --input "$YAP_FIXTURE/recovery.age" --passphrase-file "$YAP_FIXTURE/passphrase"
"$YAP_SERVER" account history --wallet "$YAP_DESTINATION" --join "$YAP_JOIN_B"
"$YAP_SERVER" feed --join "$YAP_JOIN_B" --identity "$YAP_FIXTURE/reader.json" > "$YAP_FIXTURE/feed-b.json"
python3 - "$YAP_FIXTURE" <<'PY'
from pathlib import Path
import json,sys
root=Path(sys.argv[1]);source=json.loads((root/'source.wallet.json').read_text());destination=json.loads((root/'destination.wallet.json').read_text())
assert source['chain'][-1]['payload']['account']==destination['chain'][-1]['payload']['account']
assert source['deviceSecret']!=destination['deviceSecret']
assert destination['nextSequence']==2, 'retry reset an active signing counter'
history=json.loads((root/'destination.wallet.history.json').read_text())
assert len(history)==2 and len({x['event']['id'] for x in history})==2
local=json.loads((root/'destination.wallet.local-data.json').read_text())
assert local['client']['pronouns']=='they/them' and local['client']['bookmarks']==['retained-reference']
posts=json.loads((root/'feed-b.json').read_text())['posts']
assert len(posts)==1 and posts[0]['headline']=='After moving home', 'private imports leaked into a public feed'
print('PASS: offline-source migration preserves account, signed history and local data; retry preserves device sequence; private imports stay out of public feeds')
PY
# Return old home, deliver the signed move without its membership capability, reject stale writer.
rm "$YAP_FIXTURE/a/address.json"
"$YAP_SERVER" run --config "$YAP_FIXTURE/a/config.json" > "$YAP_FIXTURE/a-restart.log" 2>&1 &
YAP_A_PID=$!
for _ in $(seq 1 100); do
  if [ -f "$YAP_FIXTURE/a/address.json" ] && kill -0 "$YAP_A_PID" 2>/dev/null; then
    if "$YAP_SERVER" account notify --wallet "$YAP_DESTINATION" --join "$YAP_JOIN_B" --address "$YAP_FIXTURE/a/address.json" >/dev/null 2>&1; then break; fi
  fi
  sleep .1
done
if "$YAP_SERVER" account post --wallet "$YAP_SOURCE" --join "$YAP_JOIN_A" --headline 'Revoked device check' --body 'Must remain queued, not accepted.' > "$YAP_FIXTURE/revoked.log" 2>&1; then
  echo 'FAIL: old home accepted revoked writer' >&2; exit 1
fi
python3 - "$YAP_SOURCE" <<'PY'
import json,sys
assert len(json.load(open(sys.argv[1]))['pending'])==1
print('PASS: previous home enforces signed move; rejected upload remains durable')
PY
