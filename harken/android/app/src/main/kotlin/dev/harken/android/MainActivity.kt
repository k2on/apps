// The activity: harken's screens inside the shared onboarding
// (`kotlin/ark-android`). It names only what is harken's — the name, the
// scheme its sign-in comes back on, the mark, the module — and the one
// permission the player wants. `launchMode="singleTask"` and the
// `harken://auth` filter in the manifest are what bring the sign-in's code
// back here.
package dev.harken.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import dev.arkdb.android.AppIdentity
import dev.arkdb.android.ArkApp
import dev.arkdb.android.Permission

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val identity =
                AppIdentity(
                    name = "Harken",
                    scheme = "harken",
                    mark = painterResource(R.drawable.mark),
                    light = lightColorScheme(primary = Color(0xFF9A741A), onPrimary = Color.White),
                    dark = darkColorScheme(primary = Color(0xFFE9BB45), onPrimary = Color(0xFF1E1E1E)),
                    module = { assets.open("harken.ark").use { it.readBytes() } },
                )
            ArkApp(identity, listOf(Permission.Notifications(why = "Show what's playing, with its controls, in a notification and on the lock screen."))) { session ->
                val model = remember(session) { Model(session) }
                DisposableEffect(model) { onDispose { model.close() } }
                HarkenApp(model)
            }
        }
    }
}
