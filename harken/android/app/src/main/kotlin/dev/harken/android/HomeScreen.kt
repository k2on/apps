// Home: where you were, and what is new. What was played last and a way
// back to it — the same list, from the same track — then what arrived most
// recently, the person's playlists, and the albums, each a shelf of covers
// opening its page. Settings is the one thing in its bar.
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** What Home draws. */
data class HomeData(
    val loaded: Boolean,
    val last: LastPlayed?,
    /** Every song, in the order added: the newest are its end. */
    val tracks: List<Track>,
    val playlists: List<Playlist>,
    val albums: List<Album>,
    /** The server's host while it cannot be reached; null while it can. */
    val offline: String?,
)

/** How many of the newest songs Home's shelf shows. */
const val RECENT = 12

@Composable
fun HomeScreen(data: HomeData, albumOf: (Track) -> Album?, act: Do) {
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            TabTitle("Home") { IconButton(onClick = act::settings) { Glyph(Glyphs.Settings, "Settings") } }
        }
        if (data.offline != null) item { Offline(data.offline) }
        when {
            !data.loaded -> item { Loading() }
            data.tracks.isEmpty() ->
                item {
                    Empty(
                        Glyphs.Note,
                        "Nothing here yet",
                        "Harken plays what your server finds. Put music in its media folder, under music/, and it appears here.",
                    )
                }
            else -> {
                val last = data.last
                if (last != null) {
                    item { Heading("Last played") }
                    val from =
                        when (val f = last.from) {
                            is Page.ForAlbum -> "From ${f.name}"
                            is Page.ForArtist -> "From ${f.name}"
                            is Page.ForPlaylist -> data.playlists.firstOrNull { it.id == f.id }?.let { "From ${it.name}" } ?: "From a playlist"
                            else -> "From your library"
                        }
                    item { LastCard(last, from, onResume = { act.resume(last) }) }
                }
                val recent = data.tracks.takeLast(RECENT).reversed()
                item { Heading("Recently added", action = "All songs", onAction = { act.library(LibraryTab.Songs) }) }
                item {
                    Shelf(recent, key = { it.id.hex }) { t ->
                        val album = albumOf(t)
                        RecordCard(album?.name ?: t.title, album?.art ?: "", t.title, t.creator, Space.shelf, onClick = {
                            act.play(recent, recent.indexOf(t), null)
                        })
                    }
                }
                item { Heading("Your playlists", action = "New", onAction = act::newPlaylist) }
                if (data.playlists.isEmpty()) {
                    item {
                        Text(
                            "None yet. Make one, then put songs on it from any song's menu.",
                            style = quiet(MaterialTheme.typography.bodyMedium),
                            modifier = Modifier.padding(horizontal = Space.page),
                        )
                    }
                } else {
                    item {
                        Shelf(data.playlists, key = { it.id.hex }) { p ->
                            RecordCard(p.name, "", p.name, "Playlist", Space.shelf, onClick = { act.open(Page.ForPlaylist(p.id)) })
                        }
                    }
                }
                if (data.albums.isNotEmpty()) {
                    item { Heading("Albums", action = "See all", onAction = { act.library(LibraryTab.Albums) }) }
                    item {
                        Shelf(data.albums, key = { it.name }) { a ->
                            RecordCard(a.name, a.art, a.name, a.creator, Space.shelf, onClick = { act.open(Page.ForAlbum(a.name)) })
                        }
                    }
                }
                item { Spacer(Modifier.height(Space.xl)) }
            }
        }
    }
}

// What was played last, and the button that plays it again from there.
@Composable
private fun LastCard(last: LastPlayed, from: String, onResume: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page).clickable(onClick = onResume),
    ) {
        Row(Modifier.padding(Space.m), verticalAlignment = Alignment.CenterVertically) {
            Cover(last.track.album.ifEmpty { last.track.title }, last.track.art, 64.dp, Modifier.size(64.dp))
            Spacer(Modifier.width(Space.m))
            Column(Modifier.weight(1f)) {
                Text(last.track.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(last.track.artist, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(from, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            FilledIconButton(onClick = onResume) { Glyph(Glyphs.Play, "Play again", size = 18.dp) }
        }
    }
}
