// Reduced motion, and the numbers a release is decided by, on the JVM: with
// motion reduced the expanding card never travels — it fades out, is at its
// other end, and fades in — where without it the card passes through every
// place between; a flick's settle bounces and a reduced one does not; a
// release goes to the rest nearest where it was going; and tracking is
// tight on big text, nothing on body, a little open on captions.
package dev.arkui

import androidx.compose.runtime.MonotonicFrameClock
import androidx.compose.ui.unit.Density
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ReducedMotionTests {
    /** A frame clock at 60 fps that writes down where the card is at every frame. */
    private class Frames(val seen: (Float, Float) -> Unit, val card: ExpandingCardState) :
        MonotonicFrameClock {
        private var now = 0L

        override suspend fun <R> withFrameNanos(onFrame: (Long) -> R): R {
            now += 16_666_667L
            val r = onFrame(now)
            seen(card.p, card.veil)
            return r
        }
    }

    /** The card opened from the bar: every (p, veil) it was drawn at on the way. */
    private fun opening(reduced: Boolean): List<Pair<Float, Float>> {
        val card = ExpandingCardState(CardFace.Collapsed, Density(1f))
        card.reach(1_000f)
        card.reduced = reduced
        val seen = mutableListOf<Pair<Float, Float>>()
        runBlocking { withContext(Frames({ p, v -> seen += p to v }, card)) { card.open() } }
        assertEquals("open at the end", 1f, card.p, 0f)
        assertEquals("and wholly there", 1f, card.veil, 0f)
        return seen
    }

    @Test
    fun reducedMotionOpensTheCardWithAFadeNotAMove() {
        val moved = opening(reduced = false)
        val faded = opening(reduced = true)
        println("open: moving ${moved.map { it.first }}; reduced ${faded}")
        assertTrue("without reduced motion it passes between the ends", moved.any { it.first > 0.05f && it.first < 0.95f })
        assertTrue("and never fades", moved.all { it.second == 1f })
        assertTrue("with it, the card is never between its ends", faded.all { it.first == 1f })
        assertTrue("it fades in instead", faded.any { it.second > 0.05f && it.second < 0.95f })
    }

    @Test
    fun aFlickSettlesWithMomentumAndAReducedOneDoesNot() {
        val d = Density(2f)
        val flick = 2f * 2f * Springs.FLICK.value
        assertTrue("fast enough is a flick", Springs.isFlick(flick, d))
        assertFalse("a drift is not", Springs.isFlick(100f, d))
        val thrown = Springs.settle(flick, d)
        val still = Springs.settle(0f, d)
        val reduced = Springs.settle(flick, d, reduced = true)
        assertNotEquals("a flick's settle is not a still one's", still, thrown)
        assertEquals("a reduced flick settles as a still one does", still, reduced)
        assertEquals("a still one is critically damped", 1f, Springs.MOVE, 0f)
    }

    @Test
    fun aReleaseGoesWhereItWasGoing() {
        val rests = listOf(0f, 1_000f)
        assertEquals("let go still, the nearer end", 1_000f, Springs.nearest(600f, 0f, rests), 0f)
        assertEquals("…either way", 0f, Springs.nearest(400f, 0f, rests), 0f)
        // Most of the way closed, flicked up: Apple's projection carries it over half.
        assertEquals("a flick up opens it", 0f, Springs.nearest(800f, -1_500f, rests), 0f)
        assertEquals("a flick back down from near open closes it", 1_000f, Springs.nearest(200f, 1_500f, rests), 0f)
        assertEquals("the projection is about half a second's worth", 499f, Springs.project(1_000f), 0.5f)
    }

    @Test
    fun trackingIsTightOnBigTextAndOpenOnSmall() {
        assertTrue("a large title is tracked in: ${Typeset.tracking(34f)}", Typeset.tracking(34f).value < 0f)
        assertEquals("body is left alone", 0f, Typeset.tracking(16f).value, 0f)
        assertTrue("a caption is tracked out: ${Typeset.tracking(11f)}", Typeset.tracking(11f).value > 0f)
        assertTrue("and big text is led tight", Typeset.leading(34f).value / 34f < 1.25f)
        assertTrue("the large title is tracked", LargeTitle.letterSpacing.value < 0f)
    }
}
