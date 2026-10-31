// The model the screens draw from, over the real kernel: the demo's
// household (`moneyDemo`, the demo window's to the cent, seeded up to a
// fixed day) through the library built for this machine (`money-android-
// lib`'s `host/`, loaded by JNA as the phone loads it). Each test drives the
// book as a screen does — a press is a method — and reads what a screen
// would read next: the Ledger it published. The book is inline, so a change
// has been spliced in by the time its method returns.
package dev.money.android

import dev.arkdb.ffi.Id
import dev.money.android.gen.Auto
import dev.money.ffi.moneyDemo
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.money_domain.Cleared

const val TODAY = "2026-10-26"

/** The demo's household on a fresh peer, inline. */
fun demoBook(): Book = Book.inline(moneyDemo(TODAY), TODAY)

class BookTests {
    private lateinit var book: Book
    private val l: Ledger
        get() = book.state.value

    @Before fun open() {
        book = demoBook()
    }

    @After fun close() = book.close()

    /** A category with money in it this month, by name. */
    private fun line(name: String) = l.lines.first { it.name == name }

    @Test fun theDemoOpensOnThisMonthsBudget() {
        assertTrue(l.ready)
        assertEquals("2026-10", l.month)
        assertEquals("2026-10", l.summary?.month)
        assertTrue("every account of the seed", l.accounts.size >= 5)
        assertTrue("groups and their categories", l.byGroup().all { (_, ls) -> ls.isNotEmpty() })
        assertTrue("the card's last few days wait for a look", l.inbox.isNotEmpty())
        assertTrue(l.demo)
        assertEquals("Alex", l.who)
    }

    @Test fun readyToAssignIsTheDomainsOwnCategory() {
        book.holdRegister(null)
        val income = l.register(null).filter { it.category == "Ready to Assign" }
        assertTrue("the seed's paychecks", income.isNotEmpty())
        assertTrue(income.all { it.categoryId == Book.READY_TO_ASSIGN })
    }

    @Test fun assigningTakesFromReadyToAssign() {
        val before = l.summary!!.readyToAssign
        val c = l.lines.first()
        book.assign(c.categoryId, c.assigned + 2_500)
        assertEquals(c.assigned + 2_500, l.line(c.categoryId)!!.assigned)
        assertEquals(before - 2_500, l.summary!!.readyToAssign)
    }

    @Test fun moneyMovesBetweenCategoriesAndBack() {
        val (a, b) = l.lines.filter { it.available > 1_000 }.take(2)
        book.moveMoney(a.categoryId, b.categoryId, 1_000)
        assertEquals(a.available - 1_000, l.line(a.categoryId)!!.available)
        assertEquals(b.available + 1_000, l.line(b.categoryId)!!.available)
        val ready = l.summary!!.readyToAssign
        book.moveMoney(b.categoryId, null, 500)
        assertEquals(ready + 500, l.summary!!.readyToAssign)
    }

    @Test fun overspendingIsCoveredFromAnotherCategory() {
        val c = l.lines.first { it.available >= 0 }
        val checking = l.accounts.first { it.onBudget && !it.closed }
        book.save(TxnForm(account = checking.id, date = TODAY, payee = "Overspent", category = c.categoryId, amount = -(c.available + 4_200)))
        assertEquals(-4_200L, l.line(c.categoryId)!!.available)
        assertTrue(l.summary!!.overspent >= 1)
        val from = l.lines.first { it.categoryId != c.categoryId && it.available >= 4_200 }
        book.cover(c.categoryId, from.categoryId)
        assertEquals(0L, l.line(c.categoryId)!!.available)
        assertEquals(from.available - 4_200, l.line(from.categoryId)!!.available)
    }

    @Test fun aTargetAndAutoAssignFundIt() {
        // Not a card's payment category, the budget's first: what it lacks is the card's debt, not a target.
        val c = l.lines.first { !Ledger.isPayment(it) && it.available >= 0 }
        book.setTarget(c.categoryId, c.assigned + 10_000)
        assertEquals(toneOf(l.line(c.categoryId)!!), dev.arkui.Tone.Caution)
        book.autoAssign(Auto.Underfunded, listOf(c.categoryId))
        assertEquals(c.assigned + 10_000, l.line(c.categoryId)!!.assigned)
    }

    @Test fun aRefusalIsSaidInTheDomainsWords() {
        book.assign(Book.READY_TO_ASSIGN, 100)
        assertTrue(book.said.last(), book.said.last().startsWith("Couldn't assign it: "))
        assertTrue(book.said.last().contains("Ready to Assign"))
    }

    @Test fun approvingEmptiesTheInbox() {
        val waiting = l.inbox.filter { !it.approved && it.categoryId != null }
        assertTrue(waiting.isNotEmpty())
        book.approve(waiting.map { it.id })
        assertTrue(l.inbox.none { w -> waiting.any { it.id == w.id } })
    }

    @Test fun aTransactionIsFiledFromTheInbox() {
        val t = l.inbox.first()
        val c = l.lines.first()
        book.categorize(listOf(t.id), c.categoryId)
        book.approve(listOf(t.id))
        assertNull(l.inbox.firstOrNull { it.id == t.id })
    }

    @Test fun theDemosBankSyncsTwiceThenHasNothingNew() {
        val before = l.inbox.size
        book.sync()
        assertTrue(book.said.last(), book.said.last().startsWith("Chase: 2 new from the bank"))
        assertTrue(l.inbox.size > before)
        assertFalse(l.syncing)
        book.sync()
        book.sync()
        assertEquals("Chase: nothing new at the bank", book.said.last())
    }

    @Test fun aTransactionIsAddedEditedClearedAndDeleted() {
        val acc = l.accounts.first { it.onBudget && !it.closed }
        book.holdRegister(acc.id)
        val c = l.lines.first()
        val form = newForm(l, acc.id).copy(payee = "Corner Shop", category = c.categoryId, amount = -1_234)
        book.save(form)
        val t = l.register(acc.id).first { it.payee == "Corner Shop" }
        assertEquals(-1_234L, t.amount)
        assertEquals(Cleared.UNCLEARED, t.cleared)
        assertEquals(acc.balance - 1_234, l.account(acc.id)!!.balance)

        book.save(formOf(t, l).copy(amount = -2_000, memo = "and milk"))
        val edited = l.register(acc.id).first { it.id == t.id }
        assertEquals(-2_000L, edited.amount)
        assertEquals("and milk", edited.memo)

        book.setCleared(t.id, true)
        assertEquals(Cleared.CLEARED, l.register(acc.id).first { it.id == t.id }.cleared)

        book.delete(listOf(t.id))
        assertNull(l.register(acc.id).firstOrNull { it.id == t.id })
        book.releaseRegister(acc.id)
        assertFalse(l.registers.containsKey(Ledger.key(acc.id)))
    }

    @Test fun aTransferMovesMoneyBetweenAccounts() {
        val (from, to) = l.accounts.filter { it.onBudget && !it.closed }.take(2)
        book.save(TxnForm(account = from.id, date = TODAY, transferTo = to.id, amount = -5_000))
        assertEquals(from.balance - 5_000, l.account(from.id)!!.balance)
        assertEquals(to.balance + 5_000, l.account(to.id)!!.balance)
    }

    @Test fun reconcilingLocksWhatCleared() {
        val acc = l.accounts.first { it.onBudget && it.cleared != 0L }
        book.holdRegister(acc.id)
        book.reconcile(acc.id, acc.cleared)
        val rows = l.register(acc.id)
        assertTrue(rows.filter { it.cleared != Cleared.UNCLEARED }.all { it.cleared == Cleared.RECONCILED })
        // A bank that says otherwise: the difference is an adjustment, and the balance agrees.
        book.reconcile(acc.id, acc.cleared + 777)
        assertEquals(acc.cleared + 777, l.account(acc.id)!!.cleared)
    }

    @Test fun anAccountIsOpenedAfterItsNameIsChecked() {
        assertNotNull(book.check("accounts.open_account", """{"name":"","kind":"checking","on_budget":true,"balance":0,"date":"2026-10-26"}"""))
        assertNull(book.check("accounts.open_account", """{"name":"Credit Union","kind":"savings","on_budget":true,"balance":0,"date":"2026-10-26"}"""))
        book.openAccount("Credit Union", "savings", onBudget = true, balance = 50_000)
        val a = l.accounts.first { it.name == "Credit Union" }
        assertEquals(50_000L, a.balance)
        assertTrue(a.onBudget)
    }

    @Test fun theBanksAccountsAreFollowedOrNot() {
        // The demo's Chase is fed through Plaid: its accounts are listed with the one they feed.
        val card = l.linked.firstOrNull()
        assertNotNull("the demo's Chase card is linked", card)
        assertEquals(card!!.account, l.accounts.first { it.id == card.accountId }.name)
        book.ignore(card.plaidAccountId, true)
        assertTrue(l.linked.first { it.plaidAccountId == card.plaidAccountId }.ignored)
        book.ignore(card.plaidAccountId, false)
        assertFalse(l.linked.first { it.plaidAccountId == card.plaidAccountId }.ignored)
    }

    @Test fun theMonthStepsAndComesBack() {
        val october = l.lines.map { it.categoryId to it.assigned }
        book.stepMonth(-1)
        assertEquals("2026-09", l.month)
        assertEquals("2026-09", l.summary?.month)
        book.stepMonth(2)
        assertEquals("2026-11", l.month)
        book.stepMonth(0)
        assertEquals("2026-10", l.month)
        assertEquals(october, l.lines.map { it.categoryId to it.assigned })
    }

    @Test fun categoriesAreAddedHiddenAndDeleted() {
        // The first group is the cards', which takes none: its categories come and go with the cards.
        assertEquals(Book.CARD_GROUP, l.byGroup().first().first.id)
        book.addCategory(Book.CARD_GROUP, "Kayak")
        assertTrue(book.said.last(), book.said.last().startsWith("Couldn't add the category: "))
        assertNull(l.lines.firstOrNull { it.name == "Kayak" })
        val g = l.byGroup().first { it.first.id != Book.CARD_GROUP }.first
        book.addCategory(g.id, "Kayak")
        val c = l.lines.first { it.name == "Kayak" }
        assertEquals(g.id, c.groupId)
        book.hideCategory(c.categoryId, true)
        assertTrue(l.byGroup().flatMap { it.second }.none { it.categoryId == c.categoryId })
        assertTrue(l.byGroup(hidden = true).flatMap { it.second }.any { it.categoryId == c.categoryId })
        book.deleteCategory(c.categoryId)
        assertNull(l.line(c.categoryId))
    }

    @Test fun aClosedBookDoesNothing() {
        book.close()
        book.assign(l.lines.first().categoryId, 1)
        // Nothing thrown, nothing said.
        assertTrue(book.said.isEmpty())
        book = demoBook()
    }

    @Test fun theFormRemembersWhatItCorrects() {
        book.holdRegister(null)
        val t = l.register(null).first { !it.transfer && it.categoryId != null }
        val f = formOf(t, l)
        assertEquals(t.id, f.editing)
        assertEquals(t.amount, f.amount)
        assertEquals(t.categoryId, f.category)
        val n = newForm(l, null)
        assertEquals(TODAY, n.date)
        assertEquals(l.accounts.first { it.onBudget && !it.closed }.id, n.account)
    }

    @Test fun theBanksOwnWordsAreShownWhenTheySayMore() {
        book.holdRegister(null)
        val t = l.register(null).first { !it.transfer }
        val chipotle = t.copy(payee = "Chipotle", imported = true, importedPayee = "CHIPOTLE 1234 SAN FRANCISCO CA ")
        assertEquals("CHIPOTLE 1234 SAN FRANCISCO CA", bankSaid(chipotle))
        assertEquals("CHIPOTLE 1234 SAN FRANCISCO CA", formOf(chipotle, l).bankSaid)
        // Entered by hand, said in nothing, or said as the payee already is: nothing more to show.
        assertEquals("", bankSaid(chipotle.copy(imported = false)))
        assertEquals("", bankSaid(chipotle.copy(importedPayee = "  ")))
        assertEquals("", bankSaid(chipotle.copy(importedPayee = "CHIPOTLE")))
        // What the demo's bank sends is kept as it sent it.
        book.sync()
        for (s in l.inbox.filter { it.imported }) {
            assertTrue(bankSaid(s).isEmpty() || bankSaid(s) == s.importedPayee.trim())
        }
    }

    @Test fun aRegistersKeyIsItsAccount() {
        assertEquals("all", Ledger.key(null))
        assertEquals("ab".repeat(16), Ledger.key(Id("ab".repeat(16))))
    }
}
