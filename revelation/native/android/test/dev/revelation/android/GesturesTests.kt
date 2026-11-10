package dev.revelation.android

import dev.revelation.android.Gestures.Phase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class GesturesTests {
    @Test
    fun aDragAndAFling() {
        val g = Gestures()
        assertFalse(g.down())
        assertTrue(g.scroll())
        assertEquals(Phase.DRAGGING, g.phase)
        assertTrue(g.fling())
        assertEquals(Phase.FLINGING, g.phase)
        assertTrue(g.moving)
        g.settled()
        assertEquals(Phase.IDLE, g.phase)
    }

    @Test
    fun aDragLetGoWithoutAFling() {
        val g = Gestures()
        g.down()
        g.scroll()
        g.up()
        assertEquals(Phase.IDLE, g.phase)
        assertFalse(g.fling())
    }

    @Test
    fun aTouchStopsAFlingWhereItIs() {
        val g = Gestures()
        g.down(); g.scroll(); g.fling()
        assertTrue(g.down())
        assertEquals(Phase.IDLE, g.phase)
        assertTrue(g.scroll())
    }

    @Test
    fun aTouchStopsAnAnimation() {
        val g = Gestures()
        assertTrue(g.animate())
        assertEquals(Phase.ANIMATING, g.phase)
        assertTrue(g.down())
        assertEquals(Phase.IDLE, g.phase)
    }

    @Test
    fun aPinchAndTheFingerLeftAfterIt() {
        val g = Gestures()
        g.down()
        g.scroll()
        g.scaleBegin()
        assertEquals(Phase.PINCHING, g.phase)
        assertFalse(g.scroll())
        g.scaleEnd(snap = false)
        assertEquals(Phase.IDLE, g.phase)
        // The one finger still down does not drag, nor fling, until a new touch.
        assertFalse(g.scroll())
        assertFalse(g.fling())
        g.up()
        g.down()
        assertTrue(g.scroll())
    }

    @Test
    fun aPinchThatSnaps() {
        val g = Gestures()
        g.down()
        g.scaleBegin()
        g.scaleEnd(snap = true)
        assertEquals(Phase.ANIMATING, g.phase)
        g.settled()
        assertEquals(Phase.IDLE, g.phase)
    }

    @Test
    fun noAnimationWhileAFingerDrives() {
        val g = Gestures()
        g.down(); g.scroll()
        assertFalse(g.animate())
        assertEquals(Phase.DRAGGING, g.phase)
        g.scaleBegin()
        assertFalse(g.animate())
        // A fling can be taken over by a fly-to.
        val h = Gestures()
        h.down(); h.scroll(); h.fling()
        assertTrue(h.animate())
        assertEquals(Phase.ANIMATING, h.phase)
    }

    @Test
    fun settledOnlyEndsMotion() {
        val g = Gestures()
        g.down(); g.scroll()
        g.settled()
        assertEquals(Phase.DRAGGING, g.phase)
    }
}
