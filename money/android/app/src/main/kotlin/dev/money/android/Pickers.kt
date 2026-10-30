// Choosing a category, or an account: a searchable list inside a sheet,
// grouped as the budget is, each category with what it has in its pill so
// the choice is made knowing where the money is. What "Ready to Assign"
// means depends on who asks — money that comes in goes to its category, but
// money moved back to it is moved to no category at all — so the caller
// says what choosing it answers.
package dev.money.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.arkui.AmountText
import dev.arkui.Pill
import dev.arkui.Space
import dev.arkui.Tone
import uniffi.money_domain.AccountLine

/** What a picker answered. */
sealed class Pick {
    /** A category; none is no category, or Ready to Assign where moving money. */
    data class Category(val id: Id?) : Pick()

    /** The other account of a transfer. */
    data class Transfer(val account: Id) : Pick()
}

/**
 * The month's categories to choose from: Ready to Assign first (answering `ready`), then each
 * group's, `skip` left out; "No category" at the end when `none`; and the accounts a transfer could
 * go to, other than `transferFrom`, when it is given.
 */
@Composable
fun CategoryPicker(
    ledger: Ledger,
    title: String,
    onPick: (Pick) -> Unit,
    ready: Pick? = Pick.Category(Book.READY_TO_ASSIGN),
    skip: Id? = null,
    none: Boolean = false,
    transferFrom: Id? = null,
    onlyWithMoney: Boolean = false,
) {
    var needle by remember { mutableStateOf("") }
    val q = needle.trim().lowercase()
    fun shown(name: String) = q.isEmpty() || name.lowercase().contains(q)
    Column {
        Text(
            title,
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = Space.xl).semantics { heading() })
        OutlinedTextField(
            value = needle,
            onValueChange = { needle = it },
            placeholder = { Text("Search") },
            leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(horizontal = Space.xl, vertical = Space.m),
        )
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = 520.dp)) {
            if (ready != null && shown("Ready to Assign")) {
                item { Label("Inflow") }
                item {
                    Option("Ready to Assign", ledger.summary?.readyToAssign) { onPick(ready) }
                }
            }
            for ((g, lines) in ledger.byGroup()) {
                val ls = lines.filter { it.categoryId != skip && shown(it.name) && (!onlyWithMoney || it.available > 0) }
                if (ls.isEmpty()) continue
                item(key = "g" + g.id.hex) { Label(g.name) }
                items(ls, key = { it.categoryId.hex }) { l ->
                    Option(l.name, l.available) { onPick(Pick.Category(l.categoryId)) }
                }
            }
            if (none && shown("No category")) {
                item { Option("No category", null) { onPick(Pick.Category(null)) } }
            }
            if (transferFrom != null) {
                val to = ledger.accounts.filter { !it.closed && it.id != transferFrom && shown(it.name) }
                if (to.isNotEmpty()) {
                    item { Label("Transfer to") }
                    items(to, key = { "t" + it.id.hex }) { a ->
                        ListItem(
                            headlineContent = { Text(a.name) },
                            leadingContent = { Icon(Glyphs.Transfer, contentDescription = null) },
                            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                            modifier = Modifier.clickable { onPick(Pick.Transfer(a.id)) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Label(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = Space.xl, top = Space.l, bottom = Space.xs).semantics { heading() })
}

@Composable
private fun Option(name: String, available: Long?, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(name) },
        trailingContent =
            if (available != null) {
                {
                    val tone = if (available < 0) Tone.Negative else if (available > 0) Tone.Positive else Tone.Neutral
                    Pill(tone) { AmountText(available, style = MaterialTheme.typography.labelLarge) }
                }
            } else null,
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickable(onClick = onClick),
    )
}

/** The open accounts to choose from, budget ones first. */
@Composable
fun AccountPicker(accounts: List<AccountLine>, title: String, onPick: (AccountLine) -> Unit, skip: Id? = null) {
    Column {
        Text(
            title,
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.s).semantics { heading() })
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = 520.dp)) {
            items(accounts.filter { !it.closed && it.id != skip }.sortedWith(compareBy({ !it.onBudget }, { it.pos })), key = { it.id.hex }) { a ->
                ListItem(
                    headlineContent = { Text(a.name) },
                    supportingContent = { Text(kindName(a.kind)) },
                    leadingContent = { Icon(Glyphs.account(a.kind), contentDescription = null) },
                    trailingContent = { AmountText(a.balance, style = MaterialTheme.typography.labelLarge) },
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    modifier = Modifier.clickable { onPick(a) },
                )
            }
        }
    }
}

/** An account's kind, as a person says it. */
fun kindName(kind: String): String =
    when (kind) {
        "checking" -> "Checking"
        "savings" -> "Savings"
        "cash" -> "Cash"
        "credit" -> "Credit card"
        "loan" -> "Loan"
        "investment" -> "Investment"
        else -> kind.replaceFirstChar { it.uppercase() }
    }
