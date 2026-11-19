#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
DEVICE_ID="${1:-$(python3 scripts/booted-iphone.py)}"
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
(cd iosApp && xcodegen generate)
xcodebuild -project iosApp/Yappers.xcodeproj -scheme Yappers -configuration Debug -destination "id=$DEVICE_ID" -derivedDataPath build/ios CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- CODE_SIGNING_REQUIRED=NO build
xcrun simctl install "$DEVICE_ID" build/ios/Build/Products/Debug-iphonesimulator/Yap.app
xcrun simctl launch "$DEVICE_ID" app.yappers.prototype
open -a Simulator
