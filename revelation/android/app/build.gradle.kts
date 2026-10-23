plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    // The surface rendered on the JVM through layoutlib at six camera states,
    // light and dark (src/test, Screens.kt): `recordPaparazziDebug` writes the PNGs.
    id("app.cash.paparazzi")
}

android {
    namespace = "dev.revelation.android"
    compileSdk = 35
    // Said explicitly, because AGP otherwise asks for its own default (34.0.0)
    // and installs it into the SDK — which, under nix, is a read-only store path.
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "dev.revelation.android"
        minSdk = 26
        targetSdk = 35
        // Rises with every commit when nix builds it: the derivation passes
        // the flake's source time in minutes as `-PversionCode`, and Android
        // refuses an update whose code is lower than the one installed. A
        // local build says neither and is 1.
        versionCode = providers.gradleProperty("versionCode").map { it.toInt() }.getOrElse(1)
        versionName = providers.gradleProperty("versionName").getOrElse("0.1")
    }

    // One key for every build, committed beside this file, and public on
    // purpose. AGP otherwise signs with a key it makes up on first use
    // under ~/.android — a different one on every machine, and under nix a
    // different one on every build, since the builder's home is new each
    // time — and Android refuses an update whose signer differs from the
    // installed app's: "App not installed", and an uninstall that takes the
    // saved place in the text with it. Signing identically everywhere is
    // what lets one APK install over the last.
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
        // What ships. Not debuggable: ART runs a debuggable app with its
        // compiler held back for a debugger, and the layout pass over the
        // whole text at launch is several times slower for it. Not minified:
        // shrinking is not what a fling needs, and R8 would be one more
        // artifact for the recorded Maven graph to hold.
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    // Lint's release check would fetch lint itself, which a recorded graph
    // does not hold, and a build that cannot reach Maven fails on it.
    lint {
        checkReleaseBuilds = false
    }

    // 21, as harken is built: D8 takes Java 21 class files, and one target
    // for the whole graph is one less thing to drift.
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
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
    // Compose brings it already; named, because the release build relies on
    // it: it installs the APK's baseline profile, so the first launch is
    // compiled rather than interpreted.
    implementation("androidx.profileinstaller:profileinstaller:1.3.1")

    // The stdlib's jdk7 and jdk8 halves at the compiler's version, rather
    // than the older ones coroutines ask for.
    constraints {
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }

    testImplementation("junit:junit:4.13.2")
}

// The text is parsed and laid out in full by several tests, and the
// screenshots hold layoutlib besides: more than the default half gigabyte.
tasks.withType<Test>().configureEach {
    maxHeapSize = "2g"
}
