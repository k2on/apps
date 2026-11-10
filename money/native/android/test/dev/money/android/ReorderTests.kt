// arkui's Reorder in sections: where a row lands as it is dragged, and what
// it landed in front of. Upper-case keys are headers. (The budget's own
// moves are Rust's now: money/app/src/budget/tests.rs.)
package dev.money.android

import dev.arkui.Landing
import dev.arkui.landing
import dev.arkui.reordered
import dev.arkui.sections
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReorderTests {
    private val head: (String) -> Boolean = { it[0].isUpperCase() }
    private val order = listOf("A", "a1", "a2", "B", "b1", "C")

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
}
