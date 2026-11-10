// An account's register: its working balance large on top with what is
// cleared and what is not beside it, the bank that feeds it and what the
// bank says, then its transactions by day under sticky headings. Each row
// has its cleared mark at the front — tapped, it clears or uncleared; a
// lock, it is reconciled and stays — and a new one from the bank is marked
// and swipes to approve. Reconcile is in the top bar: does the bank say the
// cleared balance? Yes locks it; no asks what the bank says and adjusts.
package dev.money.android

import uniffi.money_app.MoneyView
import dev.arkui.watch
import androidx.compose.foundation.ExperimentalFoundationApi
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.font.FontWeight
import dev.arkui.CardCorner
import dev.arkui.RowDivider
import dev.arkui.Tokens
import dev.arkdb.ffi.Id
import dev.arkui.AmountField
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.Space
import dev.arkui.reflowing
import dev.arkui.Stat
import dev.arkui.SwipeToAct
import uniffi.money_core.Cleared
import uniffi.money_core.TxnLine
import dev.arkui.pressable
import dev.arkui.Press
import uniffi.ark_format.formatDayHeading

@Composable
fun RegisterScreen(ledger: Ledger, account: Id?, onBack: () -> Unit) {
    val shell = LocalShell.current
    val book = shell.book
    val acc = account?.let { ledger.account(it) }
    var reconciling by rememberSaveable { mutableStateOf(false) }
    val list = rememberLazyListState()
    val expanded by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }
    val rows = book.watched.watch(MoneyView.Register(account)) { book.session.register(account) }

    Page(
        title = if (account == null) "All Accounts" else acc?.name ?: "Account",
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
                            Text("Working Balance", style = MaterialTheme.typography.titleMedium)
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
            days(rows, ledger.today, showAccount = account == null)
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

/**
 * `rows` under a heading per day, newest first, each day's rows in a rounded card with hairlines
 * between them, as a register and every account's transactions lay them out.
 */
@OptIn(ExperimentalFoundationApi::class)
fun LazyListScope.days(rows: List<TxnLine>, today: String, showAccount: Boolean, swipe: Boolean = true) {
    val byDay = rows.groupBy { it.date }.toSortedMap(compareByDescending { it })
    for ((day, ts) in byDay) {
        stickyHeader(key = "d$day") {
            Text(
                formatDayHeading(day, today),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier =
                    Modifier.fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surface)
                        .padding(start = Space.page + Space.l, end = Space.page, top = Space.m, bottom = Space.s)
                        .semantics { heading() })
        }
        itemsIndexed(ts, key = { _, t -> t.id.hex }) { i, t ->
            TxnRow(
                t,
                showAccount,
                swipe,
                reflowing().padding(horizontal = Space.page).clip(inCard(i, ts.size)),
                divided = i > 0)
        }
    }
}

/** The corners of a card's `i`th row of `n`: the first rounds its top, the last its bottom. */
fun inCard(i: Int, n: Int): Shape {
    val r = CardCorner
    return RoundedCornerShape(topStart = if (i == 0) r else 0.dp, topEnd = if (i == 0) r else 0.dp, bottomStart = if (i == n - 1) r else 0.dp, bottomEnd = if (i == n - 1) r else 0.dp)
}

/** What came in, in the theme's positive green: the iPhone's `.positive`. */
@Composable
fun positiveColor(): Color = Tokens.of(dark = MaterialTheme.colorScheme.background.luminance() < 0.5f).positive

/**
 * One transaction, as the iPhone draws it: a dot while it is new, the payee (bold while new), its
 * category and account under it (the category in amber when it needs one), its memo, and at the end
 * the amount (what came in in green) over its receipt and cleared marks. A tap is `onClick` (its
 * form, unless said); a new one swipes to approve. `divided` draws the hairline over it.
 */
@Composable
fun TxnRow(
    t: TxnLine,
    showAccount: Boolean,
    swipe: Boolean = true,
    modifier: Modifier = Modifier,
    divided: Boolean = false,
    onClick: (() -> Unit)? = null,
) {
    val shell = LocalShell.current
    val book = shell.book
    val a = LocalAmounts.current
    val needs = t.needsCategory()
    val quiet = MaterialTheme.colorScheme.onSurfaceVariant
    val row: @Composable () -> Unit = {
        Column(Modifier.background(MaterialTheme.colorScheme.surfaceContainerLow)) {
            if (divided) RowDivider(inset = Space.l + 8.dp + Space.m)
            Row(
                Modifier.fillMaxWidth()
                    .pressable(onClickLabel = if (onClick == null) "Edit" else "Open", onClick = { onClick?.invoke() ?: shell.onEdit(t) }, press = Press.Highlight)
                    .padding(start = Space.l, end = Space.l, top = Space.m, bottom = Space.m)
                    .semantics(mergeDescendants = true) {
                        contentDescription =
                            "${t.payee.ifEmpty { "No payee" }}, ${a.signed(t.amount)}, ${t.category}" + (if (!t.approved) ", waiting for review" else "") +
                                (if (t.receipt.isNotEmpty() || t.photos > 0) ", has a receipt" else "") +
                                when (t.cleared) {
                                    Cleared.RECONCILED -> ", reconciled"
                                    Cleared.CLEARED -> ", cleared"
                                    Cleared.UNCLEARED -> ", uncleared"
                                }
                    },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(if (t.approved) Color.Transparent else MaterialTheme.colorScheme.primary))
                Spacer(Modifier.width(Space.m))
                Column(Modifier.weight(1f)) {
                    Text(
                        t.payee.ifEmpty { "No Payee" },
                        style = MaterialTheme.typography.bodyLarge,
                        fontWeight = if (t.approved) null else FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (t.transfer) {
                            Icon(Glyphs.Transfer, contentDescription = null, tint = quiet, modifier = Modifier.size(12.dp))
                            Spacer(Modifier.width(Space.xs))
                        }
                        Text(
                            t.category,
                            style = MaterialTheme.typography.bodySmall,
                            color = if (needs) MaterialTheme.colorScheme.tertiary else quiet,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false))
                        if (showAccount) {
                            Text(" · ${t.account}", style = MaterialTheme.typography.bodySmall, color = quiet, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                    if (t.memo.isNotEmpty()) {
                        Text(t.memo, style = MaterialTheme.typography.bodySmall, color = quiet, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                Spacer(Modifier.width(Space.s))
                Column(horizontalAlignment = Alignment.End) {
                    AmountText(
                        t.amount,
                        signed = t.amount > 0,
                        style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
                        color = if (t.amount > 0) positiveColor() else MaterialTheme.colorScheme.onSurface)
                    Spacer(Modifier.height(2.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        // A receipt paid by it, or a photo of one taken for it.
                        if (t.receipt.isNotEmpty() || t.photos > 0) {
                            Icon(Glyphs.Receipt, contentDescription = null, tint = quiet, modifier = Modifier.size(16.dp))
                            Spacer(Modifier.width(Space.xs))
                        }
                        ClearedMark(t)
                    }
                }
            }
        }
    }
    if (swipe && !t.approved) {
        SwipeToAct(label = "Approve", icon = Icons.Default.Check, onAct = { book.approve(listOf(t.id)) }, modifier = modifier, row = row)
    } else {
        Box(modifier) { row() }
    }
}

/**
 * The cleared mark, small at the end of a row as the iPhone's is: an outlined check while
 * uncleared, a filled one once cleared — a tap toggles it — and a lock once reconciled.
 */
@Composable
private fun ClearedMark(t: TxnLine) {
    val book = LocalShell.current.book
    when (t.cleared) {
        Cleared.RECONCILED ->
            Icon(Glyphs.Locked, contentDescription = "Reconciled", tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
        else -> {
            val on = t.cleared == Cleared.CLEARED
            Box(
                Modifier.size(20.dp)
                    .clip(CircleShape)
                    .pressable(onClickLabel = if (on) "Mark uncleared" else "Mark cleared", onClick = { book.setCleared(t.id, !on) })
                    .semantics { stateDescription = if (on) "Cleared" else "Uncleared" },
                contentAlignment = Alignment.Center,
            ) {
                if (on) {
                    Box(Modifier.size(18.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary), contentAlignment = Alignment.Center) {
                        Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimary, modifier = Modifier.size(12.dp))
                    }
                } else {
                    Icon(Glyphs.Cleared, contentDescription = null, tint = MaterialTheme.colorScheme.outline, modifier = Modifier.size(18.dp))
                }
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
