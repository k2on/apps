// The system splash, held over a remembered launch until the app's first
// screen has what it draws — so that what lifts it is Home, populated, and
// nothing in between is a loading page. Android 12's SplashScreen, through
// `androidx.core:core-splashscreen`, which draws the same splash back to
// API 23.
//
//     override fun onCreate(savedInstanceState: Bundle?) {
//         holdSplash()                      // before super.onCreate
//         super.onCreate(savedInstanceState)
//         setContent { ArkApp(identity, permissions) { session -> … } }
//     }
//
// The activity's theme is a `Theme.SplashScreen` naming the splash's ground,
// its icon and `postSplashScreenTheme`, the theme the app draws in after.
//
// What it waits for is one state, [ArkHost.ready]: true as soon as the
// onboarding has a step to draw (there is no splash to hold over a server
// field), and over a session once the app says its first screen has what
// it draws ([ArkSession.ready]) — which spans the module, the replica's
// open and the first publish as one wait. With a ceiling: past
// `ceilingMs` the splash goes whatever is ready, and the app shows with
// what it has — a shell that fills in — rather than holding the phone
// hostage to a slow open.
package dev.arkdb.android

import android.os.SystemClock
import androidx.activity.ComponentActivity
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.lifecycle.ViewModelProvider

/** How long the splash may be held for, at most. */
public const val SPLASH_CEILING_MS: Long = 1500

/**
 * Install the system splash and keep it until [ArkHost.ready], or `ceilingMs` — called before
 * `super.onCreate`.
 */
public fun ComponentActivity.holdSplash(ceilingMs: Long = SPLASH_CEILING_MS) {
    val splash = installSplashScreen()
    val since = SystemClock.uptimeMillis()
    var host: ArkHost? = null
    splash.setKeepOnScreenCondition {
        // Asked on every frame the splash would let through, from the first, after
        // `onCreate`: the host is the one `ArkApp` holds, from the same store.
        val h = host ?: ViewModelProvider(this)[ArkHost::class.java].also { host = it }
        keepsSplash(h.ready.value, SystemClock.uptimeMillis() - since, ceilingMs)
    }
}

/** Whether the splash stays: nothing ready yet, and the ceiling not reached. */
internal fun keepsSplash(ready: Boolean, elapsedMs: Long, ceilingMs: Long): Boolean =
    !ready && elapsedMs < ceilingMs
