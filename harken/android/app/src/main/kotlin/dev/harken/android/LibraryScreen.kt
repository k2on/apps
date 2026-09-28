// The library: everything, in the order it was added, read against the
// selected playlist so each row knows whether it is on it. The row's button
// puts it on that playlist or takes it off; its menu does the same for any
// playlist, ticked by `playlists_of`. With no playlist at all, adding makes
// "Favorites" first. Songs arrive from the server's scanner — this build
// carries no `add_song` — so the list is empty alone or signed out.
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
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
    val settings by model.settings.collectAsStateWithLifecycle()
    val standings by model.standings.collectAsStateWithLifecycle()
    val target = playlists.firstOrNull { it.id == selected }

    Column(Modifier.fillMaxSize()) {
        Text(
            text = if (target == null) "No playlist yet — adding makes \"${Phone.DEFAULT_PLAYLIST}\"." else "Adding to: ${target.name}",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
        if (tracks.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(16.dp), contentAlignment = Alignment.Center) {
                val why = when {
                    settings.workAlone -> "The library arrives from a server's scanner; alone, it stays empty."
                    settings.user == null -> "Sign in (Settings) to sync the library. Playlists made meanwhile are kept."
                    else -> "The library arrives from the server's scanner."
                }
                Text(why, style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(tracks, key = { it.id.hex }) { t ->
                    val caption = target?.let { p -> model.captionFor(t, p.id, standings) }
                    TrackRow(
                        t,
                        caption = caption,
                        onToggle = {
                            if (target != null && t.playlistPos != null) model.removeFromPlaylist(target.id, t) else model.addToPlaylist(t)
                        },
                        playlists = playlists,
                        onOf = { model.playlistsOf(t) },
                        onPick = { p -> model.toggle(t, p.id) },
                    )
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
fun TrackRow(
    t: Track,
    caption: String?,
    onToggle: () -> Unit,
    playlists: List<Playlist>,
    onOf: () -> Set<dev.arkdb.Id>,
    onPick: (Playlist) -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(t.title, style = MaterialTheme.typography.bodyLarge)
            Text(t.creator, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (caption != null) Caption(caption)
        }
        Text(clock(t.durationMs), style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 8.dp))
        IconButton(onClick = onToggle) {
            if (t.playlistPos != null) {
                Icon(Icons.Default.Check, contentDescription = "Take off the playlist", tint = MaterialTheme.colorScheme.primary)
            } else {
                Icon(Icons.Default.Add, contentDescription = "Add to playlist")
            }
        }
        Box {
            IconButton(onClick = { menu = true }, enabled = playlists.isNotEmpty()) {
                Icon(Icons.Default.MoreVert, contentDescription = "Playlists")
            }
            if (menu) {
                val on = onOf()
                DropdownMenu(expanded = true, onDismissRequest = { menu = false }) {
                    for (p in playlists) {
                        DropdownMenuItem(
                            text = { Text(p.name) },
                            leadingIcon = { if (p.id in on) Icon(Icons.Default.Check, contentDescription = "On it") },
                            onClick = {
                                menu = false
                                onPick(p)
                            },
                        )
                    }
                }
            }
        }
    }
}

/** A standing, under the item it is about: grey while pending, the error colour with the reason when not saved. */
@Composable
fun Caption(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = if (text.startsWith("not saved")) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
