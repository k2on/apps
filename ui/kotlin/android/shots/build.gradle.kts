// ArkUI's screenshot mode and ready signal (ui/kotlin/shots): Android's
// framework and androidx.activity alone, so an app that draws nothing of
// ArkUI's (revelation) links it too. nix takes its classes as `shots.jar`
// (`packages.arkui-kotlin-shots`).
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.arkui.shots"
    compileSdk = 35
    // Said, or AGP asks for its own default and installs it into the SDK, a
    // read-only store path under nix.
    buildToolsVersion = "35.0.0"
    defaultConfig { minSdk = 26 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
    kotlinOptions { jvmTarget = "21" }
    sourceSets { getByName("main") { kotlin.srcDir(rootProject.file("../shots")) } }
}

dependencies {
    // Compiled against exactly what harken's app declares (its
    // app/build.gradle.kts; ui/prebuild.py's ANDROID_DEPENDENCIES and
    // money's own): the same roots resolve to the same versions, which the
    // apps' recorded graph (deps.json) holds every jar of. compileOnly:
    // the jar carries none of them, and every app that links it declares
    // them itself.
    compileOnly("net.java.dev.jna:jna:5.17.0@aar")
    compileOnly(platform("androidx.compose:compose-bom:2024.12.01"))
    compileOnly("androidx.compose.ui:ui")
    compileOnly("androidx.compose.material3:material3")
    compileOnly("androidx.compose.ui:ui-tooling-preview")
    compileOnly("androidx.activity:activity-compose:1.9.3")
    compileOnly("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    compileOnly("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    compileOnly("androidx.navigation:navigation-compose:2.8.5")
    compileOnly("androidx.core:core-ktx:1.15.0")
    compileOnly("androidx.browser:browser:1.8.0")
    compileOnly("androidx.core:core-splashscreen:1.0.1")
    compileOnly("androidx.profileinstaller:profileinstaller:1.3.1")
    compileOnly("androidx.media3:media3-exoplayer:1.5.1")
    compileOnly("androidx.media3:media3-session:1.5.1")
    constraints {
        compileOnly("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        compileOnly("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }
}
