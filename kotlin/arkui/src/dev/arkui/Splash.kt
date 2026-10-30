// The system splash, held until the app's first screen has what it draws —
// so that what lifts it is that screen, populated, and nothing in between is
// a loading page. Android 12's SplashScreen, through
// `androidx.core:core-splashscreen`, which draws the same splash back to
// API 23.
//
//     override fun onCreate(savedInstanceState: Bundle?) {
//         holdSplashUntil { ready }           // before super.onCreate
//         super.onCreate(savedInstanceState)
//         …
//     }
//
// The activity's theme is a `Theme.SplashScreen` naming the splash's ground,
// its icon and `postSplashScreenTheme`, the theme the app draws in after.
// What it waits for is the app's to say; `ark-android`'s `holdSplash` waits
// for its host to be ready. With a ceiling: past `ceilingMs` the splash goes
// whatever is ready, and the app shows with what it has — a shell that fills
// in — rather than holding the phone hostage to a slow open.
package dev.harken.android.ui

import android.os.SystemClock
import androidx.activity.ComponentActivity
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen

/** How long the splash may be held for, at most. */
public const val SPLASH_CEILING_MS: Long = 1500

/**
 * Install the system splash and keep it until `ready` says so, or `ceilingMs` — called before
 * `super.onCreate`. `ready` is asked on every frame the splash would let through, from the first,
 * after `onCreate`.
 */
public fun ComponentActivity.holdSplashUntil(
    ceilingMs: Long = SPLASH_CEILING_MS,
    ready: () -> Boolean
) {
    val splash = installSplashScreen()
    val since = SystemClock.uptimeMillis()
    splash.setKeepOnScreenCondition {
        keepsSplash(ready(), SystemClock.uptimeMillis() - since, ceilingMs)
    }
}

/** Whether the splash stays: nothing ready yet, and the ceiling not reached. */
public fun keepsSplash(ready: Boolean, elapsedMs: Long, ceilingMs: Long): Boolean =
    !ready && elapsedMs < ceilingMs
