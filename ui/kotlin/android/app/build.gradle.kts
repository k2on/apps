// The shared app library (ui/kotlin/app, package `dev.arkui.app`): what every
// Ark app on Android is underneath its screens — the process's app host owned
// by the app's Application, the activity, the browser sign-in, the links, the
// secure store's hookup, the background's hookup and the screens' shared
// holder. nix takes its classes as `app.jar` (`packages.ark-app-android`),
// which every Ark app links beside ArkUI's.
//
// It is written over the kernel's Kotlin (ark-ffi's `dev.arkdb.ffi`, the
// modules' `uniffi.ark_*`), which every app's own library carries, generated
// for that library: compiled against one copy of it (`:bindings`), linked
// against the app's at run time, so none of it is in the jar.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val shotsLib: String? = providers.gradleProperty("shotsLib").orNull
val arkuiLib: String? = providers.gradleProperty("arkuiLib").orNull

android {
    namespace = "dev.arkui.app"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    defaultConfig { minSdk = 26 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
    kotlinOptions { jvmTarget = "21" }
    buildFeatures { compose = true }
    sourceSets { getByName("main") { kotlin.srcDir(rootProject.file("../app")) } }
}

dependencies {
    // ArkUI's jars when nix built them, its projects otherwise; the kernel's
    // Kotlin to compile against, never to carry.
    if (arkuiLib != null) compileOnly(fileTree(arkuiLib) { include("*.jar") }) else compileOnly(project(":arkui"))
    if (shotsLib != null) compileOnly(files("$shotsLib/shots.jar"))
    compileOnly(project(":bindings"))
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
