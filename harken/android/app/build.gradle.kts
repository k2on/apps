plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
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
        versionCode = 1
        versionName = "0.1"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
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

    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.navigation:navigation-compose:2.8.5")
    implementation("androidx.core:core-ktx:1.15.0")
}
