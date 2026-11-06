// Library: everything, five ways — the person's playlists, the albums, the
// artists, the composers (where there are works), and every song in the
// order it arrived — each opening the page it stands for. The playlists list's first row, New
// Playlist, is where a new one is made; the module's own checks decide whether a name will do, and
// say why not. And the podcasts and the sermons, always there, whose first row (or, with none yet,
// the empty page's button) finds a show to follow (`FindShow`, Sheets.kt). The large title's line
// has the account's face alone: no +.
package dev.harken.android

import uniffi.harken_domain.Playlist
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkui.AccountFace
import dev.arkui.Cover
import dev.arkui.Empty
import dev.arkui.ListRow
import dev.arkui.Recompositions
import dev.arkui.ScrollsToTop
import dev.arkui.Space
import dev.arkui.TabTitle
import dev.arkui.quiet
import dev.arkui.rememberDestinationGridState
import dev.arkui.rememberDestinationListState
import dev.arkui.rememberDestinationScrollState
import dev.arkui.rememberHero
import dev.arkui.Press
import dev.arkui.pressable
import uniffi.ark_format.formatCount
import uniffi.ark_format.formatSpan

/** The Library's lists; a list of shows says which kind (`podcast`, `sermon`), which its + follows a feed as. */
enum class LibraryTab(val title: String, val kind: String? = null) {
    Playlists("Playlists"),
    Albums("Albums"),
    Artists("Artists"),
    Composers("Composers"),
    Songs("Songs"),
    Podcasts("Podcasts", "podcast"),
    Sermons("Sermons", "sermon"),
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
    /** Every podcast and series of sermons. */
    val shows: List<Show> = emptyList(),
    /** The feeds the library follows: one that is no show yet is listed over the shows of its kind. */
    val feeds: List<Subscription> = emptyList(),
    /** Who is signed in: the face at the end of the title, which raises the account sheet. */
    val who: Who = Who(""),
)

@Composable
fun LibraryScreen(tab: LibraryTab, onTab: (LibraryTab) -> Unit, data: LibraryData, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    Recompositions.hit("LibraryScreen")
    Column(Modifier.fillMaxSize()) {
        // The face alone at the end of the line: what the list makes is its own first row.
        TabTitle("Library", account = { AccountFace(data.who.name, data.who.picture, onClick = act::account) })
        // The title's gap over the chips (`TabTitle.BELOW`), less the Space.s of touch Material
        // keeps over a chip (its 32 dp in a 48 dp target): the iPhone's capsules are as far down.
        Row(
            Modifier.fillMaxWidth()
                .padding(top = TabTitle.BELOW - Space.s)
                .horizontalScroll(rememberDestinationScrollState("chips"))
                .padding(horizontal = Space.page),
            horizontalArrangement = Arrangement.spacedBy(Space.s),
        ) {
            // A library of pop has no works, so no Composers — unless it is the list already shown.
            // Capsules, as the iPhone's: the one shown filled in the accent, the rest a quiet fill
            // with no outline.
            for (t in LibraryTab.entries.filter { it == tab || offers(it, data) }) {
                val on = t == tab
                FilterChip(
                    selected = on,
                    onClick = { onTab(t) },
                    label = { Text(t.title) },
                    shape = CircleShape,
                    colors =
                        FilterChipDefaults.filterChipColors(
                            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                            labelColor = MaterialTheme.colorScheme.onSurface,
                            selectedContainerColor = MaterialTheme.colorScheme.primary,
                            selectedLabelColor = MaterialTheme.colorScheme.onPrimary,
                        ),
                    border = FilterChipDefaults.filterChipBorder(enabled = true, selected = on, borderColor = Color.Transparent, selectedBorderColor = Color.Transparent),
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
            LibraryTab.Podcasts -> if (data.complete) ShowsGrid(data.shows.filter { it.kind == "podcast" }, unread("podcast", data.feeds, data.shows), "podcast", act)
            LibraryTab.Sermons -> if (data.complete) ShowsGrid(data.shows.filter { it.kind == "sermon" }, unread("sermon", data.feeds, data.shows), "sermon", act)
        }
    }
}

/**
 * Whether the Library offers `tab` over `data`: a list only where there is something on it — but
 * Podcasts and Sermons always, since their page is where the first show is followed.
 */
fun offers(tab: LibraryTab, data: LibraryData): Boolean =
    when (tab) {
        LibraryTab.Composers -> data.composers.isNotEmpty()
        else -> true
    }

/** The feeds of `kind` the library follows that are no show yet: not read, or never read right. */
fun unread(kind: String, feeds: List<Subscription>, shows: List<Show>): List<Subscription> =
    feeds.filter { f -> f.kind == kind && shows.none { it.id == f.showId } }

@Composable
private fun ShowsGrid(shows: List<Show>, waiting: List<Subscription>, kind: String, act: Do) {
    if (shows.isEmpty() && waiting.isEmpty()) {
        Empty(
            if (kind == "sermon") Glyphs.Pulpit else Glyphs.Show,
            if (kind == "sermon") "No sermons yet" else "No podcasts yet",
            "Follow one: search Apple Podcasts, or paste its feed's address. A folder of it under ${kind}s/ on the server appears here too.",
            action = if (kind == "sermon") "Follow Sermons" else "Follow a Podcast",
            onAction = { act.findShow(kind) },
        )
        return
    }
    val grid = rememberDestinationGridState("shows:$kind")
    ScrollsToTop(grid)
    LazyVerticalGrid(
        state = grid,
        columns = GridCells.Adaptive(150.dp),
        contentPadding = PaddingValues(Space.page),
        horizontalArrangement = Arrangement.spacedBy(Space.l),
        verticalArrangement = Arrangement.spacedBy(Space.xl),
        modifier = Modifier.fillMaxSize(),
    ) {
        // Following another, first, as New Playlist is the playlists' first row.
        item(key = "follow", span = { GridItemSpan(maxLineSpan) }) {
            FollowRow(kind, act)
        }
        // A feed followed that is no show yet, over the shows: that it is waiting to be read, or why
        // it could not be, and its Unfollow — the one place it can be let go before it is a show.
        items(waiting, key = { "feed:" + it.url }, span = { GridItemSpan(maxLineSpan) }) { f ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(f.title.ifEmpty { f.url }, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(
                        if (f.error.isNotEmpty()) "Could not read it: ${f.error}" else "Not read yet",
                        style = quiet(MaterialTheme.typography.bodySmall),
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                TextButton(onClick = { act.unfollow(f) }) { Text("Unfollow") }
            }
        }
        items(shows, key = { it.id }) { s ->
            val hero = rememberHero(Space.corner)
            Column(Modifier.pressable(onClick = hero.lifting { act.open(Page.ForShow(s.id)) })) {
                Cover(s.title, s.art, 180.dp, Modifier.fillMaxWidth().aspectRatio(1f).then(hero.modifier))
                Spacer(Modifier.height(Space.s))
                Text(s.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(s.author.ifEmpty { formatCount(s.episodes, "episode") }, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        item(span = { GridItemSpan(maxLineSpan) }) { Spacer(Modifier.height(Space.s)) }
    }
}

/**
 * The shows' first row, as New Playlist is the playlists': a + tile and "Follow a Podcast" or
 * "Follow Sermons", finding a show to follow (`FindShow`). In the grid's own padding, so it has none
 * of its own at the sides.
 */
@Composable
private fun FollowRow(kind: String, act: Do) {
    Row(
        Modifier.fillMaxWidth().pressable(onClick = { act.findShow(kind) }, press = Press.Highlight).padding(vertical = Space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = RoundedCornerShape(Space.corner), modifier = Modifier.size(56.dp)) {
            Box(contentAlignment = Alignment.Center) {
                Icon(Glyphs.Plus, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            }
        }
        Spacer(Modifier.width(Space.l))
        Text(if (kind == "sermon") "Follow Sermons" else "Follow a Podcast", style = MaterialTheme.typography.titleMedium, maxLines = 1)
    }
}

@Composable
private fun Playlists(playlists: List<Playlist>, act: Do) {
    val list = rememberDestinationListState("playlists")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(vertical = Space.s)) {
        item {
            ListRow(onClick = act::newPlaylist, title = "New Playlist", under = null) {
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
        items(albums, key = { it.id }) { a ->
            val hero = rememberHero(Space.corner)
            Column(Modifier.pressable(onClick = hero.lifting { act.open(Page.ForAlbum(a.id)) })) {
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
            ListRow(onClick = hero.lifting { act.open(Page.ForArtist(a.name)) }, title = a.name, under = formatCount(a.tracks, "song")) {
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
            val under = formatSpan(c.born, c.died).ifEmpty { formatCount(c.works, "work") }
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
