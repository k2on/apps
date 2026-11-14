// The prebuild self-test's fixture that declares `[background]` (ui/prebuild.py
// `selftest`: money's app.toml with every `[background]` key): its generated
// Application, with the background's hookup only such an app has, compiled
// against the app library (`:app`) and the kernel's Kotlin (`:bindings`), so
// what only `[background]` writes is held to compile. Nothing of it is
// carried anywhere: `prebuild.py fixture --kotlin fixture/generated` writes
// it, and nix compiles it with the library (`packages.ark-app-android`).
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val arkuiLib: String? = providers.gradleProperty("arkuiLib").orNull
val shotsLib: String? = providers.gradleProperty("shotsLib").orNull

android {
    namespace = "dev.arkui.fixture"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    defaultConfig { minSdk = 26 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
    kotlinOptions { jvmTarget = "21" }
    buildFeatures { compose = true }
    sourceSets { getByName("main") { kotlin.srcDir("generated") } }
}

dependencies {
    compileOnly(project(":app"))
    compileOnly(project(":bindings"))
    if (arkuiLib != null) compileOnly(fileTree(arkuiLib) { include("*.jar") }) else compileOnly(project(":arkui"))
    if (shotsLib != null) compileOnly(files("$shotsLib/shots.jar"))
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
