// Connecting banks from Settings (Banks.kt): what follows an ask, what
// Settings says of it and of a bank, worked out from the asks alone; then
// the book's asks over the demo's household as a server's would be — the
// book not a demo, the demo's peer answering as the importer (it holds the
// importer's role): Plaid's page opened once when the answer arrives, a
// login run out and logged in again, a bank disconnected, a new bank's
// account waiting for its choice.
package dev.money.android

import dev.arkdb.ffi.Id
import dev.money.android.gen.Answer
import dev.money.android.gen.BankAccount
import dev.money.android.gen.BankFailed
import dev.money.android.gen.Import
import dev.money.ffi.moneyDemo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.money_domain.AskLine
import uniffi.money_domain.AskState
import uniffi.money_domain.BankLine

class BanksTests {
    private fun id(n: Int) = Id("%032x".format(n))

    private fun ask(
        n: Int,
        state: AskState,
        what: String = "connect",
        url: String = "",
        message: String = "",
        institution: String = "",
        askedMs: Long = 1_000_000,
        answeredMs: Long = 0,
    ) = AskLine(id(n), what, "", institution, state, url, message, 0L, askedMs, answeredMs)

    private fun bank(needsLogin: Boolean = false, error: String = "", syncedMs: Long = 0, failedMs: Long = 0) =
        BankLine("ally", "Ally Bank", syncedMs, 3, 1, 0, 0, 2, if (needsLogin) "ITEM_LOGIN_REQUIRED" else "", error, failedMs, needsLogin, 0)

    // * What follows an ask

    @Test fun anAskIsFollowedUntilItsPageArrives() {
        assertEquals(Follow.Wait, follow(emptyList(), null))
        // The newest there was before it: not this one's answer.
        assertEquals(Follow.Wait, follow(listOf(ask(1, AskState.WAITING, url = "https://old")), id(1)))
        assertEquals(Follow.Wait, follow(listOf(ask(2, AskState.ASKED), ask(1, AskState.DONE)), id(1)))
        assertEquals(Follow.Wait, follow(listOf(ask(2, AskState.WAITING)), id(1)))
        assertEquals(Follow.Open("https://plaid/page"), follow(listOf(ask(2, AskState.WAITING, url = "https://plaid/page")), id(1)))
        assertEquals(Follow.Open("https://plaid/page"), follow(listOf(ask(2, AskState.WAITING, url = "https://plaid/page")), null))
        for (s in listOf(AskState.DONE, AskState.FAILED, AskState.EXPIRED)) assertEquals(Follow.Over, follow(listOf(ask(2, s)), id(1)))
    }

    @Test fun anAskIsShownWhileOpenAndAWhileAfter() {
        val now = 10_000_000L
        assertNotNull(askShown(listOf(ask(1, AskState.ASKED)), null, now))
        assertNotNull(askShown(listOf(ask(1, AskState.WAITING)), null, now))
        assertNotNull(askShown(listOf(ask(1, AskState.DONE, answeredMs = now - 60_000)), null, now))
        assertNull("answered long ago", askShown(listOf(ask(1, AskState.DONE, answeredMs = now - ASK_SHOWN_MS - 1)), null, now))
        assertNull("put away", askShown(listOf(ask(1, AskState.FAILED, answeredMs = now)), id(1), now))
        assertNull(askShown(emptyList(), null, now))
    }

    @Test fun anAskIsSaidInASentence() {
        val asked = 1_000_000L
        assertEquals("Asking the server for Plaid's page…", askSays(ask(1, AskState.ASKED), asked + 1_000))
        assertEquals("The server hasn't answered yet. Does it have Plaid's keys?", askSays(ask(1, AskState.ASKED), asked + ASK_SLOW_MS + 1))
        assertEquals("Asking the server to disconnect Ally Bank…", askSays(ask(1, AskState.ASKED, what = "disconnect", institution = "Ally Bank"), asked))
        assertTrue(askSays(ask(1, AskState.WAITING), asked).startsWith("Log in to your bank on Plaid's page"))
        assertEquals("Connected Ally Bank. Choose below what each of its accounts feeds.", askSays(ask(1, AskState.DONE, institution = "Ally Bank"), asked))
        assertEquals("Logged in to the bank again.", askSays(ask(1, AskState.DONE, what = "reconnect"), asked))
        assertEquals("Couldn't connect the bank: Plaid said no", askSays(ask(1, AskState.FAILED, message = "Plaid said no"), asked))
        assertEquals("Plaid's page expired before the login was finished.", askSays(ask(1, AskState.EXPIRED), asked))
        assertEquals("Nobody logged in", askSays(ask(1, AskState.EXPIRED, message = "Nobody logged in"), asked))
    }

    @Test fun aBanksStatusSaysWhatItWants() {
        val now = 100 * 60_000L
        assertEquals(BankStatus("The bank wants its login again", alarm = true), bankStatus(bank(needsLogin = true), now))
        assertEquals(BankStatus("The last import failed 5 min ago: no route", alarm = true), bankStatus(bank(error = "no route", failedMs = now - 5 * 60_000), now))
        assertEquals(BankStatus("Waiting for its first import"), bankStatus(bank(), now))
        assertEquals(BankStatus("Synced just now · 3 new, 1 changed"), bankStatus(bank(syncedMs = now), now))
        assertEquals("Ally Bank", bankName(bank()))
        assertEquals("ally", bankName(bank().copy(institution = "")))
    }

    // * The book's asks, answered as the server's importer would

    /** The demo's household as a server's replica: asks go out rather than being refused. */
    private fun served(): Book = Book.inline(moneyDemo(TODAY), TODAY, demo = false)

    private fun answer(book: Book, a: AskLine, state: AskState, url: String = "", bank: String = "", institution: String = "", message: String = "") =
        book.change("answer") { it.answer(Answer(a.id, state, bank, institution, url, message, 0)) }

    @Test fun theDemoConnectsNothing() {
        val book = demoBook()
        try {
            book.connect()
            assertEquals("The demo has no server to connect a bank through", book.said.last())
            assertTrue(book.state.value.asks.isEmpty())
        } finally {
            book.close()
        }
    }

    @Test fun connectingOpensPlaidsPageOnceWhenTheServerAnswers() {
        val book = served()
        try {
            book.connect()
            val asked = book.state.value.asks.first()
            assertEquals(AskState.ASKED, asked.state)
            assertEquals("connect", asked.what)
            assertTrue(book.opened.isEmpty())
            answer(book, asked, AskState.WAITING, url = "https://hosted.plaid.com/link/abc")
            assertEquals(listOf("https://hosted.plaid.com/link/abc"), book.opened)
            // The server finishes on its own: done, the bank is known at once, and nothing opens again.
            answer(book, asked, AskState.DONE, bank = "ally", institution = "Ally Bank")
            assertEquals(1, book.opened.size)
            val done = book.state.value.asks.first()
            assertEquals(AskState.DONE, done.state)
            assertTrue(book.state.value.banks.any { it.name == "ally" && bankName(it) == "Ally Bank" })
            assertEquals("Connected Ally Bank. Choose below what each of its accounts feeds.", askSays(done, 0))
            // Asked again, the first's answer is not the second's.
            book.connect()
            assertEquals(1, book.opened.size)
            val again = book.state.value.asks.first()
            assertTrue(again.id != asked.id)
            answer(book, again, AskState.FAILED, message = "Plaid has no keys")
            assertEquals(1, book.opened.size)
        } finally {
            book.close()
        }
    }

    @Test fun aLoginRunOutIsReconnected() {
        val book = served()
        try {
            val chase = book.state.value.banks.first()
            book.change("fail") { it.failed(BankFailed(chase.name, "", "ITEM_LOGIN_REQUIRED", "the login ran out")) }
            val failed = book.state.value.banks.first { it.name == chase.name }
            assertTrue(failed.needsLogin)
            assertTrue(bankStatus(failed, 0).alarm)
            assertTrue(book.state.value.banksWanting >= 1)
            book.reconnect(chase.name)
            val asked = book.state.value.asks.first()
            assertEquals("reconnect", asked.what)
            assertEquals(chase.name, asked.bank)
            answer(book, asked, AskState.WAITING, url = "https://hosted.plaid.com/link/update")
            assertEquals(listOf("https://hosted.plaid.com/link/update"), book.opened)
            answer(book, asked, AskState.DONE)
            assertFalse(book.state.value.banks.first { it.name == chase.name }.needsLogin)
        } finally {
            book.close()
        }
    }

    @Test fun aBankDisconnectedLeavesItsAccounts() {
        val book = served()
        try {
            val chase = book.state.value.banks.first()
            val fed = book.state.value.linked.filter { it.bank == chase.name }.mapNotNull { it.accountId }
            assertTrue(fed.isNotEmpty())
            book.disconnect(chase.name)
            assertEquals("Asked the server to disconnect the bank", book.said.last())
            val asked = book.state.value.asks.first()
            assertEquals("disconnect", asked.what)
            answer(book, asked, AskState.DONE)
            assertTrue(book.state.value.banks.none { it.name == chase.name })
            assertTrue(book.state.value.linked.none { it.bank == chase.name })
            for (a in fed) assertNotNull(book.state.value.account(a))
            // Nothing to open for a disconnection.
            assertTrue(book.opened.isEmpty())
        } finally {
            book.close()
        }
    }

    @Test fun aNewBanksAccountsWaitForAChoice() {
        val book = served()
        try {
            val card = BankAccount("ally-card", "Ally Card", "1234", "credit", "credit card", -12_000, "", false)
            val checking = BankAccount("ally-chk", "Ally Checking", "9876", "depository", "checking", 50_000, "", false)
            val spare = BankAccount("ally-sav", "Ally Savings", "5555", "depository", "savings", 1_000, "", false)
            book.change("import") { it.import(Import("ally", "Ally Bank", "ally-1", listOf(card, checking, spare), emptyList(), emptyList(), true, false)) }
            val l = book.state.value
            val waiting = l.linked.filter { it.bank == "ally" }
            assertEquals(3, waiting.size)
            assertTrue(waiting.all { it.waiting })
            assertEquals(3L, l.banks.first { it.name == "ally" }.waiting)
            // A new account of its kind: a card is a credit account.
            book.openLinked("ally-card")
            val opened = book.state.value.linked.first { it.plaidAccountId == "ally-card" }
            assertFalse(opened.waiting)
            assertEquals("credit", book.state.value.account(opened.accountId!!)!!.kind)
            // An account kept by hand: one fed by no bank.
            val savings = linkTargets(book.state.value.accounts).first { it.kind == "savings" }
            assertTrue(linkTargets(book.state.value.accounts).none { it.bank.isNotEmpty() || it.closed })
            book.link("ally-chk", savings.id)
            assertEquals(savings.id, book.state.value.linked.first { it.plaidAccountId == "ally-chk" }.accountId)
            // Nothing.
            book.ignore("ally-sav", true)
            val ignored = book.state.value.linked.first { it.plaidAccountId == "ally-sav" }
            assertTrue(ignored.ignored)
            assertFalse(ignored.waiting)
            assertEquals(0L, book.state.value.banks.first { it.name == "ally" }.waiting)
        } finally {
            book.close()
        }
    }
}
