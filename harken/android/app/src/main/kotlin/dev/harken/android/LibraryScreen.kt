// Library: everything, five ways — the person's playlists, the albums, the
// artists, the composers (where there are works), and every song in the
// order it arrived — each opening the page it stands for. The playlists tab is where a new one is made; the
// module's own checks decide whether a name will do, and say why not.
package dev.harken.android

import uniffi.harken_domain.Playlist
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
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
import dev.harken.android.ui.Cover
import dev.harken.android.ui.Empty
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.ListRow
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.ScrollsToTop
import dev.harken.android.ui.Space
import dev.harken.android.ui.TabTitle
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberDestinationGridState
import dev.harken.android.ui.rememberDestinationListState
import dev.harken.android.ui.rememberDestinationScrollState
import dev.harken.android.ui.rememberHero

enum class LibraryTab(val title: String) {
    Playlists("Playlists"),
    Albums("Albums"),
    Artists("Artists"),
    Composers("Composers"),
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
    /** Everyone some work is by: none in a library of pop, which has no Composers then. */
    val composers: List<Composer> = emptyList(),
)

@Composable
fun LibraryScreen(tab: LibraryTab, onTab: (LibraryTab) -> Unit, data: LibraryData, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    Recompositions.hit("LibraryScreen")
    Column(Modifier.fillMaxSize()) {
        TabTitle("Library") { IconButton(onClick = act::newPlaylist) { Glyph(Glyphs.Plus, "New playlist") } }
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberDestinationScrollState("chips")).padding(horizontal = Space.page),
            horizontalArrangement = Arrangement.spacedBy(Space.s),
        ) {
            // A library of pop has no works, so no Composers — unless it is the list already shown.
            for (t in LibraryTab.entries.filter { it != LibraryTab.Composers || data.composers.isNotEmpty() || it == tab }) {
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
            LibraryTab.Composers -> if (data.complete) Composers(data.composers, act)
            LibraryTab.Songs -> Songs(data.tracks, albumOf, playing, act)
        }
    }
}

@Composable
private fun Playlists(playlists: List<Playlist>, act: Do) {
    val list = rememberDestinationListState("playlists")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(vertical = Space.s)) {
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
    val grid = rememberDestinationGridState("albums")
    ScrollsToTop(grid)
    LazyVerticalGrid(
        state = grid,
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
                // Who played it, as a catalogue lists a record: whose it is when nobody is credited.
                Text(a.by, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
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
    val list = rememberDestinationListState("artists")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(vertical = Space.s)) {
        items(artists, key = { it.name }) { a ->
            val hero = rememberHero(Space.corner, round = true)
            ListRow(onClick = hero.lifting { act.open(Page.ForArtist(a.name)) }, title = a.name, under = Media.count(a.tracks.toInt(), "song")) {
                Cover(a.name, a.art, 56.dp, Modifier.size(56.dp).then(hero.modifier), round = true)
            }
        }
    }
}

@Composable
private fun Composers(composers: List<Composer>, act: Do) {
    if (composers.isEmpty()) {
        Empty(Glyphs.Artist, "No composers yet", "Whoever a work the server finds is by appears here.")
        return
    }
    val list = rememberDestinationListState("composers")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(vertical = Space.s)) {
        items(composers, key = { it.name }) { c ->
            val hero = rememberHero(Space.corner, round = true)
            // When they lived; nobody's years known, and it is how many works of theirs there are.
            val under = Catalogue.span(c.born, c.died).ifEmpty { Media.count(c.works.toInt(), "work") }
            ListRow(onClick = hero.lifting { act.open(Page.ForArtist(c.name)) }, title = c.name, under = under) {
                Cover(c.name, c.art, 56.dp, Modifier.size(56.dp).then(hero.modifier), round = true)
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
    val list = rememberDestinationListState("songs")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list) {
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
