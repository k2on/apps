// `ark-ui` (dev.arkdb.ui): the look and the motion an Ark app on Android is
// drawn with, and nothing about any app or any database — the springs and
// the curve every move follows, pages grown out of what was tapped and the
// back gesture that previews them, a card that expands under the finger,
// the derived square and the cover over it, the rows, shelves and headings
// a list is made of, and the onboarding's screens with no state of their
// own (kotlin/ark-ui/README.md). The Kotlin twin of `rust/arkui`.
//
// Compose only: no ArkDB, no network. `ark-android` depends on it and drives
// the onboarding's screens from its machine; an app depends on it for the
// rest. Like `ark-android` it is an Android library, so a project of the
// app's build (`include(":ark-ui")` with its `projectDir` here), under that
// build's plugin versions. Its tests are JVM unit tests
// (`:ark-ui:testDebugUnitTest`).
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.arkdb.ui"
    compileSdk = 35
    buildToolsVersion = "35.0.0"

    defaultConfig {
        minSdk = 26
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

    testOptions {
        unitTests.isReturnDefaultValues = false
    }
}

// Everything another module may call says `public`, as `ark-android` does by
// hand: here the compiler holds it to that.
kotlin {
    explicitApi()
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    api(composeBom)
    api("androidx.compose.ui:ui")
    api("androidx.compose.material3:material3")
    // The back dispatcher a page's gesture is said to, and the predictive back.
    api("androidx.activity:activity-compose:1.9.3")
    // A page's move is decided per back stack entry.
    api("androidx.navigation:navigation-compose:2.8.5")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    // The system splash, kept on screen until the app says (`holdSplashUntil`).
    api("androidx.core:core-splashscreen:1.0.1")

    testImplementation("junit:junit:4.13.2")

    // The Kotlin standard library's two old halves at the compiler's own
    // version, as every other module of the app's build resolves them. On its
    // own this module would take coroutines 1.7.3's word for 1.8.20, which no
    // build here otherwise asks for and the recorded graph does not hold.
    constraints {
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }
}

// A mistake in how an app uses this library throws in a test (`Strict`), where
// it is a failure to read, and not in a release, where it is a person's app.
tasks.withType<Test>().configureEach { systemProperty("dev.arkdb.ui.strict", "true") }
