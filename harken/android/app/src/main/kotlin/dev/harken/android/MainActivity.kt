// The activity: harken's screens inside the shared onboarding
// (`onboarding/`). It names only what is harken's — the name, the scheme its
// sign-in comes back on, the mark, the device this phone is in the
// account's listening session — and the one permission the
// player wants. `launchMode="singleTask"` and the
// `harken://auth` filter in the manifest are what bring the sign-in's code
// back here. The activity's theme is the splash's (res/values/themes.xml).
package dev.harken.android

import android.content.pm.ApplicationInfo
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.ui.res.painterResource
import dev.harken.android.onboarding.AppIdentity
import dev.harken.android.onboarding.ArkApp
import dev.harken.android.onboarding.Permission
import dev.harken.android.onboarding.holdSplash
import dev.arkui.Strict

/** Why harken asks for notifications, in the onboarding's words. */
const val WHY_NOTIFY = "So what's playing, and its controls, can be in a notification and on the lock screen."

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        // The system splash, kept until Home has what it draws (or 1.5 s): a remembered
        // launch opens on Home, populated, and never on a page that says it is opening.
        holdSplash()
        super.onCreate(savedInstanceState)
        // A debuggable build is the developer's: a mistake in how the screens use ark-ui throws
        // there, as in a test. A release says it once in the log and keeps working.
        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) Strict.on = true
        enableEdgeToEdge()
        setContent {
            val identity =
                AppIdentity(
                    name = "Harken",
                    scheme = "harken",
                    mark = painterResource(R.drawable.mark),
                    light = Palette.LightScheme,
                    dark = Palette.DarkScheme,
                    typography = HarkenType,
                    // A phone, which can play, named as its owner named it; its id is its login's
                    // session, so that a device is a login (docs/plan-live.md). The same two the
                    // media service opens the session with when it is the first to (Kept.kt).
                    device = Kept.device(this),
                )
            ArkApp(identity, listOf(Permission.Notifications(why = WHY_NOTIFY))) { session -> HarkenApp(session) }
        }
    }
}
