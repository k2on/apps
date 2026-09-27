// The library: every track, and a button per row that puts it on the
// selected playlist. Tracks arrive by facts — this build carries no
// `add_track` — so the list is whatever the server has scanned, and empty
// when working alone.
package dev.harken.android

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
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

fun clock(ms: Long): String {
    val s = ms / 1000
    return "%d:%02d".format(s / 60, s % 60)
}

@Composable
fun LibraryScreen(model: Model) {
    val tracks by model.library.collectAsStateWithLifecycle()
    val playlists by model.playlists.collectAsStateWithLifecycle()
    val selected by model.selected.collectAsStateWithLifecycle()
    val target = playlists.firstOrNull { it.id == selected }

    Column(Modifier.fillMaxSize()) {
        Text(
            text = if (target == null) "No playlist yet — make one under Playlists." else "Adding to: ${target.name}",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
        if (tracks.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("The library is empty. Tracks arrive from a server's scanner.", style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(tracks, key = { it.id.hex }) { t ->
                    TrackRow(t, onAdd = if (target == null) null else ({ model.addToPlaylist(t.id, target.id) }))
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
fun TrackRow(t: Track, onAdd: (() -> Unit)?) {
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(t.title, style = MaterialTheme.typography.bodyLarge)
            Text(
                listOfNotNull(t.artist.ifEmpty { null }, t.album).joinToString(" — "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(clock(t.durationMs), style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 8.dp))
        IconButton(onClick = { onAdd?.invoke() }, enabled = onAdd != null) {
            Icon(Icons.Default.Add, contentDescription = "Add to playlist")
        }
    }
}
