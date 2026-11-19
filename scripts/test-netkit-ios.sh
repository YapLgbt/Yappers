#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
NETKIT_ROOT="$PWD"
NETKIT_DEVICE="${1:-$(python3 scripts/booted-iphone.py)}"
NETKIT_CARGO="${CARGO_HOME:-$HOME/.cargo}/bin/cargo"
if [ ! -x "$NETKIT_CARGO" ]; then NETKIT_CARGO=$(command -v cargo); fi
mkdir -p build iosApp/.generated
NETKIT_FIXTURE=$(mktemp -d "$NETKIT_ROOT/build/netkit-ios-fixture.XXXXXX")
NETKIT_SERVER_PID=''
NETKIT_MAIL_PID=''
NETKIT_PREVIOUS_APPEARANCE=''
NETKIT_PREVIOUS_CONTENT_SIZE=''
cleanup() {
  if [ -n "$NETKIT_PREVIOUS_APPEARANCE" ]; then xcrun simctl ui "$NETKIT_DEVICE" appearance "$NETKIT_PREVIOUS_APPEARANCE" >/dev/null 2>&1 || true; fi
  if [ -n "$NETKIT_PREVIOUS_CONTENT_SIZE" ]; then xcrun simctl ui "$NETKIT_DEVICE" content_size "$NETKIT_PREVIOUS_CONTENT_SIZE" >/dev/null 2>&1 || true; fi
  printf '{"skip":true}\n' > "$NETKIT_ROOT/iosApp/.generated/NetkitTestJoin.json"
  if [ -n "$NETKIT_MAIL_PID" ]; then kill "$NETKIT_MAIL_PID" 2>/dev/null || true; fi
  if [ -n "$NETKIT_SERVER_PID" ]; then
    kill -INT "$NETKIT_SERVER_PID" 2>/dev/null || true
    wait "$NETKIT_SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT
"$NETKIT_CARGO" build --locked -p yappers-backend
if [ "${NETKIT_TEST_SCOPE:-all}" = welcome ]; then
    "$NETKIT_ROOT/build/rust/debug/yappers-server" public-home --directory "$NETKIT_FIXTURE/home" --manifest "$NETKIT_FIXTURE/join-a.json" > "$NETKIT_FIXTURE/server.log" 2>&1 &
elif [ "${NETKIT_TEST_SCOPE:-all}" = messaging ]; then
    "$NETKIT_ROOT/build/rust/debug/yappers-server" init --directory "$NETKIT_FIXTURE/home" > /dev/null
    ln -s home/join-0.json "$NETKIT_FIXTURE/join-a.json"
    YAP_REPORT_INBOX="$NETKIT_FIXTURE/reports.sqlite" "$NETKIT_ROOT/build/rust/debug/yappers-server" run --config "$NETKIT_FIXTURE/home/config.json" > "$NETKIT_FIXTURE/server.log" 2>&1 &
else
    "$NETKIT_ROOT/build/rust/debug/yappers-server" demo --directory "$NETKIT_FIXTURE" > "$NETKIT_FIXTURE/server.log" 2>&1 &
fi
NETKIT_SERVER_PID=$!
for _ in $(seq 1 100); do
  if [ -f "$NETKIT_FIXTURE/join-a.json" ]; then break; fi
  if ! kill -0 "$NETKIT_SERVER_PID" 2>/dev/null; then cat "$NETKIT_FIXTURE/server.log" >&2; exit 1; fi
  sleep 0.1
done
if [ ! -f "$NETKIT_FIXTURE/join-a.json" ]; then echo 'Local federation fixture did not start' >&2; exit 1; fi
if [ "${NETKIT_TEST_SCOPE:-all}" = welcome ]; then
    kill -INT "$NETKIT_SERVER_PID"
    wait "$NETKIT_SERVER_PID"
    mv "$NETKIT_FIXTURE/join-a.json" "$NETKIT_FIXTURE/bootstrap.json"
    home_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["join"]["serverAddr"]["id"])' "$NETKIT_FIXTURE/bootstrap.json")
    YAP_USERNAME_DOMAINS="$home_id=neighborhood.test" "$NETKIT_ROOT/build/rust/debug/yappers-server" public-home --directory "$NETKIT_FIXTURE/home" --manifest "$NETKIT_FIXTURE/join-a.json" >> "$NETKIT_FIXTURE/server.log" 2>&1 &
    NETKIT_SERVER_PID=$!
    for _ in $(seq 1 100); do
        test -s "$NETKIT_FIXTURE/join-a.json" && break
        kill -0 "$NETKIT_SERVER_PID"
        sleep .1
    done
fi
if [ "${NETKIT_TEST_SCOPE:-all}" = recovery ]; then
  python3 - "$NETKIT_FIXTURE" > iosApp/.generated/NetkitTestJoin.json <<'PY_FIXTURE'
import json, pathlib, sys
directory = pathlib.Path(sys.argv[1])
print(json.dumps({"source": json.loads((directory / "join-a.json").read_text()),
                  "destination": json.loads((directory / "join-b.json").read_text())}))
PY_FIXTURE
else
  cp "$NETKIT_FIXTURE/join-a.json" iosApp/.generated/NetkitTestJoin.json
fi
(cd iosApp && xcodegen generate)
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
NETKIT_TEST_ARGS=(-only-testing:YappersUITests/YappersUITests/testLocalGroupsAutoFederation)
if [ "${NETKIT_TEST_SCOPE:-all}" = welcome ]; then
  xcrun simctl addmedia "$NETKIT_DEVICE" "$NETKIT_ROOT/iosApp/iosApp/Assets.xcassets/AppIcon.appiconset/AppIcon.png"
  NETKIT_TEST_ARGS=(-only-testing:YappersUITests/YappersUITests/testFirstVisitAndReturningAccount -only-testing:YappersUITests/YappersUITests/testRestoreMethodSelection)
elif [ "${NETKIT_TEST_SCOPE:-all}" = recovery ]; then
  NETKIT_TEST_ARGS=(-only-testing:YappersUITests/YappersUITests/testNativeAccountBackupAndMove)
elif [ "${NETKIT_TEST_SCOPE:-all}" = messaging ]; then
  NETKIT_TEST_ARGS=(-only-testing:YappersUITests/YappersUITests/testEncryptedConversation -only-testing:YappersUITests/YappersUITests/testBlockAndUnblockFromInbox)
  YAP_IOS_FIXTURE="$NETKIT_FIXTURE" "$NETKIT_CARGO" test --locked -p yappers-netkit --test portability ios_messaging_fixture -- --ignored > "$NETKIT_FIXTURE/messaging.log" 2>&1 &
  NETKIT_MAIL_PID=$!
  for _ in $(seq 1 120); do
    [ -f "$NETKIT_FIXTURE/dm-peer-ready" ] && break
    if ! kill -0 "$NETKIT_MAIL_PID" 2>/dev/null; then cat "$NETKIT_FIXTURE/messaging.log" >&2; exit 1; fi
    sleep 1
  done
  test -f "$NETKIT_FIXTURE/dm-peer-ready"
elif [ "${NETKIT_TEST_SCOPE:-all}" != groups ]; then
  # Seed a project-owned image so photo-picker coverage works on a clean simulator.
  xcrun simctl addmedia "$NETKIT_DEVICE" "$NETKIT_ROOT/iosApp/iosApp/Assets.xcassets/AppIcon.appiconset/AppIcon.png"
  NETKIT_TEST_ARGS+=(-only-testing:YappersUITests/YappersUITests/testFederatedNetkit -only-testing:YappersUITests/YappersUITests/testFeedComposerAndSaved -only-testing:YappersUITests/YappersUITests/testFeedFiltersAndLocalSafetyActions -only-testing:YappersUITests/YappersUITests/testExpressiveProfileAndNamedPost -only-testing:YappersUITests/YappersUITests/testProfileEditorScrollingStaysAnchored -only-testing:YappersUITests/YappersUITests/testNativeComposerDismissalAndDiscard -only-testing:YappersUITests/YappersUITests/testMultilineCommentIsRevealedAfterPosting -only-testing:YappersUITests/YappersUITests/testNativeRootToolbarAndTabs)
fi
xcodebuild -project iosApp/Yappers.xcodeproj -scheme Yappers -configuration Debug \
  -destination "platform=iOS Simulator,id=$NETKIT_DEVICE" -derivedDataPath build/ios \
  -resultBundlePath "$NETKIT_FIXTURE/ui.xcresult" -parallel-testing-enabled NO \
  "${NETKIT_TEST_ARGS[@]}" CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- CODE_SIGNING_REQUIRED=NO test
if [ "${NETKIT_TEST_SCOPE:-all}" = welcome ] || [ "${NETKIT_TEST_SCOPE:-all}" = recovery ]; then exit 0; fi
if [ "${NETKIT_TEST_SCOPE:-all}" = messaging ]; then
  wait "$NETKIT_MAIL_PID"
  NETKIT_MAIL_PID=''
  exit 0
fi
if [ "${NETKIT_TEST_SCOPE:-all}" != groups ]; then
NETKIT_PRIVATE_OK=false
for _ in $(seq 1 10); do
  "$NETKIT_ROOT/build/rust/debug/yappers-server" feed --join "$NETKIT_FIXTURE/join-b.json" \
    --identity "$NETKIT_FIXTURE/reader.json" > "$NETKIT_FIXTURE/feed-b.json"
  if python3 - "$NETKIT_FIXTURE/feed-b.json" <<'PY'
import json,sys
feed=json.load(open(sys.argv[1]))
posts=[p for p in feed['posts'] if p['body'].startswith('Federated iPhone smoke ')]
if len(posts)!=1: sys.exit('Waiting for iPhone post at home B')
p=posts[0]
if p['score']!=1: sys.exit('Waiting for iPhone vote at home B')
if not any(r['body']=='Reply from the Kotlin iPhone client.' for r in p['replies']): sys.exit('Waiting for iPhone reply at home B')
print('PASS: Kotlin iPhone → Rust C ABI → Iroh home A → Iroh federation → independent home B')
PY
  then NETKIT_PRIVATE_OK=true; break; fi
  sleep 1
done
if [ "$NETKIT_PRIVATE_OK" != true ]; then echo "Private feed did not converge" >&2; exit 1; fi
fi
# Join the UI-created groups on B through the public catalog, with no manual group peer entries.
"$NETKIT_ROOT/build/rust/debug/yappers-server" groups --join "$NETKIT_FIXTURE/join-b.json" \
  --identity "$NETKIT_FIXTURE/group-reader.json" > "$NETKIT_FIXTURE/groups-b.json"
python3 - "$NETKIT_FIXTURE/groups-b.json" > "$NETKIT_FIXTURE/group-ids.txt" <<'PY_IDS'
import json,sys
catalog=json.load(open(sys.argv[1]))['groups']
selected=[g for g in catalog if g['name']=='Neighborhood book club' or g['name']=='Queer everywhere' or g['name'].startswith('iPhone circle ')]
assert len(selected)==3, 'Expected local, worldwide, and iPhone-created groups in B catalog'
for g in selected: print(g['id'])
PY_IDS
while IFS= read -r NETKIT_GROUP_ID; do
  NETKIT_GROUP_KEY=$(printf '%s' "$NETKIT_GROUP_ID" | shasum -a 256 | cut -d ' ' -f 1)
  "$NETKIT_ROOT/build/rust/debug/yappers-server" join-group --join "$NETKIT_FIXTURE/join-b.json" \
    --identity "$NETKIT_FIXTURE/group-reader.json" --group "$NETKIT_GROUP_ID" --output "$NETKIT_FIXTURE/$NETKIT_GROUP_KEY.join.json" > /dev/null
  NETKIT_GROUP_OK=false
  for _ in $(seq 1 15); do
    "$NETKIT_ROOT/build/rust/debug/yappers-server" feed --join "$NETKIT_FIXTURE/$NETKIT_GROUP_KEY.join.json" \
      --identity "$NETKIT_FIXTURE/group-reader.json" > "$NETKIT_FIXTURE/$NETKIT_GROUP_KEY.feed.json"
    if python3 - "$NETKIT_FIXTURE/$NETKIT_GROUP_KEY.feed.json" <<'PY_FEED'
import json,sys
posts=json.load(open(sys.argv[1]))['posts']
if not any(p['body'].startswith(('Local group iPhone ','Worldwide group iPhone ','Created group iPhone ')) for p in posts): sys.exit('Waiting for group history at home B')
assert not any(p['body'].startswith('Federated iPhone smoke ') for p in posts), 'Private home post leaked into group'
PY_FEED
    then NETKIT_GROUP_OK=true; break; fi
    sleep 1
  done
  if [ "$NETKIT_GROUP_OK" != true ]; then echo 'Group did not converge on home B' >&2; exit 1; fi
done < "$NETKIT_FIXTURE/group-ids.txt"
echo 'PASS: locality, worldwide, and newly created iPhone groups automatically federate to home B'

if [ "${NETKIT_CAPTURE_UI:-0}" = 1 ]; then
  NETKIT_PREVIOUS_APPEARANCE=$(xcrun simctl ui "$NETKIT_DEVICE" appearance)
  NETKIT_PREVIOUS_CONTENT_SIZE=$(xcrun simctl ui "$NETKIT_DEVICE" content_size)
  mkdir -p "$NETKIT_FIXTURE/screenshots"
  xcrun simctl ui "$NETKIT_DEVICE" appearance light
  xcrun simctl ui "$NETKIT_DEVICE" content_size large
  xcrun simctl terminate "$NETKIT_DEVICE" app.yappers.prototype 2>/dev/null || true
  xcrun simctl launch "$NETKIT_DEVICE" app.yappers.prototype --netkit-ui-test --show-groups
  sleep 6
  xcrun simctl io "$NETKIT_DEVICE" screenshot "$NETKIT_FIXTURE/screenshots/groups-light.png"
  xcrun simctl ui "$NETKIT_DEVICE" appearance dark
  sleep 2
  xcrun simctl io "$NETKIT_DEVICE" screenshot "$NETKIT_FIXTURE/screenshots/groups-dark.png"
  xcrun simctl ui "$NETKIT_DEVICE" appearance light
  xcrun simctl ui "$NETKIT_DEVICE" content_size accessibility-medium
  sleep 2
  xcrun simctl io "$NETKIT_DEVICE" screenshot "$NETKIT_FIXTURE/screenshots/groups-large-text.png"
  echo "Screenshots: $NETKIT_FIXTURE/screenshots"
fi
