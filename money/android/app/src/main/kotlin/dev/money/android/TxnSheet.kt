// A transaction, entered or corrected, in a full-height modal bottom sheet
// as Google's own apps take one: the amount first and large, money going out
// or coming in as a segmented button, then who was paid (with the payees
// seen before as chips, each bringing the category it went to last time,
// and under it, for one the bank sent, what the bank called it whole), the
// category, the account, the day (Material's date picker), a memo, and
// whether the bank has seen it. A transfer is chosen where a category is:
// its payee is then the other account. A transaction already there is made
// a transfer the same way (`transfer_to`), paired with the matching one in
// the other account when there is one, as a card payment the banks tell of
// from both ends is. The cards' payment categories are not offered: a
// transaction is not for one. Choosing a category or an account is the
// sheet's second page.
package dev.money.android

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.DateRange
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Button
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import dev.arkui.AmountField
import dev.arkui.LocalAmounts
import dev.arkui.Space

private enum class Choosing {
    None,
    Category,
    Account,
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TxnSheet(ledger: Ledger, start: TxnForm, onDismiss: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var f by remember(start) { mutableStateOf(start) }
    var inflow by remember(start) { mutableStateOf(start.amount > 0) }
    var typed by remember(start) { mutableStateOf(a.editable(kotlin.math.abs(start.amount))) }
    var choosing by remember { mutableStateOf(Choosing.None) }
    var picking by remember { mutableStateOf(false) }
    val amount = a.parse(typed)
    val signed = amount?.let { if (inflow) it else -it }
    val account = f.account?.let { ledger.account(it) }
    val transfer = f.transferTo?.let { ledger.account(it) }
    val categoryName =
        when {
            transfer != null -> "Transfer to ${transfer.name}"
            f.category == Book.READY_TO_ASSIGN -> "Ready to Assign"
            f.category != null -> ledger.line(f.category!!)?.name ?: "A category"
            else -> "Choose a category"
        }
    val ok = signed != null && f.account != null && (f.payee.isNotBlank() || transfer != null)

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        AnimatedContent(choosing, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "txn-sheet") { now ->
            when (now) {
                Choosing.Category ->
                    Column(Modifier.navigationBarsPadding()) {
                        TextButton(onClick = { choosing = Choosing.None }, modifier = Modifier.padding(start = Space.l)) { Text("Back") }
                        CategoryPicker(
                            ledger,
                            title = "Category",
                            // One already there is turned into a transfer as one entered is made one.
                            transferFrom = f.account,
                            onPick = { p ->
                                f =
                                    when (p) {
                                        is Pick.Category -> f.copy(category = p.id, transferTo = null)
                                        is Pick.Transfer -> f.copy(category = null, transferTo = p.account)
                                    }
                                if (p is Pick.Category && p.id == Book.READY_TO_ASSIGN) inflow = true
                                choosing = Choosing.None
                            },
                        )
                    }
                Choosing.Account ->
                    Column(Modifier.navigationBarsPadding()) {
                        TextButton(onClick = { choosing = Choosing.None }, modifier = Modifier.padding(start = Space.l)) { Text("Back") }
                        AccountPicker(ledger.accounts, title = "Account", onPick = { acc ->
                            f = f.copy(account = acc.id, transferTo = f.transferTo?.takeIf { it != acc.id })
                            choosing = Choosing.None
                        })
                    }
                Choosing.None ->
                    Column(
                        Modifier.fillMaxWidth()
                            .verticalScroll(rememberScrollState())
                            .padding(horizontal = Space.xl)
                            .navigationBarsPadding()
                            .imePadding()) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, contentDescription = "Close") }
                                Text(
                                    if (f.editing == null) "New transaction" else "Transaction",
                                    style = MaterialTheme.typography.titleLarge,
                                    modifier = Modifier.weight(1f).semantics { heading() })
                                if (f.editing != null) {
                                    IconButton(onClick = { book.delete(listOf(f.editing!!)); onDismiss() }) {
                                        Icon(Icons.Default.Delete, contentDescription = "Delete")
                                    }
                                }
                                Button(
                                    onClick = {
                                        if (signed != null) {
                                            book.save(f.copy(amount = signed))
                                            onDismiss()
                                        }
                                    },
                                    enabled = ok) { Text("Save") }
                            }
                            Spacer(Modifier.height(Space.l))
                            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                                SegmentedButton(
                                    selected = !inflow,
                                    onClick = { inflow = false },
                                    shape = SegmentedButtonDefaults.itemShape(0, 2)) { Text("Money out") }
                                SegmentedButton(
                                    selected = inflow,
                                    onClick = { inflow = true },
                                    shape = SegmentedButtonDefaults.itemShape(1, 2)) { Text("Money in") }
                            }
                            Spacer(Modifier.height(Space.m))
                            AmountField(
                                typed,
                                onType = { typed = it.removePrefix("-") },
                                label = "Amount",
                                error = if (amount == null) "That isn't an amount" else null,
                                imeAction = ImeAction.Next,
                                textStyle = MaterialTheme.typography.headlineMedium,
                                modifier = Modifier.fillMaxWidth(),
                            )
                            if (transfer == null) {
                                OutlinedTextField(
                                    value = f.payee,
                                    onValueChange = { p ->
                                        // A name is one line, however long: wrapped to be read whole, not cut.
                                        val one = p.replace('\n', ' ')
                                        // A payee somebody has paid before brings the category it was for last time.
                                        val known = ledger.payees.firstOrNull { it.name.equals(one, ignoreCase = true) }
                                        f = f.copy(payee = one, category = f.category ?: known?.categoryId)
                                    },
                                    label = { Text(if (inflow) "From" else "Payee") },
                                    maxLines = 4,
                                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                                    modifier = Modifier.fillMaxWidth(),
                                )
                                val q = f.payee.trim()
                                val near =
                                    ledger.payees
                                        .filter { q.isNotEmpty() && it.name.startsWith(q, ignoreCase = true) && !it.name.equals(q, ignoreCase = true) }
                                        .sortedByDescending { it.count }
                                        .take(4)
                                if (near.isNotEmpty()) {
                                    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Space.s)) {
                                        for (p in near) {
                                            SuggestionChip(
                                                onClick = { f = f.copy(payee = p.name, category = f.category ?: p.categoryId) },
                                                label = { Text(p.name) })
                                        }
                                    }
                                }
                            }
                            if (f.bankSaid.isNotEmpty()) {
                                // The bank's own words for it, whole and selectable: what the payee was made from.
                                ListItem(
                                    overlineContent = { Text("Bank description") },
                                    headlineContent = { SelectionContainer { Text(f.bankSaid) } },
                                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                )
                            }
                            Spacer(Modifier.height(Space.s))
                            Field("Category", categoryName, onClick = { choosing = Choosing.Category })
                            Field("Account", account?.name ?: "Choose an account", onClick = {
                                if (f.editing == null) choosing = Choosing.Account
                            })
                            Field("Date", Dates.long(f.date), icon = { Icon(Icons.Default.DateRange, contentDescription = null) }, onClick = { picking = true })
                            OutlinedTextField(
                                value = f.memo,
                                onValueChange = { f = f.copy(memo = it) },
                                label = { Text("Memo") },
                                modifier = Modifier.fillMaxWidth(),
                            )
                            if (f.editing == null) {
                                ListItem(
                                    headlineContent = { Text("Cleared") },
                                    supportingContent = { Text("The bank has it already") },
                                    trailingContent = { Switch(checked = f.cleared, onCheckedChange = { f = f.copy(cleared = it) }) },
                                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                )
                            } else if (f.by.isNotEmpty()) {
                                Text(
                                    "Entered by ${f.by}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(vertical = Space.m))
                            }
                            Spacer(Modifier.height(Space.xl))
                        }
            }
        }
    }

    if (picking) {
        val state = rememberDatePickerState(initialSelectedDateMillis = Dates.millis(f.date))
        DatePickerDialog(
            onDismissRequest = { picking = false },
            confirmButton = {
                TextButton(onClick = {
                    state.selectedDateMillis?.let { f = f.copy(date = Dates.ofMillis(it)) }
                    picking = false
                }) { Text("OK") }
            },
            dismissButton = { TextButton(onClick = { picking = false }) { Text("Cancel") } },
        ) {
            DatePicker(state)
        }
    }
}

/** A form's row that opens a choice: its label over what is chosen. */
@Composable
private fun Field(label: String, value: String, icon: (@Composable () -> Unit)? = null, onClick: () -> Unit) {
    ListItem(
        overlineContent = { Text(label) },
        headlineContent = { Text(value) },
        trailingContent = icon,
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickable(onClickLabel = "Choose", onClick = onClick),
    )
}
