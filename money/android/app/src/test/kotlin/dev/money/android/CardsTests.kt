// Credit cards as the screens read them (`money/domain/src/cards.rs`), over
// the demo's household: a card on the budget has a payment category among
// the cards; its debt is covered in one press; spending on it more than a
// category had is debt (amber), where cash overspent is next month's (red);
// a transaction already there is made a transfer; and no transaction is
// offered a payment category.
package dev.money.android

import dev.arkui.Amounts
import dev.arkui.Tone
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.money_domain.AccountLine
import uniffi.money_domain.BudgetLine

class CardsTests {
    private lateinit var book: Book
    private val l: Ledger
        get() = book.state.value
    private val a = Amounts(Locale.US)

    @Before fun open() {
        book = demoBook()
    }

    @After fun close() = book.close()

    /** A new card on the budget owing `owed`, and its payment category. */
    private fun visa(owed: Long = 0): Pair<AccountLine, BudgetLine> {
        book.openAccount("Visa", "credit", onBudget = true, balance = -owed)
        val card = l.accounts.first { it.name == "Visa" }
        return card to l.lines.first { it.card == card.id }
    }

    /** A category of the people's with money in it. */
    private fun funded(): BudgetLine = l.lines.first { !Ledger.isPayment(it) && it.available > 1_000 }

    @Test fun aCardOnTheBudgetHasAPaymentCategoryAmongTheCards() {
        val (card, pay) = visa()
        assertEquals(Book.CARD_GROUP, pay.groupId)
        assertEquals("Visa", pay.name)
        assertTrue(Ledger.isPayment(pay))
        assertTrue(l.cards.any { it.card == card.id })
        assertEquals("Nothing owed on the card", storyOf(pay, a))
        assertEquals(Tone.Neutral, toneOf(pay))
        // A card only tracked has none.
        book.openAccount("Store card", "credit", onBudget = false, balance = -1_000)
        val store = l.accounts.first { it.name == "Store card" }
        assertTrue(l.lines.none { it.card == store.id && !it.hidden })
    }

    @Test fun aCardsDebtIsCoveredInOnePress() {
        val ready = l.summary!!.readyToAssign
        val (_, pay) = visa(owed = 50_000)
        assertEquals(50_000L, pay.debt)
        assertEquals(50_000L, pay.underfunded)
        assertEquals(Tone.Caution, toneOf(pay))
        assertEquals("$500.00 short of the $500.00 owed", storyOf(pay, a))
        assertTrue(l.cardsShort >= 50_000)
        // The debt it came with is nobody's money: Ready to Assign did not move.
        assertEquals(ready, l.summary!!.readyToAssign)

        book.coverCard(pay.categoryId)
        val now = l.line(pay.categoryId)!!
        assertEquals(0L, now.underfunded)
        assertEquals(50_000L, now.assigned)
        assertEquals(ready - 50_000, l.summary!!.readyToAssign)
        assertEquals("Covered: $500.00 owed", storyOf(now, a))
        assertEquals(Tone.Positive, toneOf(now))
        // Nothing left to cover: pressed again, nothing moves.
        book.coverCard(pay.categoryId)
        assertEquals(50_000L, l.line(pay.categoryId)!!.assigned)
        assertEquals(emptyList<String>(), book.said)
    }

    @Test fun spendingMoreOnACardThanACategoryHadIsDebt() {
        val (card, pay) = visa()
        val c = funded()
        val ready = l.summary!!.readyToAssign
        book.save(TxnForm(account = card.id, date = TODAY, payee = "Market", category = c.categoryId, amount = -(c.available + 3_000)))
        val spent = l.line(c.categoryId)!!
        assertEquals(-3_000L, spent.available)
        assertEquals(3_000L, spent.creditOverspent)
        assertTrue(creditOverspent(spent))
        // Amber, as the card's debt, not red as next month's.
        assertEquals(Tone.Caution, toneOf(spent))
        assertEquals("$30.00 on a card, not covered", storyOf(spent, a))
        // What the category had went to the card's payment; what it lacked is not covered there.
        val p = l.line(pay.categoryId)!!
        assertEquals(c.available + 3_000, p.debt)
        assertEquals(3_000L, p.underfunded)
        assertEquals(Tone.Caution, toneOf(p))
        assertEquals(ready, l.summary!!.readyToAssign)
    }

    @Test fun cashOverspentIsNextMonthsAndRed() {
        val checking = l.accounts.first { it.onBudget && !it.closed && it.kind == "checking" }
        val c = funded()
        book.save(TxnForm(account = checking.id, date = TODAY, payee = "Market", category = c.categoryId, amount = -(c.available + 3_000)))
        val spent = l.line(c.categoryId)!!
        assertEquals(0L, spent.creditOverspent)
        assertFalse(creditOverspent(spent))
        assertEquals(Tone.Negative, toneOf(spent))
        assertEquals("Overspent by $30.00", storyOf(spent, a))
    }

    @Test fun aTransactionAlreadyThereIsMadeATransfer() {
        val (from, to) = l.accounts.filter { it.onBudget && !it.closed && it.kind != "credit" }.take(2)
        book.holdRegister(from.id)
        val c = l.lines.first { !Ledger.isPayment(it) }
        book.save(newForm(l, from.id).copy(payee = "Moved", category = c.categoryId, amount = -5_000))
        val t = l.register(from.id).first { it.payee == "Moved" }
        assertFalse(t.transfer)
        val toBefore = l.account(to.id)!!.balance
        book.save(formOf(t, l).copy(category = null, transferTo = to.id))
        val made = l.register(from.id).first { it.id == t.id }
        assertTrue(made.transfer)
        assertEquals(toBefore + 5_000, l.account(to.id)!!.balance)
        // Corrected again, the form knows it for the transfer it is.
        assertEquals(to.id, formOf(made, l).transferTo)
        assertEquals(emptyList<String>(), book.said)
    }

    @Test fun aTransactionIsNotOfferedAPaymentCategory() {
        visa()
        val forTxns = choosable(l, cards = false)
        assertTrue(forTxns.isNotEmpty())
        assertTrue(forTxns.none { (g, ls) -> g.id == Book.CARD_GROUP || ls.any { Ledger.isPayment(it) } })
        // Moving money, the cards' payments are there to move it to.
        val forMoving = choosable(l, cards = true)
        assertEquals(Book.CARD_GROUP, forMoving.first().first.id)
        assertTrue(forMoving.first().second.all { Ledger.isPayment(it) })
        assertEquals(l.byGroup().size, forMoving.size)
    }

    @Test fun aPaymentCategoryIsTheCards() {
        val (_, pay) = visa()
        book.renameCategory(pay.categoryId, "Mine")
        assertTrue(book.said.last(), book.said.last().contains("the card's"))
        assertEquals("Visa", l.line(pay.categoryId)!!.name)
        // Hidden, as any category may be.
        book.hideCategory(pay.categoryId, true)
        assertTrue(l.line(pay.categoryId)!!.hidden)
    }
}
