// The onboarding's three screens, drawn from a step and nothing else — so
// an app can render each of them, in each of its states, without a
// network or a phone (harken's screenshots do). Every colour is the
// theme's: an app's identity is its mark, its name and its scheme.
package dev.arkdb.android

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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/** Where the session is being opened: the mark, and nothing to press. */
@Composable
public fun Opening(identity: AppIdentity) {
    Page(identity, title = identity.name, sentence = "Opening your library…") {
        CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally))
    }
}

/** Step one: the server's address. */
@Composable
public fun ServerStep(
    identity: AppIdentity,
    step: Step.Server,
    onType: (String) -> Unit,
    onContinue: () -> Unit
) {
    Page(
        identity,
        title = "Welcome to ${identity.name}",
        sentence = "${identity.name} plays from your own server. Where is it?",
    ) {
        OutlinedTextField(
            value = step.typed,
            onValueChange = onType,
            label = { Text("Server address") },
            placeholder = { Text("music.example.com") },
            isError = step.error != null,
            supportingText = {
                Text(step.error ?: "A name or an address, with a port if it has one.")
            },
            enabled = !step.checking,
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
            enabled = !step.checking && step.typed.isNotBlank(),
            modifier = Modifier.fillMaxWidth()) {
                if (step.checking) Busy("Checking…") else Text("Continue")
            }
    }
}

/** Step two: signing in, in a browser tab, on the server's own page. */
@Composable
public fun SignInStep(
    identity: AppIdentity,
    step: Step.SignIn,
    onSignIn: () -> Unit,
    onChangeServer: () -> Unit
) {
    val host = Probe.display(step.server)
    Page(
        identity,
        title = "Sign in",
        sentence =
            "You'll sign in on $host's own page, in a browser tab, and come straight back here.",
    ) {
        if (step.error != null) {
            Text(
                step.error,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp),
            )
        }
        Button(
            onClick = onSignIn,
            enabled = step.phase != Step.Phase.Exchanging,
            modifier = Modifier.fillMaxWidth()) {
                when (step.phase) {
                    Step.Phase.Idle -> Text("Sign in")
                    Step.Phase.Browser -> Text("Open the sign-in again")
                    Step.Phase.Exchanging -> Busy("Signing in…")
                }
            }
        if (step.phase == Step.Phase.Browser) {
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
            enabled = step.phase != Step.Phase.Exchanging,
            modifier = Modifier.fillMaxWidth()) {
                Text("Use another server")
            }
    }
}

/** Step three: one permission, with the app's reason, asked once. */
@Composable
public fun PermissionStep(
    identity: AppIdentity,
    step: Step.Ask,
    onAllow: () -> Unit,
    onNotNow: () -> Unit
) {
    Page(identity, title = step.permission.title, sentence = step.permission.why) {
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

// One onboarding page: the mark, a title, a sentence, and what to do —
// centred in a column no wider than a phone held upright.
@Composable
private fun Page(
    identity: AppIdentity,
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
                        .verticalScroll(rememberScrollState())
                        .padding(horizontal = 32.dp, vertical = 24.dp),
                ) {
                    Image(
                        identity.mark,
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
