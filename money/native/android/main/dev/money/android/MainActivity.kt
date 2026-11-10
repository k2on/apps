// The activity: the sign-in machine's steps (Start.kt), then the budget. It opens the browser the
// sign-in asks for (a Custom Tab), hands the code that comes back to
// `money://auth` — as a new intent, or as the launch intent when the system
// reclaimed the app behind the tab — and tells the book when the app comes to
// the front, so a server that is there is dialled before a screen says it is
// not. `launchMode="singleTask"` and the `money://auth` filter in the manifest
// are what bring the code back here. The splash is held until the first
// screen has what it draws.
package dev.money.android

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import dev.arkdb.auth.Event
import dev.arkdb.auth.Phase
import dev.arkdb.auth.Said
import dev.arkdb.auth.Step
import dev.arkui.ConfirmScreen
import dev.arkui.DeviceLinkShown
import dev.arkui.OpeningScreen
import dev.arkui.ServerScreen
import dev.arkui.SignInPhase
import dev.arkui.SignInScreen
import dev.arkui.holdSplashUntil
import dev.arkui.keepMotion
import dev.arkui.server.Probe

class MainActivity : ComponentActivity() {
    private val start: Start by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        holdSplashUntil {
            when (start.step.value) {
                is Step.Running, is Step.Demo -> start.book.value?.state?.value?.ready ?: false
                else -> true
            }
        }
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        eventOf(intent)?.let {
            intent = Intent(intent).setData(null)
            start.send(it)
        }
        addOnNewIntentListener { i -> eventOf(i)?.let { start.send(it) } }
        // Compose on arkui's duration scale: "Remove animations" is the app's to interpret
        // (`dev.arkui.ReducedMotion`), not a jump to every animation's end.
        keepMotion()
        setContent {
            MoneyTheme {
                Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
                    Root(start)
                }
            }
        }
    }

    /**
     * What a link the app was opened with means: `money://auth?code=…` a sign-in coming back,
     * `money://join?…` a server (and a code) to join, asked about first; null for any other. The
     * join is read as the domain's route (`Route.Join`), as every shell reads it.
     */
    private fun eventOf(i: Intent?): Event? {
        val data = i?.data ?: return null
        if (data.scheme != MONEY.scheme) return null
        if (data.host == "auth") return data.getQueryParameter("code")?.takeIf { it.isNotEmpty() }?.let { Event.Code(it) }
        val join = dev.arkui.Navigator.routeOf(data.toString(), MONEY.scheme, { uniffi.money_core.routeFromPath(it) }) as? uniffi.money_core.Route.Join
        return join?.let { Event.Joined(it.server, it.code) }
    }
}

@Composable
private fun Root(start: Start) {
    val step by start.step.collectAsStateWithLifecycle()
    val said by start.said.collectAsStateWithLifecycle()
    val book by start.book.collectAsStateWithLifecycle()
    val browse by start.browse.collectAsStateWithLifecycle()
    val context = androidx.compose.ui.platform.LocalContext.current
    LaunchedEffect(browse) {
        val url = browse ?: return@LaunchedEffect
        start.browsed()
        CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url))
    }
    // Back in front after it was away — from the browser tab, or from elsewhere: the machine
    // decides which (`Event.Foreground`), and the book dials now.
    var away by remember { mutableStateOf(false) }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { away = true }
    LifecycleEventEffect(Lifecycle.Event.ON_START) {
        if (away) {
            away = false
            start.send(Event.Foreground)
        }
    }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { book?.dialNow() }
    StartScreens(step, said, book, start::send, onSignOut = start::signOut, addDevice = start::deviceLink)
}

/** Each step drawn: arkui's onboarding pages with money's words, or the budget. */
@Composable
fun StartScreens(
    step: Step,
    said: Said,
    book: Book?,
    send: (Event) -> Unit,
    onSignOut: () -> Unit,
    addDevice: suspend () -> DeviceLinkShown = { DeviceLinkShown(null, 0, emptyList(), "Sign in first.") },
) {
    val mark = painterResource(R.drawable.mark)
    when (step) {
        is Step.Server ->
            ServerScreen(
                name = "money",
                mark = mark,
                typed = step.typed,
                error = step.error,
                checking = step.checking,
                onType = { send(Event.Typed(it)) },
                onContinue = { send(Event.Check) },
                sentence = "A budget the household shares, kept on your own server. Where is it?",
                placeholder = DEFAULT_SERVER,
            ) {
                TextButton(onClick = { send(Event.TryDemo) }, enabled = !step.checking, modifier = Modifier.fillMaxWidth()) {
                    Text("Try the demo")
                }
            }
        is Step.SignIn ->
            SignInScreen(
                mark = mark,
                host = Probe.display(step.server),
                error = step.error,
                phase = step.phase.screen(),
                onSignIn = { send(Event.OpenSignIn) },
                onChangeServer = { send(Event.ChangeServer) },
            )
        is Step.Confirm ->
            ConfirmScreen(
                mark = mark,
                question = said.question,
                sentence = step.error ?: said.status ?: "",
                canJoin = step.error == null,
                onJoin = { send(Event.Accept) },
                onCancel = { send(Event.Decline) },
            )
        is Step.Opening -> {
            OpeningScreen(name = "money", mark = mark, host = Probe.display(step.server), what = "your budget")
            // Signed in just now: opening, until the replica has what the server had — or the
            // server cannot be reached, and there is nothing more to wait for.
            val b = book ?: return
            val ledger by b.state.collectAsStateWithLifecycle()
            val synced = ledger.ready && (ledger.link.linked || ledger.link.word == "waiting" || ledger.link.word == "offline")
            LaunchedEffect(synced) { if (synced) send(Event.Synced) }
            LaunchedEffect(ledger.denied) { ledger.denied?.let { send(Event.Denied(it)) } }
        }
        is Step.Running, is Step.Demo -> {
            val b = book ?: return
            val ledger by b.state.collectAsStateWithLifecycle()
            LaunchedEffect(ledger.denied) { ledger.denied?.let { send(Event.Denied(it)) } }
            MoneyApp(b, onSignOut = onSignOut, addDevice = addDevice)
        }
    }
}

/** A sign-in's phase, as arkui's page draws it. */
fun Phase.screen(): SignInPhase =
    when (this) {
        Phase.IDLE -> SignInPhase.Idle
        Phase.BROWSER -> SignInPhase.Browser
        Phase.EXCHANGING -> SignInPhase.Exchanging
    }
