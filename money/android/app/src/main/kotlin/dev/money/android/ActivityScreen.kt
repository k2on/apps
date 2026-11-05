// Transactions, laid out as the iPhone app lays them: a search over payee,
// category, memo, account and amount; then what needs a look first —
// imported and not yet approved, or for no category — and every account's
// transactions after, as two segments of one page. What waits for review is
// one card under "N to Review" and Approve All, its rows the register's:
// one swipes to approve (or says Approve to TalkBack), a tap on one with no
// category asks for one, and a tap on any other opens its form. Nothing to
// review is said as the phone's own apps say an empty inbox: a glyph, a
// sentence, and nothing else.
package dev.money.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.rememberModalBottomSheetState
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.arkui.Space
import dev.arkui.reflowing
import uniffi.money_domain.TxnLine

/**
 * The rows `needle` finds: its payee, category, memo or account holds it, or its amount written
 * plainly does ("9.05", "1,850"), as the iPhone's search finds them. Every row for an empty needle.
 */
fun search(rows: List<TxnLine>, needle: String): List<TxnLine> {
    val n = needle.trim().lowercase()
    if (n.isEmpty()) return rows
    return rows.filter { r ->
        r.payee.lowercase().contains(n) || r.category.lowercase().contains(n) || r.memo.lowercase().contains(n) ||
            r.account.lowercase().contains(n) || plain(r.amount).contains(n)
    }
}

/**
 * Whether a transaction wants a category before it can be approved: the domain says so in its
 * category's place (a split with a line that has none says it too), as the iPhone reads it.
 */
fun needsCategory(t: TxnLine): Boolean = t.category == "Needs a category"

/** Cents as a person types them: `-1,850.00`. */
fun plain(cents: Long): String {
    val sign = if (cents < 0) "-" else ""
    val c = kotlin.math.abs(cents)
    return sign + "%,d.%02d".format(java.util.Locale.US, c / 100, c % 100)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActivityScreen(ledger: Ledger) {
    val shell = LocalShell.current
    val book = shell.book
    var all by rememberSaveable { mutableStateOf(false) }
    var needle by rememberSaveable { mutableStateOf("") }
    var filing by remember { mutableStateOf<List<Id>?>(null) }
    val list = rememberLazyListState()
    val expanded by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }
    DisposableEffect(Unit) {
        book.holdRegister(null)
        onDispose { book.releaseRegister(null) }
    }
    val waiting = ledger.inbox
    val review = search(waiting, needle)

    Page(
        title = "Transactions",
        fab = {
            ExtendedFloatingActionButton(
                onClick = { shell.onAdd(null) },
                expanded = expanded,
                icon = { Icon(Icons.Default.Add, contentDescription = null) },
                text = { Text("Transaction") })
        },
    ) { pad, _ ->
        LazyColumn(state = list, contentPadding = PaddingValues(bottom = pad.calculateBottomPadding() + 96.dp)) {
            item(key = "search") { SearchField(needle, onType = { needle = it }) }
            item(key = "segments") {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s)) {
                    SegmentedButton(selected = !all, onClick = { all = false }, shape = SegmentedButtonDefaults.itemShape(0, 2), icon = {}) {
                        Text(if (waiting.isEmpty()) "To Review" else "To Review (${waiting.size})")
                    }
                    SegmentedButton(selected = all, onClick = { all = true }, shape = SegmentedButtonDefaults.itemShape(1, 2), icon = {}) {
                        Text("All")
                    }
                }
            }
            if (all) {
                days(search(ledger.register(null), needle), ledger.today, showAccount = true)
            } else if (review.isEmpty()) {
                item(key = "caught-up") { CaughtUp(needle) }
            } else {
                val ready = review.filter { !it.approved && !needsCategory(it) }.map { it.id }
                item(key = "h-review") {
                    Row(
                        Modifier.fillMaxWidth().padding(start = Space.page + Space.l, end = Space.page, top = Space.l),
                        verticalAlignment = Alignment.CenterVertically) {
                            GroupHeadingText("${review.size} to Review", Modifier.weight(1f))
                            if (ready.isNotEmpty()) {
                                TextButton(onClick = { book.approve(ready) }) {
                                    Text("Approve All", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                                }
                            }
                        }
                }
                itemsIndexed(review, key = { _, t -> "r" + t.id.hex }) { i, t ->
                    val needs = needsCategory(t)
                    TxnRow(
                        t,
                        showAccount = true,
                        modifier = reflowing().padding(horizontal = Space.page).clip(inCard(i, review.size)),
                        divided = i > 0,
                        onClick = if (needs) { { filing = listOf(t.id) } } else null,
                    )
                }
                item(key = "f-review") {
                    Text(
                        "Swipe right to approve. Tap one to choose its category or change it.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = Space.page + Space.l, end = Space.page, top = Space.s))
                }
            }
        }
    }

    filing?.let { ids ->
        val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
        ModalBottomSheet(onDismissRequest = { filing = null }, sheetState = sheet) {
            Column(Modifier.navigationBarsPadding()) {
                CategoryPicker(
                    ledger,
                    title = "File It Under",
                    none = true,
                    onPick = { p ->
                        if (p is Pick.Category) book.categorize(ids, p.id)
                        filing = null
                    },
                )
            }
        }
    }
}

/** A heading's words alone, quiet: a section's name where something else shares its row. */
@Composable
private fun GroupHeadingText(text: String, modifier: Modifier = Modifier) {
    Text(text, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier)
}

/** The search field over the list, a filled capsule as the iPhone's is. */
@Composable
private fun SearchField(needle: String, onType: (String) -> Unit) {
    TextField(
        value = needle,
        onValueChange = onType,
        placeholder = { Text("Payee, category, memo or amount", maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
        trailingIcon = {
            if (needle.isNotEmpty()) {
                IconButton(onClick = { onType("") }) { Icon(Icons.Default.Clear, contentDescription = "Clear the search") }
            }
        },
        singleLine = true,
        shape = CircleShape,
        colors =
            TextFieldDefaults.colors(
                focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
            ),
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.xs),
    )
}

@Composable
private fun CaughtUp(needle: String) {
    Column(
        Modifier.fillMaxWidth().padding(top = 96.dp, start = Space.xxl, end = Space.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center) {
            Icon(if (needle.isEmpty()) Glyphs.Done else Icons.Default.Search, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(56.dp))
            Spacer(Modifier.height(Space.l))
            Text(if (needle.isEmpty()) "All Caught Up" else "No Results", style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
            Text(
                if (needle.isEmpty()) "Everything from the bank has been looked at, and everything has a category. Pull down to ask the bank for anything new."
                else "Nothing waiting matches “$needle”.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center)
        }
}
