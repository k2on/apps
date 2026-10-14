// `ArkApp`: the onboarding every Ark app on Android shares, and the app
// once it is behind it.
//
//     ArkApp(identity, permissions) { session -> TheAppsScreens(session) }
//
// What an app names is only what is its own — its name, the scheme its
// sign-in comes back on, its mark, its colours, its module — and the
// permissions it wants with a sentence each. The rest is here.
//
// **The redirect is the app's manifest's.** The sign-in opens the server's
// `/auth/login?redirect=<scheme>://auth` in a Custom Tab, and the server
// sends the browser to `<scheme>://auth?code=…`. For that to come back to
// the activity running `ArkApp`, the app's manifest declares it, on that
// activity, with `launchMode="singleTask"` so the existing activity gets it
// (`onNewIntent`) rather than a second one being started over the tab:
//
//     <activity android:name=".MainActivity" android:launchMode="singleTask" …>
//         <intent-filter>
//             <action android:name="android.intent.action.VIEW" />
//             <category android:name="android.intent.category.DEFAULT" />
//             <category android:name="android.intent.category.BROWSABLE" />
//             <data android:scheme="harken" android:host="auth" />
//         </intent-filter>
//     </activity>
//
// The server has to allow that redirect too: `ark-auth` sends a code only
// to loopback, its own origin and the prefixes it is configured with
// (harken's server allows `harken://`). A code that arrives while the app
// was killed — the person signed in, and the system reclaimed the app
// behind the tab — arrives as the launch intent instead, and is exchanged
// the same way: the server step was remembered before the tab opened.
package dev.arkdb.android

import android.app.Activity
import android.content.Intent
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.platform.LocalContext
import androidx.core.util.Consumer
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel

/**
 * Who the app is, as the onboarding draws it: its name, the scheme its sign-in comes back on
 * (`<scheme>://auth`), its mark, its colours in a light and a dark scheme and its type, and its
 * module — the `.ark` the session's peer interprets, read once.
 */
public class AppIdentity(
    public val name: String,
    public val scheme: String,
    public val mark: Painter,
    public val light: ColorScheme,
    public val dark: ColorScheme,
    public val typography: Typography = Typography(),
    public val module: () -> ByteArray,
)

/**
 * The onboarding, then `app` with the session it opened. The theme is the identity's, light or dark
 * as the system is, and wraps `app` too.
 */
@Composable
public fun ArkApp(
    identity: AppIdentity,
    permissions: List<Permission>,
    app: @Composable (ArkSession) -> Unit
) {
    val host: ArkHost = viewModel()
    host.begin(identity, permissions)
    val step by host.step.collectAsStateWithLifecycle()
    val session by host.session.collectAsStateWithLifecycle()
    val browse by host.browse.collectAsStateWithLifecycle()
    val request by host.request.collectAsStateWithLifecycle()
    val context = LocalContext.current

    // The redirect: as the launch intent, and as every new intent after it.
    val activity = context as? ComponentActivity
    DisposableEffect(activity) {
        activity
            ?.intent
            ?.let { codeOf(it, identity.scheme) }
            ?.let {
                activity.intent = Intent(activity.intent).setData(null)
                host.send(Event.Code(it))
            }
        val listener =
            Consumer<Intent> { i -> codeOf(i, identity.scheme)?.let { host.send(Event.Code(it)) } }
        activity?.addOnNewIntentListener(listener)
        onDispose { activity?.removeOnNewIntentListener(listener) }
    }

    // Back from the tab with no code: the person closed it.
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val o = LifecycleEventObserver { _, e ->
            if (e == Lifecycle.Event.ON_RESUME) host.send(Event.Returned)
        }
        owner.lifecycle.addObserver(o)
        onDispose { owner.lifecycle.removeObserver(o) }
    }

    LaunchedEffect(browse) {
        val url = browse ?: return@LaunchedEffect
        host.browsed()
        CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url))
    }

    val ask =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
            host.send(Event.Answered)
        }
    LaunchedEffect(request) {
        val p = request ?: return@LaunchedEffect
        host.requested()
        if (context is Activity) ask.launch(p.android) else host.send(Event.Answered)
    }

    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) identity.dark else identity.light,
        typography = identity.typography) {
            val s = step
            val open = session
            when {
                s is Step.Running && open != null -> app(open)
                // The session is made with the step, so this is a frame at most; the
                // splash is over it. Never a page that says it is opening.
                s is Step.Running ->
                    Surface(
                        color = MaterialTheme.colorScheme.background,
                        modifier = Modifier.fillMaxSize()) {}
                s is Step.Opening -> Opening(identity, s.server)
                s is Step.Server ->
                    ServerStep(
                        identity,
                        s,
                        onType = { host.send(Event.Typed(it)) },
                        onContinue = { host.send(Event.Check) })
                s is Step.SignIn ->
                    SignInStep(
                        identity,
                        s,
                        onSignIn = { host.send(Event.OpenSignIn) },
                        onChangeServer = { host.send(Event.ChangeServer) })
                s is Step.Ask ->
                    PermissionStep(
                        identity,
                        s,
                        onAllow = { host.send(Event.Allow) },
                        onNotNow = { host.send(Event.Answered) })
            }
        }
}

/** The code in `<scheme>://auth?code=…`, or null for any other intent. */
internal fun codeOf(intent: Intent, scheme: String): String? {
    val data = intent.data ?: return null
    if (data.scheme != scheme || data.host != "auth") return null
    return data.getQueryParameter("code")?.takeIf { it.isNotEmpty() }
}
