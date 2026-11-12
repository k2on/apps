// The activity every Ark app shows itself in: its app ([ArkApp], the process's), the system splash
// held until the first screen has what it draws, a link the app was opened with or sent handed to
// the host — `<scheme>://auth?code=…` the sign-in coming back, as a new intent or as the launch
// intent when the system reclaimed the app behind the tab — the sign-in opened in a Custom Tab,
// and the app coming back to the front said, so a server that is there is dialled before a screen
// says it is not. `launchMode="singleTask"` and the scheme's filters in the generated manifest are
// what bring a link back here. What it draws is the app's ([Content]).
package dev.arkui.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.arkui.Shots
import dev.arkui.holdSplashUntil
import dev.arkui.keepMotion

public abstract class ArkActivity : ComponentActivity() {
    /** What the app draws over its app: its first run's pages, then itself. */
    @Composable protected abstract fun Content(app: ArkApp)

    override fun onCreate(savedInstanceState: Bundle?) {
        // Whether this is a screenshot's launch, before the app is made: it goes past the app's pages.
        Shots.read(intent)
        val ark = application as ArkApplication
        val app = ark.launch(intent)
        holdSplashUntil { app.ready() }
        super.onCreate(savedInstanceState)
        // Its screens attached to the process's runtime while it lives (across a rotation).
        ViewModelProvider(this)[Attachment::class.java]
        enableEdgeToEdge()
        ark.background?.opened(intent)
        intent?.data?.let { app.open(it.toString()) }
        // Once: the launch intent is still the activity's when it is recreated.
        if (intent?.data != null) intent = Intent(intent).setData(null)
        addOnNewIntentListener { i ->
            ark.background?.opened(i)
            i.data?.let { app.open(it.toString()) }
        }
        // Compose on arkui's duration scale: "Remove animations" is the app's to interpret
        // (`dev.arkui.ReducedMotion`), not a jump to every animation's end.
        keepMotion()
        setContent {
            Frame(app)
            Content(app)
        }
        // The window is there: the launch's screen may open.
        app.shown()
    }
}

/**
 * The activity's screens attached to the process's runtime ([dev.arkdb.ffi.ArkProcess]): online
 * while they show, as the iPhone's app is for its life (ArkApp.swift), and let go when the activity
 * is finished, not when it is only recreated. Without it the session's peer never dialled: money's
 * server session never linked, never settled, and a screenshot never ready (app-screens
 * 43168945655, "not ready (after generation -1)" after signing in).
 */
internal class Attachment : ViewModel() {
    private val attached = dev.arkdb.ffi.ArkProcess.shared().attach()

    override fun onCleared() {
        attached.detach()
    }
}

/** What every app's activity does around what it draws: the sign-in's tab, the app's phase. */
@Composable
private fun Frame(app: ArkApp) {
    val browse by app.browse.collectAsStateWithLifecycle()
    val context = LocalContext.current
    LaunchedEffect(browse) {
        val url = browse ?: return@LaunchedEffect
        app.browsed()
        // Custom Tabs, never a WebView: the provider's session is the browser's, so signing in
        // to the next app is one tap.
        CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url))
    }
    // Back in front after it was away — from the browser tab, or from elsewhere: the host decides
    // which, and dials now.
    var away by remember { mutableStateOf(false) }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { away = true }
    LifecycleEventEffect(Lifecycle.Event.ON_START) {
        if (away) {
            away = false
            app.foreground()
        }
    }
}
