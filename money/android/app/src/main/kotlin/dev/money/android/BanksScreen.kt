// The banks the server's importer follows through Plaid, and each of their
// accounts: which account here it feeds, or that it is not followed. An
// account is pointed at one of the household's (`banks.link`) from a
// chooser, and followed or not with a switch (`banks.ignore`). Adding a
// bank is the server's: its tokens are secrets, and never on a phone.
// Pulling down asks the importer to ask the banks now.
package dev.money.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import dev.arkui.AmountText
import dev.arkui.SectionHeader
import dev.arkui.Space
import uniffi.money_domain.LinkedLine

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BanksScreen(ledger: Ledger, onBack: () -> Unit) {
    val book = LocalShell.current.book
    var linking by remember { mutableStateOf<LinkedLine?>(null) }
    Page(title = "Bank connections", onBack = onBack) { pad, _ ->
        LazyColumn(contentPadding = PaddingValues(bottom = pad.calculateBottomPadding() + Space.xl)) {
            if (ledger.banks.isEmpty()) {
                item(key = "none") {
                    Text(
                        "No bank is connected yet. A bank is added on the server, which keeps its keys; " +
                            "its accounts appear here after its first import.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(Space.xl))
                }
            }
            for (b in ledger.banks) {
                item(key = "b" + b.name) { Column {
                    SectionHeader(b.institution.ifEmpty { b.name })
                    Text(
                        "Synced ${Dates.ago(b.syncedMs)} · last time ${b.added} new, ${b.updated} changed" +
                            (if (b.locked > 0) ", ${b.locked} left alone (reconciled)" else ""),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = Space.page))
                } }
                items(ledger.linked.filter { it.bank == b.name }, key = { "l" + it.plaidAccountId }) { l ->
                    ListItem(
                        headlineContent = { Text(if (l.mask.isNotEmpty()) "${l.name} ••${l.mask}" else l.name) },
                        supportingContent = {
                            Text(
                                when {
                                    l.ignored -> "Not followed"
                                    l.account.isNotEmpty() -> "Feeds ${l.account}"
                                    else -> "Feeds no account yet: tap to choose one"
                                })
                        },
                        leadingContent = { Icon(Glyphs.account(l.kind), contentDescription = null) },
                        trailingContent = {
                            Switch(
                                checked = !l.ignored,
                                onCheckedChange = { book.ignore(l.plaidAccountId, !it) },
                                modifier = Modifier.semantics { contentDescription = "Follow ${l.name}" })
                        },
                        overlineContent = if (l.balance != null) { { AmountText(l.balance!!) } } else null,
                        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                        modifier = Modifier.clickable(onClickLabel = "Choose the account it feeds") { linking = l },
                    )
                }
            }
        }
    }
    linking?.let { l ->
        val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
        ModalBottomSheet(onDismissRequest = { linking = null }, sheetState = sheet) {
            Column(Modifier.navigationBarsPadding()) {
                AccountPicker(
                    ledger.accounts,
                    title = "${l.name} feeds",
                    skip = l.accountId,
                    onPick = { a ->
                        book.link(l.plaidAccountId, a.id)
                        linking = null
                    })
            }
        }
    }
}
