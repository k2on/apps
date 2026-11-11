// The apps' versions (harken/android/build.gradle.kts, ui/prebuild.py's
// ANDROID_ROOT): a library compiled by one Kotlin and Compose compiler is
// linked by apps compiled with the same.
plugins {
    id("com.android.library") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
