// The kernel's Kotlin, to compile the app library (`:app`) and the prebuild
// self-test's fixture (`:fixture`) against and never to carry: ark-ffi's
// (`dev.arkdb.ffi`, `dev.arkdb.auth`, its typed half) and the modules'
// (`uniffi.ark_*`) as UniFFI writes them for an app's library — nix passes
// money's library's `kotlin/` as `-ParkBindings` (the ffi check's), whose
// `MoneyHost` the fixture, money's app.toml with `[background]`, names — and
// the modules' own Kotlin an app that declares `[background]` hands the
// library (ark-notifications', ark-background's) and the secure store's.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val arkBindings: String = providers.gradleProperty("arkBindings").getOrElse(rootProject.file("../../../money/.native/ffi/android/kotlin").path)

android {
    namespace = "dev.arkui.bindings"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    defaultConfig { minSdk = 26 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
    kotlinOptions { jvmTarget = "21" }
    buildFeatures { compose = true }
    sourceSets {
        getByName("main") {
            kotlin.srcDir(file(arkBindings))
            // The bridge's typed half (`Id`), which UniFFI's Kotlin names.
            kotlin.srcDir(rootProject.file("../../../rust/ark-ffi/kotlin"))
            for (m in listOf("ark-notifications", "ark-background", "ark-secure-store")) {
                kotlin.srcDir(rootProject.file("../../../modules/$m/kotlin"))
            }
        }
    }
}

dependencies {
    // ArkUI's own roots, every one (arkui/build.gradle.kts): the same roots resolve to the
    // same versions, which the recorded graph (deps.json) holds every jar of.
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.navigation:navigation-compose:2.8.5")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.browser:browser:1.8.0")
    implementation("androidx.core:core-splashscreen:1.0.1")
    implementation("androidx.profileinstaller:profileinstaller:1.3.1")
    implementation("androidx.media3:media3-exoplayer:1.5.1")
    implementation("androidx.media3:media3-session:1.5.1")
    constraints {
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }
}
