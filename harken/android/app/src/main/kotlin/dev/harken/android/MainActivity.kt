// The activity: harken's screens inside the shared onboarding
// (`kotlin/ark-android`). It names only what is harken's — the name, the
// scheme its sign-in comes back on, the mark, the module, the device this
// phone is in the account's listening session — and the one permission the
// player wants. `launchMode="singleTask"` and the
// `harken://auth` filter in the manifest are what bring the sign-in's code
// back here. The activity's theme is the splash's (res/values/themes.xml).
package dev.harken.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.ui.res.painterResource
import dev.arkdb.DeviceDecl
import dev.arkdb.android.AppIdentity
import dev.arkdb.android.ArkApp
import dev.arkdb.android.Permission
import dev.arkdb.android.holdSplash

/** Why harken asks for notifications, in the onboarding's words. */
const val WHY_NOTIFY = "So what's playing, and its controls, can be in a notification and on the lock screen."

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        // The system splash, kept until Home has what it draws (or 1.5 s): a remembered
        // launch opens on Home, populated, and never on a page that says it is opening.
        holdSplash()
        super.onCreate(savedInstanceState)
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
                    // session, so that a device is a login (docs/plan-live.md).
                    device = DeviceDecl("", "phone", Device.name(this), true),
                    module = { assets.open("harken.ark").use { it.readBytes() } },
                )
            ArkApp(identity, listOf(Permission.Notifications(why = WHY_NOTIFY))) { session -> HarkenApp(session) }
        }
    }
}
