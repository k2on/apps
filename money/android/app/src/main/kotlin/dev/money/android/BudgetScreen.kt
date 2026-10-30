// The month's budget, as YNAB taught people to read one and as a phone's
// own app would lay it out: what is ready to assign, large, in a card
// tinted by whether it is (money waiting is the primary's container, money
// over-assigned the error's); what needs a look under it as chips; then each
// group of categories as a card, every category a row with a meter of how
// much of its money has gone and what is left in a pill — green-ish where
// there is some, amber where its target is not met, red where it is
// overspent. A category opens its sheet (CategorySheet.kt). The month steps
// back and forth in the top bar.
package dev.money.android

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.Meter
import dev.arkui.Pill
import dev.arkui.Space
import dev.arkui.Stat
import dev.arkui.Tone
import dev.arkui.toneColors
import dev.money.android.gen.Auto
import uniffi.money_domain.BudgetLine
import uniffi.money_domain.GroupLine

/** What a category's pill says about it. */
fun toneOf(l: BudgetLine): Tone =
    when {
        l.available < 0 -> Tone.Negative
        l.target > 0 && l.assigned < l.target -> Tone.Caution
        l.available > 0 -> Tone.Positive
        else -> Tone.Neutral
    }

/** How far along a category is, and in what: spent of what it had, its target funded, or overspent. */
@Composable
private fun meterOf(l: BudgetLine): Pair<Float, Color> {
    val c = MaterialTheme.colorScheme
    val had = l.carried + l.assigned
    return when {
        l.available < 0 -> 1f to c.error
        l.target > 0 && l.assigned < l.target -> (l.assigned.toFloat() / l.target) to c.tertiary
        had > 0 -> ((-l.activity).toFloat() / had) to c.primary
        else -> 0f to c.primary
    }
}

/** The line under a category's name: what is happening to it, in words. */
@Composable
fun storyOf(l: BudgetLine): String {
    val a = LocalAmounts.current
    val had = l.carried + l.assigned
    return when {
        l.available < 0 -> "Overspent by ${a.format(-l.available)}"
        l.target > 0 && l.assigned < l.target -> "${a.format(l.target - l.assigned)} more needed this month"
        l.activity < 0 && had > 0 -> "Spent ${a.format(-l.activity)} of ${a.format(had)}"
        l.activity < 0 -> "Spent ${a.format(-l.activity)}"
        l.target > 0 -> "Funded"
        else -> ""
    }
}

@Composable
fun BudgetScreen(ledger: Ledger) {
    val shell = LocalShell.current
    val book = shell.book
    var open by rememberSaveable { mutableStateOf<String?>(null) }
    var menu by remember { mutableStateOf(false) }
    var showHidden by rememberSaveable { mutableStateOf(false) }
    var collapsed by rememberSaveable { mutableStateOf(setOf<String>()) }
    var newGroup by remember { mutableStateOf<String?>(null) }
    var newCategory by remember { mutableStateOf<Pair<Id, String>?>(null) }
    val list = rememberLazyListState()
    val expanded by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }

    Page(
        title = Dates.month(ledger.month),
        actions = {
            IconButton(onClick = { book.stepMonth(-1) }) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = "Previous month")
            }
            IconButton(onClick = { book.stepMonth(1) }) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = "Next month")
            }
            IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "More") }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                if (ledger.month != ledger.today.take(7)) {
                    DropdownMenuItem(text = { Text("This month") }, onClick = { menu = false; book.stepMonth(0) })
                }
                DropdownMenuItem(text = { Text("Assign what's underfunded") }, onClick = { menu = false; book.autoAssign(Auto.Underfunded) })
                DropdownMenuItem(text = { Text("Assign what last month did") }, onClick = { menu = false; book.autoAssign(Auto.LastMonth) })
                DropdownMenuItem(text = { Text("Reset assigned amounts") }, onClick = { menu = false; book.autoAssign(Auto.Reset) })
                HorizontalDivider()
                DropdownMenuItem(text = { Text("New category group") }, onClick = { menu = false; newGroup = "" })
                DropdownMenuItem(
                    text = { Text(if (showHidden) "Leave hidden categories out" else "Show hidden categories") },
                    onClick = { menu = false; showHidden = !showHidden })
            }
        },
        fab = {
            ExtendedFloatingActionButton(
                onClick = { shell.onAdd(null) },
                expanded = expanded,
                icon = { Icon(Icons.Default.Add, contentDescription = null) },
                text = { Text("Transaction") },
            )
        },
    ) { pad, _ ->
        LazyColumn(
            state = list,
            contentPadding = PaddingValues(start = Space.page, end = Space.page, bottom = pad.calculateBottomPadding() + 96.dp),
            verticalArrangement = Arrangement.spacedBy(Space.m),
        ) {
            item(key = "ready") { ReadyCard(ledger) }
            item(key = "notices") { Notices(ledger) }
            for ((g, lines) in ledger.byGroup(showHidden)) {
                item(key = g.id.hex) {
                    GroupCard(
                        g,
                        lines,
                        collapsed = g.id.hex in collapsed,
                        onToggle = { collapsed = if (g.id.hex in collapsed) collapsed - g.id.hex else collapsed + g.id.hex },
                        onOpen = { open = it.hex },
                        onAdd = { newCategory = g.id to "" },
                        modifier = Modifier.animateItem(),
                    )
                }
            }
            if (ledger.groups.isEmpty() && ledger.ready) {
                item(key = "empty") {
                    Text(
                        "No categories yet. Add a group from the menu to start a budget.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(Space.xl))
                }
            }
        }
    }

    open?.let { hex -> CategorySheet(ledger, Id(hex), onDismiss = { open = null }) }
    newGroup?.let { typed ->
        NameDialog(
            title = "New category group",
            typed = typed,
            onType = { newGroup = it },
            onDone = { book.addGroup(typed.trim()); newGroup = null },
            onDismiss = { newGroup = null })
    }
    newCategory?.let { (g, typed) ->
        NameDialog(
            title = "New category",
            typed = typed,
            onType = { newCategory = g to it },
            onDone = { book.addCategory(g, typed.trim()); newCategory = null },
            onDismiss = { newCategory = null })
    }
}

/** Ready to Assign: the one figure the budget is about. */
@Composable
private fun ReadyCard(ledger: Ledger) {
    val book = LocalShell.current.book
    val s = ledger.summary
    val ready = s?.readyToAssign ?: 0L
    val tone = when {
        ready > 0 -> Tone.Positive
        ready < 0 -> Tone.Negative
        else -> Tone.Neutral
    }
    val (container, on) = toneColors(tone)
    val a = LocalAmounts.current
    Card(
        colors = CardDefaults.cardColors(containerColor = container, contentColor = on),
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(Space.xl).animateContentSize()) {
            Text(
                when {
                    ready < 0 -> "Assigned more than you have"
                    ready == 0L -> "Every dollar has a job"
                    else -> "Ready to assign"
                },
                style = MaterialTheme.typography.titleMedium)
            AmountText(
                ready,
                style = MaterialTheme.typography.displayMedium,
                modifier = Modifier.semantics { contentDescription = "Ready to assign, ${a.format(ready)}" })
            if (s != null && s.overspentLastMonth != 0L) {
                Text(
                    "${a.format(s.overspentLastMonth)} overspent last month came out of this month",
                    style = MaterialTheme.typography.bodyMedium)
            }
            if (ready > 0 && s != null && s.underfunded > 0) {
                Spacer(Modifier.height(Space.m))
                FilledTonalButton(onClick = { book.autoAssign(Auto.Underfunded) }) {
                    Icon(Glyphs.Auto, contentDescription = null, modifier = Modifier.padding(end = Space.s))
                    Text("Fund ${s.underfunded} underfunded")
                }
            }
            if (s != null) {
                Spacer(Modifier.height(Space.l))
                Row(horizontalArrangement = Arrangement.spacedBy(Space.xl)) {
                    Stat("Income") { AmountText(s.income, style = MaterialTheme.typography.titleSmall) }
                    Stat("Assigned") { AmountText(s.assigned, style = MaterialTheme.typography.titleSmall) }
                    Stat("Activity") { AmountText(s.activity, style = MaterialTheme.typography.titleSmall) }
                }
            }
        }
    }
}

/** What needs a look, as chips: transactions to review, categories overspent. */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun Notices(ledger: Ledger) {
    val shell = LocalShell.current
    val s = ledger.summary
    val review = ledger.inbox.size
    val over = s?.overspent ?: 0L
    if (review == 0 && over == 0L) return
    FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.s)) {
        if (review > 0) {
            AssistChip(
                onClick = { shell.onTab(Tab.Activity) },
                label = { Text(if (review == 1) "1 transaction to review" else "$review transactions to review") },
                leadingIcon = { Icon(Glyphs.Activity, contentDescription = null, modifier = Modifier.padding(2.dp)) },
            )
        }
        if (over > 0) {
            val (container, on) = toneColors(Tone.Negative)
            AssistChip(
                onClick = {},
                label = { Text(if (over == 1L) "1 category overspent" else "$over categories overspent") },
                colors = AssistChipDefaults.assistChipColors(containerColor = container, labelColor = on),
                border = null,
            )
        }
    }
}

/** A group of categories, as a card that folds. */
@Composable
private fun GroupCard(
    g: GroupLine,
    lines: List<BudgetLine>,
    collapsed: Boolean,
    onToggle: () -> Unit,
    onOpen: (Id) -> Unit,
    onAdd: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val turn by animateFloatAsState(if (collapsed) -90f else 0f, label = "fold")
    val available = lines.sumOf { it.available }
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        shape = MaterialTheme.shapes.large,
        modifier = modifier.fillMaxWidth(),
    ) {
        Column(Modifier.animateContentSize()) {
            Row(
                Modifier.fillMaxWidth()
                    .clickable(role = Role.Button, onClickLabel = if (collapsed) "Show" else "Fold", onClick = onToggle)
                    .padding(start = Space.l, end = Space.l, top = Space.l, bottom = Space.s)
                    .semantics { stateDescription = if (collapsed) "Folded" else "Open" },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.Default.KeyboardArrowDown, contentDescription = null, modifier = Modifier.rotate(turn))
                Spacer(Modifier.width(Space.s))
                Text(
                    g.name,
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f).semantics { heading() },
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis)
                AmountText(available, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (!collapsed) {
                for (l in lines) CategoryRow(l, onClick = { onOpen(l.categoryId) })
                TextButton(onClick = onAdd, modifier = Modifier.padding(start = Space.s, bottom = Space.xs)) {
                    Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.padding(end = Space.xs))
                    Text("Category")
                }
            } else Spacer(Modifier.height(Space.s))
        }
    }
}

@Composable
private fun CategoryRow(l: BudgetLine, onClick: () -> Unit) {
    val a = LocalAmounts.current
    val (fraction, color) = meterOf(l)
    val story = storyOf(l)
    Column(
        Modifier.fillMaxWidth()
            .clickable(onClickLabel = "Open", onClick = onClick)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) {
                contentDescription = "${l.name}, ${a.format(l.available)} available" + if (story.isNotEmpty()) ". $story" else ""
            },
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    l.name + if (l.hidden) " (hidden)" else "",
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis)
                if (story.isNotEmpty()) {
                    Text(story, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                }
            }
            Spacer(Modifier.width(Space.m))
            Pill(toneOf(l)) { AmountText(l.available, style = MaterialTheme.typography.labelLarge) }
        }
        Spacer(Modifier.height(Space.s))
        Meter(fraction, color = color)
    }
}

/** A name, typed into a dialog: a new group, a new category, a rename. */
@Composable
fun NameDialog(title: String, typed: String, onType: (String) -> Unit, onDone: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = typed,
                onValueChange = onType,
                label = { Text("Name") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth())
        },
        confirmButton = { TextButton(onClick = onDone, enabled = typed.isNotBlank()) { Text("Add") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
