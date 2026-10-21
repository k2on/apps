// harken for Android: one app project. The kernel is Rust, linked in as
// `libharken_ffi.so` beside the Kotlin UniFFI writes for it and the bridge's
// typed half (`packages.harken-android-lib`; `harkenLib` in
// app/build.gradle.kts says where), and the onboarding and the look every
// screen is drawn with are the app's own code (`onboarding/`, `ui/`).
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "harken-android"

include(":app")
