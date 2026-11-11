// The apps' versions (harken/android/build.gradle.kts, ui/prebuild.py's
// ANDROID_ROOT): a library compiled by one Kotlin and Compose compiler is
// linked by apps compiled with the same. Paparazzi is applied nowhere here,
// but named, so the plugins' classpath resolves as the apps' does (its Kotlin
// is what lifts kotlin-stdlib-jdk7/jdk8 to 2.0.21) and the apps' recorded
// graph (deps.json) holds every jar of it.
plugins {
    id("com.android.library") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
    id("app.cash.paparazzi") version "1.3.5" apply false
}
