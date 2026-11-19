import org.jetbrains.kotlin.gradle.plugin.mpp.KotlinNativeTarget
import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    id("com.android.kotlin.multiplatform.library")
    kotlin("multiplatform")
    kotlin("plugin.compose")
    kotlin("plugin.serialization")
    id("org.jetbrains.compose")
}
compose.resources {
    packageOfResClass = "app.yappers.resources"
}

kotlin {
    android {
        namespace = "app.yappers.shared"
        compileSdk = 37
        minSdk = 28
        androidResources.enable = true
    }
    iosArm64()
    iosSimulatorArm64()
    jvm()
    targets.withType<KotlinNativeTarget>().configureEach {
        binaries.framework { baseName = "Yappers"; isStatic = true }
    }
    sourceSets {
        commonMain.dependencies {
            implementation("org.jetbrains.compose.components:components-resources:1.12.2")
            implementation("org.jetbrains.compose.runtime:runtime:1.12.2")
            implementation("org.jetbrains.compose.foundation:foundation:1.12.2")
            implementation("org.jetbrains.compose.material3:material3:1.9.0")
            implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
        }
        commonTest.dependencies { implementation(kotlin("test")) }
        iosMain { kotlin.srcDir("src/skiaMain/kotlin") }
        jvmMain { kotlin.srcDir("src/skiaMain/kotlin") }
        androidMain.dependencies {
            implementation("androidx.activity:activity-compose:1.12.0")
            implementation("androidx.credentials:credentials:1.5.0")
            implementation("androidx.credentials:credentials-play-services-auth:1.5.0")
            implementation("androidx.work:work-runtime-ktx:2.11.0")
        }
        jvmMain.dependencies {
            implementation(compose.desktop.currentOs)
            implementation("net.java.dev.jna:jna:5.18.1")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-swing:1.9.0")
        }
        jvmTest.dependencies { implementation("org.jetbrains.compose.ui:ui-test-junit4:1.12.2") }
    }
    jvmToolchain(21)
}

// C ABI headers are stable; each Native target embeds its matching Rust static library.
kotlin.targets.withType<KotlinNativeTarget>().configureEach {
    val taskSuffix = name.replaceFirstChar { it.uppercase() }
    val rustTriple = when (name) {
        "iosArm64" -> "aarch64-apple-ios"
        "iosSimulatorArm64" -> "aarch64-apple-ios-sim"
        else -> error("Unsupported Netkit target: $name")
    }
    val rustLibrary = rootProject.layout.projectDirectory.file("build/rust/$rustTriple/release/libyappers_netkit.a")
    val cargoBuild = tasks.register<Exec>("buildNetkit${taskSuffix}") {
        inputs.files(
            rootProject.file("rust-toolchain.toml"),
            rootProject.file(".cargo/config.toml"),
            rootProject.file("Cargo.toml"),
            rootProject.file("Cargo.lock"),
            rootProject.file("netkit/Cargo.toml"),
            rootProject.fileTree("netkit/src"),
            rootProject.file("scripts/build-netkit.sh")
        )
        outputs.file(rustLibrary)
        workingDir(rootProject.projectDir)
        commandLine(rootProject.file("scripts/build-netkit.sh").absolutePath, rustTriple)
    }
    val baseDefinition = layout.projectDirectory.file("src/nativeInterop/cinterop/netkit.def")
    val generatedDef = layout.buildDirectory.file("generated/netkit/${name}.def")
    val definition = tasks.register("prepareNetkit${taskSuffix}") {
        inputs.file(baseDefinition)
        outputs.file(generatedDef)
        doLast {
            generatedDef.get().asFile.apply {
                parentFile.mkdirs()
                writeText(baseDefinition.asFile.readText() +
                    "\nstaticLibraries = libyappers_netkit.a\nlibraryPaths = ${rustLibrary.asFile.parent}\n" +
                    "linkerOpts = -framework Network -framework Security -framework SystemConfiguration -framework CoreFoundation -lresolv -liconv\n")
            }
        }
    }
    compilations.getByName("main").cinterops.create("netkit") {
        definitionFile.set(generatedDef)
        includeDirs(project.file("src/nativeInterop/cinterop"))
    }
    tasks.matching { it.name == "cinteropNetkit${taskSuffix}" }.configureEach {
        dependsOn(cargoBuild, definition)
        inputs.file(rustLibrary)
        inputs.file("src/nativeInterop/cinterop/netkit.h")
    }
}

// Desktop uses exactly the same JSON C ABI as iOS, loaded from a bundled dylib.
val desktopRust = rootProject.layout.projectDirectory.file("build/rust/release/libyappers_netkit.dylib")
val desktopResources = layout.buildDirectory.dir("generated/desktopResources")
val buildNetkitDesktop by tasks.registering(Exec::class) {
    inputs.files(rootProject.file("Cargo.lock"), rootProject.file("Cargo.toml"),
        rootProject.file("netkit/Cargo.toml"), rootProject.fileTree("netkit/src"))
    outputs.file(desktopRust)
    workingDir(rootProject.projectDir)
    commandLine(System.getProperty("user.home") + "/.cargo/bin/cargo", "build", "--locked", "-p", "yappers-netkit", "--lib", "--release")
}
val bundleNetkitDesktop by tasks.registering(Copy::class) {
    dependsOn(buildNetkitDesktop)
    from(desktopRust)
    into(desktopResources.map { it.dir("netkit") })
}
kotlin.sourceSets.named("jvmMain") { resources.srcDir(desktopResources) }
tasks.named("jvmProcessResources") { dependsOn(bundleNetkitDesktop) }
compose.desktop {
    application {
        mainClass = "app.yappers.DesktopMainKt"
        nativeDistributions {
            targetFormats(TargetFormat.Dmg)
            packageName = "Yap"
            packageVersion = "1.0.0"
            modules("java.desktop", "java.logging", "jdk.unsupported")
            macOS {
                bundleID = "app.yappers.desktop"
                iconFile.set(project.file("src/jvmMain/resources/icons/Yap.icns"))
            }
        }
    }
}

val nearbyResources = layout.buildDirectory.dir("generated/nearbyResources")
val nearbyBundle = nearbyResources.map { it.dir("common/YapNearby.app") }
val buildNearbyHelper by tasks.registering(Exec::class) {
    val apple = rootProject.file("iosApp/iosApp/AppleNearbyServices.swift")
    val main = file("src/macosHelper/main.swift")
    val plist = file("src/macosHelper/Info.plist")
    inputs.files(apple, main, plist)
    outputs.dir(nearbyBundle)
    val script = rootProject.file("scripts/build-nearby-helper.sh")
    inputs.file(script)
    commandLine(script, nearbyBundle.get().asFile)

}
compose.desktop.application.nativeDistributions.appResourcesRootDir.set(nearbyResources)
tasks.matching { it.name == "prepareAppResources" }.configureEach { dependsOn(buildNearbyHelper) }
