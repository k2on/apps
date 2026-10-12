// The three pages a record opens: an album, in the order the work goes and
// in sections where it has parts; an artist, with their albums and then
// their songs; a playlist, in the order it was made. Each opens with a
// header saying what it is and the numbers true of the whole of it, and
// Play and Shuffle under them. The lists are views held while the page
// is shown (`Library.hold`), spliced as the log moves.
package dev.harken.android

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.arkdb.Id

private fun facts(vararg parts: String?): String = parts.filterNotNull().filter { it.isNotEmpty() }.joinToString(" · ")

private fun length(tracks: List<Track>): String? = tracks.sumOf { it.durationMs }.takeIf { it > 0 }?.let(Media::spell)

@Composable
fun AlbumPage(name: String, album: Album?, tracks: List<Track>, details: Map<Id, TrackDetail>, playing: String?, act: Do) {
    val page = Page.ForAlbum(name)
    val creator = album?.creator ?: tracks.firstOrNull()?.creator
    // Sections only where the album has more than one part: a heading over
    // every row of a one-work album would say its name twice.
    val parts = tracks.map { details[it.id]?.part ?: "" }
    val sectioned = parts.distinct().size > 1
    LazyColumn(Modifier.fillMaxSize()) {
        item { PageBar("", onBack = act::back) }
        item {
            PageHeader(
                cover = { Cover(name, album?.art ?: "", Space.header, Modifier.size(Space.header)) },
                kind = "Album",
                title = name,
                by = creator,
                onBy = creator?.let { c -> { act.open(Page.ForArtist(c)) } },
                facts = facts(Media.count(tracks.size, "song"), length(tracks)),
                canPlay = tracks.isNotEmpty(),
                onPlay = { act.play(tracks, 0, page) },
                onShuffle = { act.shuffle(tracks, page) },
            )
        }
        itemsIndexed(tracks, key = { _, t -> t.id.hex }) { i, t ->
            if (sectioned && (i == 0 || parts[i] != parts[i - 1]) && parts[i].isNotEmpty()) {
                Text(
                    parts[i],
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(start = Space.page, end = Space.page, top = Space.l, bottom = Space.xs),
                )
            }
            val d = details[t.id]
            TrackRow(
                t,
                under = d?.performer?.takeIf { it.isNotEmpty() } ?: t.creator,
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, i, page) },
                onMore = { act.more(t, page) },
            ) { TrackNumber(d?.track ?: 0, t.id.hex == playing) }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

@Composable
fun ArtistPage(name: String, artist: Artist?, tracks: List<Track>, albums: List<Album>, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    val page = Page.ForArtist(name)
    LazyColumn(Modifier.fillMaxSize()) {
        item { PageBar("", onBack = act::back) }
        item {
            PageHeader(
                cover = { Cover(name, artist?.art ?: "", 180.dp, Modifier.size(180.dp), round = true) },
                kind = "Artist",
                title = name,
                by = null,
                onBy = null,
                facts = facts(if (albums.isEmpty()) null else Media.count(albums.size, "album"), Media.count(tracks.size, "song"), length(tracks)),
                canPlay = tracks.isNotEmpty(),
                onPlay = { act.play(tracks, 0, page) },
                onShuffle = { act.shuffle(tracks, page) },
            )
        }
        if (albums.isNotEmpty()) {
            item { Heading("Albums") }
            item {
                Shelf(albums, key = { it.name }) { a ->
                    RecordCard(a.name, a.art, a.name, Media.count(a.tracks.toInt(), "song"), 136.dp, onClick = { act.open(Page.ForAlbum(a.name)) })
                }
            }
        }
        item { Heading("Songs") }
        itemsIndexed(tracks, key = { _, t -> t.id.hex }) { i, t ->
            val album = albumOf(t)
            TrackRow(
                t,
                under = album?.name ?: t.creator,
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, i, page) },
                onMore = { act.more(t, page) },
            ) { Cover(album?.name ?: t.title, album?.art ?: "", Space.thumb, Modifier.size(Space.thumb), corner = 4.dp) }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

@Composable
fun PlaylistPage(id: Id, playlist: Playlist?, tracks: List<Track>, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    val page = Page.ForPlaylist(id)
    val name = playlist?.name ?: "Playlist"
    LazyColumn(Modifier.fillMaxSize()) {
        item { PageBar("", onBack = act::back) }
        item {
            PageHeader(
                cover = { Cover(name, "", Space.header, Modifier.size(Space.header)) },
                kind = "Playlist",
                title = name,
                by = null,
                onBy = null,
                facts = facts(Media.count(tracks.size, "song"), length(tracks)),
                canPlay = tracks.isNotEmpty(),
                onPlay = { act.play(tracks, 0, page) },
                onShuffle = { act.shuffle(tracks, page) },
            )
        }
        if (playlist == null) {
            item { Empty(Glyphs.Playlist, "This playlist is gone", "It was taken off this server, or it isn't yours.") }
        } else if (tracks.isEmpty()) {
            item { Empty(Glyphs.Playlist, "Nothing on it yet", "Put a song on it from the song's menu, anywhere in your library.") }
        }
        itemsIndexed(tracks, key = { _, t -> t.id.hex }) { i, t ->
            val album = albumOf(t)
            TrackRow(
                t,
                under = listOfNotNull(t.creator, album?.name).joinToString(" · "),
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, i, page) },
                onMore = { act.more(t, page) },
            ) { Cover(album?.name ?: t.title, album?.art ?: "", Space.thumb, Modifier.size(Space.thumb), corner = 4.dp) }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}
