// What rises over a page: a song's menu (play it next, put it on a
// playlist, go to its album or its artist, take it off the playlist it was
// opened from), the playlists a song can go on — each ticked when it is
// there, a tap putting it on or taking it off — and naming a new playlist,
// checked as it is typed by `create_playlist`'s own input checks, whose
// sentence is shown under the field. And the device picker: where the
// sound is, where it could go, each device's volume and the speakers
// joined.
package dev.harken.android

import dev.harken.android.gen.Playlist
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import dev.arkdb.ffi.Id
import dev.harken.android.ui.Cover
import dev.harken.android.ui.SheetRow
import dev.harken.android.ui.Space
import dev.harken.android.ui.VolumeSlider
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberTransientListState
import dev.harken.android.ui.rememberTransientScrollState

/** What a song's menu can do. */
interface MenuActions {
    fun playNext() {}

    fun addToQueue() {}

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
        SheetRow(Glyphs.Queue, "Add to queue", actions::addToQueue)
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
