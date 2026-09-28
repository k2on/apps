plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

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

    // 21, as the runtime is: the domain calls the authoring vocabulary's
    // inline functions (`router<S>()`, `col<T, V>()`, `ctx.newId(..)`), and
    // Kotlin will not inline bytecode built for a newer JVM than the caller.
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
            // harken's domain in the authoring vocabulary, as `arkc gen kotlin`
            // prints it (package harken.gen), referenced where it is written —
            // never copied. The phone's print carries only what it calls:
            //   nix run .#arkc -- gen kotlin harken/domain/harken.ark \
            //     harken/domain/gen/kotlin --package harken.gen \
            //     --only create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlist_items
            kotlin.srcDir("../../domain/gen/kotlin")
        }
    }
}

dependencies {
    // Substituted from the included build at ../../kotlin.
    implementation("dev.arkdb:ark-runtime")
    implementation("dev.arkdb:ark-client")

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
