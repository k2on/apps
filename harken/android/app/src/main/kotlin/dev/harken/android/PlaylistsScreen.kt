// The person's playlists in the order they were made, and a dialog that
// names a new one. Creating one does not select it: the row appears when
// `create_playlist` has applied, which is at once — the view is optimistic —
// and a tap selects it. A name the person already has is not refused: the
// log keeps the new playlist and calls it "Name (1)".
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.arkdb.Id

@Composable
fun PlaylistsScreen(model: Model, onOpen: (Id) -> Unit) {
    val playlists by model.playlists.collectAsStateWithLifecycle()
    val selected by model.selected.collectAsStateWithLifecycle()
    val standings by model.standings.collectAsStateWithLifecycle()
    var naming by remember { mutableStateOf(false) }

    Box(Modifier.fillMaxSize()) {
        if (playlists.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("No playlists. Make one with +.", style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(playlists, key = { it.id.hex }) { p ->
                    Row(
                        Modifier.fillMaxWidth().clickable { onOpen(p.id) }.padding(horizontal = 16.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(p.name, style = MaterialTheme.typography.bodyLarge)
                            model.captionForPlaylist(p.id, standings)?.let { Caption(it) }
                        }
                        if (p.id == selected) Icon(Icons.Default.Check, contentDescription = "Selected", tint = MaterialTheme.colorScheme.primary)
                    }
                    HorizontalDivider()
                }
            }
        }
        FloatingActionButton(onClick = { naming = true }, modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp)) {
            Icon(Icons.Default.Add, contentDescription = "New playlist")
        }
    }

    if (naming) {
        NamePlaylistDialog(
            problem = model::playlistNameProblem,
            onDismiss = { naming = false },
            onCreate = { name ->
                naming = false
                model.createPlaylist(name)
            },
        )
    }
}

/**
 * The name, checked as it is typed by `create_playlist`'s own input checks
 * (the form validator): the message under the field is the one the
 * mutation would refuse with, so the two cannot disagree. Nothing is shown
 * until something has been typed.
 */
@Composable
fun NamePlaylistDialog(problem: (String) -> String?, onDismiss: () -> Unit, onCreate: (String) -> Unit) {
    var name by remember { mutableStateOf("") }
    var touched by remember { mutableStateOf(false) }
    val message = problem(name)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("New playlist") },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = {
                    name = it
                    touched = true
                },
                label = { Text("Name") },
                singleLine = true,
                isError = touched && message != null,
                supportingText = { if (touched && message != null) Text(message) },
            )
        },
        confirmButton = { TextButton(onClick = { onCreate(name) }, enabled = message == null) { Text("Create") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
