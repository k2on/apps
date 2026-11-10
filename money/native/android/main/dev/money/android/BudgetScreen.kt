// The month's budget, as YNAB taught people to read one and laid out as the
// iPhone app lays it: the month, paged under the page's title; what is
// ready to assign, the page's one large figure, on a card washed in its
// tone (green while there is money to give a job, red when more was
// assigned than there is), with what needs a look under it as pills — how
// many are overspent, how many underfunded (a tap funds them); then each
// group of categories under its quiet heading and total, as a card, every
// category a row with what is left in a pill — green-ish where there is
// some, amber where its target is not met, red where it is overspent — and,
// when it has a target, a bar of how far it is funded. A category opens its
// sheet (CategorySheet.kt).
//
// Credit cards are budgeted as YNAB budgets them (`money/domain/src/cards.rs`):
// the budget's own Credit Card Payments group comes first, a payment category
// per card, each marked with the card's glyph and a bar of what of the card's
// debt is set aside, "Covered" or "$X short" — short in amber, and the
// group's heading adds it up. Spending on a card more than a category had is debt, not next
// month's money: that category's pill is amber too ("on a card, not
// covered"), where cash overspent is red. The cards' group takes no new
// category, and a payment category is dragged only among the cards.
//
// A category can be assigned by rule (`money/domain/src/rules.rs`, set on
// the desktop): in a month that follows one, its amount is the rule's, so
// its pill carries the rule's mark and nothing here assigns it, moves money
// to or from it, or auto-assigns it ([assignable]).
//
// The budget is put in order by hand, as a launcher's apps are: a category
// held lifts and is dragged among the rest, and past a group's header into
// that group; a group's header held folds every group to its header, so the
// groups are dragged past one another as single rows, and opens them again
// on the drop (arkui's Reorder, in sections). So that a category can cross
// from one card into the next, each card is the lazy list's rows rather than
// one row of it: a heading over the card's top corners, a row per category,
// and a foot with its bottom corners. The drop is the domain's
// `move_category` or `move_group`, in front of whatever it landed before.
package dev.money.android

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.CornerBasedShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.shape.ZeroCornerSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Warning
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
import androidx.compose.ui.graphics.graphicsLayer
import dev.arkui.CardCorner
import dev.arkui.RowDivider
import dev.arkui.unlessReduced
import dev.arkui.Springs
import dev.arkui.AmountText
import dev.arkui.Amounts
import dev.arkui.Landing
import dev.arkui.LocalAmounts
import dev.arkui.MarkedAmount
import dev.arkui.ProgressBar
import dev.arkui.Pill
import dev.arkui.ReorderableItem
import dev.arkui.Space
import dev.arkui.Stat
import dev.arkui.Tint
import dev.arkui.makingWay
import dev.arkui.rememberReorder
import dev.arkui.sections
import dev.arkui.tintColors
import uniffi.money_domain.Auto
import uniffi.money_domain.Tone
import uniffi.money_domain.cardGroupId
import uniffi.money_domain.BudgetLine
import uniffi.money_domain.GroupLine
import dev.arkui.pressable
import dev.arkui.Press

/** A bar in a line's meter tone ([meterColor] of the core's `Meter`). */
@Composable
private fun meterColor(t: Tone): Color {
    val c = MaterialTheme.colorScheme
    return when (t) {
        Tone.OVERSPENT -> c.error
        Tone.SHORT, Tone.ON_CARD -> c.tertiary
        else -> c.primary
    }
}

/**
 * Whether a category's key may land under a group's: a card's payment category stays with the cards,
 * and nothing else goes among them (the domain's `move_category`).
 */
fun fitsOf(budget: List<Pair<GroupLine, List<BudgetLine>>>): (Any, Any?) -> Boolean {
    val payments = budget.flatMap { it.second }.filter { it.isPayment() }.map { categoryKey(it.categoryId) }.toSet()
    val cards = groupKey(cardGroupId())
    return { row, header -> (row in payments) == (header == cards) }
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
    // Under the shell's bar (ArkUI's), the month and its arrows are the bar's.
    val underBar = dev.arkui.LocalUnderToolbar.current
    val expanded by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }

    // The groups and their categories as the reorder holds them: each group's key, then its
    // categories', in the budget's order.
    val budget = remember(ledger.groups, ledger.lines, showHidden) { ledger.byGroup(showHidden) }
    val groups = remember(budget) { budget.associate { (g, _) -> groupKey(g.id) to g } }
    val lines = remember(budget) { budget.flatMap { it.second }.associateBy { categoryKey(it.categoryId) } }
    val keys = remember(budget) { keysOf(budget) }
    // The cards' payment categories stay among themselves, and nothing else joins them.
    val fits = remember(budget) { fitsOf(budget) }
    // After the ready card and the notices.
    val reorder =
        rememberReorder(list, lead = if (underBar) 1 else 2, isHeader = ::isGroupKey, fits = fits) { key, landing ->
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
        title = "Budget",
        actions = {
            IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "Budget options") }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                if (ledger.month != ledger.today.take(7)) {
                    DropdownMenuItem(text = { Text("This Month") }, onClick = { menu = false; book.stepMonth(0) })
                    HorizontalDivider()
                }
                DropdownMenuItem(text = { Text("Auto-Assign Underfunded") }, onClick = { menu = false; book.autoAssign(Auto.UNDERFUNDED) })
                DropdownMenuItem(text = { Text("Auto-Assign Last Month") }, onClick = { menu = false; book.autoAssign(Auto.LAST_MONTH) })
                DropdownMenuItem(text = { Text("Reset Assigned Amounts") }, onClick = { menu = false; book.autoAssign(Auto.RESET) })
                HorizontalDivider()
                DropdownMenuItem(text = { Text("New Group") }, onClick = { menu = false; newGroup = "" })
                DropdownMenuItem(
                    text = { Text(if (showHidden) "Hide Hidden Categories" else "Show Hidden Categories") },
                    onClick = { menu = false; showHidden = !showHidden })
                HorizontalDivider()
                DropdownMenuItem(text = { Text("Settings") }, onClick = { menu = false; shell.onSettings() })
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
            contentPadding = PaddingValues(start = Space.page, end = Space.page, top = pad.calculateTopPadding(), bottom = pad.calculateBottomPadding() + 96.dp),
        ) {
            if (!underBar) item(key = "month") { MonthPager(ledger) }
            item(key = "ready") { ReadyCard(ledger, Modifier.padding(top = Space.s)) }
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
                        shape = RoundedCornerShape(CardCorner).let { if (closed) it else cardTop(it) },
                    ) {
                        GroupHeader(
                            g,
                            available = rows.sumOf { lines[it]?.available ?: 0L },
                            short = if (g.id == cardGroupId()) rows.sumOf { lines[it]?.underfunded ?: 0L } else 0L,
                            folded = folded,
                            closed = closed,
                            lifted = reorder.dragged == gk,
                            actions = reorder.actions(gk),
                            onToggle = { collapsed = if (folded) collapsed - hex else collapsed + hex },
                        )
                    }
                }
                for ((i, ck) in shown.withIndex()) {
                    val l = lines[ck] ?: continue
                    item(key = ck) {
                        ReorderableItem(reorder, ck, keys, shape = MaterialTheme.shapes.medium) {
                            CategoryRow(l, first = i == 0, lifted = reorder.dragged == ck, actions = reorder.actions(ck), onClick = { open = l.categoryId.hex })
                        }
                    }
                }
                if (!closed) {
                    item(key = "foot/$hex") {
                        GroupFoot(
                            // The cards' group is the cards': its categories come and go with them.
                            onAdd = if (g.id == cardGroupId()) null else { { newCategory = g.id to "" } },
                            modifier = Modifier.makingWay(this))
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
            title = "New Group",
            typed = typed,
            onType = { newGroup = it },
            onDone = { book.addGroup(typed.trim()); newGroup = null },
            onDismiss = { newGroup = null })
    }
    newCategory?.let { (g, typed) ->
        NameDialog(
            title = "New Category",
            typed = typed,
            onType = { newCategory = g to it },
            onDone = { book.addCategory(g, typed.trim()); newCategory = null },
            onDismiss = { newCategory = null })
    }
}

/**
 * Ready to Assign: the one figure the budget is about, and under it what needs a look as pills —
 * how many categories are overspent, how many are underfunded (a tap funds them), or that the
 * month is on track.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun ReadyCard(ledger: Ledger, modifier: Modifier = Modifier) {
    val book = LocalShell.current.book
    val s = ledger.summary
    val ready = s?.readyToAssign ?: 0L
    val tone = s?.tone()?.tint() ?: Tint.Neutral
    val over = s?.overspent ?: 0L
    val fund = s?.underfunded ?: 0L
    HeroCard("Ready to Assign", ready, tone, note = s?.readyWords().orEmpty(), modifier = modifier) {
        if (over > 0 || fund > 0 || (s != null && s.assigned > 0)) {
            FlowRow(Modifier.padding(top = Space.s), horizontalArrangement = Arrangement.spacedBy(Space.s)) {
                if (over > 0) {
                    Pill(Tint.Negative) { PillLabel(Icons.Default.Warning, "$over overspent") }
                }
                if (fund > 0) {
                    Pill(
                        Tint.Caution,
                        Modifier.clip(CircleShape).pressable(onClickLabel = "Assign every underfunded category its target", onClick = {
                            book.autoAssign(Auto.UNDERFUNDED)
                        })) { PillLabel(Glyphs.Auto, "$fund underfunded") }
                }
                if (over == 0L && fund == 0L) {
                    Pill(Tint.Positive) { PillLabel(Glyphs.Done, "On track") }
                }
            }
        }
    }
}

/** A pill's glyph and words. */
@Composable
private fun PillLabel(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(Space.xs))
        Text(text, style = MaterialTheme.typography.labelLarge)
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
 * A group's heading: its name and what its categories have, quiet, over the top of its card, or
 * alone while it is `closed` (folded, or every group is while one is dragged). A tap folds it or
 * opens it; held, it is dragged among the groups. The cards' group says, in place of its total,
 * what of their debt is not covered (`short`).
 */
@Composable
private fun GroupHeader(
    g: GroupLine,
    available: Long,
    short: Long,
    folded: Boolean,
    closed: Boolean,
    lifted: Boolean,
    actions: List<CustomAccessibilityAction>,
    onToggle: () -> Unit,
) {
    // The chevron turns a quarter as the group folds, on arkui's spring for a move and drawn, not
    // laid out; with motion reduced it is simply turned, the rows going saying the rest.
    val fold by animateFloatAsState(if (folded) 1f else 0f, unlessReduced(Springs.move()), label = "fold")
    val quiet = MaterialTheme.colorScheme.onSurfaceVariant
    Column(Modifier.background(if (lifted) MaterialTheme.colorScheme.surfaceContainerLow else Color.Transparent)) {
        Row(
            Modifier.fillMaxWidth()
                .pressable(role = Role.Button, onClickLabel = if (folded) "Show" else "Fold", onClick = onToggle, press = Press.Highlight)
                .padding(start = Space.xs, end = Space.l, top = Space.l, bottom = Space.s)
                .semantics {
                    stateDescription = if (folded) "Folded" else "Open"
                    customActions = actions
                },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Default.KeyboardArrowDown, contentDescription = null, tint = quiet, modifier = Modifier.size(20.dp).graphicsLayer { rotationZ = -90f * fold })
            Spacer(Modifier.width(Space.xs))
            Text(
                g.name,
                style = MaterialTheme.typography.titleMedium,
                color = quiet,
                modifier = Modifier.weight(1f).semantics { heading() },
                maxLines = 1,
                overflow = TextOverflow.Ellipsis)
            if (short > 0) {
                Text(
                    "${LocalAmounts.current.format(short)} not covered",
                    style = MaterialTheme.typography.titleSmall.copy(fontFeatureSettings = "tnum"),
                    color = MaterialTheme.colorScheme.tertiary,
                    maxLines = 1)
            } else {
                AmountText(available, style = MaterialTheme.typography.titleSmall, color = if (available < 0) MaterialTheme.colorScheme.error else quiet)
            }
        }
    }
}

/** A group's card's top: its top corners rounded, its bottom square where its rows go on. */
private fun cardTop(card: CornerBasedShape): Shape = card.copy(bottomStart = ZeroCornerSize, bottomEnd = ZeroCornerSize)

/** The foot of a group's card: its bottom corners, and a new category where the group takes one. */
@Composable
private fun GroupFoot(onAdd: (() -> Unit)?, modifier: Modifier = Modifier) {
    val shape = RoundedCornerShape(CardCorner).copy(topStart = ZeroCornerSize, topEnd = ZeroCornerSize)
    Surface(color = MaterialTheme.colorScheme.surfaceContainerLow, shape = shape, modifier = modifier.fillMaxWidth()) {
        Column {
            if (onAdd != null) {
                RowDivider()
                TextButton(onClick = onAdd, modifier = Modifier.padding(start = Space.xs)) {
                    Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.padding(end = Space.xs).size(18.dp))
                    Text("New Category")
                }
            } else {
                Spacer(Modifier.height(Space.xs))
            }
        }
    }
}

/** A card's payment category's mark: the card's own, as its account wears it. */
@Composable
fun CardTile(modifier: Modifier = Modifier) {
    Box(modifier) { KindTile("credit", side = 28.dp) }
}

/**
 * A category's row in its group's card: its name and its pill, then — when it has a target, or is
 * a card's payment with something owed — the bar of how far it is, and the line under it. Lifted,
 * the drag's own ground shows through; `first` has no hairline over it.
 */
@Composable
private fun CategoryRow(l: BudgetLine, first: Boolean, lifted: Boolean, actions: List<CustomAccessibilityAction>, onClick: () -> Unit) {
    val a = LocalAmounts.current
    val meter = l.meter()
    val story = l.story(a.style)
    val captions = l.captions(a.style)
    val (left, right) = captions.left to captions.right
    val card = l.isPayment()
    val quiet = MaterialTheme.colorScheme.onSurfaceVariant
    // The first row is the top of its group's card, under the group's heading.
    val top = if (first) Modifier.clip(cardTop(RoundedCornerShape(CardCorner))) else Modifier
    Column(Modifier.fillMaxWidth().then(top).background(if (lifted) Color.Transparent else MaterialTheme.colorScheme.surfaceContainerLow)) {
        if (!first) RowDivider(inset = if (card) Space.l + 28.dp + Space.m else Space.l)
        Row(
            Modifier.fillMaxWidth()
                .pressable(onClickLabel = "Open", onClick = onClick, press = Press.Highlight)
                .padding(horizontal = Space.l, vertical = Space.m)
                .semantics(mergeDescendants = true) {
                    contentDescription = "${l.name}, ${a.format(l.available)} available" + if (story.isNotEmpty()) ". $story" else ""
                    customActions = actions
                },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (card) {
                CardTile()
                Spacer(Modifier.width(Space.m))
            }
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        l.name + if (l.hidden) " (hidden)" else "",
                        style = MaterialTheme.typography.bodyLarge,
                        color = if (l.hidden) quiet else MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f))
                    Spacer(Modifier.width(Space.s))
                    // A month its rule assigns is marked, as the desktop marks its cell: not one to assign here.
                    Pill(l.tone().tint()) {
                        if (l.ruled) MarkedAmount(l.available, Glyphs.Rule, "assigned by rule", style = MaterialTheme.typography.labelLarge)
                        else AmountText(l.available, style = MaterialTheme.typography.labelLarge)
                    }
                }
                if (meter != null) {
                    Spacer(Modifier.height(Space.s))
                    ProgressBar(meter.fraction().toFloat(), color = meterColor(meter.tone), height = 4.dp)
                }
                if (left.isNotEmpty() || right.isNotEmpty()) {
                    Spacer(Modifier.height(Space.xs))
                    Row {
                        val leftColor =
                            when {
                                card && l.underfunded > 0 -> MaterialTheme.colorScheme.tertiary
                                card && l.debt > 0 -> MaterialTheme.colorScheme.primary
                                else -> quiet
                            }
                        Text(left, style = MaterialTheme.typography.bodySmall.copy(fontFeatureSettings = "tnum"), color = leftColor, maxLines = 1, modifier = Modifier.weight(1f))
                        Text(right, style = MaterialTheme.typography.bodySmall.copy(fontFeatureSettings = "tnum"), color = quiet, maxLines = 1)
                    }
                }
                if (!card && l.creditOverspent > 0) {
                    Text(
                        "${a.format(l.creditOverspent)} on card",
                        style = MaterialTheme.typography.bodySmall.copy(fontFeatureSettings = "tnum"),
                        color = MaterialTheme.colorScheme.tertiary)
                }
            }
        }
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
