// A table laid out in Rust (`ark_app::view`'s `data_table`, ui/docs/parts.md, Tables) and a day
// picked in a form: the parts the generated screens call by these names. A table is rows of cells
// under its columns' headings from a medium width, and a list's two-line rows when compact — each
// cell where its column's slot puts it. The part decides which, by the width it is given.
package dev.arkui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.background
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.layout
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/** Where a column's cells line up. */
public enum class ColumnAlign { Start, End, Center }

/** How wide a column is: the theme's figure or mark column, or [DataColumn.share] parts of what is left. */
public enum class ColumnWidth { Figure, Mark, Share }

/** Where a column's cell goes in a compact row: [None] leaves it out. */
public enum class ColumnSlot { None, Title, Subtitle, Trailing, TrailingSub }

/** A table's column: its heading, its width, its share of what is left, its alignment and compact slot. */
public class DataColumn(
    public val heading: String,
    public val width: ColumnWidth,
    public val share: Int,
    public val align: ColumnAlign,
    public val slot: ColumnSlot,
)

private class TableFrame(val columns: List<DataColumn>, val compact: Boolean)

private val LocalTable = compositionLocalOf { TableFrame(emptyList(), true) }

/** The gutter a row's dot sits in. */
private val GUTTER = 8.dp

@Composable
private fun RowScope.framed(c: DataColumn, content: @Composable () -> Unit) {
    val align = when (c.align) {
        ColumnAlign.Start -> Alignment.CenterStart
        ColumnAlign.End -> Alignment.CenterEnd
        ColumnAlign.Center -> Alignment.Center
    }
    val m = when (c.width) {
        ColumnWidth.Share -> Modifier.weight(c.share.coerceAtLeast(1).toFloat())
        ColumnWidth.Figure -> Modifier.width(LayoutTokens.AMOUNT_COLUMN_WIDTH.dp)
        ColumnWidth.Mark -> Modifier.width(LayoutTokens.MARK_COLUMN_WIDTH.dp)
    }
    Box(m.padding(end = Space.s), contentAlignment = align) { content() }
}

/** A table: its headings over its rows, in a card; a list's rows in a card when compact. */
@Composable
public fun DataTable(columns: List<DataColumn>, content: @Composable () -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val compact = sizeClassOf(maxWidth) == SizeClass.Compact
        CompositionLocalProvider(LocalTable provides TableFrame(columns, compact)) {
            GroupCard {
                if (!compact) {
                    Row(Modifier.fillMaxWidth().padding(horizontal = Space.l, vertical = Space.s), verticalAlignment = Alignment.CenterVertically) {
                        Spacer(Modifier.width(GUTTER + Space.m))
                        for (c in columns) {
                            framed(c) {
                                Text(c.heading, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                            }
                        }
                    }
                }
                content()
            }
        }
    }
}

/** A heading among a table's rows (a day). */
@Composable
public fun DataTableHeading(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = Space.l, end = Space.l, top = Space.m, bottom = Space.xs).semantics { heading() },
    )
}

@Composable
private fun tintOf(name: String): Color = when (name) {
    "" -> MaterialTheme.colorScheme.onSurface
    "quiet" -> MaterialTheme.colorScheme.onSurfaceVariant
    else -> tintInk(tintNamed(name))
}

@Composable
private fun Cell(text: String, tint: String, glyph: String, end: Boolean = false, strong: Boolean = false) {
    val color = tintOf(tint)
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
        if (glyph.isNotEmpty()) {
            Icon(ArkGlyphs.named(glyph) ?: ArkGlyphs.Chevron, contentDescription = null, tint = color, modifier = Modifier.size(14.dp))
        }
        if (text.isNotEmpty()) {
            Text(
                text,
                color = color,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textAlign = if (end) TextAlign.End else TextAlign.Start,
                fontWeight = if (strong) FontWeight.SemiBold else null,
                style = MaterialTheme.typography.bodyMedium.copy(fontFeatureSettings = if (end) "tnum" else null),
            )
        }
    }
}

/**
 * One row of a table: a cell for each column (their tints and glyphs beside them), a dot before it
 * while [marked], lit while [selected]; tapped, [onTap]; held, its menu.
 */
@Composable
public fun DataTableRow(
    cells: List<String>,
    tints: List<String> = emptyList(),
    glyphs: List<String> = emptyList(),
    marked: Boolean = false,
    selected: Boolean = false,
    said: String? = null,
    menu: List<MenuAction> = emptyList(),
    onTap: (() -> Unit)? = null,
) {
    val frame = LocalTable.current
    var open by remember { mutableStateOf(false) }
    val offered = menu.filter { it.shown }
    val press =
        if (offered.isEmpty()) Modifier.pressable(onClick = { onTap?.invoke() }, press = Press.Highlight)
        else Modifier.pressable(onClick = { onTap?.invoke() }, onLongClick = { open = true }, press = Press.Highlight)
    val lit = if (selected) Modifier.background(MaterialTheme.colorScheme.secondaryContainer) else Modifier
    val at: @Composable (Int, Boolean) -> Unit = { i, strong ->
        Cell(cells[i], tints.getOrElse(i) { "" }, glyphs.getOrElse(i) { "" }, frame.columns.getOrNull(i)?.align == ColumnAlign.End, strong)
    }
    fun slotted(s: ColumnSlot): List<Int> = cells.indices.filter { frame.columns.getOrNull(it)?.slot == s }
    Row(
        Modifier.fillMaxWidth()
            .then(lit)
            .then(press)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) {
                contentDescription = said ?: cells.firstOrNull().orEmpty()
                this.selected = selected
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val dot = Modifier.size(GUTTER).clip(CircleShape).background(if (marked) MaterialTheme.colorScheme.primary else Color.Transparent)
        if (frame.compact) {
            // A list's row: the words at the card's inset, as every other row's; the dot in the
            // inset before them.
            Box(
                Modifier.layout { m, c ->
                    val p = m.measure(c)
                    layout(0, p.height) { p.place(-(GUTTER + Space.xs).roundToPx(), 0) }
                }.then(dot)
            )
        } else {
            Box(dot)
            Spacer(Modifier.width(Space.m))
        }
        if (frame.compact) {
            Column(Modifier.weight(1f)) {
                for (i in slotted(ColumnSlot.Title)) at(i, marked)
                Row(horizontalArrangement = Arrangement.spacedBy(Space.xs)) { for (i in slotted(ColumnSlot.Subtitle)) at(i, false) }
            }
            Spacer(Modifier.width(Space.s))
            Column(horizontalAlignment = Alignment.End) {
                for (i in slotted(ColumnSlot.Trailing)) at(i, false)
                Row(horizontalArrangement = Arrangement.spacedBy(Space.xs)) { for (i in slotted(ColumnSlot.TrailingSub)) at(i, false) }
            }
        } else {
            cells.indices.forEach { i ->
                frame.columns.getOrNull(i)?.let { c -> framed(c) { at(i, false) } }
            }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (m in offered) {
                DropdownMenuItem(
                    text = { Text(m.title, color = if (m.destructive) MaterialTheme.colorScheme.error else Color.Unspecified) },
                    leadingIcon = {
                        Icon(ArkGlyphs.named(m.glyph) ?: ArkGlyphs.Chevron, contentDescription = null,
                            tint = if (m.destructive) MaterialTheme.colorScheme.error else LocalContentColor.current)
                    },
                    onClick = {
                        open = false
                        m.onTap()
                    })
            }
        }
    }
}

/** Bars against the largest ([LabelledBars]), from a view's three lists. */
@Composable
public fun BarChart(labels: List<String>, values: List<Long>, said: List<String>) {
    // In a card, inset as its rows are.
    Box(Modifier.fillMaxWidth().padding(horizontal = Space.l, vertical = Space.m)) {
        LabelledBars(labels.zip(values).zip(said).map { (lv, s) -> LabelledBar(lv.first, lv.second, s) })
    }
}

private val DAY = DateTimeFormatter.ISO_LOCAL_DATE
private val SHOWN = DateTimeFormatter.ofPattern("EEE, MMM d, yyyy")

/** A day picked: a form's row with the day, Material's date picker over it when tapped. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun DateField(label: String, day: String, onChange: (String) -> Unit) {
    var picking by remember { mutableStateOf(false) }
    val date = runCatching { LocalDate.parse(day, DAY) }.getOrNull()
    ListItem(
        headlineContent = { Text(label) },
        trailingContent = { Text(date?.format(SHOWN) ?: day, color = MaterialTheme.colorScheme.onSurfaceVariant) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.pressable(onClick = { picking = true }, press = Press.Highlight),
    )
    if (picking) {
        val state = rememberDatePickerState(initialSelectedDateMillis = date?.atStartOfDay(ZoneOffset.UTC)?.toInstant()?.toEpochMilli())
        DatePickerDialog(
            onDismissRequest = { picking = false },
            confirmButton = {
                TextButton(onClick = {
                    state.selectedDateMillis?.let { ms -> onChange(Instant.ofEpochMilli(ms).atZone(ZoneOffset.UTC).toLocalDate().format(DAY)) }
                    picking = false
                }) { Text("OK") }
            },
            dismissButton = { TextButton(onClick = { picking = false }) { Text("Cancel") } },
        ) { DatePicker(state) }
    }
}
