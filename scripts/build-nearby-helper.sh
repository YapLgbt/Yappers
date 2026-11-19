#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
bundle="$1"
mkdir -p "$bundle/Contents/MacOS"
cp composeApp/src/macosHelper/Info.plist "$bundle/Contents/Info.plist"
xcrun swiftc -O iosApp/iosApp/AppleNearbyServices.swift composeApp/src/macosHelper/main.swift -o "$bundle/Contents/MacOS/YapNearby"
codesign --force --sign - "$bundle"
