// What rises over a page: a song's menu (play it next, put it on a
// playlist, go to its album or its artist, take it off the playlist it was
// opened from), the playlists a song can go on — each ticked when it is
// there, a tap putting it on or taking it off — and naming a new playlist,
// checked as it is typed by `create_playlist`'s own input checks, whose
// sentence is shown under the field. And the device picker: where the
// sound is, where it could go, each device's volume and the speakers
// joined. And finding a show to follow, from a shows page's +: Apple's
// podcast directory searched as it is typed (Directory.kt), or a feed's
// address pasted.
package dev.harken.android

import uniffi.harken_domain.Playlist
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.harken.android.ui.Cover
import dev.harken.android.ui.SheetRow
import dev.harken.android.ui.Space
import dev.harken.android.ui.VolumeSlider
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberTransientListState
import dev.harken.android.ui.rememberTransientScrollState
import kotlinx.coroutines.delay

/** What a song's menu can do. */
interface MenuActions {
    fun playNext() {}

    fun addToQueue() {}

    fun addTo() {}

    fun goTo(page: Page) {}

    fun remove() {}

    /** An episode said played (`true`) or not. */
    fun markPlayed(finished: Boolean) {}

    /** An episode from this far in: one of its chapters. */
    fun playFrom(ms: Long) {}
}

/**
 * A song's menu. `from` is the page its row was on: a playlist offers to take it off. An episode
 * goes to its `show` rather than to an artist.
 */
@Composable
fun TrackMenu(
    track: Track,
    album: Album?,
    from: Page?,
    actions: MenuActions,
    show: Show? = null,
    listened: Listened? = null,
    chapters: List<Chapter> = emptyList(),
) {
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
        SheetRow(Glyphs.Queue, "Add to queue", actions::addToQueue)
        SheetRow(Glyphs.AddTo, "Add to playlist", actions::addTo, trailing = { Icon(Glyphs.Chevron, null, Modifier.size(18.dp)) })
        if (album != null && from != Page.ForAlbum(album.name)) SheetRow(Glyphs.Album, "Go to album", { actions.goTo(Page.ForAlbum(album.name)) })
        if (show != null && from != Page.ForShow(show.id)) SheetRow(Glyphs.Show, "Go to show", { actions.goTo(Page.ForShow(show.id)) })
        if (track.kind == "song" && track.creator.isNotEmpty() && from != Page.ForArtist(track.creator))
            SheetRow(Glyphs.Artist, "Go to artist", { actions.goTo(Page.ForArtist(track.creator)) })
        if (from is Page.ForPlaylist) SheetRow(Glyphs.Remove, "Remove from this playlist", actions::remove)
        // An episode: played or not by hand, and each of its chapters to start from.
        if (track.kind != "song") {
            val played = listened?.finished == true
            SheetRow(Glyphs.Show, if (played) "Mark as unplayed" else "Mark as played", { actions.markPlayed(!played) })
            for (c in chapters) SheetRow(Glyphs.Play, "${Media.clock(c.startMs)}  ${c.title}", { actions.playFrom(c.startMs) })
        }
    }
}

/** Every playlist, ticked where `track` is on it; "New playlist" first. */
@Composable
fun AddToPlaylist(track: Track, playlists: List<Playlist>, on: Set<Id>?, onToggle: (Playlist) -> Unit, onNew: () -> Unit) {
    LazyColumn(Modifier.fillMaxWidth().padding(bottom = Space.l), state = rememberTransientListState()) {
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

/** What the device picker's rows can do: hand the sound over, and each row's volume and group. */
interface PickerActions {
    /** The name tapped: the sound to that device. */
    fun pick(row: DeviceRow) {}

    /** A row's slider dragged to `level`, 0 to 100. */
    fun level(row: DeviceRow, level: Int) {}

    /** …and let go. */
    fun levelDone() {}

    /** A row's mute, the other way from how it is. */
    fun mute(row: DeviceRow) {}

    /** A row's `+` or `−`: the speaker joins the group that plays, or leaves it. */
    fun join(row: DeviceRow) {}
}

/**
 * Where the sound is, and everywhere it could be (docs/plan-live.md): a row per device of this
 * account, ticked where the sound is — every speaker of the group that plays, when it is one — or,
 * while it is nowhere, on this phone, which is where the next press plays — the desktop's picker
 * (23c40a0), with no row that stops it everywhere, since pausing is the transport's. A device that
 * cannot make a sound, or whose socket has gone, is drawn and not selectable: the laptop the music
 * still belongs to is the one row most worth drawing. The cursor starts on the output, or on this
 * phone, so a tap straight away changes nothing.
 *
 * A row with a volume carries its slider and its mute under its name; a speaker that can join what
 * plays a `+`, one in its group a `−`; and while the output leads a group, the group comes first as
 * itself — "Kitchen + 1" — its slider every member's. The name hands the sound over; the controls
 * beside and under it are their own targets.
 */
@Composable
fun DevicesSheet(rows: List<DeviceRow>, cursor: Int, actions: PickerActions) {
    // Scrolled, for a house of speakers taller than the screen; a sheet's place is not kept.
    Column(Modifier.fillMaxWidth().verticalScroll(rememberTransientScrollState()).padding(bottom = Space.l)) {
        Text("Playing on", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.s))
        if (rows.isEmpty())
            Text(
                "Nothing else is listening. A device signed in as you appears here once it is open.",
                style = quiet(MaterialTheme.typography.bodyMedium),
                modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.s),
            )
        rows.forEachIndexed { i, r -> DeviceLine(r, at = i == cursor, actions) }
    }
}

// One device: its kind, its name, what it cannot do, its join, a tick where the sound is — and
// under it, where it has one, its volume.
@Composable
private fun DeviceLine(r: DeviceRow, at: Boolean, actions: PickerActions) {
    val ink =
        if ((r.ticked || r.group) && !r.away) MaterialTheme.colorScheme.primary
        else if (r.takeable) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant
    val under =
        when {
            r.group -> "every speaker in the group"
            r.away -> "not answering"
            r.moving -> "connecting…"
            !r.takeable -> "can't make a sound"
            r.me -> "this phone"
            else -> null
        }
    Column(Modifier.fillMaxWidth().let { if (at) it.background(MaterialTheme.colorScheme.surfaceContainerHighest) else it }) {
        Row(
            Modifier.fillMaxWidth()
                .let { if (r.takeable) it.clickable { actions.pick(r) } else it }
                .padding(horizontal = Space.xl)
                .height(60.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Glyphs.device(r.kind, r.name), contentDescription = null, tint = ink, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(Space.l))
            Column(Modifier.weight(1f)) {
                Text(r.name, style = MaterialTheme.typography.bodyLarge, color = ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (under != null) Text(under, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1)
            }
            r.join?.let { join ->
                IconButton(onClick = { actions.join(r) }) {
                    Icon(
                        if (join) Glyphs.Plus else Glyphs.Minus,
                        contentDescription = if (join) "Add ${r.name} to the group" else "Take ${r.name} out of the group",
                        tint = MaterialTheme.colorScheme.onSurface,
                        modifier = Modifier.size(20.dp),
                    )
                }
            }
            if (r.group) Icon(Glyphs.Playing, contentDescription = "Playing", tint = ink, modifier = Modifier.size(20.dp))
            else if (r.ticked) Icon(Glyphs.Tick, contentDescription = "Playing here", tint = ink, modifier = Modifier.size(20.dp))
            // A join button keeps its column whether or not the row is ticked.
            else if (r.join != null) Spacer(Modifier.size(20.dp))
        }
        r.volume?.let { level ->
            // Under the name, from where the name starts: the glyph's column and its gap.
            VolumeSlider(
                level,
                r.muted,
                { actions.level(r, it) },
                actions::levelDone,
                { actions.mute(r) },
                Modifier.fillMaxWidth().padding(start = Space.xl + 22.dp + Space.l - 12.dp, end = Space.xl).padding(bottom = Space.xs),
                about = r.name,
            )
        }
    }
}

/**
 * Finding a show to follow, from a shows page's +: the box — the keys in it as the sheet rises —
 * searched in Apple's podcast directory a moment after the typing stops, or a feed's address
 * pasted, which is the one show it names; each show found with its picture, who makes it and its
 * Follow, which says Following once the library follows that feed. Followed as `kind`, the kind
 * of show the page lists: what `onFollow` is handed is the feed.
 */
@Composable
fun FindShow(kind: String, feeds: List<Subscription>, onFollow: (String) -> Unit) {
    var query by remember { mutableStateOf("") }
    var found by remember { mutableStateOf(emptyList<Directory.Found>()) }
    // What the search came to when it is not a list: searching, nothing found, or why it failed.
    var said by remember { mutableStateOf<String?>(null) }
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    // What opening it was for is typing.
    LaunchedEffect(Unit) { focus.requestFocus() }
    // A key typed starts this again, which cancels the last: a search goes only once the typing
    // has paused, and an answer for words the box no longer says is never shown.
    LaunchedEffect(query) {
        said = null
        if (query.isBlank() || Directory.isFeed(query)) {
            found = emptyList()
            return@LaunchedEffect
        }
        delay(Directory.PAUSE)
        said = "Searching…"
        Directory.search(query).fold(
            onSuccess = { shows ->
                found = shows
                said = if (shows.isEmpty()) "Nothing found for “${query.trim()}”." else null
            },
            onFailure = { e ->
                found = emptyList()
                said = e.message ?: "Could not search."
            },
        )
    }
    val rows = Directory.rows(query, found)
    Column(Modifier.fillMaxWidth()) {
        Text(
            if (kind == "sermon") "Add sermons" else "Add a podcast",
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.s),
        )
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text("Search Apple Podcasts, or paste a feed") },
            leadingIcon = { Icon(Glyphs.Search, contentDescription = null, modifier = Modifier.size(20.dp)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { keyboard?.hide() }),
            modifier = Modifier.fillMaxWidth().padding(horizontal = Space.xl).focusRequester(focus),
        )
        AnimatedVisibility(visible = said != null) {
            Text(said.orEmpty(), style = quiet(MaterialTheme.typography.bodyMedium), modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.m))
        }
        LazyColumn(Modifier.fillMaxWidth(), state = rememberTransientListState(), contentPadding = PaddingValues(top = Space.s, bottom = Space.l)) {
            items(rows, key = { it.feed }) { s ->
                // Each comes in, and goes, gently, as the answers for the words change under it.
                FoundShow(s, Directory.following(s, feeds), onFollow = { onFollow(s.feed) }, modifier = Modifier.animateItem())
            }
        }
    }
}

// One show the directory found: its picture, its title, who makes it, and its Follow — or, once
// followed, the tick.
@Composable
private fun FoundShow(s: Directory.Found, following: Boolean, onFollow: () -> Unit, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().padding(horizontal = Space.xl, vertical = Space.s), verticalAlignment = Alignment.CenterVertically) {
        Cover(s.title, s.art, 56.dp, Modifier.size(56.dp), corner = 6.dp)
        Spacer(Modifier.width(Space.m))
        Column(Modifier.weight(1f)) {
            Text(s.title, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (s.author.isNotEmpty()) Text(s.author, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.width(Space.s))
        Crossfade(following, label = "following") { on ->
            if (on) {
                Row(Modifier.height(40.dp).padding(horizontal = Space.s), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Glyphs.Tick, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(Space.xs))
                    Text("Following", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                }
            } else {
                FilledTonalButton(onClick = onFollow) { Text("Follow") }
            }
        }
    }
}
