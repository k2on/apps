// The onboarding's screens, with no state of their own: the server's
// address, signing in, a permission, and the library on its way — each
// drawn from plain values and two callbacks, so an app can render every one
// of them, in each of its states, without a network or a phone (harken's
// screenshots do). The machine that decides which to show, and what each
// button does, is `ark-android`'s (`ArkApp`), which draws these. Every
// colour is the theme's: an app's identity here is its mark and its name.
package dev.harken.android.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/**
 * How long [OpeningScreen] waits before it says anything: a library already here never shows it.
 */
public const val OPENING_DELAY_MS: Long = 600

/**
 * The library on its way from `host`, just after signing in: the mark and a sentence saying where
 * from, quietly, and only after [OPENING_DELAY_MS] — the page's own ground until then.
 */
@Composable
public fun OpeningScreen(
    name: String,
    mark: Painter,
    host: String,
    delayMs: Long = OPENING_DELAY_MS
) {
    var shown by remember { mutableStateOf(delayMs <= 0) }
    LaunchedEffect(Unit) {
        if (!shown) {
            delay(delayMs)
            shown = true
        }
    }
    if (!shown) {
        Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {}
        return
    }
    OnboardingPage(mark, title = name, sentence = "Getting your library from $host…") {
        LinearProgressIndicator(
            Modifier.width(96.dp).align(Alignment.CenterHorizontally),
            color = MaterialTheme.colorScheme.primary,
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
        )
    }
}

/**
 * The server's address: one field, what is `typed` so far, and under it the `error` of the last
 * check or a sentence on what to type. While `checking`, nothing can be changed and the button says
 * so.
 */
@Composable
public fun ServerScreen(
    name: String,
    mark: Painter,
    typed: String,
    error: String?,
    checking: Boolean,
    onType: (String) -> Unit,
    onContinue: () -> Unit,
) {
    OnboardingPage(
        mark,
        title = "Welcome to $name",
        sentence = "$name plays from your own server. Where is it?",
    ) {
        OutlinedTextField(
            value = typed,
            onValueChange = onType,
            label = { Text("Server address") },
            placeholder = { Text("music.example.com") },
            isError = error != null,
            supportingText = { Text(error ?: "A name or an address, with a port if it has one.") },
            enabled = !checking,
            singleLine = true,
            keyboardOptions =
                KeyboardOptions(
                    keyboardType = KeyboardType.Uri,
                    imeAction = ImeAction.Go,
                    autoCorrectEnabled = false),
            keyboardActions = KeyboardActions(onGo = { onContinue() }),
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(16.dp))
        Button(
            onClick = onContinue,
            enabled = !checking && typed.isNotBlank(),
            modifier = Modifier.fillMaxWidth()) {
                if (checking) Busy("Checking…") else Text("Continue")
            }
    }
}

/**
 * Where a sign-in is: not started, waiting for the browser it opened, or trading the code it got
 * back.
 */
public enum class SignInPhase {
    Idle,
    Browser,
    Exchanging,
}

/**
 * Signing in, in a browser tab, on `host`'s own page: the button, what it is waiting for, and the
 * `error` the last attempt or the server said.
 */
@Composable
public fun SignInScreen(
    mark: Painter,
    host: String,
    error: String?,
    phase: SignInPhase,
    onSignIn: () -> Unit,
    onChangeServer: () -> Unit,
) {
    OnboardingPage(
        mark,
        title = "Sign in",
        sentence =
            "You'll sign in on $host's own page, in a browser tab, and come straight back here.",
    ) {
        if (error != null) {
            Text(
                error,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp),
            )
        }
        Button(
            onClick = onSignIn,
            enabled = phase != SignInPhase.Exchanging,
            modifier = Modifier.fillMaxWidth()) {
                when (phase) {
                    SignInPhase.Idle -> Text("Sign in")
                    SignInPhase.Browser -> Text("Open the sign-in again")
                    SignInPhase.Exchanging -> Busy("Signing in…")
                }
            }
        if (phase == SignInPhase.Browser) {
            Text(
                "Waiting for the browser…",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            )
        }
        Spacer(Modifier.height(8.dp))
        TextButton(
            onClick = onChangeServer,
            enabled = phase != SignInPhase.Exchanging,
            modifier = Modifier.fillMaxWidth()) {
                Text("Use another server")
            }
    }
}

/** One permission: what it is, the app's reason in a sentence, Allow and Not now. */
@Composable
public fun PermissionScreen(
    mark: Painter,
    title: String,
    why: String,
    onAllow: () -> Unit,
    onNotNow: () -> Unit
) {
    OnboardingPage(mark, title = title, sentence = why) {
        Button(onClick = onAllow, modifier = Modifier.fillMaxWidth()) { Text("Allow") }
        Spacer(Modifier.height(8.dp))
        TextButton(onClick = onNotNow, modifier = Modifier.fillMaxWidth()) { Text("Not now") }
        Text(
            "You can change this later in the phone's settings.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
        )
    }
}

@Composable
private fun Busy(what: String) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Center) {
            CircularProgressIndicator(
                Modifier.size(18.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.onPrimary)
            Spacer(Modifier.width(12.dp))
            Text(what)
        }
}

/**
 * One onboarding page: the `mark` in the theme's primary, a title, a sentence, and what to do —
 * centred in a column no wider than a phone held upright, scrolling when the keyboard is up.
 */
@Composable
public fun OnboardingPage(
    mark: Painter,
    title: String,
    sentence: String,
    body: @Composable ColumnScope.() -> Unit
) {
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        Box(
            Modifier.fillMaxSize().safeDrawingPadding().imePadding(),
            contentAlignment = Alignment.Center) {
                Column(
                    Modifier.fillMaxWidth()
                        .verticalScroll(rememberTransientScrollState())
                        .padding(horizontal = 32.dp, vertical = 24.dp)) {
                        Image(
                            mark,
                            contentDescription = null,
                            colorFilter = ColorFilter.tint(MaterialTheme.colorScheme.primary),
                            modifier = Modifier.size(72.dp).align(Alignment.CenterHorizontally),
                        )
                        Spacer(Modifier.height(24.dp))
                        Text(
                            title,
                            style = MaterialTheme.typography.headlineMedium,
                            color = MaterialTheme.colorScheme.onBackground,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Spacer(Modifier.height(8.dp))
                        Text(
                            sentence,
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Spacer(Modifier.height(32.dp))
                        body()
                    }
            }
    }
}
