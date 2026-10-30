// The numbers everything moves by: a gesture's preview comes free at first,
// then harder, and never past its limit; and a settle is never longer than
// its cap, however long the spring under it would have run.
package dev.arkui

import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.tween
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SpringsTests {
    @Test
    fun thePreviewSaturates() {
        val a = Springs.preview(0.3f)
        val b = Springs.preview(0.9f)
        println(
            "preview: 0.1 → ${Springs.preview(0.1f)}, 0.2 → ${Springs.preview(0.2f)}, 0.3 → $a, 0.5 → ${Springs.preview(0.5f)}, 0.9 → $b, 1 → ${Springs.preview(1f)}")
        assertTrue("both within the limit: $a, $b", a <= Springs.PREVIEW && b <= Springs.PREVIEW)
        // Three times the gesture is barely more preview: it has stiffened.
        assertTrue("0.9 only a little past 0.3: $b against $a", b - a < 0.15f * a)
        // And free at first: the first tenth of the gesture is most of a tenth of the limit's
        // worth.
        assertTrue(
            "free at first: ${Springs.preview(0.02f)}",
            Springs.preview(0.02f) > 0.15f * Springs.PREVIEW)
    }

    @Test
    fun aCappedSettleIsWhereItWasGoingByItsCap() {
        val v = Capped(tween<Float>(1_000), 300).vectorize(Float.VectorConverter)
        val from = AnimationVector1D(0f)
        val to = AnimationVector1D(100f)
        val still = AnimationVector1D(0f)
        assertEquals("no longer than the cap", 300_000_000L, v.getDurationNanos(from, to, still))
        val before = v.getValueFromNanos(299_000_000L, from, to, still).value
        assertTrue("short of it before: $before", before < 100f)
        assertEquals(
            "there at it", 100f, v.getValueFromNanos(300_000_000L, from, to, still).value, 0f)
        // And a settle, which is a spring under the same cap.
        val s = Springs.settle().vectorize(Float.VectorConverter)
        assertTrue(
            "a settle is done by the cap",
            s.getDurationNanos(from, AnimationVector1D(2_000f), still) <=
                Springs.CAP_MS * 1_000_000L)
    }
}
