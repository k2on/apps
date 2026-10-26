// What is playing, two ways: the mini player above the tab bar — a cover,
// the title, play or pause and the next track, with the progress along its
// foot — and the full player it opens into: the cover, the title and who
// made it, a scrubber with the time gone and the time left, previous,
// play or pause and next, shuffle and repeat, and the queue. They are the
// two faces of one card (PlayerSheet.kt), which draws the cover itself as
// it travels between them. Both draw one [PlayerState]; neither knows
// there is a service behind it, or that the sound may be on another device
// of the account. The position is a flow beside it, read only by the two
// leaves that draw it — the hairline and the scrubber — so a tick
// recomposes those and nothing else; so is where another device's scrubber
// is being dragged to, read by the scrubber alone.
//
// While the phone is part of the listening session the full player says
// where the sound is (`PlayerState.on`): a chip beside the queue's, the
// device's kind and name, that opens the picker; and the mini player says it
// under the title when the sound is somewhere else. The session has no
// shuffle and no repeat, so neither toggle is drawn then.
//
// Under the transport the full player draws what is playing's volume
// (`PlayerState.volume`): a mute and a slider, ark-ui's `VolumeSlider` — the
// group's while the output leads one, the output's while it is another
// device, this phone's own while the sound is here or nowhere, and its media
// stream while it is its own session. The mini player draws none.
package dev.harken.android

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.harken.android.ui.Cover
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.Space
import dev.harken.android.ui.VolumeSlider
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberTransientListState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** What the player's controls do. */
interface Controls {
    fun open() {}

    fun close() {}

    fun toggle() {}

    fun next() {}

    fun previous() {}

    fun seek(ms: Long) {}

    fun shuffle() {}

    fun repeat() {}

    fun queue() {}

    fun skipAhead(n: Int) {}

    fun more() {}

    /** Open a page — what plays' album or its person's — closing the player for it. */
    fun goTo(page: Page) {}

    /** Open the device picker. */
    fun devices() {}

    /** The scrubber is being dragged to `ms`: said to the session's other devices. */
    fun scrub(ms: Long) {}

    /** What is playing's volume dragged to `level`, 0 to 100. */
    fun volume(level: Int) {}

    /** …and let go. */
    fun volumeDone() {}

    /** What is playing's mute, the other way from how it is. */
    fun mute() {}
}

/** Controls that do nothing: what a screenshot hands the player. */
object Still : Controls

/** A position that never moves: the start of the track, or `ms` into it (a screenshot's). */
fun still(ms: Long = 0): StateFlow<Long> = MutableStateFlow(ms)

/** Nobody else scrubbing — or, for a screenshot, another device holding its scrubber at `ms`. */
fun nobody(ms: Long? = null): StateFlow<Long?> = MutableStateFlow(ms)

private val Start = still()

private val Nobody = nobody()

/** What the device chip says of where the sound is. */
fun Where.label(): String =
    when {
        moving -> "Connecting to $name…"
        away -> "$name (not answering)"
        else -> name
    }

private fun seedOf(p: Playing): String = p.album.ifEmpty { p.title }

/** The bar as a card of its own: what a screenshot draws. In the app it is the player's collapsed face. */
@Composable
fun MiniPlayer(state: PlayerState, controls: Controls, position: StateFlow<Long> = Start) {
    if (state.now == null) return
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = RoundedCornerShape(Space.corner),
        shadowElevation = 2.dp,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.s, vertical = Space.xs).clickable(onClick = controls::open),
    ) {
        MiniFace(state, controls, position)
    }
}

/**
 * The bar's face: the cover, the title, play or pause and the next track, with the progress along its
 * foot. Without its `art` when the player draws the art itself, travelling between the two faces.
 */
@Composable
fun MiniFace(state: PlayerState, controls: Controls, position: StateFlow<Long> = Start, art: Boolean = true, modifier: Modifier = Modifier) {
    val now = state.now ?: return
    Recompositions.hit("MiniPlayer")
    Column(modifier) {
        Row(Modifier.padding(start = Space.s, end = Space.xs).height(56.dp), verticalAlignment = Alignment.CenterVertically) {
            if (art) Cover(seedOf(now), now.art, 40.dp, Modifier.size(40.dp), corner = 4.dp) else Spacer(Modifier.size(40.dp))
            Spacer(Modifier.width(Space.m))
            Column(Modifier.weight(1f)) {
                Text(now.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val on = state.on
                // The sound somewhere else: said where the artist was, in the accent.
                if (on != null && !on.here)
                    Text(
                        if (on.moving || on.away) on.label() else "On ${on.name}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.primary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                else Text(now.artist, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            IconButton(onClick = controls::toggle) {
                Glyph(if (state.playing) Glyphs.Pause else Glyphs.Play, if (state.playing) "Pause" else "Play", size = 20.dp)
            }
            IconButton(onClick = controls::next, enabled = state.upNext.isNotEmpty() || state.repeat != Repeat.Off) {
                Glyph(Glyphs.Next, "Next", size = 20.dp)
            }
        }
        Hairline(state.durationMs, position)
    }
}

// The progress, as a hairline along the foot: the one part of the mini
// player that reads the position, so the one part a tick recomposes.
@Composable
private fun Hairline(durationMs: Long, position: StateFlow<Long>) {
    val at by position.collectAsStateWithLifecycle()
    BoxWithConstraints(Modifier.fillMaxWidth().height(2.dp).padding(horizontal = Space.s)) {
        // Counted here, where the position is read: a tick recomposes this and not even the
        // function around it.
        Recompositions.hit("Progress")
        val done = if (durationMs > 0) (at.toFloat() / durationMs).coerceIn(0f, 1f) else 0f
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.outlineVariant))
        Box(Modifier.width(maxWidth * done).height(2.dp).background(MaterialTheme.colorScheme.primary))
    }
}

/** The full player on a page of its own: what a screenshot draws. In the app it is the player's open face. */
@Composable
fun FullPlayer(state: PlayerState, controls: Controls, position: StateFlow<Long> = Start, scrubbing: StateFlow<Long?> = Nobody) {
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) { FullFace(state, controls, position, scrubbing = scrubbing) }
}

/**
 * The full player's face. With a `slot`, the cover's place is left empty and `slot` is told where it
 * is: the player draws the art there itself, having brought it up from the bar.
 */
@Composable
fun FullFace(
    state: PlayerState,
    controls: Controls,
    position: StateFlow<Long> = Start,
    slot: Modifier? = null,
    modifier: Modifier = Modifier,
    scrubbing: StateFlow<Long?> = Nobody,
) {
    val now = state.now
    Column(modifier.fillMaxSize().safeDrawingPadding().padding(horizontal = Space.xl)) {
        Row(Modifier.fillMaxWidth().height(56.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = controls::close) { Glyph(Glyphs.Down, "Close the player") }
            Text(
                if (now?.album.isNullOrEmpty()) "Now playing" else now!!.album,
                style = quiet(MaterialTheme.typography.labelLarge),
                textAlign = TextAlign.Center,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = controls::more, enabled = now != null) { Glyph(Glyphs.More, "More") }
        }
        Spacer(Modifier.weight(0.5f))
        if (now != null && slot != null) Spacer(Modifier.fillMaxWidth().aspectRatio(1f).then(slot))
        else if (now != null) Cover(seedOf(now), now.art, 360.dp, Modifier.fillMaxWidth().aspectRatio(1f), corner = 12.dp)
        Spacer(Modifier.weight(0.5f))
        // The title opens its album and the name its person's page, as the desktop's bar and the
        // iPhone's player do.
        val album = now?.album.orEmpty()
        val artist = now?.artist.orEmpty()
        Text(
            now?.title ?: "Nothing playing",
            style = MaterialTheme.typography.headlineSmall,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = if (album.isEmpty()) Modifier else Modifier.clickable { controls.goTo(Page.ForAlbum(album)) },
        )
        Text(
            artist,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            maxLines = 1,
            modifier = if (artist.isEmpty()) Modifier else Modifier.clickable { controls.goTo(Page.ForArtist(artist)) },
        )
        Spacer(Modifier.height(Space.l))
        Scrubber(state, position, scrubbing, controls)
        // The session has no shuffle and no repeat: neither is drawn while the phone is part of it,
        // and the transport keeps its place.
        val alone = state.on == null
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            if (alone) Toggle(Glyphs.Shuffle, if (state.shuffle) "Shuffle is on" else "Shuffle is off", state.shuffle, controls::shuffle)
            else Spacer(Modifier.size(48.dp))
            IconButton(onClick = controls::previous, modifier = Modifier.size(56.dp)) { Glyph(Glyphs.Previous, "Previous", size = 28.dp) }
            FilledIconButton(
                onClick = controls::toggle,
                modifier = Modifier.size(72.dp),
                shape = CircleShape,
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
            ) {
                Glyph(if (state.playing) Glyphs.Pause else Glyphs.Play, if (state.playing) "Pause" else "Play", size = 30.dp)
            }
            IconButton(onClick = controls::next, modifier = Modifier.size(56.dp)) { Glyph(Glyphs.Next, "Next", size = 28.dp) }
            if (alone) Toggle(
                if (state.repeat == Repeat.One) Glyphs.RepeatOne else Glyphs.Repeat,
                when (state.repeat) {
                    Repeat.Off -> "Repeat is off"
                    Repeat.All -> "Repeating the queue"
                    Repeat.One -> "Repeating this song"
                },
                state.repeat != Repeat.Off,
                controls::repeat,
            )
            else Spacer(Modifier.size(48.dp))
        }
        state.volume?.let { v ->
            Spacer(Modifier.height(Space.s))
            VolumeSlider(v.level, v.muted, controls::volume, controls::volumeDone, controls::mute, Modifier.fillMaxWidth(), about = state.on?.name ?: "this phone")
        }
        Spacer(Modifier.height(Space.l))
        val on = state.on
        if (on == null) Box(Modifier.align(Alignment.CenterHorizontally)) { QueueChip(state, controls) }
        else
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                DeviceChip(on, controls, Modifier.weight(1f, fill = false))
                Spacer(Modifier.width(Space.s))
                QueueChip(state, controls, Modifier.weight(1f, fill = false))
            }
        Spacer(Modifier.height(Space.l))
    }
}

// What plays after this: the queue's sheet.
@Composable
private fun QueueChip(state: PlayerState, controls: Controls, modifier: Modifier = Modifier) {
    Row(
        modifier.clip(RoundedCornerShape(50)).clickable(onClick = controls::queue).padding(horizontal = Space.l, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Glyph(Glyphs.Queue, null, size = 18.dp)
        Spacer(Modifier.width(Space.s))
        Text(if (state.upNext.isEmpty()) "Nothing up next" else "Up next: ${state.upNext.first().title}", style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

// Where the sound is, and the way to move it: lit while it is this phone's — or nowhere, which is
// where the next press plays.
@Composable
private fun DeviceChip(on: Where, controls: Controls, modifier: Modifier = Modifier) {
    val tint = if (on.here && !on.away) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
    Row(
        modifier.clip(RoundedCornerShape(50)).clickable(onClick = controls::devices).padding(horizontal = Space.l, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Glyphs.device(on.kind, on.name), contentDescription = "Playing on", tint = tint, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(Space.s))
        Text(on.label(), style = MaterialTheme.typography.labelLarge, color = tint, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

// A scrubber that follows the finger while it is dragged, another device's while one is dragging
// it, and the player otherwise: the time gone under its start and the time left under its end.
// Dragged, it says where to the session's other devices (`scrub`), and seeks on release.
@Composable
private fun Scrubber(state: PlayerState, position: StateFlow<Long>, scrubbing: StateFlow<Long?>, controls: Controls) {
    val ms by position.collectAsStateWithLifecycle()
    val theirs by scrubbing.collectAsStateWithLifecycle()
    Recompositions.hit("Progress")
    var dragging by remember { mutableStateOf<Float?>(null) }
    val total = state.durationMs.coerceAtLeast(1)
    val at = dragging ?: ((theirs ?: ms).toFloat() / total).coerceIn(0f, 1f)
    Slider(
        value = at,
        onValueChange = {
            dragging = it
            controls.scrub((it * total).toLong())
        },
        onValueChangeFinished = {
            dragging?.let { controls.seek((it * total).toLong()) }
            dragging = null
        },
        enabled = state.durationMs > 0,
        colors =
            SliderDefaults.colors(
                thumbColor = MaterialTheme.colorScheme.primary,
                activeTrackColor = MaterialTheme.colorScheme.primary,
                inactiveTrackColor = MaterialTheme.colorScheme.outlineVariant,
            ),
    )
    val gone = (at * total).toLong()
    Row(Modifier.fillMaxWidth()) {
        Text(Media.clock(gone), style = quiet(MaterialTheme.typography.labelMedium), modifier = Modifier.weight(1f))
        Text("-" + Media.clock((state.durationMs - gone).coerceAtLeast(0)), style = quiet(MaterialTheme.typography.labelMedium))
    }
}

@Composable
private fun Toggle(icon: androidx.compose.ui.graphics.vector.ImageVector, description: String, on: Boolean, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(
            icon,
            contentDescription = description,
            tint = if (on) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(22.dp),
        )
    }
}

/** The queue: what is playing, and what plays after it in the order it will. */
@Composable
fun QueueSheet(state: PlayerState, controls: Controls) {
    LazyColumn(Modifier.fillMaxWidth().padding(bottom = Space.xl), state = rememberTransientListState()) {
        item { Text("Now playing", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.s)) }
        state.now?.let { now -> item { QueueRow(now, current = true, onClick = {}) } }
        item {
            Text(
                if (state.shuffle) "Up next, shuffled" else "Up next",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(start = Space.xl, end = Space.xl, top = Space.l, bottom = Space.s),
            )
        }
        if (state.upNext.isEmpty()) {
            item {
                Text(
                    "Nothing after this one.",
                    style = quiet(MaterialTheme.typography.bodyMedium),
                    modifier = Modifier.padding(horizontal = Space.xl),
                )
            }
        }
        itemsIndexed(state.upNext) { i, p -> QueueRow(p, current = false, onClick = { controls.skipAhead(i) }) }
    }
}

@Composable
private fun QueueRow(p: Playing, current: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = Space.xl, vertical = Space.s), verticalAlignment = Alignment.CenterVertically) {
        Cover(seedOf(p), p.art, 40.dp, Modifier.size(40.dp), corner = 4.dp)
        Spacer(Modifier.width(Space.m))
        Column(Modifier.weight(1f)) {
            Text(
                p.title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(p.artist, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (current) Icon(Glyphs.Playing, contentDescription = "Playing", tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
    }
}
