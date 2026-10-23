package dev.revelation.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CameraTests {
    private fun camera() = Camera(800.0, 3_000_000.0)

    @Test
    fun screenAndDocumentRoundTrip() {
        val c = camera()
        c.set(1_234_567.0, 0.37)
        for (s in listOf(0.0, 100.0, 400.0, 799.0)) assertEquals(s, c.screenY(c.docY(s)), 1e-6)
        for (d in listOf(1_234_000.0, 1_234_567.0, 1_235_000.0)) assertEquals(d, c.docY(c.screenY(d)), 1e-6)
        assertEquals(1_234_567.0, c.docY(400.0), 1e-9)
    }

    @Test
    fun theBoundsOfZoom() {
        val c = camera()
        assertEquals(1.0, c.scaleMax, 0.0)
        assertEquals((800.0 - 64) / 3_000_000.0, c.scaleMin, 1e-15)
        c.set(0.0, 5.0)
        assertEquals(1.0, c.scale, 0.0)
        c.set(0.0, 1e-9)
        assertEquals(c.scaleMin, c.scale, 0.0)
    }

    @Test
    fun theDocumentNeverLeavesTheScreen() {
        val c = camera()
        c.set(-1e9, 1.0)
        assertEquals(0.0, c.screenY(0.0), 1e-9)
        c.set(1e9, 1.0)
        assertEquals(800.0, c.screenY(3_000_000.0), 1e-6)
        c.set(1e9, 0.01)
        assertEquals(800.0, c.screenY(3_000_000.0), 1e-6)
        // All the way out, it fits with the pad above and below, centred.
        c.set(0.0, 0.0)
        assertEquals(32.0, c.screenY(0.0), 1e-6)
        assertEquals(800.0 - 32.0, c.screenY(3_000_000.0), 1e-6)
        c.set(1e9, c.scaleMin)
        assertEquals(1_500_000.0, c.y, 1e-6)
    }

    @Test
    fun aDragPastAnEndReportsTheOverscroll() {
        val c = camera()
        c.set(0.0, 1.0)
        assertEquals(400.0, c.y, 1e-9)
        val over = c.dragBy(50.0)
        assertEquals(50.0, over, 1e-9)
        assertEquals(400.0, c.y, 1e-9)
        assertEquals(0.0, c.dragBy(-50.0), 1e-9)
        assertEquals(450.0, c.y, 1e-9)
        c.set(1e9, 0.5)
        assertEquals(-20.0, c.dragBy(-20.0), 1e-9)
    }

    @Test
    fun aPinchKeepsTheDocumentUnderTheFingersStill() {
        val c = camera()
        c.set(1_000_000.0, 0.5)
        val under = c.docY(300.0)
        c.pinch(300.0, 300.0, 0.125)
        assertEquals(under, c.docY(300.0), 1e-6)
        // And the focus's drift pans: the point follows the fingers.
        val held = c.docY(300.0)
        c.pinch(300.0, 340.0, 0.125)
        assertEquals(held, c.docY(340.0), 1e-6)
        val (y, s) = c.zoomedAbout(200.0, 1.0)
        val after = Camera(800.0, 3_000_000.0).apply { set(y, s) }
        assertEquals(c.docY(200.0), after.docY(200.0), 1e-6)
    }

    @Test
    fun theRubberBandGivesALittleAndNoMore() {
        assertEquals(0.8, Camera.rubber(0.8, 0.1, 1.0), 0.0)
        var last = 1.0
        for (raw in listOf(1.1, 1.5, 2.0, 4.0, 100.0)) {
            val s = Camera.rubber(raw, 0.1, 1.0)
            assertTrue(s > last && s < 1.25)
            last = s
        }
        val low = Camera.rubber(0.01, 0.1, 1.0)
        assertTrue(low < 0.1 && low > 0.1 / 1.25)
    }
}
