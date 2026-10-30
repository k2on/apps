// The activity: the start's steps, then the budget. It opens the browser the
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
import dev.arkui.ServerScreen
import dev.arkui.SignInScreen
import dev.arkui.holdSplashUntil
import dev.arkui.server.Probe

class MainActivity : ComponentActivity() {
    private val start: Start by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        holdSplashUntil {
            when (val s = start.starter.step.value) {
                is Step.Running -> s.book.state.value.ready
                else -> true
            }
        }
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        codeOf(intent)?.let {
            intent = Intent(intent).setData(null)
            start.starter.code(it)
        }
        addOnNewIntentListener { i -> codeOf(i)?.let { start.starter.code(it) } }
        setContent {
            MoneyTheme {
                Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
                    Root(start)
                }
            }
        }
    }

    /** The code in `money://auth?code=…`, or null for any other intent. */
    private fun codeOf(i: Intent?): String? {
        val data = i?.data ?: return null
        if (data.scheme != SCHEME || data.host != "auth") return null
        return data.getQueryParameter("code")?.takeIf { it.isNotEmpty() }
    }
}

@Composable
private fun Root(start: Start) {
    val step by start.starter.step.collectAsStateWithLifecycle()
    val browse by start.browse.collectAsStateWithLifecycle()
    val context = androidx.compose.ui.platform.LocalContext.current
    LaunchedEffect(browse) {
        val url = browse ?: return@LaunchedEffect
        start.browsed()
        CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url))
    }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        start.starter.returned()
        (start.starter.step.value as? Step.Running)?.book?.dialNow()
    }
    StartScreens(
        step,
        onType = start.starter::type,
        onContinue = start.starter::check,
        onDemo = start.starter::demo,
        onSignIn = start.starter::openSignIn,
        onChangeServer = start.starter::changeServer,
        onSignOut = { start.starter.signOut() },
    )
}

/** Each step drawn: arkui's onboarding pages with money's words, or the budget. */
@Composable
fun StartScreens(
    step: Step,
    onType: (String) -> Unit,
    onContinue: () -> Unit,
    onDemo: () -> Unit,
    onSignIn: () -> Unit,
    onChangeServer: () -> Unit,
    onSignOut: () -> Unit,
) {
    val mark = painterResource(R.drawable.mark)
    when (step) {
        is Step.Welcome ->
            ServerScreen(
                name = "money",
                mark = mark,
                typed = step.typed,
                error = step.error,
                checking = step.checking,
                onType = onType,
                onContinue = onContinue,
                sentence = "A budget the household shares, kept on your own server. Where is it?",
                placeholder = DEFAULT_SERVER,
            ) {
                TextButton(onClick = onDemo, enabled = !step.checking, modifier = Modifier.fillMaxWidth()) {
                    Text("Try the demo")
                }
            }
        is Step.SignIn ->
            SignInScreen(
                mark = mark,
                host = Probe.display(step.server),
                error = step.error,
                phase = step.phase,
                onSignIn = onSignIn,
                onChangeServer = onChangeServer,
            )
        is Step.Running -> {
            val ledger by step.book.state.collectAsStateWithLifecycle()
            LaunchedEffect(ledger.denied) {
                if (ledger.denied != null) onSignOut()
            }
            MoneyApp(step.book, onSignOut = onSignOut)
        }
    }
}
