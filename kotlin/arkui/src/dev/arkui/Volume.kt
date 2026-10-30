// A volume, as a control: a mute at its start and a slider, 0 to 100. Under
// the finger it is exactly where the finger is, and says each level as it
// goes and once more when it is let go; anywhere else it is where the level
// it is handed says — and a level that arrives from somewhere else (another
// device moved it, a button, the system's panel) is sprung to rather than
// jumped to, on the spring a thing's place moves by (`Springs.move`), so a
// slider moving by itself reads as moving rather than flickering. Muted, the
// loudspeaker is struck through and the track drawn quiet; the level stays
// where it was, since unmuting goes back to it. Nothing here knows what is
// loud: a player's volume, a speaker's, a group's.
package dev.arkui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

/**
 * A volume: its mute, then its slider. `onLevel` is said for every level the finger passes and
 * `onDone` when it lets go — throttling what is sent is the caller's, which knows what a send
 * costs; `onMute` is the loudspeaker tapped, the other way from [muted].
 */
@Composable
public fun VolumeSlider(
    level: Int,
    muted: Boolean,
    onLevel: (Int) -> Unit,
    onDone: () -> Unit,
    onMute: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    about: String = "Volume",
) {
    val scope = rememberCoroutineScope()
    var dragging by remember { mutableStateOf(false) }
    val shown = remember { Animatable(level.coerceIn(0, 100) / 100f) }
    // A level from elsewhere is sprung to; under the finger, nothing but the finger moves it —
    // and letting go springs to whatever the level is by then.
    LaunchedEffect(level, dragging) {
        if (!dragging) shown.animateTo(level.coerceIn(0, 100) / 100f, Springs.move())
    }
    val quiet = muted || !enabled
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = onMute, enabled = enabled) {
            Icon(
                if (muted) ArkGlyphs.Muted else ArkGlyphs.Volume,
                contentDescription = if (muted) "Unmute $about" else "Mute $about",
                tint =
                    if (muted) MaterialTheme.colorScheme.onSurfaceVariant
                    else MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.size(22.dp),
            )
        }
        Slider(
            value = shown.value.coerceIn(0f, 1f),
            onValueChange = {
                dragging = true
                scope.launch { shown.snapTo(it) }
                onLevel((it * 100).roundToInt())
            },
            onValueChangeFinished = {
                dragging = false
                onDone()
            },
            enabled = enabled,
            colors =
                SliderDefaults.colors(
                    thumbColor =
                        if (quiet) MaterialTheme.colorScheme.onSurfaceVariant
                        else MaterialTheme.colorScheme.primary,
                    activeTrackColor =
                        if (quiet) MaterialTheme.colorScheme.onSurfaceVariant
                        else MaterialTheme.colorScheme.primary,
                    inactiveTrackColor = MaterialTheme.colorScheme.outlineVariant,
                ),
            modifier = Modifier.weight(1f),
        )
    }
}
