// A slider with no handle: a rounded track, filled from its start to where the value is, as the
// iPhone's player draws its scrubber and its volume. Pressed, the track thickens a little (on the
// spring a thing's size moves by), so it is plain that it is held; a tap puts the value where it
// is tapped and a drag follows the finger. Said to accessibility as a slider: its value, and a
// setting of it. The iPhone's twin is ArkUI's `Bar` (swift/ArkUI).
package dev.arkui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.lerp

/** How thick a [Bar]'s track is at rest, and held. */
public object BarDefaults {
    public val Thickness: Dp = 6.dp
    public val Held: Dp = 10.dp
    /** The height the bar takes, for the finger: more than the track. */
    public val Touch: Dp = 28.dp
}

/**
 * A handleless slider over `0..1`. `onValue` is said for every value the finger passes and
 * `onDone` when it lets go. `fill` is the part done; the rest is drawn in `track`.
 */
@Composable
public fun Bar(
    value: Float,
    onValue: (Float) -> Unit,
    modifier: Modifier = Modifier,
    onDone: () -> Unit = {},
    enabled: Boolean = true,
    fill: Color = MaterialTheme.colorScheme.primary,
    track: Color = MaterialTheme.colorScheme.outlineVariant,
    label: String? = null,
) {
    var held by remember { mutableStateOf(false) }
    // Held, over 0..1, on the spring a thing's size moves by; the thickness is drawn from it.
    val hold by animateFloatAsState(if (held) 1f else 0f, Springs.size(), label = "bar")
    val thick = lerp(BarDefaults.Thickness, BarDefaults.Held, hold)
    val say by rememberUpdatedState(onValue)
    val done by rememberUpdatedState(onDone)
    val shown = value.coerceIn(0f, 1f)
    Box(
        modifier
            .fillMaxWidth()
            .height(BarDefaults.Touch)
            .semantics {
                label?.let { contentDescription = it }
                progressBarRangeInfo = ProgressBarRangeInfo(shown, 0f..1f)
                if (!enabled) disabled()
                setProgress { v ->
                    if (!enabled) return@setProgress false
                    say(v.coerceIn(0f, 1f))
                    done()
                    true
                }
            }
            .then(
                if (!enabled) Modifier
                else
                    Modifier
                        .pointerInput(Unit) {
                            detectTapGestures(
                                onPress = {
                                    held = true
                                    tryAwaitRelease()
                                    held = false
                                },
                                onTap = {
                                    say((it.x / size.width).coerceIn(0f, 1f))
                                    done()
                                },
                            )
                        }
                        .pointerInput(Unit) {
                            detectHorizontalDragGestures(
                                onDragStart = {
                                    held = true
                                    say((it.x / size.width).coerceIn(0f, 1f))
                                },
                                onDragEnd = {
                                    held = false
                                    done()
                                },
                                onDragCancel = {
                                    held = false
                                    done()
                                },
                            ) { change, _ ->
                                change.consume()
                                say((change.position.x / size.width).coerceIn(0f, 1f))
                            }
                        }
            ),
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.fillMaxWidth().height(thick)) {
            val r = CornerRadius(size.height / 2, size.height / 2)
            drawRoundRect(track, cornerRadius = r)
            if (shown > 0f) {
                // At least as wide as it is thick, so a start reads as a dot rather than nothing.
                val w = (size.width * shown).coerceAtLeast(size.height)
                drawRoundRect(fill, Offset.Zero, Size(w, size.height), r)
            }
        }
    }
}
