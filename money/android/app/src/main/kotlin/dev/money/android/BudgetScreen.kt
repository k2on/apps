// The month's budget, as YNAB taught people to read one and as a phone's
// own app would lay it out: what is ready to assign, large, in a card
// tinted by whether it is (money waiting is the primary's container, money
// over-assigned the error's); what needs a look under it as chips; then each
// group of categories as a card, every category a row with a meter of how
// much of its money has gone and what is left in a pill — green-ish where
// there is some, amber where its target is not met, red where it is
// overspent. A category opens its sheet (CategorySheet.kt). The month steps
// back and forth in the top bar.
//
// The budget is put in order by hand, as a launcher's apps are: a category
// held lifts and is dragged among the rest, and past a group's header into
// that group; a group's header held folds every group to its header, so the
// groups are dragged past one another as single rows, and opens them again
// on the drop (arkui's Reorder, in sections). So that a category can cross
// from one card into the next, each card is the lazy list's rows rather than
// one row of it: a header with the card's top corners, a row per category,
// and a foot with its bottom corners. The drop is the domain's
// `move_category` or `move_group`, in front of whatever it landed before.
package dev.money.android

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CornerBasedShape
import androidx.compose.foundation.shape.ZeroCornerSize
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
import androidx.compose.material3.Surface
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
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.arkui.AmountText
import dev.arkui.Landing
import dev.arkui.LocalAmounts
import dev.arkui.Meter
import dev.arkui.Pill
import dev.arkui.ReorderableItem
import dev.arkui.Space
import dev.arkui.Stat
import dev.arkui.Tone
import dev.arkui.makingWay
import dev.arkui.rememberReorder
import dev.arkui.sections
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

    // The groups and their categories as the reorder holds them: each group's key, then its
    // categories', in the budget's order.
    val budget = remember(ledger.groups, ledger.lines, showHidden) { ledger.byGroup(showHidden) }
    val groups = remember(budget) { budget.associate { (g, _) -> groupKey(g.id) to g } }
    val lines = remember(budget) { budget.flatMap { it.second }.associateBy { categoryKey(it.categoryId) } }
    val keys = remember(budget) { keysOf(budget) }
    // After the ready card and the notices.
    val reorder =
        rememberReorder(list, lead = 2, isHeader = ::isGroupKey) { key, landing ->
            when (val d = dropOf(key, landing)) {
                is Drop.Category -> {
                    book.moveCategory(d.id, d.group, d.before)
                    // Let go in a folded group, it opens: the category is seen where it went.
                    collapsed = collapsed - d.group.hex
                }
                is Drop.Group -> book.moveGroup(d.id, d.before)
                null -> {}
            }
        }
    val drawn = reorder.order(keys) { it }

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
        ) {
            item(key = "ready") { ReadyCard(ledger, Modifier.padding(bottom = Space.m)) }
            item(key = "notices") { Notices(ledger) }
            for ((gk, rows) in sections(drawn, ::isGroupKey)) {
                val g = groups[gk] ?: continue
                val hex = g.id.hex
                val folded = hex in collapsed
                // Every group is its header alone while one is dragged; a category dragged into a
                // folded group is drawn there all the same.
                val closed = folded || reorder.folded
                val shown = if (reorder.folded) emptyList() else rows.filter { !folded || it == reorder.dragged }
                item(key = gk) {
                    ReorderableItem(
                        reorder,
                        gk,
                        keys,
                        modifier = if (closed) Modifier.padding(bottom = Space.m) else Modifier,
                        shape = MaterialTheme.shapes.large.let { if (closed) it else cardTop(it) },
                    ) {
                        GroupHeader(
                            g,
                            available = rows.sumOf { lines[it]?.available ?: 0L },
                            folded = folded,
                            closed = closed,
                            lifted = reorder.dragged == gk,
                            actions = reorder.actions(gk),
                            onToggle = { collapsed = if (folded) collapsed - hex else collapsed + hex },
                        )
                    }
                }
                for (ck in shown) {
                    val l = lines[ck] ?: continue
                    item(key = ck) {
                        ReorderableItem(reorder, ck, keys, shape = MaterialTheme.shapes.medium) {
                            CategoryRow(l, lifted = reorder.dragged == ck, actions = reorder.actions(ck), onClick = { open = l.categoryId.hex })
                        }
                    }
                }
                if (!closed) {
                    item(key = "foot/$hex") {
                        GroupFoot(
                            onAdd = { newCategory = g.id to "" },
                            modifier = Modifier.makingWay(this).padding(bottom = Space.m))
                    }
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
private fun ReadyCard(ledger: Ledger, modifier: Modifier = Modifier) {
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
        modifier = modifier.fillMaxWidth(),
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
    FlowRow(Modifier.padding(bottom = Space.m), horizontalArrangement = Arrangement.spacedBy(Space.s)) {
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

/** A group's key in the budget's list, as the reorder holds it. */
fun groupKey(g: Id): String = "group/${g.hex}"

/** A category's key in the budget's list. */
fun categoryKey(c: Id): String = "category/${c.hex}"

/** Whether a key of the budget's list is a group's: a header, in the reorder's sections. */
fun isGroupKey(k: Any): Boolean = (k as? String)?.startsWith("group/") == true

private fun idOf(k: Any): Id = Id((k as String).substringAfter('/'))

/** The budget's keys, in its order: each group's, then its categories'. */
fun keysOf(budget: List<Pair<GroupLine, List<BudgetLine>>>): List<String> =
    budget.flatMap { (g, ls) -> listOf(groupKey(g.id)) + ls.map { categoryKey(it.categoryId) } }

/** What a drop on the budget asks of the domain. */
sealed interface Drop {
    /** `move_category`: into `group`, in front of `before` or at the group's end. */
    data class Category(val id: Id, val group: Id, val before: Id?) : Drop

    /** `move_group`: in front of `before`, or after every other. */
    data class Group(val id: Id, val before: Id?) : Drop
}

/** `key` let go where `landing` says, as the domain's change: none for a category under no group. */
fun dropOf(key: Any, landing: Landing<Any>): Drop? =
    if (isGroupKey(key)) Drop.Group(idOf(key), landing.before?.let(::idOf))
    else landing.section?.let { Drop.Category(idOf(key), idOf(it), landing.before?.let(::idOf)) }

/**
 * A group's header: the top of its card, or the whole card while it is `closed` (folded, or every
 * group is while one is dragged). A tap folds it or opens it; held, it is dragged among the groups.
 */
@Composable
private fun GroupHeader(
    g: GroupLine,
    available: Long,
    folded: Boolean,
    closed: Boolean,
    lifted: Boolean,
    actions: List<CustomAccessibilityAction>,
    onToggle: () -> Unit,
) {
    val turn by animateFloatAsState(if (folded) -90f else 0f, label = "fold")
    val card = MaterialTheme.shapes.large
    val shape = if (closed) card else cardTop(card)
    Surface(color = if (lifted) Color.Transparent else MaterialTheme.colorScheme.surfaceContainerLow, shape = shape) {
        Row(
            Modifier.fillMaxWidth()
                .clickable(role = Role.Button, onClickLabel = if (folded) "Show" else "Fold", onClick = onToggle)
                .padding(start = Space.l, end = Space.l, top = Space.l, bottom = if (closed) Space.l else Space.s)
                .semantics {
                    stateDescription = if (folded) "Folded" else "Open"
                    customActions = actions
                },
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
    }
}

/** A group's card's top: its top corners rounded, its bottom square where its rows go on. */
private fun cardTop(card: CornerBasedShape): Shape = card.copy(bottomStart = ZeroCornerSize, bottomEnd = ZeroCornerSize)

/** The foot of a group's card: its bottom corners, and a new category. */
@Composable
private fun GroupFoot(onAdd: () -> Unit, modifier: Modifier = Modifier) {
    val shape = MaterialTheme.shapes.large.copy(topStart = ZeroCornerSize, topEnd = ZeroCornerSize)
    Surface(color = MaterialTheme.colorScheme.surfaceContainerLow, shape = shape, modifier = modifier.fillMaxWidth()) {
        Row {
            TextButton(onClick = onAdd, modifier = Modifier.padding(start = Space.s, bottom = Space.xs)) {
                Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.padding(end = Space.xs))
                Text("Category")
            }
        }
    }
}

/** A category's row in its group's card; lifted, the drag's own ground shows through. */
@Composable
private fun CategoryRow(l: BudgetLine, lifted: Boolean, actions: List<CustomAccessibilityAction>, onClick: () -> Unit) {
    val a = LocalAmounts.current
    val (fraction, color) = meterOf(l)
    val story = storyOf(l)
    Column(
        Modifier.fillMaxWidth()
            .background(if (lifted) Color.Transparent else MaterialTheme.colorScheme.surfaceContainerLow)
            .clickable(onClickLabel = "Open", onClick = onClick)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) {
                contentDescription = "${l.name}, ${a.format(l.available)} available" + if (story.isNotEmpty()) ". $story" else ""
                customActions = actions
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
