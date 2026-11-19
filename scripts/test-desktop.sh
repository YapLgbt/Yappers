#!/bin/bash
# Isolated two-home Iroh fixture plus real desktop UI and native-ABI tests.
set -euo pipefail
cd "$(dirname "$0")/.."
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
NETKIT_CARGO="${CARGO_HOME:-$HOME/.cargo}/bin/cargo"
"$NETKIT_CARGO" build --locked -p yappers-backend
mkdir -p build
export YAPPERS_DESKTOP_FIXTURE=$(mktemp -d "$PWD/build/netkit-desktop-fixture.XXXXXX")
build/rust/debug/yappers-server demo --directory "$YAPPERS_DESKTOP_FIXTURE" > "$YAPPERS_DESKTOP_FIXTURE/server.log" 2>&1 &
DESKTOP_SERVER_PID=$!
cleanup() { kill -INT "$DESKTOP_SERVER_PID" 2>/dev/null || true; wait "$DESKTOP_SERVER_PID" 2>/dev/null || true; }
trap cleanup EXIT
for _ in $(seq 1 100); do
  if [ -f "$YAPPERS_DESKTOP_FIXTURE/join-a.json" ]; then break; fi
  if ! kill -0 "$DESKTOP_SERVER_PID" 2>/dev/null; then cat "$YAPPERS_DESKTOP_FIXTURE/server.log" >&2; exit 1; fi
  sleep .1
done
test -f "$YAPPERS_DESKTOP_FIXTURE/join-a.json"
./gradlew :composeApp:jvmTest --rerun-tasks
for _ in $(seq 1 15); do
  build/rust/debug/yappers-server feed --join "$YAPPERS_DESKTOP_FIXTURE/join-b.json" --identity "$YAPPERS_DESKTOP_FIXTURE/reader.json" > "$YAPPERS_DESKTOP_FIXTURE/feed-b.json"
  if python3 - "$YAPPERS_DESKTOP_FIXTURE/feed-b.json" <<'PY'
import json,sys
posts=json.load(open(sys.argv[1]))['posts']
post=next((p for p in posts if p.get('headline')=='Desktop federation smoke'),None)
if not post or post['score']!=1 or not any(r['body']=='Desktop comment over Iroh.' for r in post['replies']): sys.exit(1)
print('PASS: desktop Kotlin → Rust C ABI → Iroh home A → independent home B (headline, body, comment, vote)')
PY
  then exit 0; fi
  sleep 1
done
echo 'Desktop publication did not converge at home B' >&2
exit 1
