// Revelation for Android: one app project, all Kotlin. The text is the
// Berean Standard Bible as the asset it was published as
// (app/src/main/assets/bsb.txt), parsed at launch; the surface is one
// custom View (BibleView) and the chrome around it is Compose.
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

rootProject.name = "revelation-android"

include(":app")
