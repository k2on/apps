// The shared components' showcase for Android: an app of its own, made of the
// shared Compose (ark-ui, today `harken/android/.../ui/`, compiled from there
// as it is) and the showcase screen, and nothing else — no app's code, no
// kernel. The docs' captures are taken from it (.github/workflows/screens.yaml).
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

rootProject.name = "showcase"
include(":app")
