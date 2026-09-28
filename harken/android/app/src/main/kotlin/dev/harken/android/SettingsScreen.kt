// Settings: who is signed in, the server and "work alone", what the session
// says, and every change this phone made with where it stands. Applying the
// server reopens the session — the old one is closed and written down
// first — in a directory per server (or alone), not per person, so work
// done signed out is there when somebody signs in.
package dev.harken.android

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

@Composable
fun SettingsScreen(model: Model) {
    val saved by model.settings.collectAsStateWithLifecycle()
    val status by model.status.collectAsStateWithLifecycle()
    val changes by model.changes.collectAsStateWithLifecycle()
    val standings by model.standings.collectAsStateWithLifecycle()
    var url by rememberSaveable { mutableStateOf(saved.serverUrl) }
    var alone by rememberSaveable { mutableStateOf(saved.workAlone) }
    var name by rememberSaveable { mutableStateOf("") }

    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text("Account", style = MaterialTheme.typography.titleMedium)
        val who = saved.user
        if (who != null) {
            Text("Signed in as $who", style = MaterialTheme.typography.bodyLarge)
            OutlinedButton(onClick = { model.signOut() }) { Text("Sign out") }
            Hint("What you made and has not synced stays yours, and goes when you sign in again.")
        } else {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Name") },
                supportingText = { Text("Dev auth: a name is a login.") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Button(onClick = { model.signIn(name) }, enabled = name.isNotBlank()) { Text("Sign in") }
            Hint("Not signed in: everything works and is kept on this phone, and nothing is sent anywhere. Signing in makes it yours and syncs it.")
        }

        Spacer(Modifier.height(24.dp))
        Text("Server", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            label = { Text("WebSocket URL") },
            supportingText = { Text("ws://host:port/sync — 10.0.2.2 reaches the emulator's host") },
            singleLine = true,
            enabled = !alone,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Work alone", style = MaterialTheme.typography.bodyLarge)
                Hint("No server: this phone sequences its own log. The library stays empty, because songs come from a server's scanner.")
            }
            Switch(checked = alone, onCheckedChange = { alone = it })
        }
        Spacer(Modifier.height(16.dp))
        Button(
            onClick = { model.applySettings(saved.copy(serverUrl = url.trim(), workAlone = alone)) },
            enabled = (alone || url.isNotBlank()) && (url.trim() != saved.serverUrl || alone != saved.workAlone),
        ) { Text("Apply and reopen") }

        Spacer(Modifier.height(24.dp))
        Text("Status", style = MaterialTheme.typography.titleMedium)
        val s = status
        if (s == null) {
            Text("opening…")
        } else {
            Text(statusLine(s, saved.user != null))
            Text(
                "cursor ${s.cursor}, not synced ${s.pending}, not saved ${s.rejected}, diverged ${s.diverged}",
                style = MaterialTheme.typography.bodySmall,
            )
            s.lastClose?.let { Text("last close: $it", style = MaterialTheme.typography.bodySmall) }
            s.denied?.let { Text("denied: $it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
        }
        Spacer(Modifier.height(12.dp))
        OutlinedButton(onClick = { model.verify() }) { Text("Verify against the authority") }
        Hint("Alone, the answer is immediate. Linked, the server's Agree arrives on a later pump and is not yet surfaced here.")

        Spacer(Modifier.height(24.dp))
        Text("Changes", style = MaterialTheme.typography.titleMedium)
        if (changes.isEmpty()) Hint("Nothing changed on this phone yet.")
        for (c in changes) {
            Column(Modifier.padding(vertical = 4.dp)) {
                Text(c.what, style = MaterialTheme.typography.bodyMedium)
                Caption(model.captionFor(c, standings))
            }
        }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
