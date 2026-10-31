// Putting the budget in order by hand: arkui's Reorder in sections (where a
// row lands as it is dragged, and what it landed in front of), then the
// budget's own reading of a drop as the domain's change, then that change
// made on the demo's household and read back. Upper-case keys are headers.
package dev.money.android

import dev.arkui.Landing
import dev.arkui.landing
import dev.arkui.reordered
import dev.arkui.sections
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
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

    // * The budget: a drop as the domain's change

    @Test fun aCategoryDropIsAMoveIntoTheGroupItLandedUnder() {
        val keys = keysOf(l.byGroup())
        val (g1, g2) = l.byGroup()
        val moved = g1.second.first()
        val key = categoryKey(moved.categoryId)
        // Dragged down past the second group's header: in front of that group's first category.
        val after = reordered(keys, key, groupKey(g2.first.id), ::isGroupKey)!!
        val drop = dropOf(key, landing(after, key, ::isGroupKey)!!)
        assertEquals(Drop.Category(moved.categoryId, g2.first.id, g2.second.firstOrNull()?.categoryId), drop)
    }

    @Test fun aCategoryAtAGroupsEndIsMovedBeforeNothing() {
        val (g1, _) = l.byGroup()
        val key = categoryKey(g1.second.first().categoryId)
        val last = categoryKey(g1.second.last().categoryId)
        val after = reordered(keysOf(l.byGroup()), key, last, ::isGroupKey)!!
        assertEquals(Drop.Category(g1.second.first().categoryId, g1.first.id, null), dropOf(key, landing(after, key, ::isGroupKey)!!))
    }

    @Test fun aGroupDropIsAMoveInFrontOfTheNextGroup() {
        val groups = l.byGroup().map { it.first }
        val key = groupKey(groups[0].id)
        val after = reordered(keysOf(l.byGroup()), key, groupKey(groups[1].id), ::isGroupKey)!!
        assertEquals(Drop.Group(groups[0].id, groups.getOrNull(2)?.id), dropOf(key, landing(after, key, ::isGroupKey)!!))
    }

    // * The demo's household: the drop made, and read back in the order it was dragged into

    private fun drop(key: String, over: String): List<String> {
        val after = reordered(keysOf(l.byGroup()), key, over, ::isGroupKey)!!
        when (val d = dropOf(key, landing(after, key, ::isGroupKey)!!)) {
            is Drop.Category -> book.moveCategory(d.id, d.group, d.before)
            is Drop.Group -> book.moveGroup(d.id, d.before)
            null -> error("no drop")
        }
        return after
    }

    @Test fun aCategoryDraggedIntoAnotherGroupIsThere() {
        val (g1, g2) = l.byGroup()
        val moved = g1.second.first()
        val after = drop(categoryKey(moved.categoryId), groupKey(g2.first.id))
        assertEquals(after, keysOf(l.byGroup()))
        assertEquals(g2.first.id, l.line(moved.categoryId)!!.groupId)
        assertEquals(emptyList<String>(), book.said)
    }

    @Test fun aCategoryDraggedWithinItsGroupIsThere() {
        val g = l.byGroup().first { it.second.size >= 3 }.second
        val after = drop(categoryKey(g[0].categoryId), categoryKey(g[2].categoryId))
        assertEquals(after, keysOf(l.byGroup()))
    }

    @Test fun aCategoryDraggedUpIntoTheGroupAboveIsItsLast() {
        val (g1, g2) = l.byGroup()
        val moved = g2.second.first()
        val after = drop(categoryKey(moved.categoryId), groupKey(g2.first.id))
        assertEquals(after, keysOf(l.byGroup()))
        assertEquals(moved.categoryId, l.byGroup().first { it.first.id == g1.first.id }.second.last().categoryId)
    }

    @Test fun aGroupDraggedDownAndBackIsThere() {
        val before = keysOf(l.byGroup())
        val groups = l.byGroup().map { it.first }
        val down = drop(groupKey(groups[0].id), groupKey(groups.last().id))
        assertEquals(down, keysOf(l.byGroup()))
        assertNotEquals(before, down)
        assertEquals(groups[0].id, l.byGroup().last().first.id)
        val back = drop(groupKey(groups[0].id), groupKey(groups[1].id))
        assertEquals(before, back)
        assertEquals(before, keysOf(l.byGroup()))
        assertEquals(emptyList<String>(), book.said)
    }
}
