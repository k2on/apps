// The apps' versions (harken/android/build.gradle.kts, ui/prebuild.py's
// ANDROID_ROOT): a library compiled by one Kotlin and Compose compiler is
// linked by apps compiled with the same. Paparazzi is applied nowhere here,
// but named: its Kotlin is what lifts the plugins' kotlin-stdlib-jdk7/jdk8 to
// 2.0.21, the version the recorded graph (deps.json) holds the jars of.
plugins {
    id("com.android.library") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
    id("app.cash.paparazzi") version "1.3.5" apply false
}
