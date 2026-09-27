plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.harken.android"
    compileSdk = 35

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

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    sourceSets {
        getByName("main") {
            // The generated domain, referenced where arkc writes it — never copied.
            // Regenerate with:
            //   nix run /home/user/apps#arkc -- gen kotlin harken/domain/harken.ark \
            //     harken/domain/gen/kotlin --name Harken \
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
