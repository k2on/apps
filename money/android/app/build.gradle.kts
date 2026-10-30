plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    // The screens rendered on the JVM through layoutlib from the demo's household (src/test):
    // `recordPaparazziDebug` writes the PNGs, which `packages.money-android-screens` lays out.
    id("app.cash.paparazzi")
}

/**
 * money's library for the phone (`packages.money-android-lib`): `jniLibs/<abi>/libmoney_ffi.so`, the
 * Rust kernel with money's domain linked in, and `kotlin/`, the Kotlin UniFFI writes for it with the
 * bridge's typed half (`dev.arkdb.ffi`). `money-apk` passes the store path as `-PmoneyLib`; a laptop
 * builds it once (`nix build .#money-android-lib -o money/ffi/android`) and is pointed at that.
 */
val moneyLib: String = providers.gradleProperty("moneyLib").getOrElse(rootProject.file("../ffi/android").path)

/** arkui, the look every Ark app on Android shares, compiled from its sources as harken does. */
val arkUi = rootProject.file("../../kotlin/arkui/src")

android {
    namespace = "dev.money.android"
    compileSdk = 35
    // Said explicitly, because AGP otherwise asks for its own default (34.0.0)
    // and installs it into the SDK — which, under nix, is a read-only store path.
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "us.koon.money"
        minSdk = 26
        targetSdk = 35
        // Rises with every commit when nix builds it (`money-apk` passes the
        // flake's source time in minutes), as harken's does.
        versionCode = providers.gradleProperty("versionCode").map { it.toInt() }.getOrElse(1)
        versionName = providers.gradleProperty("versionName").getOrElse("0.1")
        // The ABIs the kernel is built for (`money-android-lib`).
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }

    // One key for every build, committed beside this file and public on
    // purpose, for the reasons harken/android/app/build.gradle.kts gives: one
    // APK installs over the last, on every machine that builds it. A PKCS#12
    // file (made with openssl), which the JDK's keystore reads as it reads a
    // JKS one. It must never sign anything published as trusted.
    signingConfigs {
        getByName("debug") {
            storeFile = rootProject.file("debug.keystore")
            storeType = "pkcs12"
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        debug {
            signingConfig = signingConfigs.getByName("debug")
        }
        // What `money-apk` ships: not debuggable, so ART compiles Compose as it
        // would any app's. Not minified: R8 is not in the recorded graph.
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    lint {
        checkReleaseBuilds = false
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    kotlinOptions {
        jvmTarget = "21"
    }

    buildFeatures {
        compose = true
    }

    sourceSets {
        getByName("main") {
            // The kernel, per ABI, and the Kotlin over it; and arkui.
            jniLibs.srcDir("$moneyLib/jniLibs")
            kotlin.srcDir("$moneyLib/kotlin")
            kotlin.srcDir(arkUi)
        }
    }
}

// harken's graph, exactly: `deps.json` is harken's recorded Maven graph, replayed as it is,
// which holds only the versions harken's resolution picked. Leaving its player out would let
// Gradle settle some shared library at an older version that graph never fetched, so Media3 stays
// on the classpath, unused (`money-apk-deps` re-records a graph of money's own, and then it can go).
dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")

    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.navigation:navigation-compose:2.8.5")
    implementation("androidx.core:core-ktx:1.15.0")
    // The sign-in in a Custom Tab, and the splash held over a launch.
    implementation("androidx.browser:browser:1.8.0")
    implementation("androidx.core:core-splashscreen:1.0.1")
    implementation("androidx.profileinstaller:profileinstaller:1.3.1")
    implementation("androidx.media3:media3-exoplayer:1.5.1")
    implementation("androidx.media3:media3-session:1.5.1")

    constraints {
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }

    // The JVM tests drive the real kernel: the library built for the machine they run on
    // (`host/`), through JNA's jar, and Android's `org.json` as the JVM has it.
    testImplementation("net.java.dev.jna:jna:5.17.0")
    testImplementation("org.json:json:20240303")
}

tasks.withType<Test>().configureEach {
    systemProperty("dev.arkdb.ui.strict", "true")
    systemProperty("jna.library.path", "$moneyLib/host")
}

// The screenshots (src/test/.../Screens.kt) are `recordPaparazziDebug`'s, which
// `money-android-screens` runs; the APK's build runs every other test, so a
// layoutlib that cannot draw a page holds back the screenshots and not the app.
val recording = gradle.startParameter.taskNames.any { it.contains("Paparazzi") }
tasks.withType<Test>().configureEach {
    if (!recording) filter { excludeTestsMatching("dev.money.android.Screens") }
}
