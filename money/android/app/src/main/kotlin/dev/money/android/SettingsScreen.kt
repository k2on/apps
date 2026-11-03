// Settings, reached from the face in the Accounts tab's top bar as Google's
// apps reach theirs (and from the budget's menu): the banks, and who is
// signed in where.
//
// **Banks.** The household's connections through Plaid (Banks.kt), each a
// card: the bank, how its last import went — in the error's colour when its
// login ran out or the import failed, with Reconnect then a button of its own
// — and Reconnect and Disconnect (asked first) behind its menu. Under it, each
// of the bank's accounts and what it feeds: tapped, it is pointed at another
// account here; its switch follows it or not. One the bank has that waits for
// a choice offers the three: a new account of its kind, an account already
// here, or nothing. **Connect a bank** asks the server for Plaid's page, which
// opens in a Custom Tab the moment the server answers (MoneyApp opens the
// book's pages); the ask's own card then follows it — the server finishes the
// connection by itself — until it is done, failed or expired. Pulling down
// asks the banks for anything new.
//
// **Account.** Who is signed in, on which server, whether this phone is in
// step with it, and the way out.
package dev.money.android

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
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
import dev.arkdb.ffi.Id
import dev.arkui.LocalAmounts
import dev.arkui.SectionHeader
import dev.arkui.Space
import dev.arkui.Tone
import dev.arkui.toneColors
import kotlinx.coroutines.delay
import uniffi.money_domain.AskLine
import uniffi.money_domain.AskState
import uniffi.money_domain.BankLine
import uniffi.money_domain.LinkedLine

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(ledger: Ledger, onBack: () -> Unit) {
    val book = LocalShell.current.book
    var linking by remember { mutableStateOf<LinkedLine?>(null) }
    var disconnecting by remember { mutableStateOf<BankLine?>(null) }
    // The ask put away, by its id's hex.
    var dismissed by rememberSaveable { mutableStateOf<String?>(null) }
    val now = rememberNow(ticking = ledger.asks.firstOrNull()?.let(::isOpen) == true)
    val ask = askShown(ledger.asks, dismissed?.let { Id(it) }, now)

    Page(title = "Settings", onBack = onBack) { pad, _ ->
        LazyColumn(contentPadding = PaddingValues(bottom = pad.calculateBottomPadding() + Space.xl)) {
            item(key = "h-banks") { SectionHeader("Banks") }
            item(key = "about") {
                Text(
                    "Banks and cards connected through Plaid. Their logins are kept on the server, never on this phone; " +
                        "each of their accounts feeds an account here, or nothing.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = Space.page, vertical = Space.xs))
            }
            if (ask != null) {
                item(key = "ask") {
                    AskCard(ask, now, onOpen = { book.browse(ask.url) }, onDismiss = { dismissed = ask.id.hex })
                }
            }
            for (b in ledger.banks) {
                item(key = "b" + b.name) {
                    BankCard(
                        b,
                        ledger.linked.filter { it.bank == b.name },
                        now,
                        onReconnect = { book.reconnect(b.name) },
                        onDisconnect = { disconnecting = b },
                        onLink = { linking = it },
                    )
                }
            }
            if (ledger.banks.isEmpty()) {
                item(key = "none") {
                    Text(
                        "No banks yet. Connect one: Plaid's page opens here, and the login stays on the server.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = Space.page, vertical = Space.l))
                }
            }
            item(key = "connect") {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.m),
                    horizontalArrangement = Arrangement.spacedBy(Space.s)) {
                        Button(onClick = { book.connect() }, enabled = ask == null || !isOpen(ask)) {
                            Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.padding(end = Space.s))
                            Text("Connect a bank")
                        }
                        if (ledger.banks.isNotEmpty()) {
                            OutlinedButton(onClick = { book.sync() }, enabled = !ledger.syncing) { Text("Sync now") }
                        }
                    }
            }
            item(key = "h-account") { SectionHeader("Account") }
            item(key = "account") { AccountCard(ledger) }
        }
    }

    linking?.let { l ->
        val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
        ModalBottomSheet(onDismissRequest = { linking = null }, sheetState = sheet) {
            Column(Modifier.navigationBarsPadding()) {
                val free = linkTargets(ledger.accounts)
                AccountPicker(
                    free,
                    title = "What does ${l.name} feed?",
                    onPick = { a ->
                        book.link(l.plaidAccountId, a.id)
                        linking = null
                    })
                Text(
                    if (free.isEmpty()) "Every open account here is fed by a bank already."
                    else "An account kept by hand takes what the bank has from the day it was connected.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = Space.xl, vertical = Space.l))
            }
        }
    }
    disconnecting?.let { b ->
        AlertDialog(
            onDismissRequest = { disconnecting = null },
            title = { Text("Disconnect ${bankName(b)}?") },
            text = { Text("Plaid forgets the login and the bank stops importing. Its accounts here, and what is in them, stay.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        book.disconnect(b.name)
                        disconnecting = null
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Disconnect") }
            },
            dismissButton = { TextButton(onClick = { disconnecting = null }) { Text("Cancel") } },
        )
    }
}

/** The time, kept current while `ticking` (an ask is open): what "3 min ago" and a slow server are told by. */
@Composable
private fun rememberNow(ticking: Boolean): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(ticking) {
        now = System.currentTimeMillis()
        while (ticking) {
            delay(5_000)
            now = System.currentTimeMillis()
        }
    }
    return now
}

/** The ask on show: what has come of it, Plaid's page again while it waits, and Dismiss once it is over. */
@Composable
private fun AskCard(a: AskLine, now: Long, onOpen: () -> Unit, onDismiss: () -> Unit) {
    val tone =
        when (a.state) {
            AskState.FAILED, AskState.EXPIRED -> Tone.Negative
            AskState.DONE -> Tone.Positive
            else -> Tone.Neutral
        }
    val (container, on) = toneColors(tone)
    Card(
        colors = CardDefaults.cardColors(containerColor = container, contentColor = on),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s),
    ) {
        Row(Modifier.padding(Space.l), verticalAlignment = Alignment.CenterVertically) {
            if (isOpen(a)) {
                CircularProgressIndicator(Modifier.size(24.dp), color = on, strokeWidth = 3.dp)
            } else {
                Icon(if (a.state == AskState.DONE) Glyphs.Done else Glyphs.Link, contentDescription = null)
            }
            Spacer(Modifier.width(Space.l))
            Column(Modifier.weight(1f)) {
                Text(askSays(a, now), style = MaterialTheme.typography.bodyMedium)
                if (a.state == AskState.WAITING && a.url.isNotEmpty()) {
                    TextButton(onClick = onOpen, colors = ButtonDefaults.textButtonColors(contentColor = on)) {
                        Text("Open Plaid's page")
                    }
                }
            }
            if (!isOpen(a)) {
                TextButton(onClick = onDismiss, colors = ButtonDefaults.textButtonColors(contentColor = on)) { Text("Dismiss") }
            }
        }
    }
}

/** A connection: the bank and how it is doing, its menu, and each of its accounts. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun BankCard(
    b: BankLine,
    linked: List<LinkedLine>,
    now: Long,
    onReconnect: () -> Unit,
    onDisconnect: () -> Unit,
    onLink: (LinkedLine) -> Unit,
) {
    val c = MaterialTheme.colorScheme
    val status = bankStatus(b, now)
    var menu by remember { mutableStateOf(false) }
    Card(
        colors = CardDefaults.cardColors(containerColor = c.surfaceContainerLow),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s),
    ) {
        ListItem(
            headlineContent = { Text(bankName(b), style = MaterialTheme.typography.titleMedium) },
            supportingContent = { Text(status.words, color = if (status.alarm) c.error else c.onSurfaceVariant) },
            leadingContent = {
                Box(Modifier.size(40.dp).clip(CircleShape).background(c.secondaryContainer), contentAlignment = Alignment.Center) {
                    Icon(Glyphs.Accounts, contentDescription = null, tint = c.onSecondaryContainer)
                }
            },
            trailingContent = {
                Box {
                    IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "More about ${bankName(b)}") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text("Reconnect") }, onClick = { menu = false; onReconnect() })
                        DropdownMenuItem(text = { Text("Disconnect…") }, onClick = { menu = false; onDisconnect() })
                    }
                }
            },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        )
        if (b.needsLogin) {
            FilledTonalButton(onClick = onReconnect, modifier = Modifier.padding(start = 72.dp, bottom = Space.s)) { Text("Reconnect") }
        }
        for (l in linked) LinkedRow(l, onLink = { onLink(l) })
        Spacer(Modifier.height(Space.s))
    }
}

/** One of a bank's accounts: what it feeds, or the three choices while it waits for one. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun LinkedRow(l: LinkedLine, onLink: () -> Unit) {
    val book = LocalShell.current.book
    val a = LocalAmounts.current
    val name = if (l.mask.isNotEmpty()) "${l.name} ••${l.mask}" else l.name
    val what = l.subtype.ifEmpty { kindName(l.kind) }.replaceFirstChar { it.uppercase() }
    val feeds =
        when {
            l.waiting -> "Waiting for a choice: what does it feed?"
            l.ignored -> "Not followed"
            l.account.isNotEmpty() -> "Feeds ${l.account}"
            else -> "Feeds no account yet: tap to choose one"
        }
    ListItem(
        headlineContent = { Text(name) },
        overlineContent = { Text(listOfNotNull(what, l.balance?.let { "the bank says ${a.format(it)}" }).joinToString(" · ")) },
        supportingContent = {
            Text(feeds, color = if (l.waiting) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant)
        },
        leadingContent = { Icon(Glyphs.account(l.kind), contentDescription = null) },
        trailingContent =
            if (l.waiting) null
            else {
                {
                    Switch(
                        checked = !l.ignored,
                        onCheckedChange = { book.ignore(l.plaidAccountId, !it) },
                        modifier = Modifier.semantics { contentDescription = "Follow ${l.name}" })
                }
            },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = if (l.waiting || l.ignored) Modifier else Modifier.clickable(onClickLabel = "Choose the account it feeds", onClick = onLink),
    )
    if (l.waiting) {
        FlowRow(
            Modifier.fillMaxWidth().padding(start = 56.dp, end = Space.l, bottom = Space.s),
            horizontalArrangement = Arrangement.spacedBy(Space.s)) {
                FilledTonalButton(onClick = { book.openLinked(l.plaidAccountId) }) {
                    Text("New ${kindName(l.opensAs).lowercase()} account")
                }
                OutlinedButton(onClick = onLink) { Text("Existing account") }
                TextButton(onClick = { book.ignore(l.plaidAccountId, true) }) { Text("Ignore") }
            }
    }
}

/** Who is signed in, where, whether this phone is in step with it, and the way out. */
@Composable
private fun AccountCard(ledger: Ledger) {
    val shell = LocalShell.current
    val c = MaterialTheme.colorScheme
    Card(
        colors = CardDefaults.cardColors(containerColor = c.surfaceContainerLow),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s),
    ) {
        Row(Modifier.padding(Space.l), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(48.dp).clip(CircleShape).background(c.primaryContainer), contentAlignment = Alignment.Center) {
                Text(ledger.who.take(1).uppercase().ifEmpty { "?" }, style = MaterialTheme.typography.titleLarge, color = c.onPrimaryContainer)
            }
            Spacer(Modifier.width(Space.l))
            Column(Modifier.weight(1f)) {
                Text(ledger.who.ifEmpty { "Signed in" }, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
                Text(
                    if (ledger.demo) "The demo household, on this phone only" else ledger.server.substringAfter("://"),
                    style = MaterialTheme.typography.bodyMedium,
                    color = c.onSurfaceVariant)
                if (!ledger.demo) {
                    val l = ledger.link
                    Text(
                        when {
                            l.linked && l.pending == 0 -> "Up to date"
                            l.linked -> "Sending ${l.pending} change${if (l.pending == 1) "" else "s"}"
                            else -> "Not connected (${l.word}); changes wait on this phone"
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = c.onSurfaceVariant)
                }
            }
        }
        OutlinedButton(
            onClick = shell.onSignOut,
            colors = ButtonDefaults.outlinedButtonColors(contentColor = c.error),
            modifier = Modifier.fillMaxWidth().padding(start = Space.l, end = Space.l, bottom = Space.l),
        ) { Text(if (ledger.demo) "Leave the demo" else "Sign out") }
    }
}
