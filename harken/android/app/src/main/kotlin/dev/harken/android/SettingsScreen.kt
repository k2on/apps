// Settings: who is signed in and signing out, the server, what the peer
// says — its cursor, what is not synced, what the server holds back, the
// link — and every change this phone made with where it stands.
package dev.harken.android

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

@Composable
fun SettingsScreen(model: Model) {
    val status by model.status.collectAsStateWithLifecycle()
    val changes by model.changes.collectAsStateWithLifecycle()
    val standings by model.standings.collectAsStateWithLifecycle()

    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text("Account", style = MaterialTheme.typography.titleMedium)
        Text("Signed in as ${model.user}", style = MaterialTheme.typography.bodyLarge)
        OutlinedButton(onClick = { model.signOut() }) { Text("Sign out") }
        Hint("What you made and has not synced stays on this phone, and goes when you sign in again.")

        Spacer(Modifier.height(24.dp))
        Text("Server", style = MaterialTheme.typography.titleMedium)
        Text(model.server, style = MaterialTheme.typography.bodyLarge)

        Spacer(Modifier.height(24.dp))
        Text("Status", style = MaterialTheme.typography.titleMedium)
        val s = status
        if (s == null) {
            Text("opening…")
        } else {
            Text(statusLine(s))
            Text(
                "cursor ${s.cursor}, not synced ${s.pending}, held ${s.held}, diverged ${s.diverged}",
                style = MaterialTheme.typography.bodySmall,
            )
            Text(
                "link ${s.link}, linked ${s.linked}, connections ${s.opens}" + (if (s.behind) ", the server runs a newer module" else ""),
                style = MaterialTheme.typography.bodySmall,
            )
            s.lastClose?.let { Text("last close: $it", style = MaterialTheme.typography.bodySmall) }
            s.denied?.let { Text("denied: $it", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
        }
        Spacer(Modifier.height(12.dp))
        OutlinedButton(onClick = { model.verify() }) { Text("Verify against the authority") }
        Hint("The server's answer arrives on a later pump, and is said when it does.")

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
