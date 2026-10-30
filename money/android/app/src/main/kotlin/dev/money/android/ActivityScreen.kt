// Transactions: what needs a look first — imported and not yet approved,
// or for no category — and every account's transactions after, as two
// segments of one page. A transaction to review swipes to approve (or says
// Approve to TalkBack), its category is a chip that opens the chooser, and
// Approve all clears the lot. Nothing to review is said as the phone's own
// apps say an empty inbox: a glyph, a sentence, and nothing else.
package dev.money.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.arkui.Space
import dev.arkui.SwipeToAct
import uniffi.money_domain.TxnLine

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActivityScreen(ledger: Ledger) {
    val shell = LocalShell.current
    val book = shell.book
    var all by rememberSaveable { mutableStateOf(false) }
    var filing by remember { mutableStateOf<List<Id>?>(null) }
    val list = rememberLazyListState()
    val expanded by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }
    DisposableEffect(Unit) {
        book.holdRegister(null)
        onDispose { book.releaseRegister(null) }
    }
    val review = ledger.inbox

    Page(
        title = "Transactions",
        actions = {
            if (!all && review.isNotEmpty()) {
                TextButton(onClick = { book.approve(review.filter { !it.approved }.map { it.id }) }) { Text("Approve all") }
            }
        },
        fab = {
            ExtendedFloatingActionButton(
                onClick = { shell.onAdd(null) },
                expanded = expanded,
                icon = { Icon(Icons.Default.Add, contentDescription = null) },
                text = { Text("Transaction") })
        },
    ) { pad, _ ->
        LazyColumn(state = list, contentPadding = PaddingValues(bottom = pad.calculateBottomPadding() + 96.dp)) {
            item(key = "segments") {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s)) {
                    SegmentedButton(selected = !all, onClick = { all = false }, shape = SegmentedButtonDefaults.itemShape(0, 2)) {
                        Text(if (review.isEmpty()) "To review" else "To review · ${review.size}")
                    }
                    SegmentedButton(selected = all, onClick = { all = true }, shape = SegmentedButtonDefaults.itemShape(1, 2)) {
                        Text("All")
                    }
                }
            }
            if (all) {
                days(ledger.register(null), ledger.today, showAccount = true)
            } else if (review.isEmpty()) {
                item(key = "caught-up") { CaughtUp() }
            } else {
                items(review, key = { "r" + it.id.hex }) { t ->
                    ReviewRow(t, onFile = { filing = listOf(t.id) }, modifier = Modifier.animateItem())
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
                    title = "File it under",
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

/** A transaction to look at: the row, swiped to approve, and its category as a chip to change. */
@Composable
private fun ReviewRow(t: TxnLine, onFile: () -> Unit, modifier: Modifier = Modifier) {
    val book = LocalShell.current.book
    val needs = t.categoryId == null && !t.transfer
    SwipeToAct(label = "Approve", icon = Icons.Default.Check, onAct = { book.approve(listOf(t.id)) }, enabled = !t.approved, modifier = modifier) {
        Column(Modifier.fillMaxWidth().padding(bottom = Space.s)) {
            TxnRow(t, showAccount = true, swipe = false)
            AssistChip(
                onClick = onFile,
                label = { Text(if (needs) "Choose a category" else t.category) },
                colors =
                    if (needs) AssistChipDefaults.assistChipColors(containerColor = MaterialTheme.colorScheme.errorContainer, labelColor = MaterialTheme.colorScheme.onErrorContainer)
                    else AssistChipDefaults.assistChipColors(),
                modifier = Modifier.padding(start = 72.dp),
            )
        }
    }
}

@Composable
private fun CaughtUp() {
    Column(
        Modifier.fillMaxWidth().padding(top = 96.dp, start = Space.xxl, end = Space.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center) {
            Icon(Glyphs.Done, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(56.dp))
            Spacer(Modifier.height(Space.l))
            Text("You're all caught up", style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
            Text(
                "New transactions from the bank wait here until you've looked at them.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center)
        }
}
