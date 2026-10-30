// Charts as a phone's own apps draw them: a ring of parts with a figure in
// its middle ([Donut]), and the same parts as bars against the largest
// ([Bars]). Colours are the scheme's — [chartPalette] takes them from it, so
// a chart is in the wallpaper's colours like everything around it — and
// each draws itself in from nothing on a spring the first time it shows.
//
// A chart is one thing to TalkBack, read as a sentence the caller writes
// (`description`): a ring of colours says nothing to somebody who cannot see
// it, and the parts are better said in words than traced.
package dev.arkui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.background

/** One part of a chart: what it is, how much, and its colour. */
public data class Part(val label: String, val value: Long, val color: Color)

/** Colours for a chart's parts, from the scheme, in an order that keeps neighbours apart. */
@Composable
public fun chartPalette(): List<Color> {
    val c = MaterialTheme.colorScheme
    return listOf(c.primary, c.tertiary, c.secondary, c.error, c.primaryContainer, c.tertiaryContainer, c.inversePrimary, c.outline)
}

/** Grows from 0 to 1 on a spring the first time it is composed: a chart drawing itself in. */
@Composable
private fun drawnIn(key: Any?): Float {
    val a = remember(key) { Animatable(0f) }
    LaunchedEffect(key) {
        a.animateTo(1f, spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = Spring.StiffnessVeryLow))
    }
    return a.value
}

/**
 * A ring of `parts` with a small gap between each, `thickness` wide, and `center` in the middle — a
 * total, say. Parts of nothing are left out; nothing at all is an empty ring.
 */
@Composable
public fun Donut(
    parts: List<Part>,
    description: String,
    modifier: Modifier = Modifier,
    thickness: Dp = 22.dp,
    center: @Composable BoxScope.() -> Unit = {},
) {
    val shown = parts.filter { it.value > 0 }
    val total = shown.sumOf { it.value }.toFloat()
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    val drawn = drawnIn(shown.map { it.label })
    Box(
        modifier.aspectRatio(1f).clearAndSetSemantics { contentDescription = description },
        contentAlignment = Alignment.Center) {
            Canvas(Modifier.matchParentSize()) {
                val w = thickness.toPx()
                val inset = w / 2
                val arc = Size(size.width - w, size.height - w)
                val at = Offset(inset, inset)
                drawArc(track, 0f, 360f, false, at, arc, style = Stroke(w))
                if (total <= 0f) return@Canvas
                // The gap between parts, as an angle that is the same length on any ring.
                val gap = if (shown.size > 1) (w / (size.width - w) * 57.3f).coerceAtMost(8f) else 0f
                var start = -90f
                val all = 360f * drawn
                for (p in shown) {
                    val sweep = all * (p.value / total)
                    if (sweep > gap) {
                        drawArc(p.color, start + gap / 2, sweep - gap, false, at, arc, style = Stroke(w, cap = StrokeCap.Butt))
                    }
                    start += sweep
                }
            }
            Box(Modifier.padding(thickness + 8.dp), contentAlignment = Alignment.Center) { center() }
        }
}

/**
 * `parts` as bars, each against the largest, with its label and its value (`value` writes it) on
 * a line above: what a legend and a comparison are at once.
 */
@Composable
public fun Bars(
    parts: List<Part>,
    description: String,
    modifier: Modifier = Modifier,
    value: (Long) -> String,
    height: Dp = 10.dp,
) {
    val most = parts.maxOfOrNull { it.value }?.coerceAtLeast(1) ?: 1
    val drawn = drawnIn(parts.map { it.label })
    Column(
        modifier.fillMaxWidth().clearAndSetSemantics { contentDescription = description },
        verticalArrangement = Arrangement.spacedBy(Space.m)) {
            for (p in parts) {
                Column {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(10.dp).clip(CircleShape).background(p.color))
                        Spacer(Modifier.width(Space.s))
                        Text(
                            p.label,
                            style = MaterialTheme.typography.bodyMedium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f))
                        Text(
                            value(p.value),
                            style = MaterialTheme.typography.labelLarge.copy(fontFeatureSettings = "tnum"),
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Spacer(Modifier.padding(top = Space.xs))
                    Meter(
                        fraction = drawn * p.value / most.toFloat(),
                        color = p.color,
                        height = height,
                    )
                }
            }
        }
}
