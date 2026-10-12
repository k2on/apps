// `ark-android` (dev.arkdb.android): the onboarding every Ark app on Android
// shares — the server, signing in through the server's own page in a
// Custom Tab, the permissions — and the session it hands the app: the
// Kotlin `Peer` over that server, pumped on its own thread (kotlin/README.md).
//
// An Android library, so it is not a project of the Kotlin workspace here,
// which is the JVM's and builds without an SDK: an app's build includes it
// by path (`include(":ark-android")` with its `projectDir` here) and gets
// `ark-client` from that workspace as an included build. Its plugin
// versions are the app build's. The machine under the screens — the
// address, the health check, the remembered store, every transition — is
// plain Kotlin, and its tests are JVM unit tests (`testDebugUnitTest`).
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.arkdb.android"
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

dependencies {
    api("dev.arkdb:ark-client")

    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    api(composeBom)
    api("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    // The sign-in's Custom Tab.
    implementation("androidx.browser:browser:1.8.0")

    testImplementation("junit:junit:4.13.2")
}
