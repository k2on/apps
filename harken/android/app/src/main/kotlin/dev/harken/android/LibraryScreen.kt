// Library: everything, four ways — the person's playlists, the albums, the
// artists, and every song in the order it arrived — each opening the page
// it stands for. The playlists tab is where a new one is made; the
// module's own checks decide whether a name will do, and say why not.
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

enum class LibraryTab(val title: String) {
    Playlists("Playlists"),
    Albums("Albums"),
    Artists("Artists"),
    Songs("Songs"),
}

/** What the Library draws: a snapshot, its lists copies, so an equal one is the same one to Compose. */
@Immutable
data class LibraryData(
    val loaded: Boolean,
    val playlists: List<Playlist>,
    val albums: List<Album>,
    val artists: List<Artist>,
    val tracks: List<Track>,
    val offline: String?,
    /** Whether the artists have been read: they are read after Home's lists, milliseconds later. */
    val complete: Boolean = true,
)

@Composable
fun LibraryScreen(tab: LibraryTab, onTab: (LibraryTab) -> Unit, data: LibraryData, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    Recompositions.hit("LibraryScreen")
    Column(Modifier.fillMaxSize()) {
        TabTitle("Library") { IconButton(onClick = act::newPlaylist) { Glyph(Glyphs.Plus, "New playlist") } }
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = Space.page),
            horizontalArrangement = Arrangement.spacedBy(Space.s),
        ) {
            for (t in LibraryTab.entries) {
                FilterChip(
                    selected = t == tab,
                    onClick = { onTab(t) },
                    label = { Text(t.title) },
                    colors =
                        FilterChipDefaults.filterChipColors(
                            selectedContainerColor = MaterialTheme.colorScheme.primary,
                            selectedLabelColor = MaterialTheme.colorScheme.onPrimary,
                        ),
                )
            }
        }
        if (data.offline != null) Offline(data.offline)
        // Not read yet: nothing, not a loading page — it is milliseconds away.
        if (!data.loaded) return@Column
        when (tab) {
            LibraryTab.Playlists -> Playlists(data.playlists, act)
            LibraryTab.Albums -> Albums(data.albums, act)
            // Before the artists arrive, nothing — not "no artists": they are milliseconds away.
            LibraryTab.Artists -> if (data.complete) Artists(data.artists, act)
            LibraryTab.Songs -> Songs(data.tracks, albumOf, playing, act)
        }
    }
}

@Composable
private fun Playlists(playlists: List<Playlist>, act: Do) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(vertical = Space.s)) {
        item {
            ListRow(onClick = act::newPlaylist, title = "New playlist", under = null) {
                Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = RoundedCornerShape(Space.corner), modifier = Modifier.size(56.dp)) {
                    Box(contentAlignment = Alignment.Center) {
                        Icon(Glyphs.Plus, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                    }
                }
            }
        }
        if (playlists.isEmpty()) {
            item {
                Empty(Glyphs.Playlist, "No playlists yet", "Make one, then put songs on it from any song's menu.")
            }
        }
        items(playlists, key = { it.id.hex }) { p ->
            val hero = rememberHero(Space.corner)
            ListRow(onClick = hero.lifting { act.open(Page.ForPlaylist(p.id)) }, title = p.name, under = "Playlist") {
                Cover(p.name, "", 56.dp, Modifier.size(56.dp).then(hero.modifier))
            }
        }
    }
}

@Composable
private fun Albums(albums: List<Album>, act: Do) {
    if (albums.isEmpty()) {
        Empty(Glyphs.Album, "No albums yet", "An album appears here when the server finds a song that names one.")
        return
    }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(150.dp),
        contentPadding = PaddingValues(Space.page),
        horizontalArrangement = Arrangement.spacedBy(Space.l),
        verticalArrangement = Arrangement.spacedBy(Space.xl),
        modifier = Modifier.fillMaxSize(),
    ) {
        items(albums, key = { it.name }) { a ->
            val hero = rememberHero(Space.corner)
            Column(Modifier.clickable(onClick = hero.lifting { act.open(Page.ForAlbum(a.name)) })) {
                Cover(a.name, a.art, 180.dp, Modifier.fillMaxWidth().aspectRatio(1f).then(hero.modifier))
                Spacer(Modifier.height(Space.s))
                Text(a.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(a.creator, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        item(span = { GridItemSpan(maxLineSpan) }) { Spacer(Modifier.height(Space.s)) }
    }
}

@Composable
private fun Artists(artists: List<Artist>, act: Do) {
    if (artists.isEmpty()) {
        Empty(Glyphs.Artist, "No artists yet", "Whoever made what the server finds appears here.")
        return
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(vertical = Space.s)) {
        items(artists, key = { it.name }) { a ->
            val hero = rememberHero(Space.corner, round = true)
            ListRow(onClick = hero.lifting { act.open(Page.ForArtist(a.name)) }, title = a.name, under = Media.count(a.tracks.toInt(), "song")) {
                Cover(a.name, a.art, 56.dp, Modifier.size(56.dp).then(hero.modifier), round = true)
            }
        }
    }
}

@Composable
private fun Songs(tracks: List<Track>, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    if (tracks.isEmpty()) {
        Empty(Glyphs.Note, "No songs yet", "Harken plays what your server finds. Put music in its media folder, under music/, and it appears here.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(tracks, key = { it.id.hex }) { t ->
            val album = albumOf(t)
            TrackRow(
                t,
                under = listOfNotNull(t.creator, album?.name).joinToString(" · "),
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, tracks.indexOf(t), Page.Songs) },
                onMore = { act.more(t, Page.Songs) },
            ) { Cover(album?.name ?: t.title, album?.art ?: "", Space.thumb, Modifier.size(Space.thumb), corner = 4.dp) }
        }
    }
}

/** A row that opens something: its picture, its name, a line under it, and a chevron. */
@Composable
fun ListRow(onClick: () -> Unit, title: String, under: String?, picture: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = Space.page, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        picture()
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (under != null) Text(under, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1)
        }
        Icon(Glyphs.Chevron, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
    }
}
