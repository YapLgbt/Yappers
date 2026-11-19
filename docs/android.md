# Android testing

Yap’s Android APK runs on 64-bit ARM and x86 phones, tablets and emulators with Android 9 or later. Open the APK on Android and follow the install prompt. If Android asks, allow the app you used to open the file to install it. Open Yap, choose a display name, then join yap.lgbt. You can save your recovery key during onboarding or later under **You → Account recovery**.

Install later tester APKs over the existing app to keep your account and conversations. Uninstalling Yap or clearing its app data removes the device’s keys and local conversation history.

## Android integrations

The Android app shares its Compose screens and Rust networking and encrypted messaging implementation with the iPhone and Mac apps. The Android Keystore protects the app’s local identity, feed and DM state. Android Credential Manager can save an account recovery key with the password provider you select. The system file picker handles encrypted backup imports and exports.

Town search works offline. The directory uses [GeoNames](https://www.geonames.org/export/) under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), with rounded coordinates. **Use my current location** asks for approximate location while using Yap. Background refresh uses WorkManager when a network is available. Android controls delivery timing.

## Build the tester APK

Use JDK 21, the Rust version in `rust-toolchain.toml`, and an Android SDK with command-line tools. Set `ANDROID_HOME` to the SDK directory and put JDK 21 on your PATH.

```sh
sdkmanager 'platforms;android-37.0' 'build-tools;37.0.0' 'platform-tools' 'ndk;28.2.13676358'
rustup target add aarch64-linux-android x86_64-linux-android
./scripts/build-android.sh
```

The script verifies the signature and APK page alignment and writes a shareable APK to `build/dist/`. It creates the tester keystore under `~/.android/yap-testing/` on the first build. Back up that directory privately. Android requires the same signing key for every tester update. A release operator can supply the existing keystore and password-file paths through `YAP_ANDROID_KEYSTORE` and `YAP_ANDROID_PASSWORD_FILE`. The signing alias is `yap-testing`. Increment the version code in `androidApp/build.gradle.kts` for each distribution.

## Check a release on a disposable emulator

Build the macOS or Linux test peer with `cargo build --locked -p yappers-backend -p yappers-netkit`. Create a clean emulator, then run these commands with its actual serial. The check creates accounts and publishes test posts on a temporary local home.

```sh
export YAP_ANDROID_KEYSTORE="$HOME/.android/yap-testing/yap-testing.p12"
export YAP_ANDROID_PASSWORD_FILE="$HOME/.android/yap-testing/store-password"
./gradlew --no-configuration-cache :androidApp:assembleReleaseAndroidTest
adb -s emulator-5554 install -r androidApp/build/outputs/apk/release/androidApp-release.apk
adb -s emulator-5554 install -r androidApp/build/outputs/apk/androidTest/release/androidApp-release-androidTest.apk
python3 scripts/test-android.py emulator-5554
```
