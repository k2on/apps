// A category assigned by rule, as the phone holds it (`money/domain/src/
// rules.rs`): the phone sets no rule — the desktop does — so the test sets
// one through the bindings as the desktop would, on the demo's household,
// and reads what the screens would: the month marked as its rule's, its
// amount the rule's, and every way the phone assigns turned away from it.
package dev.money.android

import dev.money.android.gen.Auto
import dev.money.android.gen.SetRule
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import uniffi.money_domain.BudgetLine

class RulesTests {
    private lateinit var book: Book
    private val l: Ledger
        get() = book.state.value

    @Before fun open() {
        book = demoBook()
    }

    @After fun close() = book.close()

    // * What decides it, alone

    private fun some(): BudgetLine = l.lines.first { !Ledger.isPayment(it) && it.categoryId != Book.READY_TO_ASSIGN }

    @Test fun aMonthItsRuleAssignsIsNotAssignedHere() {
        val c = some()
        assertTrue("assigned by hand", assignable(c.copy(ruled = false, rule = "")))
        assertFalse("its rule's", assignable(c.copy(ruled = true, rule = "10% of income matching ACME in any account")))
        assertFalse("Ready to Assign is nobody's to assign", assignable(c.copy(categoryId = Book.READY_TO_ASSIGN)))
    }

    @Test fun theSheetSaysTheRule() {
        val c = some().copy(ruled = true, rule = "10% of income matching ACME PAYROLL in Checking, Savings")
        assertEquals("Assigned by rule: 10% of income matching ACME PAYROLL in Checking, Savings", rulesLine(c))
        assertEquals("Assigned by rule", rulesLine(c.copy(rule = "")))
        assertEquals("Assigned by rule", storyOf(c.copy(target = 0, activity = 0, available = 0, carried = 0, assigned = 0), dev.arkui.Amounts()))
    }

    // * The demo, with a rule set as the desktop sets one

    /** A paycheck of this month's, and the category given a tenth of every one like it from this month on. */
    private fun ruled(): Pair<BudgetLine, Long> {
        book.holdRegister(null)
        val onBudget = l.accounts.filter { it.onBudget }.map { it.id }.toSet()
        val pay = l.register(null).first { it.categoryId == Book.READY_TO_ASSIGN && it.amount > 0 && !it.transfer && it.date.startsWith(l.month) }
        val text = pay.payee.lowercase()
        val matched =
            l.register(null)
                .filter { t ->
                    t.amount > 0 && !t.transfer && t.date.startsWith(l.month) && t.accountId in onBudget &&
                        (t.payee.lowercase().contains(text) || t.importedPayee.lowercase().contains(text))
                }
                .sumOf { it.amount }
        val c = l.lines.first { !Ledger.isPayment(it) && it.categoryId != Book.READY_TO_ASSIGN && it.available > 0 }
        book.change("set the rule") { it.setRule(SetRule(c.categoryId, l.month, pay.payee, emptyList(), 1, 10)) }
        assertTrue(book.said.toString(), book.said.none { it.startsWith("Couldn't set the rule") })
        return l.line(c.categoryId)!! to (matched + 5) / 10
    }

    @Test fun aRuledMonthIsMarkedAndAssignedByItsRule() {
        val (c, tenth) = ruled()
        assertTrue(c.ruled)
        assertTrue(c.rule, c.rule.startsWith("10% of income matching "))
        assertEquals("a tenth of the month's paychecks", tenth, c.assigned)
        assertFalse(assignable(c))
        assertTrue(rulesLine(c).startsWith("Assigned by rule: 10% of income matching "))
    }

    @Test fun theMonthBeforeTheRuleIsAssignedByHand() {
        val (c, _) = ruled()
        book.stepMonth(-1)
        val before = l.line(c.categoryId)!!
        assertFalse(before.ruled)
        assertTrue(assignable(before))
    }

    @Test fun moneyIsNotMovedToOrFromIt() {
        val (c, _) = ruled()
        val moving = choosable(l, cards = true, moving = true).flatMap { it.second }.map { it.categoryId }
        assertFalse("not a move's source or target", c.categoryId in moving)
        assertTrue("others are", moving.isNotEmpty())
        val filing = choosable(l, cards = false).flatMap { it.second }.map { it.categoryId }
        assertTrue("a transaction is still filed under it", c.categoryId in filing)
    }

    @Test fun assigningOrMovingItIsRefusedInTheDomainsWords() {
        val (c, tenth) = ruled()
        book.assign(c.categoryId, tenth + 1_000)
        assertTrue(book.said.last(), book.said.last().contains("follows its rule"))
        val other = l.lines.first { assignable(it) && !Ledger.isPayment(it) && it.available > 100 }
        book.moveMoney(other.categoryId, c.categoryId, 100)
        assertTrue(book.said.last(), book.said.last().contains("follows its rule"))
        book.moveMoney(c.categoryId, null, 100)
        assertTrue(book.said.last(), book.said.last().contains("follows its rule"))
        assertEquals(tenth, l.line(c.categoryId)!!.assigned)
    }

    @Test fun autoAssignPassesItBy() {
        val (c, tenth) = ruled()
        book.autoAssign(Auto.Reset)
        assertEquals("kept the rule's", tenth, l.line(c.categoryId)!!.assigned)
        assertTrue("the rest reset", l.lines.filter { assignable(it) && !Ledger.isPayment(it) }.all { it.assigned == 0L })
    }

    @Test fun aRuleShortOfItsTargetIsNotCountedAsFundable() {
        val (c, tenth) = ruled()
        book.setTarget(c.categoryId, tenth + 10_000)
        val short = l.summary!!.underfunded
        assertEquals(short - 1, fundable(l))
    }
}
