// Putting the budget in order by hand: arkui's Reorder in sections (where a
// row lands as it is dragged, and what it landed in front of), then the
// budget's own reading of a drop as the domain's change, then that change
// made on the demo's household and read back — the cards' payment
// categories kept among themselves. Upper-case keys are headers.
package dev.money.android

import dev.arkdb.ffi.Id
import dev.arkui.Landing
import dev.arkui.landing
import dev.arkui.reordered
import dev.arkui.sections
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class ReorderTests {
    private val head: (String) -> Boolean = { it[0].isUpperCase() }
    private val order = listOf("A", "a1", "a2", "B", "b1", "C")

    private lateinit var book: Book
    private val l: Ledger
        get() = book.state.value

    @Before fun open() {
        book = demoBook()
    }

    @After fun close() = book.close()

    // * arkui: where a drag puts a row

    @Test fun aFlatListMovesOneRow() {
        val q = listOf("x", "y", "z")
        assertEquals(listOf("y", "z", "x"), reordered(q, "x", "z"))
        assertEquals(listOf("z", "x", "y"), reordered(q, "z", "x"))
        assertNull("nowhere to go", reordered(q, "x", "x"))
        assertNull("not in the list", reordered(q, "w", "x"))
    }

    @Test fun aRowMovesWithinItsSection() {
        assertEquals(listOf("A", "a2", "a1", "B", "b1", "C"), reordered(order, "a1", "a2", head))
        assertEquals(listOf("A", "a2", "a1", "B", "b1", "C"), reordered(order, "a2", "a1", head))
    }

    @Test fun aRowPastAHeaderChangesSection() {
        // Down past B: B's first row. Up past B: A's last.
        assertEquals(listOf("A", "a1", "B", "a2", "b1", "C"), reordered(order, "a2", "B", head))
        assertEquals(listOf("A", "a1", "a2", "b1", "B", "C"), reordered(order, "b1", "B", head))
        // Into a section with no rows, and past several at once.
        assertEquals(listOf("A", "a1", "a2", "B", "C", "b1"), reordered(order, "b1", "C", head))
        assertEquals(listOf("A", "a2", "B", "b1", "C", "a1"), reordered(order, "a1", "C", head))
    }

    @Test fun aRowNeverGoesAboveTheFirstHeader() {
        assertNull(reordered(order, "a1", "A", head))
        assertNull(reordered(order, "b1", "A", head))
    }

    @Test fun aHeaderMovesItsSection() {
        assertEquals(listOf("B", "b1", "A", "a1", "a2", "C"), reordered(order, "A", "B", head))
        assertEquals(listOf("B", "b1", "C", "A", "a1", "a2"), reordered(order, "A", "C", head))
        assertEquals(listOf("C", "A", "a1", "a2", "B", "b1"), reordered(order, "C", "A", head))
        assertEquals(listOf("A", "a1", "a2", "C", "B", "b1"), reordered(order, "C", "B", head))
    }

    @Test fun aHeaderPassesOnlyHeaders() {
        assertNull(reordered(order, "A", "b1", head))
        assertNull(reordered(order, "B", "a2", head))
    }

    @Test fun aLandingIsTheSectionAndWhatComesNext() {
        assertEquals(Landing("A", "a2"), landing(order, "a1", head))
        assertEquals(Landing("A", null), landing(order, "a2", head))
        assertEquals(Landing("B", null), landing(order, "b1", head))
        assertEquals(Landing(null, "B"), landing(order, "A", head))
        assertEquals(Landing(null, null), landing(order, "C", head))
        assertNull(landing(order, "z", head))
    }

    @Test fun sectionsAreEachHeaderAndItsRows() {
        assertEquals(listOf("A" to listOf("a1", "a2"), "B" to listOf("b1"), "C" to emptyList()), sections(order, head))
    }

    @Test fun aRowStaysInASectionOfItsOwn() {
        // B is b1's alone: b1 stays in it, and nothing else goes in.
        val fits: (String, String?) -> Boolean = { r, h -> (r == "b1") == (h == "B") }
        assertNull("a2 down past B", reordered(order, "a2", "B", head, fits))
        assertNull("b1 up past B", reordered(order, "b1", "B", head, fits))
        assertNull("b1 down past C", reordered(order, "b1", "C", head, fits))
        assertNull("a1 past several, into B", reordered(order, "a1", "b1", head, fits))
        // Within its section, and the headers, as before.
        assertEquals(listOf("A", "a2", "a1", "B", "b1", "C"), reordered(order, "a1", "a2", head, fits))
        assertEquals(listOf("B", "b1", "A", "a1", "a2", "C"), reordered(order, "A", "B", head, fits))
    }

    // * The budget: a drop as the domain's change

    // The demo starts with the cards' group (its Chase card's payment category), then the people's.
    private val budget
        get() = l.byGroup()
    private val people
        get() = budget.filter { it.first.id != Book.CARD_GROUP }
    private val fits
        get() = fitsOf(budget)

    private fun dragged(keys: List<String>, key: String, over: String) = reordered(keys, key, over, ::isGroupKey, fits)

    @Test fun theCardsGroupComesFirst() {
        val (cards, rest) = budget.first()
        assertEquals(Book.CARD_GROUP, cards.id)
        assertEquals("Credit Card Payments", cards.name)
        assertTrue(rest.isNotEmpty())
        assertTrue(rest.all { Ledger.isPayment(it) && it.card != null })
        assertTrue(people.flatMap { it.second }.none { Ledger.isPayment(it) })
    }

    @Test fun aCategoryDropIsAMoveIntoTheGroupItLandedUnder() {
        val keys = keysOf(budget)
        val (g1, g2) = people
        val moved = g1.second.first()
        val key = categoryKey(moved.categoryId)
        // Dragged down past the second group's header: in front of that group's first category.
        val after = dragged(keys, key, groupKey(g2.first.id))!!
        val drop = dropOf(key, landing(after, key, ::isGroupKey)!!)
        assertEquals(Drop.Category(moved.categoryId, g2.first.id, g2.second.firstOrNull()?.categoryId), drop)
    }

    @Test fun aCategoryAtAGroupsEndIsMovedBeforeNothing() {
        val (g1, _) = people
        val key = categoryKey(g1.second.first().categoryId)
        val last = categoryKey(g1.second.last().categoryId)
        val after = dragged(keysOf(budget), key, last)!!
        assertEquals(Drop.Category(g1.second.first().categoryId, g1.first.id, null), dropOf(key, landing(after, key, ::isGroupKey)!!))
    }

    @Test fun aGroupDropIsAMoveInFrontOfTheNextGroup() {
        val groups = budget.map { it.first }
        val key = groupKey(groups[0].id)
        val after = dragged(keysOf(budget), key, groupKey(groups[1].id))!!
        assertEquals(Drop.Group(groups[0].id, groups.getOrNull(2)?.id), dropOf(key, landing(after, key, ::isGroupKey)!!))
    }

    @Test fun aPaymentCategoryStaysWithTheCards() {
        val keys = keysOf(budget)
        val pay = categoryKey(budget.first().second.last().categoryId)
        val (g1, _) = people
        // Down past the next group's header, or past its categories: it stops at the cards' edge.
        assertNull(dragged(keys, pay, groupKey(g1.first.id)))
        assertNull(dragged(keys, pay, categoryKey(g1.second.first().categoryId)))
        // Nor does a category of the people's go up among the cards.
        assertNull(dragged(keys, categoryKey(g1.second.first().categoryId), groupKey(g1.first.id)))
        // And were it let go there all the same, the domain says no in its own words.
        book.moveCategory(Id(pay.substringAfter('/')), g1.first.id, null)
        assertTrue(book.said.last(), book.said.last().contains("stays with the cards"))
        assertEquals(Book.CARD_GROUP, l.line(Id(pay.substringAfter('/')))!!.groupId)
    }

    // * The demo's household: the drop made, and read back in the order it was dragged into

    private fun drop(key: String, over: String): List<String> {
        val after = dragged(keysOf(budget), key, over)!!
        when (val d = dropOf(key, landing(after, key, ::isGroupKey)!!)) {
            is Drop.Category -> book.moveCategory(d.id, d.group, d.before)
            is Drop.Group -> book.moveGroup(d.id, d.before)
            null -> error("no drop")
        }
        return after
    }

    @Test fun aCategoryDraggedIntoAnotherGroupIsThere() {
        val (g1, g2) = people
        val moved = g1.second.first()
        val after = drop(categoryKey(moved.categoryId), groupKey(g2.first.id))
        assertEquals(after, keysOf(budget))
        assertEquals(g2.first.id, l.line(moved.categoryId)!!.groupId)
        assertEquals(emptyList<String>(), book.said)
    }

    @Test fun aCategoryDraggedWithinItsGroupIsThere() {
        val g = people.first { it.second.size >= 3 }.second
        val after = drop(categoryKey(g[0].categoryId), categoryKey(g[2].categoryId))
        assertEquals(after, keysOf(budget))
        assertEquals(emptyList<String>(), book.said)
    }

    @Test fun aCategoryDraggedUpIntoTheGroupAboveIsItsLast() {
        val (g1, g2) = people
        val moved = g2.second.first()
        val after = drop(categoryKey(moved.categoryId), groupKey(g2.first.id))
        assertEquals(after, keysOf(budget))
        assertEquals(moved.categoryId, budget.first { it.first.id == g1.first.id }.second.last().categoryId)
    }

    @Test fun theCardsPaymentsAreDraggedAmongThemselves() {
        // A second card, so the cards' group has two to put in order.
        book.openAccount("Visa", "credit", onBudget = true, balance = 0)
        val pays = budget.first().second
        assertEquals(2, pays.size)
        val after = drop(categoryKey(pays[0].categoryId), categoryKey(pays[1].categoryId))
        assertEquals(after, keysOf(budget))
        assertEquals(listOf(pays[1].categoryId, pays[0].categoryId), budget.first().second.map { it.categoryId })
        assertEquals(emptyList<String>(), book.said)
    }

    @Test fun aGroupDraggedDownAndBackIsThere() {
        val before = keysOf(budget)
        val groups = budget.map { it.first }
        val down = drop(groupKey(groups[0].id), groupKey(groups.last().id))
        assertEquals(down, keysOf(budget))
        assertNotEquals(before, down)
        assertEquals(groups[0].id, budget.last().first.id)
        val back = drop(groupKey(groups[0].id), groupKey(groups[1].id))
        assertEquals(before, back)
        assertEquals(before, keysOf(budget))
        assertEquals(emptyList<String>(), book.said)
    }
}
