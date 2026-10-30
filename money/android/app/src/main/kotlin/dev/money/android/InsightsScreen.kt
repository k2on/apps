// Where the month's money went (the desktop's Reflect): a ring of spending
// by group with the total in its middle, the same groups as bars against the
// largest, the month in four figures, and the categories that spent the
// most. Every chart says itself to TalkBack in a sentence. The month steps
// in the top bar, as the budget's does, and is the same month.
package dev.money.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.arkui.AmountText
import dev.arkui.Bars
import dev.arkui.Donut
import dev.arkui.LocalAmounts
import dev.arkui.Meter
import dev.arkui.Part
import dev.arkui.Space
import dev.arkui.Stat
import dev.arkui.chartPalette

@Composable
fun InsightsScreen(ledger: Ledger) {
    val book = LocalShell.current.book
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

    Page(
        title = "Insights",
        actions = {
            IconButton(onClick = { book.stepMonth(-1) }) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = "Previous month")
            }
            IconButton(onClick = { book.stepMonth(1) }) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = "Next month")
            }
        },
    ) { pad, _ ->
        LazyColumn(
            contentPadding = PaddingValues(start = Space.page, end = Space.page, bottom = pad.calculateBottomPadding() + Space.xl),
            verticalArrangement = Arrangement.spacedBy(Space.m),
        ) {
            item(key = "month") {
                Text(
                    Dates.month(ledger.month),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(bottom = Space.xs))
            }
            item(key = "ring") {
                Card(
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                    shape = MaterialTheme.shapes.extraLarge,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
                        Donut(parts, description = said, modifier = Modifier.widthIn(max = 260.dp).fillMaxWidth()) {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                AmountText(total, style = MaterialTheme.typography.headlineMedium)
                                Text(
                                    "spent in $month",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    textAlign = TextAlign.Center)
                            }
                        }
                        if (parts.isNotEmpty()) {
                            Spacer(Modifier.height(Space.xl))
                            Bars(parts, description = said, value = { a.format(it) })
                        }
                    }
                }
            }
            ledger.summary?.let { s ->
                item(key = "figures") {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                        shape = MaterialTheme.shapes.large,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(Space.xl)) {
                            Text("The month", style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
                            Spacer(Modifier.height(Space.m))
                            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                                Stat("Came in") { AmountText(s.income, style = MaterialTheme.typography.titleMedium) }
                                Stat("Assigned") { AmountText(s.assigned, style = MaterialTheme.typography.titleMedium) }
                                Stat("Spent") { AmountText(-s.activity, style = MaterialTheme.typography.titleMedium) }
                            }
                            if (s.income > 0) {
                                Spacer(Modifier.height(Space.l))
                                val kept = (s.income + s.activity).toFloat() / s.income
                                Text(
                                    if (kept >= 0) "You kept ${(kept * 100).toInt()}% of what came in" else "You spent more than came in",
                                    style = MaterialTheme.typography.bodyMedium)
                                Spacer(Modifier.height(Space.s))
                                Meter(kept, color = if (kept >= 0) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error, height = 10.dp)
                            }
                        }
                    }
                }
            }
            val top =
                ledger.lines.filter { it.activity < 0 }.sortedBy { it.activity }.take(5)
            if (top.isNotEmpty()) {
                item(key = "top") {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                        shape = MaterialTheme.shapes.large,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(Modifier.padding(Space.xl)) {
                            Text("Where it went", style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
                            Spacer(Modifier.height(Space.m))
                            Bars(
                                top.mapIndexed { i, l -> Part(l.name, -l.activity, palette[i % palette.size]) },
                                description = "The categories that spent the most: " + top.joinToString(", ") { "${it.name} ${a.format(-it.activity)}" },
                                value = { a.format(it) })
                        }
                    }
                }
            }
        }
    }
}
