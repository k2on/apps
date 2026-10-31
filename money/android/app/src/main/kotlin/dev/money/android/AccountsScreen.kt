// The household's accounts: what it is worth on top, as a large figure in a
// tinted card, then the budget's accounts and the ones only tracked, each a
// row with its kind's glyph, the bank that feeds it, how many of its
// transactions want a look, and its balance. An account opens its register;
// the banks are Settings' (SettingsScreen.kt); adding an account is a sheet.
// Settings — the banks, the person and the server, and signing out — are
// behind the face in the top bar, as Google's apps put them.
package dev.money.android

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
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.arkui.AmountField
import dev.arkui.AmountText
import dev.arkui.LocalAmounts
import dev.arkui.SectionHeader
import dev.arkui.Space
import dev.arkui.Stat
import org.json.JSONObject
import uniffi.money_domain.AccountLine

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
        title = "Accounts",
        actions = {
            // The face opens Settings, badged while a bank wants somebody there.
            IconButton(onClick = shell.onSettings) {
                BadgedBox(badge = { if (ledger.banksWanting > 0) Badge() }) { Avatar(ledger.who.ifEmpty { "?" }) }
            }
        },
        fab = {
            androidx.compose.material3.FloatingActionButton(onClick = { shell.onAdd(null) }) {
                Icon(Icons.Default.Add, contentDescription = "Add a transaction")
            }
        },
    ) { pad, _ ->
        LazyColumn(contentPadding = PaddingValues(bottom = pad.calculateBottomPadding() + 96.dp)) {
            item(key = "worth") {
                val worth = open.sumOf { it.balance }
                Card(
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer),
                    shape = MaterialTheme.shapes.extraLarge,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page),
                ) {
                    Column(Modifier.padding(Space.xl)) {
                        Text("Net worth", style = MaterialTheme.typography.titleMedium)
                        AmountText(
                            worth,
                            style = MaterialTheme.typography.displaySmall,
                            modifier = Modifier.semantics { contentDescription = "Net worth, ${a.format(worth)}" })
                        Spacer(Modifier.height(Space.m))
                        Row(horizontalArrangement = Arrangement.spacedBy(Space.xl)) {
                            Stat("In the budget") { AmountText(budget.sumOf { it.balance }, style = MaterialTheme.typography.titleSmall) }
                            Stat("Tracking") { AmountText(tracking.sumOf { it.balance }, style = MaterialTheme.typography.titleSmall) }
                        }
                    }
                }
            }
            if (budget.isNotEmpty()) {
                item(key = "h-budget") { SectionHeader("Budget accounts") }
                items(budget, key = { it.id.hex }) { AccountRow(it, onClick = { shell.onAccount(it.id) }) }
            }
            if (tracking.isNotEmpty()) {
                item(key = "h-tracking") { SectionHeader("Tracking") }
                items(tracking, key = { it.id.hex }) { AccountRow(it, onClick = { shell.onAccount(it.id) }) }
            }
            if (closed.isNotEmpty()) {
                item(key = "h-closed") { SectionHeader("Closed") }
                items(closed, key = { it.id.hex }) { AccountRow(it, onClick = { shell.onAccount(it.id) }) }
            }
            item(key = "actions") {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.l),
                    horizontalArrangement = Arrangement.spacedBy(Space.s)) {
                        FilledTonalButton(onClick = { adding = true }) {
                            Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.padding(end = Space.s))
                            Text("Add account")
                        }
                        OutlinedButton(onClick = shell.onSettings) {
                            Icon(Glyphs.Link, contentDescription = null, modifier = Modifier.padding(end = Space.s))
                            Text(if (ledger.banks.isEmpty()) "Connect a bank" else "Banks")
                        }
                    }
            }
        }
    }
    if (adding) AddAccountSheet(onDismiss = { adding = false })
}

@Composable
fun Avatar(name: String) {
    Box(
        Modifier.size(32.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
        contentAlignment = Alignment.Center) {
            Text(
                name.take(1).uppercase(),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
                modifier = Modifier.semantics { contentDescription = "Signed in as $name" })
        }
}

@Composable
private fun AccountRow(acc: AccountLine, onClick: () -> Unit) {
    val supporting =
        listOfNotNull(
                kindName(acc.kind),
                acc.bank.takeIf { it.isNotEmpty() }?.let { b -> if (acc.mask.isNotEmpty()) "$b ••${acc.mask}" else b },
                acc.unapproved.takeIf { it > 0 }?.let { "$it to review" },
            )
            .joinToString(" · ")
    ListItem(
        headlineContent = { Text(acc.name) },
        supportingContent = { Text(supporting) },
        leadingContent = {
            Box(
                Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
                contentAlignment = Alignment.Center) {
                    Icon(Glyphs.account(acc.kind), contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
        },
        trailingContent = {
            AmountText(
                acc.balance,
                style = MaterialTheme.typography.titleMedium,
                color = if (acc.balance < 0) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickable(onClickLabel = "Open the register", onClick = onClick),
    )
}

/** The kinds an account can be, in the order a sheet offers them. */
val KINDS: List<String> = listOf("checking", "savings", "cash", "credit", "loan", "investment")

/** Whether an account of `kind` is in the budget unless somebody says otherwise (the domain's rule). */
fun onBudgetByDefault(kind: String): Boolean = kind in setOf("checking", "savings", "cash", "credit")

@OptIn(ExperimentalMaterial3Api::class, androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun AddAccountSheet(onDismiss: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var name by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf("checking") }
    var onBudget by remember { mutableStateOf(true) }
    var typed by remember { mutableStateOf("") }
    val cents = a.parse(typed)
    val owed = kind == "credit" || kind == "loan"
    val balance = cents?.let { if (owed) -kotlin.math.abs(it) else it }
    // The domain's own sentence for the name, asked as it is typed.
    val why =
        if (name.isEmpty()) null
        else book.check("accounts.open_account", JSONObject().put("name", name).put("kind", kind).put("on_budget", onBudget).put("balance", balance ?: 0L).put("date", "2000-01-01").toString())
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(Modifier.padding(horizontal = Space.xl).navigationBarsPadding().imePadding()) {
            Text("Add an account", style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
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
                for (k in KINDS) {
                    FilterChip(
                        selected = kind == k,
                        onClick = {
                            kind = k
                            onBudget = onBudgetByDefault(k)
                        },
                        label = { Text(kindName(k)) },
                        leadingIcon = if (kind == k) { { Icon(Glyphs.account(k), contentDescription = null, modifier = Modifier.size(18.dp)) } } else null,
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
                headlineContent = { Text("In the budget") },
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
                modifier = Modifier.fillMaxWidth()) { Text("Add account") }
            Spacer(Modifier.height(Space.xl))
        }
    }
}

