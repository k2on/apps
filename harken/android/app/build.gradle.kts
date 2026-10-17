plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    // Every screen rendered on the JVM through layoutlib, light and dark, from
    // a fixed library (src/test): `recordPaparazziDebug` writes the PNGs,
    // which `packages.harken-android-screens` lays out.
    id("app.cash.paparazzi")
}

/** Where `harken.ark` is copied for the APK's assets. */
val arkAssets: File = layout.buildDirectory.dir("generated/ark-assets").get().asFile

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
            // harken's module, `harken.ark`, as the domain crate writes it and
            // the repository commits it: the app loads it from its assets,
            // verifies it, and interprets it (docs/plan-phones.md). Copied in
            // at build time from where it is written, never committed twice.
            assets.srcDir(arkAssets)
        }
    }
}

val copyArk by tasks.registering(Copy::class) {
    from(rootProject.file("../domain/harken.ark"))
    into(arkAssets)
}

tasks.named("preBuild") {
    dependsOn(copyArk)
}

dependencies {
    // Substituted from the included build at ../../kotlin: the runtime that
    // interprets the module, and the client — storage, the link, sign-in,
    // maintained views.
    implementation("dev.arkdb:ark-runtime")
    implementation("dev.arkdb:ark-client")
    // The onboarding every Ark app on Android shares, and the session it opens
    // (kotlin/ark-android, a project of this build).
    implementation(project(":ark-android"))
    // What it looks like and how it moves, where that is not harken's own: the
    // springs, the pages grown out of their art, the expanding card the player
    // is, the derived square and the rows (kotlin/ark-ui).
    implementation(project(":ark-ui"))

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
    // Compose brings it already; named, because the release build relies on
    // it: it installs the APK's baseline profile, so the first fling after an
    // install is compiled rather than interpreted.
    implementation("androidx.profileinstaller:profileinstaller:1.3.1")
    // The player: ExoPlayer streaming /media/…, inside a MediaSessionService
    // whose session is the notification, the lock screen and Bluetooth.
    implementation("androidx.media3:media3-exoplayer:1.5.1")
    implementation("androidx.media3:media3-session:1.5.1")
}

// `ark-ui`'s guards against a mistake in how the app uses it throw here
// (`dev.arkdb.ui.Strict`), as in a debuggable build; a release says them once
// in the log and keeps working.
tasks.withType<Test>().configureEach { systemProperty("dev.arkdb.ui.strict", "true") }
