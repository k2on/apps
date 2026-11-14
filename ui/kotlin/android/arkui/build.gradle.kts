// ArkUI for Android (ui/kotlin/src): Compose and Material 3, over `:shots`.
// nix takes its classes as `arkui.jar` (`packages.arkui-kotlin`), compiled
// against `shots.jar` (`-PshotsLib`, `packages.arkui-kotlin-shots`) rather
// than its sources, so an edit to one compiles that one and what links it.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val shotsLib: String? = providers.gradleProperty("shotsLib").orNull

android {
    namespace = "dev.arkui"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    defaultConfig { minSdk = 26 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
    kotlinOptions { jvmTarget = "21" }
    buildFeatures { compose = true }
    sourceSets { getByName("main") { kotlin.srcDir(rootProject.file("../src")) } }
}

dependencies {
    if (shotsLib != null) compileOnly(files("$shotsLib/shots.jar")) else api(project(":shots"))
    // Compiled against exactly what harken's app declares (its
    // app/build.gradle.kts; ui/prebuild.py's ANDROID_DEPENDENCIES and
    // money's own): the same roots resolve to the same versions, which the
    // apps' recorded graph (deps.json) holds every jar of. As
    // `implementation`, as the apps say them: AGP pins a compile classpath to
    // its runtime one's versions, and with compileOnly there was no runtime
    // one to pin to. The jar carries none of them all the same; every app
    // that links it declares them itself.
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
