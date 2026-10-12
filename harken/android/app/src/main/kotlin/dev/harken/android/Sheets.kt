// What rises over a page: a song's menu (play it next, put it on a
// playlist, go to its album or its artist, take it off the playlist it was
// opened from), the playlists a song can go on — each ticked when it is
// there, a tap putting it on or taking it off — and naming a new playlist,
// checked as it is typed by `create_playlist`'s own input checks, whose
// sentence is shown under the field.
package dev.harken.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.Id

/** What a song's menu can do. */
interface MenuActions {
    fun playNext() {}

    fun addTo() {}

    fun goTo(page: Page) {}

    fun remove() {}
}

/** A song's menu. `from` is the page its row was on: a playlist offers to take it off. */
@Composable
fun TrackMenu(track: Track, album: Album?, from: Page?, actions: MenuActions) {
    Column(Modifier.fillMaxWidth().padding(bottom = Space.l)) {
        Row(Modifier.padding(horizontal = Space.xl, vertical = Space.s), verticalAlignment = Alignment.CenterVertically) {
            Cover(album?.name ?: track.title, album?.art ?: "", 48.dp, Modifier.size(48.dp), corner = 4.dp)
            Spacer(Modifier.width(Space.m))
            Column(Modifier.weight(1f)) {
                Text(track.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(track.creator, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        HorizontalDivider(Modifier.padding(vertical = Space.s), color = MaterialTheme.colorScheme.outlineVariant)
        SheetRow(Glyphs.PlayNext, "Play next", actions::playNext)
        SheetRow(Glyphs.AddTo, "Add to playlist", actions::addTo, trailing = { Icon(Glyphs.Chevron, null, Modifier.size(18.dp)) })
        if (album != null && from != Page.ForAlbum(album.name)) SheetRow(Glyphs.Album, "Go to album", { actions.goTo(Page.ForAlbum(album.name)) })
        if (track.creator.isNotEmpty() && from != Page.ForArtist(track.creator))
            SheetRow(Glyphs.Artist, "Go to artist", { actions.goTo(Page.ForArtist(track.creator)) })
        if (from is Page.ForPlaylist) SheetRow(Glyphs.Remove, "Remove from this playlist", actions::remove)
    }
}

/** Every playlist, ticked where `track` is on it; "New playlist" first. */
@Composable
fun AddToPlaylist(track: Track, playlists: List<Playlist>, on: Set<Id>?, onToggle: (Playlist) -> Unit, onNew: () -> Unit) {
    LazyColumn(Modifier.fillMaxWidth().padding(bottom = Space.l)) {
        item {
            Text(
                "Add “${track.title}” to",
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.s),
            )
        }
        item { SheetRow(Glyphs.Plus, "New playlist", onNew, emphasis = true) }
        items(playlists, key = { it.id.hex }) { p ->
            val ticked = on?.contains(p.id) == true
            SheetRow(
                if (ticked) Glyphs.Ticked else Glyphs.Untick,
                p.name,
                { onToggle(p) },
                emphasis = ticked,
            )
        }
    }
}

/** The dialog naming a new playlist: what the app raises, and what a screenshot draws. */
@Composable
fun NewPlaylistCard(name: String, problem: String?, adding: Boolean, onName: (String) -> Unit, onMake: () -> Unit, onCancel: () -> Unit) {
    Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh, tonalElevation = 6.dp) {
        Column(Modifier.padding(Space.xl)) {
            Text("New playlist", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.height(Space.l))
            NewPlaylist(name, problem, onName)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onCancel) { Text("Cancel") }
                TextButton(onClick = onMake, enabled = name.isNotBlank() && problem == null) { Text(if (adding) "Make and add" else "Make it") }
            }
        }
    }
}

/** Naming a new playlist: the field, and the module's sentence when the name will not do. */
@Composable
fun NewPlaylist(name: String, problem: String?, onName: (String) -> Unit) {
    Column {
        OutlinedTextField(
            value = name,
            onValueChange = onName,
            label = { Text("Name") },
            singleLine = true,
            isError = problem != null && name.isNotEmpty(),
            supportingText = { Text(if (problem != null && name.isNotEmpty()) problem.replaceFirstChar { it.uppercase() } else "A name you already use gets a number after it.") },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(Space.xs))
    }
}
