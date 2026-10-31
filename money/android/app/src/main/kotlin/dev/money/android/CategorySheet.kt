// One category's story this month, in a modal bottom sheet: what it has,
// what it was given and what it spent, a field for what it is given with
// the amounts people reach for as chips (its target, what last month gave
// it, what it spends on average), its target, and the two ways money moves —
// Move Money out of it, or Cover Overspending into it from another category.
// Moving and covering are the sheet's second page, a category chosen from
// the list, sliding over the first as the desktop's inspector's chooser
// does. The system's back gesture shrinks the sheet as it goes (Material's
// own predictive back for sheets).
//
// A card's payment category has the card's balance in place of a target:
// what the card owes at the month's end, what is set aside for it, what is
// not covered, and "Assign $X to cover the debt" in one press. It is the
// card's, so it is not renamed or deleted here. A category that spent more on
// a card than it had says so in amber: that is debt, not next month's money.
package dev.money.android

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import dev.arkdb.ffi.Id
import dev.arkui.AmountField
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.Meter
import dev.arkui.Pill
import dev.arkui.Space
import dev.arkui.Stat
import dev.arkui.Tone
import dev.arkui.toneColors
import uniffi.money_domain.BudgetLine

/** Which page of the sheet is showing. */
private enum class Leaf {
    Story,
    Move,
    Cover,
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CategorySheet(ledger: Ledger, category: Id, onDismiss: () -> Unit) {
    val l = ledger.line(category)
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    // Gone from the month (deleted, or the month changed under it): nothing to show.
    LaunchedEffect(l == null) { if (l == null && ledger.ready) onDismiss() }
    if (l == null) return
    var leaf by remember { mutableStateOf(Leaf.Story) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        AnimatedContent(
            targetState = leaf,
            transitionSpec = {
                val forward = targetState != Leaf.Story
                val d = if (forward) 1 else -1
                (slideInHorizontally { w -> d * w / 4 } + fadeIn()) togetherWith (slideOutHorizontally { w -> -d * w / 4 } + fadeOut())
            },
            label = "category-sheet",
        ) { now ->
            when (now) {
                Leaf.Story -> Story(ledger, l, onMove = { leaf = Leaf.Move }, onCover = { leaf = Leaf.Cover }, onDismiss = onDismiss)
                Leaf.Move -> MoveMoney(ledger, l, onBack = { leaf = Leaf.Story })
                Leaf.Cover -> Cover(ledger, l, onDone = { leaf = Leaf.Story })
            }
        }
    }
}

@Composable
private fun Story(ledger: Ledger, l: BudgetLine, onMove: () -> Unit, onCover: () -> Unit, onDismiss: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    var assigned by remember(l.categoryId, ledger.month) { mutableStateOf(a.editable(l.assigned)) }
    var target by remember(l.categoryId) { mutableStateOf(a.editable(l.target)) }
    var menu by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<String?>(null) }
    var deleting by remember { mutableStateOf(false) }
    val typed = a.parse(assigned)
    val payment = Ledger.isPayment(l)

    Column(
        Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = Space.xl).navigationBarsPadding(),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (payment) {
                CardTile()
                Spacer(Modifier.width(Space.m))
            }
            Column(Modifier.weight(1f)) {
                Text(l.group, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(l.name, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
            }
            IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "More") }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                // A card's payment category is the card's: named as it is, and gone when it closes.
                if (!payment) DropdownMenuItem(text = { Text("Rename") }, onClick = { menu = false; renaming = l.name })
                DropdownMenuItem(
                    text = { Text(if (l.hidden) "Unhide" else "Hide") },
                    onClick = { menu = false; book.hideCategory(l.categoryId, !l.hidden) })
                if (!payment) DropdownMenuItem(text = { Text("Delete") }, onClick = { menu = false; deleting = true })
            }
        }
        Spacer(Modifier.height(Space.l))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Available", style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
            Pill(toneOf(l)) { AmountText(l.available, style = MaterialTheme.typography.titleLarge) }
        }
        val story = storyOf(l, a)
        if (story.isNotEmpty()) {
            Text(story, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(Space.l))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.xl)) {
            Stat("Carried over") { AmountText(l.carried, style = MaterialTheme.typography.titleSmall) }
            Stat("Assigned") { AmountText(l.assigned, style = MaterialTheme.typography.titleSmall) }
            // A payment category's activity is the spending its card covered, less what was paid.
            Stat(if (payment) "Covered, less paid" else "Activity") { AmountText(l.activity, style = MaterialTheme.typography.titleSmall) }
        }
        if (payment) {
            Spacer(Modifier.height(Space.l))
            CardBalance(l)
        } else if (l.creditOverspent > 0) {
            Spacer(Modifier.height(Space.l))
            CreditOverspent(l)
        }

        Spacer(Modifier.height(Space.xl))
        AmountField(
            assigned,
            onType = { assigned = it },
            label = "Assigned in ${Dates.monthAlone(ledger.month)}",
            error = if (typed == null) "That isn't an amount" else null,
            onDone = { typed?.let { book.assign(l.categoryId, it) } },
            modifier = Modifier.fillMaxWidth(),
        )
        // The amounts people reach for.
        Row(horizontalArrangement = Arrangement.spacedBy(Space.s), modifier = Modifier.horizontalScrollSafe()) {
            if (l.target > 0) Quick("Target", l.target) { assigned = a.editable(it); book.assign(l.categoryId, it) }
            if (l.assignedLastMonth > 0) Quick("Last month", l.assignedLastMonth) { assigned = a.editable(it); book.assign(l.categoryId, it) }
            if (l.averageSpent > 0) Quick("Average spent", l.averageSpent) { assigned = a.editable(it); book.assign(l.categoryId, it) }
        }
        if (typed != null && typed != l.assigned) {
            Button(onClick = { book.assign(l.categoryId, typed) }, modifier = Modifier.fillMaxWidth().padding(top = Space.s)) {
                Text("Assign ${a.format(typed)}")
            }
        }

        // A card's payment category is funded toward what the card owes, not a target.
        if (!payment) {
            Spacer(Modifier.height(Space.l))
            Text("Target", style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
            Text(
                "What this category sets aside each month.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (l.target > 0) {
                Spacer(Modifier.height(Space.s))
                Meter(l.assigned.toFloat() / l.target, color = if (l.assigned >= l.target) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.tertiary)
            }
            val t = a.parse(target)
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = Space.s)) {
                AmountField(
                    target,
                    onType = { target = it },
                    label = "Each month",
                    error = if (t == null) "That isn't an amount" else null,
                    imeAction = ImeAction.Done,
                    onDone = { t?.let { book.setTarget(l.categoryId, it) } },
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(Space.s))
                FilledTonalButton(onClick = { t?.let { book.setTarget(l.categoryId, it) } }, enabled = t != null && t != l.target) {
                    Text(if (t == 0L && l.target > 0) "Remove" else "Set")
                }
            }
        }

        Spacer(Modifier.height(Space.xl))
        if (l.available < 0) {
            Button(onClick = onCover, modifier = Modifier.fillMaxWidth()) {
                Text(if (creditOverspent(l)) "Cover ${a.format(-l.available)} spent on a card" else "Cover ${a.format(-l.available)} overspending")
            }
        }
        if (l.available > 0) {
            OutlinedButton(onClick = onMove, modifier = Modifier.fillMaxWidth()) {
                Icon(Glyphs.Move, contentDescription = null, modifier = Modifier.padding(end = Space.s))
                Text("Move money")
            }
        }
        Spacer(Modifier.height(Space.xl))
    }

    renaming?.let { typedName ->
        NameDialog(
            title = "Rename",
            typed = typedName,
            onType = { renaming = it },
            onDone = { book.renameCategory(l.categoryId, typedName.trim()); renaming = null },
            onDismiss = { renaming = null })
    }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text("Delete ${l.name}?") },
            text = { Text("Its transactions will need a category again.") },
            confirmButton = {
                TextButton(onClick = { deleting = false; book.deleteCategory(l.categoryId); onDismiss() }) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun Quick(label: String, amount: Long, onPick: (Long) -> Unit) {
    val a = LocalAmounts.current
    SuggestionChip(onClick = { onPick(amount) }, label = { Text("$label · ${a.format(amount)}") })
}

/**
 * A card's payment category's card: what the card owes at the month's end, what of it the category
 * has, and what it does not — with the one press that covers it, from Ready to Assign.
 */
@Composable
private fun CardBalance(l: BudgetLine) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    val c = MaterialTheme.colorScheme
    Card(
        colors = CardDefaults.cardColors(containerColor = c.surfaceContainerHigh),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(Space.l)) {
            Text("Card balance", style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
            Spacer(Modifier.height(Space.s))
            Figure("Owed at the end of the month", l.debt)
            Figure("Set aside to pay it", l.available.coerceAtLeast(0L))
            Figure("Not covered", l.underfunded, color = if (l.underfunded > 0) c.tertiary else c.primary)
            if (l.debt > 0) {
                Spacer(Modifier.height(Space.s))
                Meter(l.available.toFloat() / l.debt, color = if (l.underfunded > 0) c.tertiary else c.primary)
            }
            Spacer(Modifier.height(Space.m))
            when {
                l.underfunded > 0 ->
                    Button(onClick = { book.coverCard(l.categoryId) }, modifier = Modifier.fillMaxWidth()) {
                        Text("Assign ${a.format(l.underfunded)} to cover the debt")
                    }
                l.debt > 0 -> Text("Enough is set aside to pay it.", style = MaterialTheme.typography.bodyMedium, color = c.primary)
                else -> Text("Nothing is owed on it.", style = MaterialTheme.typography.bodyMedium, color = c.onSurfaceVariant)
            }
            Spacer(Modifier.height(Space.s))
            Text(
                "Spending on the card moves what its category had here. What a category couldn't cover is debt, for money assigned here to pay.",
                style = MaterialTheme.typography.bodySmall,
                color = c.onSurfaceVariant)
        }
    }
}

/** A category that spent more on a card than it had: debt now, not next month's money. */
@Composable
private fun CreditOverspent(l: BudgetLine) {
    val (container, on) = toneColors(Tone.Caution)
    Card(
        colors = CardDefaults.cardColors(containerColor = container, contentColor = on),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(Space.l)) {
            Figure("Spent on a card, not covered", l.creditOverspent, color = on)
            Text(
                "It's the card's debt now: it doesn't come out of next month. Cover it here, or assign to the card's payment category.",
                style = MaterialTheme.typography.bodySmall)
        }
    }
}

/** A label and its figure, on one line. */
@Composable
private fun Figure(label: String, amount: Long, color: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.onSurface) {
    Row(Modifier.fillMaxWidth().padding(vertical = Space.xs), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        AmountText(amount, style = MaterialTheme.typography.titleSmall, color = color)
    }
}

/** Money out of `l`: how much, then where to. */
@Composable
private fun MoveMoney(ledger: Ledger, l: BudgetLine, onBack: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    var typed by remember { mutableStateOf(a.editable(l.available.coerceAtLeast(0L))) }
    val amount = a.parse(typed)
    Column(Modifier.navigationBarsPadding()) {
        Row(Modifier.padding(horizontal = Space.l), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = onBack) { Text("Back") }
        }
        AmountField(
            typed,
            onType = { typed = it },
            label = "Move from ${l.name}",
            error = if (amount == null || amount <= 0) "An amount to move" else null,
            modifier = Modifier.fillMaxWidth().padding(horizontal = Space.xl),
        )
        Spacer(Modifier.height(Space.s))
        CategoryPicker(
            ledger,
            title = "To",
            ready = Pick.Category(null),
            skip = l.categoryId,
            cards = true,
            onPick = { p ->
                if (p is Pick.Category && amount != null && amount > 0) {
                    book.moveMoney(l.categoryId, p.id, amount)
                    onBack()
                }
            },
        )
    }
}

/** An overspent `l` made whole from another category, or from Ready to Assign. */
@Composable
private fun Cover(ledger: Ledger, l: BudgetLine, onDone: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    Column(Modifier.navigationBarsPadding()) {
        Row(Modifier.padding(horizontal = Space.l), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = onDone) { Text("Back") }
        }
        CategoryPicker(
            ledger,
            title = "Cover ${a.format(-l.available)} from",
            ready = Pick.Category(null),
            skip = l.categoryId,
            onlyWithMoney = true,
            cards = true,
            onPick = { p ->
                if (p is Pick.Category) {
                    book.cover(l.categoryId, p.id)
                    onDone()
                }
            },
        )
    }
}

/** A row of chips that scrolls sideways when it is wider than the sheet. */
@Composable
private fun Modifier.horizontalScrollSafe(): Modifier = this.fillMaxWidth().horizontalScroll(rememberScrollState())
