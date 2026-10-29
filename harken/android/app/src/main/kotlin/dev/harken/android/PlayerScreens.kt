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
//
// An episode with a transcript has a Transcript beside the queue's chip,
// which opens the transcript's sheet ([TranscriptSheet]): its lines followed
// along by the same position flow, the one being said kept at the middle.
package dev.harken.android

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.BasicAlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.harken.android.ui.Cover
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.ReorderableItem
import dev.harken.android.ui.Space
import dev.harken.android.ui.VolumeSlider
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberReorder
import dev.harken.android.ui.rememberTransientListState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first

/** How far an episode's skip goes back, and on, in seconds: what a podcast player's do. */
const val JOG_BACK = 15
const val JOG_FORWARD = 30

/** What the player's controls do. */
interface Controls {
    fun open() {}

    fun close() {}

    fun toggle() {}

    fun next() {}

    fun previous() {}

    /**
     * Whether what plays is an episode of a show, whose transport skips back [JOG_BACK] and on
     * [JOG_FORWARD] seconds rather than a track.
     */
    fun jogs(): Boolean = false

    /** Back or on this many seconds in what plays. */
    fun jog(seconds: Int) {}

    fun seek(ms: Long) {}

    fun shuffle() {}

    fun repeat() {}

    fun queue() {}

    fun skipAhead(n: Int) {}

    /** The `from`th of [PlayerState.upNext] moved to be the `to`th, what is playing playing on. */
    fun moveUpNext(from: Int, to: Int) {}

    fun more() {}

    /** Open a page — what plays' album or its person's — closing the player for it. */
    fun goTo(page: Page) {}

    /** The page of the album `now` is on, found by its track — a name is not one album's; null for none. */
    fun albumOf(now: Playing): Page? = null

    /** Open the device picker. */
    fun devices() {}

    /** Open what plays' transcript, following along. */
    fun transcript() {}

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
            if (controls.jogs())
                IconButton(onClick = { controls.jog(JOG_FORWARD) }) { Glyph(Glyphs.SkipForward, "Forward $JOG_FORWARD seconds", size = 20.dp) }
            else
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
/** The chapter `position` is in, read on its own so the ticks recompose this line alone. */
@Composable
private fun ChapterLine(chapters: List<Chapter>, position: StateFlow<Long>) {
    val at by position.collectAsStateWithLifecycle()
    Chapter.at(chapters, at)?.title?.takeIf { it.isNotEmpty() }?.let {
        Text(it, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

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
        val album = now?.let(controls::albumOf)
        val artist = now?.artist.orEmpty()
        Text(
            now?.title ?: "Nothing playing",
            style = MaterialTheme.typography.headlineSmall,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = if (album == null) Modifier else Modifier.clickable { controls.goTo(album) },
        )
        Text(
            artist,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            maxLines = 1,
            modifier = if (artist.isEmpty()) Modifier else Modifier.clickable { controls.goTo(Page.ForArtist(artist)) },
        )
        // The chapter it is in, for an episode that has them.
        val chapters = LocalChapters.current
        if (chapters.isNotEmpty()) ChapterLine(chapters, position)
        Spacer(Modifier.height(Space.l))
        Scrubber(state, position, scrubbing, controls)
        // The session has no shuffle and no repeat: neither is drawn while the phone is part of it,
        // and the transport keeps its place.
        val alone = state.on == null
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            if (alone) Toggle(Glyphs.Shuffle, if (state.shuffle) "Shuffle is on" else "Shuffle is off", state.shuffle, controls::shuffle)
            else Spacer(Modifier.size(48.dp))
            // An episode skips back and on a few seconds; a song goes a track.
            val jogs = controls.jogs()
            if (jogs) IconButton(onClick = { controls.jog(-JOG_BACK) }, modifier = Modifier.size(56.dp)) { Glyph(Glyphs.SkipBack, "Back $JOG_BACK seconds", size = 28.dp) }
            else IconButton(onClick = controls::previous, modifier = Modifier.size(56.dp)) { Glyph(Glyphs.Previous, "Previous", size = 28.dp) }
            FilledIconButton(
                onClick = controls::toggle,
                modifier = Modifier.size(72.dp),
                shape = CircleShape,
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
            ) {
                Glyph(if (state.playing) Glyphs.Pause else Glyphs.Play, if (state.playing) "Pause" else "Play", size = 30.dp)
            }
            if (jogs) IconButton(onClick = { controls.jog(JOG_FORWARD) }, modifier = Modifier.size(56.dp)) { Glyph(Glyphs.SkipForward, "Forward $JOG_FORWARD seconds", size = 28.dp) }
            else IconButton(onClick = controls::next, modifier = Modifier.size(56.dp)) { Glyph(Glyphs.Next, "Next", size = 28.dp) }
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
        // An episode with a transcript has it beside the queue: named while there is room.
        val lines = LocalTranscript.current.isNotEmpty()
        if (on == null && !lines) Box(Modifier.align(Alignment.CenterHorizontally)) { QueueChip(state, controls) }
        else
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                if (on != null) {
                    DeviceChip(on, controls, Modifier.weight(1f, fill = false))
                    Spacer(Modifier.width(Space.s))
                }
                if (lines) {
                    TranscriptChip(controls, named = on == null)
                    Spacer(Modifier.width(Space.s))
                }
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

// What is being said, a line at a time: the transcript's sheet. Its glyph alone beside the device's chip.
@Composable
private fun TranscriptChip(controls: Controls, named: Boolean) {
    Row(
        Modifier.clip(RoundedCornerShape(50)).clickable(onClick = controls::transcript).padding(horizontal = Space.l, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Glyph(Glyphs.Transcript, if (named) null else "Transcript", size = 18.dp)
        if (named) {
            Spacer(Modifier.width(Space.s))
            Text("Transcript", style = MaterialTheme.typography.labelLarge, maxLines = 1)
        }
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

/**
 * The queue: what is playing, and what plays after it in the order it will. A row of what plays next
 * held and dragged moves it there, the rest gliding out of its way (`ui/Reorder.kt`).
 */
@Composable
fun QueueSheet(state: PlayerState, controls: Controls) {
    val list = rememberTransientListState()
    val reorder = rememberReorder(list) { from, to -> controls.moveUpNext(from, to) }
    // Each row of what plays next by the track and which time it is queued, so a track queued twice
    // is two rows, and a row keeps its key as it moves.
    val keyed = remember(state.upNext) { upNextKeys(state.upNext).zip(state.upNext) }
    val keys = remember(keyed) { keyed.map { it.first } }
    val drawn = reorder.order(keyed) { it.first }
    LazyColumn(Modifier.fillMaxWidth().padding(bottom = Space.xl), state = list) {
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
        itemsIndexed(drawn, key = { _, (key, _) -> key }) { _, (key, p) ->
            ReorderableItem(reorder, key, keys) {
                // A tap plays the row where the app has it, whatever the drawing shows.
                QueueRow(p, current = false, onClick = { keys.indexOf(key).takeIf { it >= 0 }?.let(controls::skipAhead) })
            }
        }
    }
}

/** A key for each of `upNext`: its id, and how many times it is queued before this one. */
internal fun upNextKeys(upNext: List<Playing>): List<String> {
    val seen = HashMap<String, Int>()
    return upNext.map { p ->
        val n = seen.merge(p.id, 1, Int::plus)!!
        "${p.id}#$n"
    }
}

/**
 * What plays' transcript, followed along as a podcast app's is: every line, the one being said lit
 * and kept at the middle, gliding there as it moves on, those already said dimmed, and a speaker's
 * name where the speaker changes. A line tapped plays from its start. Dragged by hand, it stops
 * following — until Follow Along is pressed, or the line being said comes to the middle of where it
 * was left. The position is read here, and only for which line it is in: a tick that stays in the
 * line recomposes nothing.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TranscriptSheet(
    title: String,
    lines: List<Cue>,
    position: StateFlow<Long> = Start,
    onSeek: (Long) -> Unit = {},
    people: List<OnEpisode> = emptyList(),
    onName: (voice: String, name: String) -> Unit = { _, _ -> },
) {
    val ms = position.collectAsStateWithLifecycle()
    // Which line is being said moves a few times a minute, not at every tick.
    val at by remember(lines) { derivedStateOf { Cue.at(lines, ms.value) } }
    val speakers = remember(lines) { Cue.speakers(lines) }
    val list = rememberTransientListState()
    var following by remember { mutableStateOf(true) }
    // The voice being named, by the line whose name was tapped.
    var naming by remember { mutableStateOf<Cue?>(null) }
    val dragged by list.interactionSource.collectIsDraggedAsState()
    LaunchedEffect(dragged) { if (dragged) following = false }
    // Following, the line being said is brought to the middle: at once as the sheet opens, gliding after.
    var placed by remember { mutableStateOf(false) }
    LaunchedEffect(at, following, lines) {
        if (!following) return@LaunchedEffect
        list.centre(at.coerceAtLeast(0), glide = placed)
        placed = true
    }
    // Left where the hand left it, it takes up following again once the line being said reaches the middle.
    LaunchedEffect(list, lines) {
        snapshotFlow { !following && !list.isScrollInProgress && at >= 0 && list.middle() == at }.collect { if (it) following = true }
    }
    Column(Modifier.fillMaxWidth().fillMaxHeight()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = Space.xl, vertical = Space.s), verticalAlignment = Alignment.CenterVertically) {
            Glyph(Glyphs.Transcript, null, size = 20.dp)
            Spacer(Modifier.width(Space.m))
            Column(Modifier.weight(1f)) {
                Text("Transcript", style = MaterialTheme.typography.titleMedium)
                if (title.isNotEmpty()) Text(title, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) {
            // Room above the first line and below the last, so that either can stand at the middle.
            LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(vertical = maxOf(maxHeight / 2 - LINE_HALF, 0.dp))) {
                itemsIndexed(lines) { i, line ->
                    TranscriptLine(
                        line,
                        speakers[i],
                        said = i < at,
                        now = i == at,
                        onClick = {
                            onSeek(line.startMs)
                            following = true
                        },
                        onSpeaker = { naming = line },
                    )
                }
            }
            FollowAlong(!following, { following = true }, Modifier.align(Alignment.BottomCenter).padding(bottom = Space.xl))
        }
    }
    naming?.let { line ->
        BasicAlertDialog(onDismissRequest = { naming = null }) {
            NameVoiceCard(line, people, onName = { name ->
                onName(line.voice, name)
                naming = null
            }, onCancel = { naming = null })
        }
    }
}

/**
 * Who a voice is: one of the episode's people, a name typed, or nobody — which leaves it to whoever
 * it sounds like. Named once, it is that person in every episode it is heard in.
 */
@Composable
fun NameVoiceCard(line: Cue, people: List<OnEpisode>, onName: (String) -> Unit, onCancel: () -> Unit) {
    var name by remember { mutableStateOf(if (line.speaker == line.voice) "" else line.speaker) }
    Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh, tonalElevation = 6.dp) {
        Column(Modifier.padding(Space.xl)) {
            Text("Who is this?", style = MaterialTheme.typography.headlineSmall)
            Text("\u201c${line.text}\u201d", style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = Space.s))
            Spacer(Modifier.height(Space.l))
            for (p in people.distinctBy { it.name }) {
                Text(
                    p.name,
                    style = MaterialTheme.typography.bodyLarge,
                    color = if (p.name == name) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(Space.corner)).clickable { onName(p.name) }.padding(vertical = Space.m, horizontal = Space.s),
                )
            }
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Name") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(top = Space.s),
            )
            Row(Modifier.fillMaxWidth().padding(top = Space.m), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = { onName("") }) { Text("Nobody") }
                TextButton(onClick = onCancel) { Text("Cancel") }
                TextButton(onClick = { onName(name) }, enabled = name.isNotBlank()) { Text("Name") }
            }
        }
    }
}

// Back to the line being said, while the transcript is not following it.
@Composable
private fun FollowAlong(shown: Boolean, onClick: () -> Unit, modifier: Modifier) {
    AnimatedVisibility(shown, enter = fadeIn(), exit = fadeOut(), modifier = modifier) {
        FilledTonalButton(onClick = onClick) {
            Glyph(Glyphs.Follow, null, size = 16.dp)
            Spacer(Modifier.width(Space.s))
            Text("Follow Along")
        }
    }
}

/** About half a line's height: what the transcript's padding leaves of the middle for its first and last line. */
private val LINE_HALF = 24.dp

// One line of a transcript: the speaker's name over it where the speaker changes, and the line, lit
// while it is said and dimmed once it has been, fading between.
@Composable
private fun TranscriptLine(line: Cue, speaker: String, said: Boolean, now: Boolean, onClick: () -> Unit, onSpeaker: () -> Unit) {
    val ink = MaterialTheme.colorScheme.onSurface
    val colour by animateColorAsState(
        when {
            now -> ink
            said -> ink.copy(alpha = 0.3f)
            else -> ink.copy(alpha = 0.55f)
        },
        label = "line",
    )
    Column(Modifier.fillMaxWidth().padding(horizontal = Space.s).clip(RoundedCornerShape(Space.corner)).clickable(onClick = onClick).padding(horizontal = Space.l, vertical = Space.s)) {
        if (speaker.isNotEmpty()) {
            // A voice told apart is named by tapping its name.
            val named = if (line.voice.isEmpty()) Modifier else Modifier.clickable(onClick = onSpeaker)
            Text(speaker.uppercase(), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = Space.m, bottom = Space.xs).then(named))
        }
        Text(line.text, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, color = colour)
    }
}

// The item at the middle of the list's viewport, or -1 for none.
private fun LazyListState.middle(): Int {
    val info = layoutInfo
    val middle = (info.viewportStartOffset + info.viewportEndOffset) / 2
    return info.visibleItemsInfo.firstOrNull { it.offset <= middle && it.offset + it.size > middle }?.index ?: -1
}

// Item `index` brought to the middle of the viewport: gliding when it is near, or after a jump to it
// when it is far — a seek's — and at once when it need not glide.
private suspend fun LazyListState.centre(index: Int, glide: Boolean) {
    // Not before the list has been laid out once: the sheet's first frame.
    snapshotFlow { layoutInfo.totalItemsCount }.first { it > index }
    if (layoutInfo.visibleItemsInfo.none { it.index == index }) scrollToItem(index)
    val info = layoutInfo
    val item = info.visibleItemsInfo.firstOrNull { it.index == index } ?: return
    val by = (item.offset + item.size / 2 - (info.viewportStartOffset + info.viewportEndOffset) / 2).toFloat()
    if (glide) animateScrollBy(by, spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessLow)) else scrollBy(by)
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
