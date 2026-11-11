// The parts a page of figures is laid out of, with the same names and inputs
// as the desktop's (`arkui::figures`) and SwiftUI's (`FigureParts.swift`);
// ui/docs/parts.md, "Figures", lists them: a figure in a capsule of its
// state, a slim bar toward a goal, a page's headline figure, a button in a
// capsule, a notice bar with its actions, a strip of labelled figures, a
// stat tile, a chart's card, and the charts (bars labelled over them,
// columns in groups with a legend, a line over its area, a sparkline).
// Nothing here names a thing any app is about: a figure is text the app
// wrote, a state a `Tint`. Every colour is the theme's (the scheme, the
// chart palette), every move the shared springs.
package dev.arkui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * A figure in a capsule of its state: wrong is filled in the error's colour with its warning
 * glyph, worth a look carries a clock, good and nothing are washed in their tint.
 */
@Composable
public fun StatusBadge(figure: String, tint: Tint) {
    val c = MaterialTheme.colorScheme
    val (ground, ink) = when (tint) {
        Tint.Negative -> c.error to c.onError
        else -> tintColors(tint)
    }
    Surface(color = ground, contentColor = ink, shape = CircleShape) {
        Row(Modifier.padding(horizontal = 8.dp, vertical = 3.dp), horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            when (tint) {
                Tint.Negative -> Icon(ArkGlyphs.Alert, contentDescription = null, modifier = Modifier.size(12.dp))
                Tint.Caution -> Icon(ArkGlyphs.Clock, contentDescription = null, modifier = Modifier.size(12.dp))
                else -> {}
            }
            Text(figure, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold)
        }
    }
}

/** A slim bar toward a goal under a row's name, `LayoutTokens.PROGRESS_WIDTH` wide at most. */
@Composable
public fun GoalBar(fraction: Float, tint: Tint) {
    ProgressBar(fraction, Modifier.widthIn(max = LayoutTokens.PROGRESS_WIDTH.dp), color = tintInk(if (tint == Tint.Neutral) Tint.Positive else tint), height = 4.dp)
}

/** A page's headline figure: what it is, the figure large and bold in its tint, a caption; `trailing` at its end. */
@Composable
public fun Headline(label: String, figure: String, tint: Tint, caption: String, trailing: @Composable () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}) {
            Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(
                figure,
                style = MaterialTheme.typography.headlineLarge.copy(fontWeight = FontWeight.Bold, fontSize = 34.sp),
                color = if (tint == Tint.Neutral) MaterialTheme.colorScheme.onSurface else tintInk(tint),
            )
            Text(caption, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        trailing()
    }
}

/** A button in a capsule: an action that sits on a page; `prominent` fills it. */
@Composable
public fun CapsuleButton(title: String, glyph: String? = null, prominent: Boolean = false, onTap: () -> Unit) {
    val label: @Composable () -> Unit = {
        glyph?.let { g -> ArkGlyphs.named(g)?.let { Icon(it, contentDescription = null, modifier = Modifier.size(16.dp).padding(end = 4.dp)) } }
        Text(title)
    }
    if (prominent) Button(onClick = onTap) { label() } else OutlinedButton(onClick = onTap) { label() }
}

/** A notice across the top of a list with what can be done about it, its actions at its end. */
@Composable
public fun NoticeBar(glyph: String, text: String, actions: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surfaceContainer).padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ArkGlyphs.named(glyph)?.let { Icon(it, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
        Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        actions()
    }
}

/** One figure of a [FigureStrip]. */
public class StripFigure(public val label: String, public val figure: String, public val tint: Tint = Tint.Neutral)

/** Figures side by side, each its label over it: an account's cleared, uncleared and working balances. */
@Composable
public fun FigureStrip(items: List<StripFigure>) {
    Row(horizontalArrangement = Arrangement.spacedBy(Space.xl)) {
        items.forEach { i ->
            Column(Modifier.semantics(mergeDescendants = true) {}) {
                Text(i.label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                // A figure never wraps.
                Text(
                    i.figure,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (i.tint == Tint.Neutral) MaterialTheme.colorScheme.onSurface else tintInk(i.tint),
                    maxLines = 1,
                    softWrap = false,
                )
            }
        }
    }
}

/** A tile with one figure: what it is, the figure large, a caption line (kept when there is none). */
@Composable
public fun StatTile(label: String, figure: String, caption: String? = null, modifier: Modifier = Modifier) {
    Column(modifier.clip(RoundedCornerShape(16.dp)).background(MaterialTheme.colorScheme.surfaceContainer).padding(Space.m).semantics(mergeDescendants = true) {}) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(figure, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
        Text(caption ?: " ", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** A chart in its card under its title. */
@Composable
public fun ChartCard(title: String, chart: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.s)) {
        Text(title, style = MaterialTheme.typography.titleSmall)
        Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(MaterialTheme.colorScheme.surfaceContainer).padding(Space.m)) { chart() }
    }
}

/** One bar of [LabelledBars]. */
public class LabelledBar(public val label: String, public val value: Long, public val said: String)

/** Bars against the largest, each its label over it and its value at its end, in the chart's first colour. */
@Composable
public fun LabelledBars(bars: List<LabelledBar>) {
    val most = (bars.maxOfOrNull { it.value } ?: 1L).coerceAtLeast(1L).toFloat()
    val colour = chartPalette().first()
    // The width it is given: the bars are shares of it, so a column as wide
    // as its longest label drew them a few points long, the labels crowded.
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        bars.forEach { b ->
            Text(b.label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Row(verticalAlignment = Alignment.CenterVertically) {
                val share = (b.value / most).coerceIn(0.01f, 1f)
                Box(Modifier.weight(share).height(6.dp).clip(CircleShape).background(colour))
                Text(b.said, style = MaterialTheme.typography.labelSmall, modifier = Modifier.weight(1f - share + 0.35f).padding(start = 6.dp))
            }
        }
    }
}

/** One group of [ColumnChart]: its label and a value per series. */
public class ColumnGroup(public val label: String, public val values: List<Long>)

/** Columns in groups side by side, each series the chart's colour of its place, a legend over them. */
@Composable
public fun ColumnChart(series: List<String>, groups: List<ColumnGroup>) {
    val palette = chartPalette()
    val most = (groups.flatMap { it.values }.maxOrNull() ?: 1L).coerceAtLeast(1L).toFloat()
    Column(verticalArrangement = Arrangement.spacedBy(Space.s)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Space.m)) {
            series.forEachIndexed { i, s ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(palette[i % palette.size]))
                    Text(s, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(start = 4.dp))
                }
            }
        }
        Row(Modifier.fillMaxWidth().height(180.dp), horizontalArrangement = Arrangement.spacedBy(Space.s)) {
            groups.forEach { g ->
                Column(Modifier.weight(1f).fillMaxHeight(), horizontalAlignment = Alignment.CenterHorizontally) {
                    Row(Modifier.weight(1f).padding(horizontal = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.Bottom) {
                        g.values.forEachIndexed { i, v ->
                            Box(Modifier.weight(1f).fillMaxHeight((v / most).coerceIn(0.01f, 1f)).clip(RoundedCornerShape(3.dp)).background(palette[i % palette.size]))
                        }
                    }
                    Text(g.label, style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}

/** A line through `points` (oldest first) over a light area that runs to the chart's foot. */
@Composable
public fun LineChart(points: List<Long>, area: Boolean = true, modifier: Modifier = Modifier.fillMaxWidth().height(160.dp)) {
    val colour = chartPalette().first()
    Canvas(modifier.semantics { contentDescription = "Trend" }) {
        if (points.size < 2) return@Canvas
        val lo = points.min().toFloat()
        val span = (points.max() - points.min()).coerceAtLeast(1L).toFloat()
        val at = points.mapIndexed { i, p ->
            Offset(i * size.width / (points.size - 1), size.height - 2f - (p - lo) / span * (size.height - 4f))
        }
        val line = Path().apply {
            moveTo(at[0].x, at[0].y)
            at.drop(1).forEach { lineTo(it.x, it.y) }
        }
        if (area) {
            val fill = Path().apply {
                addPath(line)
                lineTo(size.width, size.height)
                lineTo(0f, size.height)
                close()
            }
            drawPath(fill, colour.copy(alpha = 0.16f))
        }
        drawPath(line, colour, style = Stroke(width = 1.5.dp.toPx()))
    }
}

/** A small line with no area: a trend beside a figure. */
@Composable
public fun Sparkline(points: List<Long>) {
    LineChart(points, area = false, modifier = Modifier.width(140.dp).height(34.dp))
}

/** The page's figure in a card, made of what is owned and what is owed, with a sparkline of how it went. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
public fun SummaryCard(label: String, cents: Long, first: String, firstCents: Long, second: String, secondCents: Long, trend: List<Long>) {
    val a = LocalAmounts.current
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(MaterialTheme.colorScheme.surfaceContainerLow).padding(Space.l),
        verticalArrangement = Arrangement.spacedBy(Space.xs),
    ) {
        Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(
            a.format(cents),
            style = MaterialTheme.typography.headlineLarge.copy(fontWeight = FontWeight.Bold),
            color = if (cents < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
        )
        // A figure never wraps: the figures and the sparkline side by side
        // while they fit, else the sparkline on a line of its own under them.
        FlowRow(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalArrangement = Arrangement.spacedBy(Space.s),
        ) {
            FigureStrip(
                listOf(
                    StripFigure(first, a.format(firstCents), if (firstCents < 0) Tint.Negative else Tint.Neutral),
                    StripFigure(second, a.format(secondCents), if (secondCents < 0) Tint.Negative else Tint.Neutral),
                ),
            )
            Sparkline(trend)
        }
    }
}
