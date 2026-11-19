#!/usr/bin/env bash
# Build an updatable, release-signed APK for the Discord testing channel.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${ANDROID_HOME:?Set ANDROID_HOME to an Android SDK with API 37 and NDK 28.2.13676358}"
if [[ -z "${YAP_ANDROID_KEYSTORE:-}" && -z "${YAP_ANDROID_PASSWORD_FILE:-}" ]]; then
    sign_dir="$HOME/.android/yap-testing"
    export YAP_ANDROID_KEYSTORE="$sign_dir/yap-testing.p12"
    export YAP_ANDROID_PASSWORD_FILE="$sign_dir/store-password"
    if [[ ! -d "$sign_dir" ]]; then
        (
            umask 077
            mkdir -p "$HOME/.android"
            mkdir "$sign_dir"
            openssl rand -hex 32 > "$YAP_ANDROID_PASSWORD_FILE"
            keytool -genkeypair -keyalg RSA -keysize 3072 -validity 10000 -storetype PKCS12 \
                -keystore "$YAP_ANDROID_KEYSTORE" -alias yap-testing \
                -storepass:file "$YAP_ANDROID_PASSWORD_FILE" -keypass:file "$YAP_ANDROID_PASSWORD_FILE" \
                -dname 'CN=Yap Android Testing'
        )
    fi
fi
if [[ ! -s "${YAP_ANDROID_KEYSTORE:-}" || ! -s "${YAP_ANDROID_PASSWORD_FILE:-}" ]]; then
    echo "Restore the Android tester keystore and its password file before building updates." >&2
    exit 1
fi
./gradlew --no-configuration-cache :androidApp:assembleRelease
apk=androidApp/build/outputs/apk/release/androidApp-release.apk
tools="$ANDROID_HOME/build-tools/37.0.0"
"$tools/zipalign" -c -P 16 4 "$apk"
"$tools/apksigner" verify --verbose "$apk"
mkdir -p build/dist
version=$("$tools/aapt" dump badging "$apk" | awk -F"'" '/^package:/ {print $6 "-test" $4}')
cp "$apk" "build/dist/Yap-Android-$version.apk"
echo "Built build/dist/Yap-Android-$version.apk"
