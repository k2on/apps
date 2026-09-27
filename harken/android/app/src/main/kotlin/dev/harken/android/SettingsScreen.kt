// Settings: the server, who this is, and "work alone". Applying reopens the
// session — the old one is closed and written down first — in a directory
// per (user, alone-or-server), so a peer that was its own authority keeps
// its log and a replica of a server keeps its cursor.
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
    var url by rememberSaveable { mutableStateOf(saved.serverUrl) }
    var user by rememberSaveable { mutableStateOf(saved.user) }
    var alone by rememberSaveable { mutableStateOf(saved.workAlone) }

    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp)) {
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
        OutlinedTextField(
            value = user,
            onValueChange = { user = it },
            label = { Text("User") },
            supportingText = { Text("Dev auth: a name is a login.") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Work alone", style = MaterialTheme.typography.bodyLarge)
                Text(
                    "No server: this phone sequences its own playlists. The library stays empty, because tracks come from a server's scanner.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(checked = alone, onCheckedChange = { alone = it })
        }
        Spacer(Modifier.height(16.dp))
        Button(
            onClick = { model.applySettings(Prefs(url.trim(), user.trim(), alone)) },
            enabled = user.isNotBlank() && (alone || url.isNotBlank()),
        ) { Text("Apply and reopen") }

        Spacer(Modifier.height(24.dp))
        Text("Status", style = MaterialTheme.typography.titleMedium)
        val s = status
        if (s == null) {
            Text("opening…")
        } else {
            Text(statusLine(s))
            for (sc in s.scopes) {
                Text(
                    "${sc.scope}: cursor ${sc.cursor}, pending ${sc.pending}, rejections ${sc.rejections}, diverged ${sc.diverged}",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            s.lastClose?.let { Text("last close: $it", style = MaterialTheme.typography.bodySmall) }
            s.denied?.let { Text("denied: $it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
        }
        Spacer(Modifier.height(12.dp))
        OutlinedButton(onClick = { model.verify() }) { Text("Verify against the authority") }
        Text(
            "Alone, the answer is immediate. Linked, the server's Agree arrives on a later pump and is not yet surfaced here.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
