// Search: one field over the library, answered as you type. Songs are the
// module's own `search` — the rows whose title or creator holds the
// needle, read through its text index — and albums and artists are the
// held lists whose names hold it, so the three groups answer one question.
// With nothing typed it offers the last few searches, which live on this
// phone and nowhere else.
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import dev.arkui.Cover
import dev.arkui.CoverCard
import dev.arkui.Empty
import dev.arkui.Glyph
import dev.arkui.Heading
import dev.arkui.Recompositions
import dev.arkui.ScrollsToTop
import dev.arkui.Shelf
import dev.arkui.Space
import dev.arkui.TabTitle
import dev.arkui.rememberDestinationListState

/** What a needle found, in three groups. */
data class Results(val needle: String, val songs: List<Track>, val albums: List<Album>, val artists: List<Artist>) {
    val isEmpty: Boolean
        get() = songs.isEmpty() && albums.isEmpty() && artists.isEmpty()

    companion object {
        /** The albums and artists whose names hold `needle`, beside the songs `search` answered. */
        fun of(needle: String, songs: List<Track>, albums: List<Album>, artists: List<Artist>): Results {
            val n = needle.trim()
            return Results(
                n,
                songs,
                albums.filter { it.name.contains(n, ignoreCase = true) || it.creator.contains(n, ignoreCase = true) },
                artists.filter { it.name.contains(n, ignoreCase = true) },
            )
        }
    }
}

@Composable
fun SearchScreen(
    needle: String,
    found: Results?,
    recent: List<String>,
    onNeedle: (String) -> Unit,
    onSearched: (String) -> Unit,
    onForget: () -> Unit,
    albumOf: (Track) -> Album?,
    playing: String?,
    offline: String?,
    act: Do,
) {
    Recompositions.hit("SearchScreen")
    val list = rememberDestinationListState("search")
    ScrollsToTop(list)
    LazyColumn(Modifier.fillMaxSize(), state = list) {
        item { TabTitle("Search") }
        item {
            OutlinedTextField(
                value = needle,
                onValueChange = onNeedle,
                placeholder = { Text("Songs, albums, artists") },
                leadingIcon = { Glyph(Glyphs.Search, null, size = 20.dp) },
                trailingIcon = {
                    if (needle.isNotEmpty()) IconButton(onClick = { onNeedle("") }) { Glyph(Glyphs.Close, "Clear", size = 20.dp) }
                },
                singleLine = true,
                shape = RoundedCornerShape(12.dp),
                colors =
                    OutlinedTextFieldDefaults.colors(
                        unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainer,
                        focusedContainerColor = MaterialTheme.colorScheme.surfaceContainer,
                        unfocusedBorderColor = MaterialTheme.colorScheme.surfaceContainer,
                    ),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { onSearched(needle) }),
                modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page),
            )
        }
        if (offline != null) item { Offline(offline) }
        when {
            needle.isBlank() ->
                if (recent.isEmpty()) {
                    item { Empty(Glyphs.Search, "Search your library", "Find a song by its title, or everything by somebody, as you type.") }
                } else {
                    item { Heading("Recent searches", action = "Clear", onAction = onForget) }
                    items(recent, key = { "recent:$it" }) { r ->
                        Row(
                            Modifier.fillMaxWidth().clickable { onNeedle(r) }.padding(horizontal = Space.page).height(48.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Glyph(Glyphs.Search, null, size = 18.dp)
                            Spacer(Modifier.width(Space.l))
                            Text(r, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            // While a new needle is asked, what the last one found stays: no frame of nothing.
            found == null -> item { Spacer(Modifier.height(Space.xl)) }
            found.isEmpty ->
                item { Empty(Glyphs.Search, "Nothing found", "Nothing in your library matches “${found.needle}”.") }
            else -> {
                if (found.songs.isNotEmpty()) {
                    item { Heading("Songs") }
                    items(found.songs.take(SEARCH_SONGS), key = { "song:" + it.id.hex }) { t ->
                        val album = albumOf(t)
                        TrackRow(
                            t,
                            under = listOfNotNull(t.creator, album?.name).joinToString(" · "),
                            playing = t.id.hex == playing,
                            onClick = {
                                onSearched(needle)
                                act.play(found.songs, found.songs.indexOf(t), null)
                            },
                            onMore = { act.more(t, null) },
                        ) { Cover(album?.name ?: t.title, album?.art ?: "", Space.thumb, Modifier.size(Space.thumb), corner = 4.dp) }
                    }
                }
                if (found.albums.isNotEmpty()) {
                    item { Heading("Albums") }
                    item {
                        Shelf("albums", found.albums, key = { it.id }) { a ->
                            CoverCard(a.name, a.art, a.name, a.creator, SEARCH_ALBUM, opens = true, onClick = {
                                onSearched(needle)
                                act.open(Page.ForAlbum(a.id))
                            })
                        }
                    }
                }
                if (found.artists.isNotEmpty()) {
                    item { Heading("Artists") }
                    item {
                        Shelf("artists", found.artists, key = { it.name }) { a ->
                            CoverCard(a.name, a.art, a.name, Media.count(a.tracks.toInt(), "song"), SEARCH_ARTIST, round = true, opens = true, onClick = {
                                onSearched(needle)
                                act.open(Page.ForArtist(a.name))
                            })
                        }
                    }
                }
                item { Spacer(Modifier.height(Space.xl)) }
            }
        }
    }
}

/** How many songs a search lists. */
const val SEARCH_SONGS = 50

/** An album's card in the results, and an artist's. */
val SEARCH_ALBUM = 128.dp
val SEARCH_ARTIST = 112.dp
