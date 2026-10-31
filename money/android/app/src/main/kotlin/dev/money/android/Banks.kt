// Connecting banks, as the screens read it (`money/domain/src/banks.rs`). A
// bank's login is the server's alone — Plaid's token is a secret, and never
// on a phone — so connecting one is an ask the server's importer answers:
// `banks.connect` (or `banks.reconnect` for a login that ran out), answered
// with the address of Plaid's own page, which the app opens in a Custom Tab
// as soon as it arrives. The person logs in to their bank there; the server
// finishes the connection on its own, asking Plaid until the login is done,
// so all the app does after is watch the ask: done, failed with a sentence,
// or expired. `banks.disconnect` is an ask too.
//
// What follows an ask is worked out here from the asks alone, so it can be
// said without a server: [follow] tells the book what to do next, and
// [askShown] and [askSays] what Settings says of it.
package dev.money.android

import dev.arkdb.ffi.Id
import uniffi.money_domain.AccountLine
import uniffi.money_domain.AskLine
import uniffi.money_domain.AskState
import uniffi.money_domain.BankLine

/** What to do about an ask this phone made, once the asks have moved. */
sealed interface Follow {
    /** The server has not answered with a page yet. */
    data object Wait : Follow

    /** Its page arrived: open it. */
    data class Open(val url: String) : Follow

    /** Answered otherwise — done, failed, expired: nothing to open. */
    data object Over : Follow
}

/**
 * Where an ask authored here stands: the newest of `asks` (newest first, as `banks.asks` reads them),
 * once it is not `after` — the newest there was before it was made.
 */
fun follow(asks: List<AskLine>, after: Id?): Follow {
    val a = asks.firstOrNull() ?: return Follow.Wait
    if (a.id == after) return Follow.Wait
    return when (a.state) {
        AskState.ASKED -> Follow.Wait
        AskState.WAITING -> if (a.url.isNotEmpty()) Follow.Open(a.url) else Follow.Wait
        AskState.DONE, AskState.FAILED, AskState.EXPIRED -> Follow.Over
    }
}

/** How long an answered ask is still told of. */
const val ASK_SHOWN_MS: Long = 15 * 60 * 1000

/** How long an ask goes unanswered before Settings wonders whether the server can. */
const val ASK_SLOW_MS: Long = 30_000

/** Whether an ask is still open: unanswered, or its page waiting for a login. */
fun isOpen(a: AskLine): Boolean = a.state == AskState.ASKED || a.state == AskState.WAITING

/**
 * The ask Settings tells of: the newest of this person's while it is open or lately answered,
 * unless it was put away (`dismissed`).
 */
fun askShown(asks: List<AskLine>, dismissed: Id?, now: Long): AskLine? {
    val a = asks.firstOrNull() ?: return null
    if (a.id == dismissed) return null
    return a.takeIf { isOpen(it) || now - it.answeredMs < ASK_SHOWN_MS }
}

/** A bank as a person names it: the institution, else the name the server keeps it under. */
fun bankName(b: BankLine): String = b.institution.ifEmpty { b.name }

/** What an ask is about, as a person names it. */
private fun askName(a: AskLine): String = a.institution.ifEmpty { a.bank }.ifEmpty { "the bank" }

/** What has come of an ask, in a sentence. */
fun askSays(a: AskLine, now: Long): String {
    val name = askName(a)
    return when (a.state) {
        AskState.ASKED ->
            when {
                now - a.askedMs > ASK_SLOW_MS -> "The server hasn't answered yet. Does it have Plaid's keys?"
                a.what == "disconnect" -> "Asking the server to disconnect $name…"
                else -> "Asking the server for Plaid's page…"
            }
        AskState.WAITING -> "Log in to your bank on Plaid's page. This follows along, and finishes on its own."
        AskState.DONE ->
            when (a.what) {
                "connect" -> "Connected $name. Choose below what each of its accounts feeds."
                "reconnect" -> "Logged in to $name again."
                else -> "Disconnected $name. Its accounts here stay, kept by hand from now on."
            }
        AskState.FAILED -> "Couldn't ${a.what} $name: ${a.message}"
        AskState.EXPIRED -> a.message.ifEmpty { "Plaid's page expired before the login was finished." }
    }
}

/** How a connection is doing, in words; `alarm` when it wants somebody. */
data class BankStatus(val words: String, val alarm: Boolean = false)

fun bankStatus(b: BankLine, now: Long): BankStatus =
    when {
        b.needsLogin -> BankStatus("The bank wants its login again", alarm = true)
        b.error.isNotEmpty() -> BankStatus("The last import failed ${Dates.ago(b.failedMs, now)}: ${b.error}", alarm = true)
        b.syncedMs == 0L -> BankStatus("Waiting for its first import")
        else ->
            BankStatus(
                "Synced ${Dates.ago(b.syncedMs, now)} · ${b.added} new, ${b.updated} changed" +
                    if (b.locked > 0) ", ${b.locked} left alone (reconciled)" else "")
    }

/** The accounts a bank's account could feed: open, and fed by no bank already. */
fun linkTargets(accounts: List<AccountLine>): List<AccountLine> = accounts.filter { !it.closed && it.bank.isEmpty() }
