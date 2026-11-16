// The native controls the primitives cannot be composed into (ui/docs/primitives.md, "The
// Boundary"), as `ark_app::view` names them: something pressed as one, with its menu
// (`Pressable`: the press and the held menu are the platform's), and where a sound is and how
// loud, counted from a clock as it is drawn or dragged by a finger (`ClockScrubber`,
// `ClockHairline`, `VolumePart`). Their twins are SwiftUI's Controls.swift, iced's `arkui::view`
// and arkui-web's parts.
package dev.arkui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/** How a pressed thing answers (ui/docs/motion.md, Press Feedback). */
public enum class PressKind {
    /** A cover, a tile, a glyph: it gives under the finger ([Press.Give]). */
    Give,

    /** A row the page's width: lit under the finger ([Press.Highlight]). */
    Highlight,
}

/** What is laid out in [content], pressed as one: its press as the finger lands, said as [label], its menu when held. */
@Composable
public fun Pressable(label: String, press: PressKind, menu: List<MenuAction> = emptyList(), onTap: () -> Unit, content: @Composable () -> Unit) {
    var open by remember { mutableStateOf(false) }
    val offered = menu.filter { it.shown }
    val kind = if (press == PressKind.Give) Press.Give else Press.Highlight
    val tap =
        if (offered.isEmpty()) Modifier.pressable(onClick = onTap, role = Role.Button, press = kind)
        else Modifier.pressable(onClick = onTap, onLongClick = { open = true }, role = Role.Button, press = kind)
    // A row is the width of its list; anything else its content's (a grid's cell is fixed by its grid).
    val width = if (press == PressKind.Highlight) Modifier.fillMaxWidth() else Modifier
    Box(width.then(tap).semantics(mergeDescendants = true) { contentDescription = label }) {
        Column { content() }
        MenuOf(offered, open) { open = false }
    }
}

/** Milliseconds as `m:ss`, or `h:mm:ss` from an hour: ark_format's `clock`. */
internal fun clockWords(ms: Long): String {
    val secs = maxOf(ms, 0L) / 1000
    val h = secs / 3600
    return if (h == 0L) "%d:%02d".format(secs / 60, secs % 60) else "%d:%02d:%02d".format(h, secs / 60 % 60, secs % 60)
}

/** The device's clock while [playing] (and not in a screenshot), read again every quarter second. */
@Composable
private fun ticking(playing: Boolean): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(playing) {
        now = System.currentTimeMillis()
        while (playing && !Shots.on) {
            delay(250)
            now = System.currentTimeMillis()
        }
    }
    return now
}

private fun clockAt(at: Long, since: Long, playing: Boolean, length: Long, now: Long): Long =
    (if (playing) at + maxOf(now - since, 0L) else at).coerceIn(0L, maxOf(length, 1L))

/**
 * Where a sound is, which a finger drags: ArkUI's [Scrubber] over its clock, moving as it plays;
 * let go, [onSeek] is sent once with where. Meeting either end under the finger is a selection's
 * haptic.
 */
@Composable
public fun ClockScrubber(at: Long, since: Long, playing: Boolean, length: Long, onSeek: (Long) -> Unit) {
    val now = clockAt(at, since, playing, length, ticking(playing))
    val whole = maxOf(length, 1L).toFloat()
    var end by remember { mutableStateOf(0) }
    SensoryFeedback(Feedback.Selection, end.takeIf { it != 0 })
    Column(Modifier.fillMaxWidth()) {
        Scrubber(
            value = now / whole,
            onValue = { v -> end = if (v <= 0f) -1 else if (v >= 1f) 1 else 0 },
            onDone = { v ->
                end = 0
                onSeek((v * whole).toLong())
            },
            left = { clockWords((it * whole).toLong()) },
            right = { clockWords(length) },
        )
    }
}

/**
 * Words lit in the accent while the clock is at or past [from] and before [until], quiet
 * otherwise: a transcript's line said now, moving on as it plays on the same clock as
 * [ClockHairline]. A colour, never motion.
 */
@Composable
public fun ClockWords(text: String, at: Long, since: Long, playing: Boolean, from: Long, until: Long) {
    val now = if (playing) at + maxOf(ticking(playing) - since, 0L) else at
    val lit = now >= from && now < until
    androidx.compose.material3.Text(text, style = ArkType.Body.style, color = if (lit) ArkInk.accent else ArkInk.quiet)
}

/** How far along a sound is, a hairline in the accent, moving as it plays. */
@Composable
public fun ClockHairline(at: Long, since: Long, playing: Boolean, length: Long) {
    val now = clockAt(at, since, playing, length, ticking(playing))
    val fraction = now.toFloat() / maxOf(length, 1L)
    val fill = MaterialTheme.colorScheme.primary
    // Drawn, not laid out: the fill's width read in the draw.
    Box(
        Modifier.fillMaxWidth()
            .height(2.dp)
            .background(MaterialTheme.colorScheme.outlineVariant)
            .drawBehind { drawRect(fill, size = Size(size.width * fraction, size.height)) }
            .semantics { progressBarRangeInfo = ProgressBarRangeInfo(fraction, 0f..1f) })
}

/**
 * How loud, as `volume` lays it out: [VolumeSlider] over a level 0 to 100, its loudspeaker sending
 * [onMute]; the level follows the finger and [onChange] is sent once, with where it was let go, as
 * the scrubber's seek is. A level from elsewhere is sprung to ([VolumeSlider]).
 */
@Composable
public fun VolumePart(level: Long, muted: Boolean, about: String, onChange: (Long) -> Unit, onMute: () -> Unit) {
    var held by remember { mutableStateOf<Int?>(null) }
    VolumeSlider(
        level.coerceIn(0L, 100L).toInt(),
        muted,
        onLevel = { held = it },
        onDone = {
            held?.let { onChange(it.toLong()) }
            held = null
        },
        onMute = onMute,
        modifier = Modifier.fillMaxWidth(),
        about = about,
    )
}
