// The household's accounts, laid out as the iPhone app lays them: what it is
// worth on top, as the page's one large figure; All Accounts, every
// account's transactions; then the budget's accounts, the ones only tracked
// and the closed ones, each section under its name and its total, in a
// rounded card of rows: the kind's mark, the name, the bank and last digits
// that feed it (or its kind), how many of its transactions want a look, and
// its balance. An account opens its register; the banks are Settings'
// (SettingsScreen.kt); adding an account is a sheet. Settings — the banks,
// the person and the server, and signing out — are behind the face in the
// top bar, as Google's apps put them.
package dev.money.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.arkui.GroupCard
import dev.arkui.RowDivider
import dev.arkui.SectionHeader
import dev.arkui.Badge
import dev.arkui.Tile
import dev.arkui.ListRow
import dev.arkui.AmountField
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.Chevron
import dev.arkui.Space
import dev.arkui.Tint
import androidx.compose.foundation.layout.width
import androidx.compose.ui.text.style.TextOverflow
import uniffi.money_domain.AccountKind
import uniffi.money_domain.AccountLine
import uniffi.money_domain.MoneyChange
import uniffi.money_domain.OpenAccount
import uniffi.money_domain.accountKinds
import dev.arkui.pressable
import dev.arkui.Press

@Composable
fun AccountsScreen(ledger: Ledger) {
    val shell = LocalShell.current
    var adding by rememberSaveable { mutableStateOf(false) }
    val a = LocalAmounts.current
    val open = ledger.accounts.filter { !it.closed }.sortedBy { it.pos }
    val budget = open.filter { it.onBudget }
    val tracking = open.filter { !it.onBudget }
    val closed = ledger.accounts.filter { it.closed }

    Page(
        // The face is the page's, at the end of the title, badged while a bank wants somebody there.
        title = "Accounts",
        fab = {
            androidx.compose.material3.FloatingActionButton(onClick = { shell.onAdd(null) }) {
                Icon(Icons.Default.Add, contentDescription = "Add a transaction")
            }
        },
    ) { pad, _ ->
        LazyColumn(contentPadding = PaddingValues(start = Space.page, end = Space.page, top = pad.calculateTopPadding(), bottom = pad.calculateBottomPadding() + 96.dp)) {
            item(key = "worth") {
                HeroCard("Net Worth", open.sumOf { it.balance }, Tint.Neutral, note = "Every open account, less what's owed")
            }
            item(key = "all") {
                GroupCard(Modifier.padding(top = Space.xl)) {
                    ListRow(
                        title = "All Accounts",
                        onClick = { shell.onAccount(null) },
                        picture = { Tile(Glyphs.Activity, MaterialTheme.colorScheme.onSurfaceVariant) },
                        trailing = {
                            AmountText(budget.sumOf { it.balance }, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.width(Space.xs))
                            Chevron()
                        },
                    )
                }
            }
            for ((name, rows) in listOf("Budget" to budget, "Tracking" to tracking, "Closed" to closed)) {
                if (rows.isEmpty()) continue
                item(key = "h-$name") {
                    SectionHeader(name) { if (name != "Closed") HeadingTotal(rows.sumOf { it.balance }) }
                }
                item(key = "s-$name") {
                    GroupCard {
                        rows.forEachIndexed { i, acc ->
                            if (i > 0) RowDivider(inset = Space.l + 32.dp + Space.m)
                            AccountRow(acc, onClick = { shell.onAccount(acc.id) })
                        }
                    }
                }
            }
            item(key = "actions") {
                GroupCard(Modifier.padding(top = Space.xl)) {
                    ListRow(
                        title = if (ledger.banks.isEmpty()) "Connect a Bank" else "Bank Connections",
                        onClick = shell.onSettings,
                        trailing = {
                            Spacer(Modifier.width(Space.xs))
                            Chevron()
                        },
                        picture = { Icon(Glyphs.Link, contentDescription = null, tint = MaterialTheme.colorScheme.primary) },
                    )
                    RowDivider(inset = Space.l + 32.dp + Space.m)
                    ListRow(
                        title = "Add Account",
                        onClick = { adding = true },
                        picture = { Icon(Icons.Default.Add, contentDescription = null, tint = MaterialTheme.colorScheme.primary) },
                        trailing = {},
                        accent = true,
                    )
                }
            }
        }
    }
    if (adding) AddAccountSheet(onDismiss = { adding = false })
}

>>>

/** An account in its section's card: its mark, its name and what feeds it, its count to review, its balance. */
@Composable
private fun AccountRow(acc: AccountLine, onClick: () -> Unit) {
    val a = LocalAmounts.current
    Row(
        Modifier.fillMaxWidth()
            .pressable(onClickLabel = "Open the register", onClick = onClick, press = Press.Highlight)
            .padding(horizontal = Space.l, vertical = Space.m)
            .semantics(mergeDescendants = true) {
                contentDescription =
                    "${acc.name}, ${acc.subtitle()}, ${a.format(acc.balance)}" + if (acc.unapproved > 0) ", ${acc.unapproved} to review" else ""
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        KindTile(acc.kind)
        Spacer(Modifier.width(Space.m))
        Column(Modifier.weight(1f)) {
            Text(acc.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(acc.subtitle(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
        }
        Spacer(Modifier.width(Space.s))
        if (acc.unapproved > 0) {
            Badge(acc.unapproved.toInt(), "${acc.unapproved} to review")
            Spacer(Modifier.width(Space.s))
        }
        AmountText(
            acc.balance,
            style = MaterialTheme.typography.bodyLarge,
            color = if (acc.balance < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
        Spacer(Modifier.width(Space.xs))
        Chevron()
    }
}

>>>

@OptIn(ExperimentalMaterial3Api::class, androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun AddAccountSheet(onDismiss: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var name by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf(AccountKind.CHECKING) }
    var onBudget by remember { mutableStateOf(true) }
    var typed by remember { mutableStateOf("") }
    val cents = a.parse(typed)
    val balance = cents?.let { if (kind.owes()) -kotlin.math.abs(it) else it }
    // The domain's own sentence for the name, asked as it is typed.
    val why =
        if (name.isEmpty()) null
        else book.check(MoneyChange.OpenAccount(OpenAccount(name, kind, onBudget, balance ?: 0L, "2000-01-01")))
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(Modifier.padding(horizontal = Space.xl).navigationBarsPadding().imePadding()) {
            Text("Add Account", style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
            Spacer(Modifier.height(Space.l))
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Name") },
                isError = why != null,
                supportingText = if (why != null) { { Text(why) } } else null,
                singleLine = true,
                modifier = Modifier.fillMaxWidth())
            Spacer(Modifier.height(Space.s))
            androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(Space.s)) {
                for (k in accountKinds()) {
                    FilterChip(
                        selected = kind == k,
                        onClick = {
                            kind = k
                            onBudget = k.onBudgetByDefault()
                        },
                        label = { Text(k.title()) },
                        leadingIcon = if (kind == k) { { Icon(Glyphs.account(k.key()), contentDescription = null, modifier = Modifier.size(18.dp)) } } else null,
                    )
                }
            }
            Spacer(Modifier.height(Space.s))
            AmountField(
                typed,
                onType = { typed = it },
                label = if (owed) "What you owe on it" else "What's in it today",
                error = if (cents == null) "That isn't an amount" else null,
                modifier = Modifier.fillMaxWidth())
            ListItem(
                headlineContent = { Text("In the Budget") },
                supportingContent = {
                    Text(if (onBudget) "What's spent from it comes out of categories" else "Tracked only: its balance counts toward net worth")
                },
                trailingContent = { Switch(checked = onBudget, onCheckedChange = { onBudget = it }) },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            )
            Button(
                onClick = {
                    if (balance != null) {
                        book.openAccount(name.trim(), kind, onBudget, balance)
                        onDismiss()
                    }
                },
                enabled = name.isNotBlank() && why == null && balance != null,
                modifier = Modifier.fillMaxWidth()) { Text("Add Account") }
            Spacer(Modifier.height(Space.xl))
        }
    }
}

