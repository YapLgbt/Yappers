plugins { id("com.android.application") }

android {
    namespace = "app.yappers.android"
    compileSdk = 37
    ndkVersion = "28.2.13676358"
    testBuildType = "release"
    defaultConfig {
        testInstrumentationRunner = "app.yappers.qa.DisposableHomeCheck"
        applicationId = "app.yappers.android"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk.abiFilters += listOf("arm64-v8a", "x86_64")
    }
    val signingFile = providers.environmentVariable("YAP_ANDROID_KEYSTORE").orNull
    if (signingFile != null) {
        signingConfigs.create("tester") {
            storeFile = file(signingFile)
            val password = file(providers.environmentVariable("YAP_ANDROID_PASSWORD_FILE").get()).readText().trimEnd()
            storePassword = password
            keyAlias = "yap-testing"
            keyPassword = password
        }
        buildTypes.getByName("release").signingConfig = signingConfigs.getByName("tester")
    }

}

abstract class AndroidNetkitBuild : Exec() {
    @get:OutputDirectory abstract val libraries: DirectoryProperty
}

val buildNetkit = tasks.register<AndroidNetkitBuild>("buildNetkit") {
    val sdk = androidComponents.sdkComponents.sdkDirectory
    val ndk = android.ndkVersion
    inputs.files(rootProject.fileTree("netkit/src"), rootProject.file("Cargo.toml"), rootProject.file("Cargo.lock"), rootProject.file("netkit/Cargo.toml"), rootProject.file("rust-toolchain.toml"), rootProject.file(".cargo/config.toml"), rootProject.file("scripts/build-netkit-android.sh"), rootProject.file("androidApp/src/main/cpp/bridge.c"), rootProject.file("composeApp/src/nativeInterop/cinterop/netkit.h"))
    libraries.set(layout.buildDirectory.dir("generated/netkit"))
    workingDir(rootProject.projectDir)
    commandLine("bash", "scripts/build-netkit-android.sh", sdk.get().dir("ndk/$ndk").asFile, layout.buildDirectory.dir("generated/netkit").get().asFile)
}
androidComponents.onVariants { variant ->
    variant.sources.jniLibs?.addGeneratedSourceDirectory(buildNetkit, AndroidNetkitBuild::libraries)
}
dependencies {
    implementation(project(":composeApp"))
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
}
