// The same plugin versions as harken/android, so one toolchain builds both.
plugins {
    id("com.android.application") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
    // The surface, rendered on the JVM at each level of zoom (src/test, Screens.kt).
    id("app.cash.paparazzi") version "1.3.5" apply false
}
