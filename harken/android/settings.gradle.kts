// harken for Android. The runtime and the client come from the Kotlin
// workspace beside the domain as a composite build: `includeBuild` keeps
// that build's own settings, plugin versions and toolchain, and Gradle
// substitutes `dev.arkdb:ark-runtime` and `dev.arkdb:ark-client` with the
// modules it finds there. A project reference (`include(":ark-runtime")`
// with a redirected `projectDir`) would work too but would drag those
// modules under this build's plugin management; the composite is the
// smaller lie.
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

includeBuild("../../kotlin")
include(":app")
