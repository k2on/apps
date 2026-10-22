plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    // Every screen rendered on the JVM through layoutlib, light and dark, from
    // a fixed library (src/test): `recordPaparazziDebug` writes the PNGs,
    // which `packages.harken-android-screens` lays out.
    id("app.cash.paparazzi")
}

/**
 * harken's library for the phone (`packages.harken-android-lib`): `jniLibs/<abi>/libharken_ffi.so`,
 * the Rust kernel with harken's domain linked in, and `kotlin/`, the Kotlin UniFFI writes for it with
 * the bridge's typed half (`dev.arkdb.ffi`) — but for the domain's own (`uniffi.harken_domain`), which
 * is checked in under `src/main/kotlin/.../gen/uniffi/`, where `ark-dev gen` writes it from the domain's
 * source, and which the library leaves out so it is not compiled twice. `harken-apk` passes the store
 * path as `-PharkenLib`; a laptop builds it once (`nix build .#harken-android-lib -o ../ffi/android`) and is pointed at that.
 */
val harkenLib: String = providers.gradleProperty("harkenLib").getOrElse(rootProject.file("../ffi/android").path)

android {
    namespace = "dev.harken.android"
    compileSdk = 35
    // Said explicitly, because AGP otherwise asks for its own default (34.0.0)
    // and installs it into the SDK — which, under nix, is a read-only store path.
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "dev.harken.android"
        minSdk = 26
        targetSdk = 35
        // Rises with every commit when nix builds it: `harken-apk` passes the
        // flake's source time in minutes as `-PversionCode`, and Android
        // refuses an update whose code is lower than the one installed. A
        // local build says neither and is 1, as it always was.
        versionCode = providers.gradleProperty("versionCode").map { it.toInt() }.getOrElse(1)
        versionName = providers.gradleProperty("versionName").getOrElse("0.1")
        // The ABIs the kernel is built for (`harken-android-lib`): JNA's own
        // libraries for any other are left out, so no phone installs an APK
        // it could not load harken on.
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }

    // One key for every build, committed beside this file, and public on
    // purpose. AGP otherwise signs with a key it makes up on first use
    // under ~/.android — a different one on every machine, and under nix a
    // different one on every build, since the builder's home is new each
    // time — and Android refuses an update whose signer differs from the
    // installed app's: "App not installed", and an uninstall that takes the
    // phone's replica with it. Signing identically everywhere is what lets
    // one APK install over the last.
    //
    // The release build signs with it too, and that is load-bearing: the
    // APKs already installed carry this key, and a release signed with any
    // other would not install over them. It is the same trade a debug key
    // makes, said plainly: anyone can read this file, so anyone can sign an
    // APK as this app. It must never sign anything published as trusted. A
    // real release key does not belong here or anywhere in the nix store,
    // which is world-readable; it means building unsigned and signing
    // outside the build.
    signingConfigs {
        getByName("debug") {
            storeFile = rootProject.file("debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        // For a phone on the desk and for the tests: the screenshots and the
        // unit tests run against this one.
        debug {
            signingConfig = signingConfigs.getByName("debug")
        }
        // What `harken-apk` ships. Not debuggable: ART runs a debuggable app
        // with its compiler held back for a debugger, and Compose is several
        // times slower for it — a fast fling through the songs most of all.
        // And with the baseline profiles Compose's own libraries ship, which
        // AGP compiles into the APK for a variant that is not debuggable and
        // profileinstaller hands to ART on install. Not minified: R8 is not
        // in the recorded Maven graph (deps.json), and shrinking is not what
        // a fling needs.
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    // Lint's release check would fetch lint itself, which the recorded graph
    // does not hold, and a build that cannot reach Maven fails on it.
    lint {
        checkReleaseBuilds = false
    }

    // 21, as the runtime and the client are built: D8 takes Java 21 class
    // files, and one target for the whole graph is one less thing to drift.
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
            // The kernel, per ABI, and the Kotlin over it.
            jniLibs.srcDir("$harkenLib/jniLibs")
            kotlin.srcDir("$harkenLib/kotlin")
        }
    }
}

dependencies {
    // What UniFFI's Kotlin calls the library through, with its native part
    // for every Android ABI.
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
    // The onboarding's: the server's sign-in in a Custom Tab, and the splash held over a launch.
    implementation("androidx.browser:browser:1.8.0")
    implementation("androidx.core:core-splashscreen:1.0.1")
    // Compose brings it already; named, because the release build relies on
    // it: it installs the APK's baseline profile, so the first fling after an
    // install is compiled rather than interpreted.
    implementation("androidx.profileinstaller:profileinstaller:1.3.1")
    // The player: ExoPlayer streaming /media/…, inside a MediaSessionService
    // whose session is the notification, the lock screen and Bluetooth.
    implementation("androidx.media3:media3-exoplayer:1.5.1")
    implementation("androidx.media3:media3-session:1.5.1")

    // The stdlib's jdk7 and jdk8 halves at the compiler's version, which the
    // recorded graph holds, rather than the 1.8 that coroutines ask for.
    constraints {
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }

    // The JVM tests drive the real kernel: the library built for the machine they run on
    // (`host/`), through JNA's jar, and Android's `org.json` as the JVM has it.
    testImplementation("net.java.dev.jna:jna:5.17.0")
    testImplementation("org.json:json:20240303")
}

// The look's guards against a mistake in how a screen uses it throw here
// (`ui.Strict`), as in a debuggable build; a release says them once in the
// log and keeps working.
tasks.withType<Test>().configureEach {
    systemProperty("dev.arkdb.ui.strict", "true")
    systemProperty("jna.library.path", "$harkenLib/host")
}
