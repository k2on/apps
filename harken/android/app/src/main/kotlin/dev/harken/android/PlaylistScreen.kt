// One playlist: its items in `pos` order, each joined to its track by the
// model, and a swipe (or the trash button) that removes one. An item whose
// track has not arrived is drawn as unavailable rather than hidden — it is
// on the list, whatever this replica knows about the track.
package dev.harken.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.arkdb.Id

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlaylistScreen(model: Model, id: Id, onBack: () -> Unit) {
    val items by model.items.collectAsStateWithLifecycle()
    val playlists by model.playlists.collectAsStateWithLifecycle()
    val playlist = playlists.firstOrNull { it.id == id }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
            Text(playlist?.name ?: "(gone)", style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
            Text("${items.size}", style = MaterialTheme.typography.labelLarge)
        }
        if (playlist == null) {
            // A duplicate that lost the rebase, or a stale route.
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("This playlist is no longer here.", style = MaterialTheme.typography.bodyMedium)
            }
        } else if (items.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("Nothing on it yet. Add from the Library.", style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(items, key = { it.trackId.hex }) { row ->
                    val state = rememberSwipeToDismissBoxState(
                        confirmValueChange = { v ->
                            if (v == SwipeToDismissBoxValue.EndToStart) {
                                model.removeFromPlaylist(id, row.trackId)
                                true
                            } else {
                                false
                            }
                        },
                    )
                    SwipeToDismissBox(
                        state = state,
                        enableDismissFromStartToEnd = false,
                        backgroundContent = {
                            Box(
                                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.errorContainer).padding(end = 24.dp),
                                contentAlignment = Alignment.CenterEnd,
                            ) { Icon(Icons.Default.Delete, contentDescription = null, tint = MaterialTheme.colorScheme.onErrorContainer) }
                        },
                    ) {
                        ItemRow(row, onRemove = { model.removeFromPlaylist(id, row.trackId) })
                    }
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
fun ItemRow(row: PlaylistRow, onRemove: () -> Unit) {
    val t = row.track
    Row(
        Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surface).padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("${row.pos}", style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(end = 12.dp))
        Column(Modifier.weight(1f)) {
            if (t == null) {
                Text("unavailable", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(row.trackId.text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                Text(t.title, style = MaterialTheme.typography.bodyLarge)
                Text(
                    listOfNotNull(t.artist.ifEmpty { null }, t.album).joinToString(" — "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (t != null) Text(clock(t.durationMs), style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 8.dp))
        IconButton(onClick = onRemove) { Icon(Icons.Default.Delete, contentDescription = "Remove from playlist") }
    }
}
