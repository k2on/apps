// Where the month's money went (the desktop's Reflect), laid out as the
// iPhone app lays it: the month, paged under the title as the budget's is
// (the same month); Spending, a ring of the month's spending by group, the
// most first, with the total in its middle, and a legend under it of each
// group's share and amount; The Month, what came in, what was spent, what
// was left over and what of the cards' debt is not covered; and each
// group's assigned beside its spent as paired bars. The colours are the
// theme's chart palette, the iPhone's too. Every chart says itself to
// TalkBack in a sentence.
package dev.money.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkui.AmountText
import dev.arkui.Donut
import dev.arkui.LocalAmounts
import dev.arkui.Meter
import dev.arkui.Part
import dev.arkui.Space
import dev.arkui.chartPalette

@Composable
fun InsightsScreen(ledger: Ledger) {
    val a = LocalAmounts.current
    val palette = chartPalette()
    val parts =
        ledger.spending
            .filter { it.spent > 0 }
            .sortedByDescending { it.spent }
            .mapIndexed { i, s -> Part(s.group, s.spent, palette[i % palette.size]) }
    val total = parts.sumOf { it.value }
    val month = Dates.monthAlone(ledger.month)
    val said =
        if (parts.isEmpty()) "Nothing spent in $month yet."
        else "Spent ${a.format(total)} in $month: " + parts.joinToString(", ") { "${it.label} ${a.format(it.value)}" }
    // The groups that did anything this month, the cards' not among them: paying a card is no spending.
    val groups = ledger.byGroup(false).filter { (g, ls) -> g.id != Book.CARD_GROUP && ls.any { it.assigned != 0L || it.activity != 0L } }

    Page(title = "Insights") { pad, _ ->
        LazyColumn(contentPadding = PaddingValues(start = Space.page, end = Space.page, top = pad.calculateTopPadding(), bottom = pad.calculateBottomPadding() + Space.xl)) {
            item(key = "month") { MonthPager(ledger) }
            item(key = "h-spending") { GroupHeading("Spending") }
            item(key = "spending") {
                GroupCard {
                    if (parts.isEmpty()) {
                        Column(Modifier.fillMaxWidth().padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text("Nothing Spent Yet", style = MaterialTheme.typography.titleMedium)
                            Text(
                                "What ${Dates.month(ledger.month)} spends shows here.",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center)
                        }
                    } else {
                        Box(Modifier.fillMaxWidth().padding(Space.l), contentAlignment = Alignment.Center) {
                            Donut(parts, description = said, thickness = 40.dp, modifier = Modifier.widthIn(max = 240.dp).fillMaxWidth()) {
                                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                    Text("Spent", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    AmountText(total, style = MaterialTheme.typography.titleLarge.copy(fontWeight = FontWeight.Bold))
                                }
                            }
                        }
                        RowDivider()
                        parts.forEachIndexed { i, p ->
                            if (i > 0) RowDivider(inset = Space.l + 10.dp + Space.m)
                            LegendRow(p, share(p.value, total))
                        }
                    }
                }
            }
            ledger.summary?.let { s ->
                item(key = "h-month") { GroupHeading("The Month") }
                item(key = "month-figures") {
                    GroupCard {
                        Figure("Income") { AmountText(s.income, style = MaterialTheme.typography.bodyLarge, color = positiveColor()) }
                        RowDivider()
                        Figure("Spent") { AmountText(-total, style = MaterialTheme.typography.bodyLarge) }
                        RowDivider()
                        val left = s.income - total
                        Figure("Left Over", strong = true) {
                            AmountText(
                                left,
                                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                                color = if (left < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
                        }
                        // Paying a card is not spending (what it bought was), so it is in neither figure;
                        // what the cards owe beyond what is set aside for them is said apart.
                        if (ledger.cards.isNotEmpty()) {
                            RowDivider()
                            val short = ledger.cardsShort
                            Figure("Card Debt Not Covered") {
                                AmountText(
                                    short,
                                    style = MaterialTheme.typography.bodyLarge,
                                    color = if (short > 0) MaterialTheme.colorScheme.tertiary else positiveColor())
                            }
                        }
                    }
                }
            }
            if (groups.isNotEmpty()) {
                item(key = "h-bars") { GroupHeading("Assigned and Spent") }
                item(key = "bars") {
                    GroupCard { PairedBars(groups.map { (g, ls) -> Triple(g.name, ls.sumOf { it.assigned }, ls.sumOf { -it.activity }.coerceAtLeast(0)) }) }
                    Text(
                        "Each group's plan for the month beside what it spent.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = Space.l, end = Space.l, top = Space.s))
                }
            }
        }
    }
}

/** A part's share of the whole, as a whole percent; empty for a whole of nothing. */
fun share(part: Long, whole: Long): String = if (whole <= 0) "" else "${Math.round(part * 100.0 / whole)}%"

/** One line of the ring's legend: its colour, its group, its share and its amount. */
@Composable
private fun LegendRow(p: Part, share: String) {
    val a = LocalAmounts.current
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Space.l, vertical = Space.m).clearAndSetSemantics {
            contentDescription = "${p.label}, $share, ${a.format(p.value)}"
        },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(10.dp).clip(CircleShape).background(p.color))
        Spacer(Modifier.width(Space.m))
        Text(p.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        Text(share, style = MaterialTheme.typography.bodyLarge.copy(fontFeatureSettings = "tnum"), color = MaterialTheme.colorScheme.onSurfaceVariant)
        AmountText(p.value, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.End, modifier = Modifier.widthIn(min = 104.dp))
    }
}

/** A figure's row: its name, and the figure at the end. */
@Composable
private fun Figure(label: String, strong: Boolean = false, value: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = Space.l, vertical = Space.m), verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = if (strong) FontWeight.SemiBold else null,
            modifier = Modifier.weight(1f))
        value()
    }
}

/**
 * Each group's assigned (the accent, faint) beside its spent (the accent), as bars against the
 * largest of either, under a legend of the two: the iPhone's paired bars.
 */
@Composable
private fun PairedBars(groups: List<Triple<String, Long, Long>>) {
    val a = LocalAmounts.current
    val accent = MaterialTheme.colorScheme.primary
    val faint = accent.copy(alpha = 0.45f)
    val most = groups.maxOfOrNull { maxOf(it.second, it.third) }?.coerceAtLeast(1) ?: 1
    Column(
        Modifier.fillMaxWidth().padding(Space.l).clearAndSetSemantics {
            contentDescription = "Assigned and spent by group: " + groups.joinToString(", ") { (n, x, y) -> "$n assigned ${a.format(x)}, spent ${a.format(y)}" }
        },
        verticalArrangement = Arrangement.spacedBy(Space.m),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Space.l)) {
            Key(faint, "Assigned")
            Key(accent, "Spent")
        }
        for ((name, assigned, spent) in groups) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.xs)) {
                Text(name, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Bar(assigned.toFloat() / most, faint, a.format(assigned))
                Bar(spent.toFloat() / most, accent, a.format(spent))
            }
        }
    }
}

@Composable
private fun Key(color: Color, label: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(10.dp).clip(CircleShape).background(color))
        Spacer(Modifier.width(Space.xs))
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun Bar(fraction: Float, color: Color, value: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Meter(fraction, color = color, track = Color.Transparent, height = 10.dp, modifier = Modifier.weight(1f))
        Spacer(Modifier.width(Space.s))
        Text(
            value,
            style = MaterialTheme.typography.labelMedium.copy(fontFeatureSettings = "tnum"),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.End,
            modifier = Modifier.widthIn(min = 80.dp))
    }
}
