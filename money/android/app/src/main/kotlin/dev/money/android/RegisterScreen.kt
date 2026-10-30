// An account's register: its working balance large on top with what is
// cleared and what is not beside it, the bank that feeds it and what the
// bank says, then its transactions by day under sticky headings. Each row
// has its cleared mark at the front — tapped, it clears or uncleared; a
// lock, it is reconciled and stays — and a new one from the bank is marked
// and swipes to approve. Reconcile is in the top bar: does the bank say the
// cleared balance? Yes locks it; no asks what the bank says and adjusts.
package dev.money.android

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.arkui.AmountField
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.Space
import dev.arkui.Stat
import dev.arkui.SwipeToAct
import uniffi.money_domain.Cleared
import uniffi.money_domain.TxnLine

@Composable
fun RegisterScreen(ledger: Ledger, account: Id, onBack: () -> Unit) {
    val shell = LocalShell.current
    val book = shell.book
    val acc = ledger.account(account)
    DisposableEffect(account) {
        book.holdRegister(account)
        onDispose { book.releaseRegister(account) }
    }
    var reconciling by rememberSaveable { mutableStateOf(false) }
    val list = rememberLazyListState()
    val expanded by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }
    val rows = ledger.register(account)

    Page(
        title = acc?.name ?: "Account",
        onBack = onBack,
        actions = {
            if (acc != null && !acc.closed) {
                TextButton(onClick = { reconciling = true }) { Text("Reconcile") }
            }
        },
        fab = {
            ExtendedFloatingActionButton(
                onClick = { shell.onAdd(account) },
                expanded = expanded,
                icon = { Icon(Icons.Default.Add, contentDescription = null) },
                text = { Text("Transaction") })
        },
    ) { pad, _ ->
        LazyColumn(state = list, contentPadding = PaddingValues(bottom = pad.calculateBottomPadding() + 96.dp)) {
            if (acc != null) {
                item(key = "balances") {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
                        shape = MaterialTheme.shapes.extraLarge,
                        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s),
                    ) {
                        Column(Modifier.padding(Space.xl)) {
                            Text("Working balance", style = MaterialTheme.typography.titleMedium)
                            AmountText(acc.balance, style = MaterialTheme.typography.displaySmall)
                            Spacer(Modifier.height(Space.m))
                            Row(horizontalArrangement = Arrangement.spacedBy(Space.xl)) {
                                Stat("Cleared") { AmountText(acc.cleared, style = MaterialTheme.typography.titleSmall) }
                                Stat("Uncleared") { AmountText(acc.uncleared, style = MaterialTheme.typography.titleSmall) }
                            }
                            if (acc.bank.isNotEmpty()) {
                                Spacer(Modifier.height(Space.m))
                                val a = LocalAmounts.current
                                Text(
                                    "${acc.bank} ••${acc.mask} · linked through Plaid" +
                                        (acc.bankBalance?.let { " · the bank says ${a.format(it)}" } ?: ""),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
            days(rows, ledger.today, showAccount = false)
            if (rows.isEmpty() && ledger.ready) {
                item(key = "empty") {
                    Text(
                        "No transactions yet.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(Space.xl))
                }
            }
        }
    }
    if (reconciling && acc != null) {
        ReconcileDialog(acc.cleared, acc.name, onReconcile = { book.reconcile(account, it); reconciling = false }, onDismiss = { reconciling = false })
    }
}

/** `rows` under a heading per day, newest first, as a register and the inbox lay them out. */
@OptIn(ExperimentalFoundationApi::class)
fun LazyListScope.days(rows: List<TxnLine>, today: String, showAccount: Boolean, swipe: Boolean = true) {
    val byDay = rows.groupBy { it.date }.toSortedMap(compareByDescending { it })
    for ((day, ts) in byDay) {
        stickyHeader(key = "d$day") {
            Text(
                Dates.heading(day, today),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier =
                    Modifier.fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surface)
                        .padding(horizontal = Space.page, vertical = Space.s)
                        .semantics { heading() })
        }
        items(ts, key = { it.id.hex }) { t -> TxnRow(t, showAccount, swipe, Modifier.animateItem()) }
    }
}

/** One transaction: cleared mark, payee, category, amount; new ones swipe to approve. */
@Composable
fun TxnRow(t: TxnLine, showAccount: Boolean, swipe: Boolean = true, modifier: Modifier = Modifier) {
    val shell = LocalShell.current
    val book = shell.book
    val a = LocalAmounts.current
    val needs = t.categoryId == null && !t.transfer
    val supporting =
        listOfNotNull(
                if (showAccount) t.account else null,
                t.category.takeIf { it.isNotEmpty() },
                t.memo.takeIf { it.isNotEmpty() },
            )
            .joinToString(" · ")
    val row: @Composable () -> Unit = {
        ListItem(
            headlineContent = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (!t.approved) {
                        Box(Modifier.size(8.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
                        Spacer(Modifier.size(Space.s))
                    }
                    Text(t.payee.ifEmpty { "No payee" }, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            },
            supportingContent = {
                Text(
                    supporting,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    color = if (needs) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
            },
            leadingContent = { ClearedMark(t) },
            trailingContent = {
                AmountText(
                    t.amount,
                    signed = true,
                    style = MaterialTheme.typography.titleSmall,
                    color = if (t.amount > 0) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
            },
            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surface),
            modifier =
                Modifier.clickable(onClickLabel = "Edit", onClick = { shell.onEdit(t) }).semantics(mergeDescendants = true) {
                    contentDescription =
                        "${t.payee}, ${a.signed(t.amount)}, ${t.category}" + (if (!t.approved) ", new" else "") +
                            when (t.cleared) {
                                Cleared.RECONCILED -> ", reconciled"
                                Cleared.CLEARED -> ", cleared"
                                Cleared.UNCLEARED -> ", uncleared"
                            }
                },
        )
    }
    if (swipe && !t.approved) {
        SwipeToAct(label = "Approve", icon = Icons.Default.Check, onAct = { book.approve(listOf(t.id)) }, modifier = modifier, row = row)
    } else {
        Box(modifier) { row() }
    }
}

/** The cleared mark: a toggle, unless the transaction is reconciled and locked. */
@Composable
private fun ClearedMark(t: TxnLine) {
    val book = LocalShell.current.book
    when (t.cleared) {
        Cleared.RECONCILED ->
            Icon(
                Glyphs.Locked,
                contentDescription = "Reconciled",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(12.dp).size(20.dp))
        else -> {
            val on = t.cleared == Cleared.CLEARED
            IconButton(
                onClick = { book.setCleared(t.id, !on) },
                modifier = Modifier.semantics { stateDescription = if (on) "Cleared" else "Uncleared" }) {
                    Icon(
                        if (on) Glyphs.ClearedFilled else Glyphs.Cleared,
                        contentDescription = if (on) "Mark uncleared" else "Mark cleared",
                        tint = if (on) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline)
                }
        }
    }
}

/** Reconciling: does the bank say the cleared balance? Yes, or what does it say. */
@Composable
fun ReconcileDialog(cleared: Long, name: String, onReconcile: (Long) -> Unit, onDismiss: () -> Unit) {
    val a = LocalAmounts.current
    var differs by remember { mutableStateOf(false) }
    var typed by remember { mutableStateOf("") }
    val said = a.parse(typed)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Reconcile $name") },
        text = {
            if (!differs) {
                Text("Is ${a.format(cleared)} what the bank says $name holds today, counting only what has cleared?")
            } else {
                Column {
                    Text("What does the bank say? The difference becomes an adjustment.")
                    Spacer(Modifier.height(Space.m))
                    AmountField(typed, onType = { typed = it }, label = "The bank's balance", error = if (said == null) "That isn't an amount" else null)
                }
            }
        },
        confirmButton = {
            if (!differs) TextButton(onClick = { onReconcile(cleared) }) { Text("Yes") }
            else TextButton(onClick = { said?.let(onReconcile) }, enabled = said != null && typed.isNotBlank()) { Text("Reconcile") }
        },
        dismissButton = {
            if (!differs) TextButton(onClick = { differs = true }) { Text("No") }
            else TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}
