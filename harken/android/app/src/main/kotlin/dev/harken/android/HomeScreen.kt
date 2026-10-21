// Home: where you were, and what is new. What was played last and a way
// back to it — the same list, from the same track — then what arrived most
// recently, the person's playlists, and the albums, each a shelf of covers
// opening its page. Settings is the one thing in its bar.
//
// **What was played last is the account's, not the phone's** (docs/plan-live.md):
// the listening session's track while there is a session to be part of — what
// is playing, or paused, on whichever device — and otherwise the newest play
// in the log (`last_played`), which is the same on every device and is there
// offline. Nothing of it is kept on the phone.
package dev.harken.android

import dev.harken.android.gen.Playlist
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
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.harken.android.ui.Cover
import dev.harken.android.ui.CoverCard
import dev.harken.android.ui.Empty
import dev.harken.android.ui.FETCHING_DELAY_MS
import dev.harken.android.ui.Fetching
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.Heading
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.ScrollsToTop
import dev.harken.android.ui.Shelf
import dev.harken.android.ui.Space
import dev.harken.android.ui.TabTitle
import dev.harken.android.ui.destinationKeepsAPlace
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberDestinationListState

/**
 * What was played last, and the page it was started from, so Home can offer it back. `live` when it
 * is the listening session's own track — then `on` says where it is playing, if it is.
 */
data class LastPlayed(val track: Playing, val from: Page?, val live: Boolean = false, val on: String? = null)

/**
 * Home's last played: the session's track while this phone is part of a session that has one,
 * else the newest play in the log. `art` is an album's cover by its name; `server` what it is
 * joined to.
 */
fun lastPlayed(h: Heard, played: Played?, playlists: List<Playlist>, art: (String) -> String, server: String, here: String): LastPlayed? {
    fun cover(album: String): String = art(album).takeIf { it.isNotEmpty() }?.let { Media.url(server, it) } ?: ""
    val a = if (h.joined) h.listening?.active else null
    if (a != null) {
        val t = a.now
        val s = h.listening!!
        val on =
            if (!s.sounding) null
            else (a.output as? dev.harken.android.gen.Output.Device)?.id?.let { id ->
                if (id == h.me) "Playing on $here" else "Playing on ${s.devices[id]?.name ?: "another device"}"
            }
        return LastPlayed(Playing(t.id.hex, t.title, t.creator, t.album, cover(t.album)), Page.ofPlace(a.from, playlists), live = true, on = on)
    }
    val p = played ?: return null
    return LastPlayed(Playing(p.track?.hex ?: "", p.title, p.creator, p.album, cover(p.album)), Page.ofPlace(p.from, playlists))
}

/** What Home draws: a snapshot, its lists copies, so an equal one is the same one to Compose. */
@Immutable
data class HomeData(
    val loaded: Boolean,
    val last: LastPlayed?,
    /** Every song, in the order added: the newest are its end. */
    val tracks: List<Track>,
    val playlists: List<Playlist>,
    val albums: List<Album>,
    /** The server's host while it cannot be reached; null while it can. */
    val offline: String?,
    /** The server's host while the library has never arrived from it; null once it has. */
    val fetching: String? = null,
)

/** How many of the newest songs Home's shelf shows. */
const val RECENT = 12

@Composable
fun HomeScreen(data: HomeData, albumOf: (Track) -> Album?, act: Do, placeholderDelayMs: Long = FETCHING_DELAY_MS) {
    Recompositions.hit("HomeScreen")
    // Derived once per library, not once per composition: the shelf's cards skip only while the
    // list they were given is the same one.
    val recent = remember(data.tracks) { data.tracks.takeLast(RECENT).reversed() }
    // Come back to with a place to go back to and nothing read yet — after the process was
    // killed, under the splash — nothing, rather than a list too short to hold the place.
    if (!data.loaded && destinationKeepsAPlace()) return
    val list = rememberDestinationListState("home")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list) {
        item {
            TabTitle("Home") { IconButton(onClick = act::settings) { Glyph(Glyphs.Settings, "Settings") } }
        }
        if (data.offline != null) item { Offline(data.offline) }
        when {
            // Not read yet: the splash is over this for a remembered launch, and past its
            // ceiling the shell shows with what it has — the lists fill in.
            !data.loaded -> {}
            data.tracks.isEmpty() && data.fetching != null -> item { Fetching(data.fetching, placeholderDelayMs) }
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
                    item { LastCard(last, last.on ?: from, onResume = { act.resume(last) }) }
                }
                item { Heading("Recently added", action = "All songs", onAction = { act.library(LibraryTab.Songs) }) }
                item {
                    Shelf("recent", recent, key = { it.id.hex }) { t ->
                        val album = albumOf(t)
                        CoverCard(album?.name ?: t.title, album?.art ?: "", t.title, t.creator, Space.shelf, onClick = {
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
                        Shelf("playlists", data.playlists, key = { it.id.hex }) { p ->
                            CoverCard(p.name, "", p.name, "Playlist", Space.shelf, onClick = { act.open(Page.ForPlaylist(p.id)) }, opens = true)
                        }
                    }
                }
                if (data.albums.isNotEmpty()) {
                    item { Heading("Albums", action = "See all", onAction = { act.library(LibraryTab.Albums) }) }
                    item {
                        Shelf("albums", data.albums, key = { it.name }) { a ->
                            CoverCard(a.name, a.art, a.name, a.creator, Space.shelf, onClick = { act.open(Page.ForAlbum(a.name)) }, opens = true)
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
