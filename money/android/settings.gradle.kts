// money for Android: one app project. The kernel is Rust, linked in as
// `libmoney_ffi.so` beside the Kotlin UniFFI writes for it and the bridge's
// typed half (`packages.money-android-lib`; `moneyLib` in
// app/build.gradle.kts says where). The look every Ark app on Android shares
// is `kotlin/arkui`, compiled in from its sources; the screens are the app's.
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

rootProject.name = "money-android"

include(":app")
