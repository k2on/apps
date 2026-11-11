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
    implementation("androidx.activity:activity:1.9.3")
    // The stdlib's jdk7 and jdk8 halves at the compiler's version, as the apps say.
    constraints {
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
        implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
    }
}
