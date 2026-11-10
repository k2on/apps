package dev.revelation.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FlyToTests {
    @Test
    fun theEndpointsAreExact() {
        val f = FlyTo(1_000.0, 800.0, 2_900_000.0, 800.0)
        assertEquals(FlyTo.View(1_000.0, 800.0), f.at(0.0))
        assertEquals(FlyTo.View(2_900_000.0, 800.0), f.at(1.0))
        // And the formula itself lands on them, not only the guard.
        val near = f.at(1 - 1e-9)
        assertEquals(2_900_000.0, near.y, 1.0)
        assertEquals(800.0, near.w, 1.0)
        val start = f.at(1e-9)
        assertEquals(1_000.0, start.y, 1.0)
        assertEquals(800.0, start.w, 1.0)
    }

    @Test
    fun wRisesThenFalls() {
        val f = FlyTo(1_000.0, 800.0, 2_900_000.0, 800.0)
        val ws = (0..100).map { f.at(it / 100.0).w }
        val peak = ws.indices.maxBy { ws[it] }
        assertTrue(peak in 30..70)
        for (i in 1..peak) assertTrue(ws[i] >= ws[i - 1])
        for (i in peak + 1..100) assertTrue(ws[i] <= ws[i - 1])
        // Across the whole Bible it zooms out about as far as the whole Bible.
        assertTrue(ws[peak] > 1_000_000)
        // And y only ever moves toward the target.
        val ys = (0..100).map { f.at(it / 100.0).y }
        for (i in 1..100) assertTrue(ys[i] >= ys[i - 1])
    }

    @Test
    fun aShortMoveBarelyZoomsOut() {
        val f = FlyTo(10_000.0, 800.0, 12_000.0, 800.0)
        val peak = (0..100).maxOf { f.at(it / 100.0).w }
        assertTrue(peak < 3_000)
        assertEquals(FlyTo.MIN_MS, f.durationMs)
        assertEquals(FlyTo.MAX_MS, FlyTo(1_000.0, 800.0, 2_900_000.0, 800.0).durationMs)
    }

    @Test
    fun theDegenerateCaseIsAPlainZoom() {
        val f = FlyTo(5_000.0, 800.0, 5_000.0, 80_000.0)
        for (i in 0..10) assertEquals(5_000.0, f.at(i / 10.0).y, 0.0)
        assertEquals(8_000.0, f.at(0.5).w, 1e-6)
        assertEquals(80_000.0, f.at(1.0).w, 0.0)
        assertTrue(f.durationMs in FlyTo.MIN_MS..FlyTo.MAX_MS)
        val still = FlyTo(5_000.0, 800.0, 5_000.0, 800.0)
        assertEquals(FlyTo.View(5_000.0, 800.0), still.at(0.5))
        assertEquals(FlyTo.MIN_MS, still.durationMs)
    }

    @Test
    fun theEaseStartsAndLandsGently() {
        assertEquals(0.0, FlyTo.ease(0.0), 0.0)
        assertEquals(0.5, FlyTo.ease(0.5), 1e-12)
        assertEquals(1.0, FlyTo.ease(1.0), 0.0)
        assertTrue(FlyTo.ease(0.05) < 0.05)
        assertTrue(FlyTo.ease(0.95) > 0.95)
    }
}
