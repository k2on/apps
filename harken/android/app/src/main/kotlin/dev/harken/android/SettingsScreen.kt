// Settings: who is signed in, which server and whether it can be reached,
// signing out, and what this is. Nothing a person sees here is a cursor, a
// hash or a count of pending intents: those are the diagnostics, behind
// the version — tapped seven times, the way Android's own build number
// unlocks its developer options — and remembered once found.
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.arkdb.client.Status

/** What Settings draws. */
data class SettingsData(
    val name: String,
    val user: String,
    val email: String,
    val server: String,
    val linked: Boolean,
    val version: String,
    val diagnostics: Boolean,
)

@Composable
fun SettingsScreen(data: SettingsData, onBack: () -> Unit, onSignOut: () -> Unit, onVersion: () -> Unit, onDiagnostics: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        PageBar("Settings", onBack)
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = Space.page)) {
            Group("Account") {
                Row(Modifier.padding(Space.l), verticalAlignment = Alignment.CenterVertically) {
                    Surface(color = MaterialTheme.colorScheme.primaryContainer, shape = CircleShape, modifier = Modifier.size(48.dp)) {
                        Box(contentAlignment = Alignment.Center) {
                            Text(
                                data.name.take(1).uppercase(),
                                style = MaterialTheme.typography.titleLarge,
                                color = MaterialTheme.colorScheme.onPrimaryContainer,
                            )
                        }
                    }
                    Spacer(Modifier.width(Space.l))
                    Column {
                        Text(data.name, style = MaterialTheme.typography.titleMedium)
                        Text(data.email.ifEmpty { data.user }, style = quiet(MaterialTheme.typography.bodyMedium))
                    }
                }
            }
            Group("Server") {
                Line(hostOf(data.server), if (data.linked) "Connected" else "Can't reach it right now — what's on this phone still works")
            }
            Spacer(Modifier.height(Space.l))
            OutlinedButton(
                onClick = onSignOut,
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Glyph(Glyphs.SignOut, null, size = 18.dp)
                Spacer(Modifier.width(Space.s))
                Text("Sign out")
            }
            Text(
                "Anything not synced yet stays on this phone, and goes when you sign in again.",
                style = quiet(MaterialTheme.typography.bodySmall),
                modifier = Modifier.padding(top = Space.s),
            )
            Group("About") {
                Line("Harken", "Your music, from your own server")
                Line("Version", data.version, onClick = onVersion)
                if (data.diagnostics) Line("Diagnostics", "What this phone's copy of the library is doing", onClick = onDiagnostics, chevron = true)
            }
            Spacer(Modifier.height(Space.xl))
        }
    }
}

// The host and port of a server URL, as a person writes it.
private fun hostOf(url: String): String = url.substringAfter("://")

@Composable
private fun Group(title: String, content: @Composable () -> Unit) {
    Text(title, style = quiet(MaterialTheme.typography.labelLarge), modifier = Modifier.padding(start = Space.xs, top = Space.xl, bottom = Space.s))
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) {
        Column { content() }
    }
}

@Composable
private fun Line(title: String, under: String, onClick: (() -> Unit)? = null, chevron: Boolean = false) {
    Row(
        Modifier.fillMaxWidth().let { if (onClick != null) it.clickable(onClick = onClick) else it }.padding(Space.l),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(under, style = quiet(MaterialTheme.typography.bodyMedium))
        }
        if (chevron) Icon(Glyphs.Chevron, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
    }
}

/** Every number the old settings screen showed, and every change this phone made with where it stands. */
@Composable
fun DiagnosticsScreen(d: Diagnostics?, onBack: () -> Unit, onVerify: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        PageBar("Diagnostics", onBack)
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = Space.page)) {
            val s = d?.status
            Group("Peer") {
                if (s == null) Line("Opening", "…") else for ((k, v) in numbers(s)) Number(k, v)
            }
            Group("Verify") {
                Line("Ask the server whether it agrees", d?.verdict ?: "Not asked yet; the answer arrives on a later pump", onClick = onVerify)
            }
            Group("Changes from this phone") {
                val changes = d?.changes.orEmpty()
                if (changes.isEmpty()) Line("None yet", "What you change here is listed with where it stands")
                for ((what, standing) in changes) Line(what, standing)
            }
            Spacer(Modifier.height(Space.xl))
        }
    }
}

/** The status's numbers, by name, in the order the old screen printed them. */
fun numbers(s: Status): List<Pair<String, String>> =
    listOf(
        "user" to s.user,
        "session" to s.session,
        "link" to s.link,
        "linked" to s.linked.toString(),
        "url" to (s.url ?: "—"),
        "connections" to s.opens.toString(),
        "epoch" to s.epoch.toString(),
        "cursor" to s.cursor.toString(),
        "not synced" to s.pending.toString(),
        "joining" to s.joining.toString(),
        "held" to s.held.toString(),
        "server runs a newer module" to s.behind.toString(),
        "diverged" to s.diverged.toString(),
        "last close" to (s.lastClose ?: "—"),
        "denied" to (s.denied ?: "—"),
        "live frames" to s.heardFrames.toString(),
        "bad frames" to s.badFrames.toString(),
    )

@Composable
private fun Number(name: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(horizontal = Space.l, vertical = Space.s)) {
        Text(name, style = quiet(MaterialTheme.typography.bodyMedium), modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace))
    }
}
