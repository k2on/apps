// The scanner's eyes (Vision.kt) held to drawn frames: a page found where it
// was drawn, tilted or not; none in a frame with no page; steady only once it
// has held still; sharp text sharper than blurred; and a page made flat lands
// its corners on the rectangle's.
package dev.money.android

import kotlin.math.cos
import kotlin.math.sin
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VisionTests {
    /** A frame of a dark, uneven table with `page` drawn on it in paper white, and some text. */
    private fun frame(w: Int, h: Int, page: Quad?, text: Boolean = true): Luma {
        val px = ByteArray(w * h)
        val r = java.util.Random(7)
        for (y in 0 until h) for (x in 0 until w) {
            var v = 60 + (x * 30 / w) + (y * 20 / h) + r.nextInt(12)
            if (page != null && inside(page, x + 0.5f, y + 0.5f)) {
                v = 225 + r.nextInt(10)
                // Lines of text across the middle of the page.
                if (text && (y / 6) % 3 == 0 && (x / 4) % 2 == 0) v = 40
            }
            px[y * w + x] = v.coerceIn(0, 255).toByte()
        }
        return Luma(w, h, px)
    }

    private fun inside(q: Quad, x: Float, y: Float): Boolean {
        val c = q.corners
        for (i in 0 until 4) {
            val a = c[i]
            val b = c[(i + 1) % 4]
            if ((b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x) < 0) return false
        }
        return true
    }

    private fun near(want: Quad, got: Quad?, within: Float) {
        assertNotNull("a page", got)
        val d = want.moved(got!!)
        assertTrue("corners within $within of where they were drawn, off by $d: $got", d <= within)
    }

    private fun turned(cx: Float, cy: Float, hw: Float, hh: Float, deg: Double): Quad {
        val t = Math.toRadians(deg)
        fun p(x: Float, y: Float) = Pt((cx + x * cos(t) - y * sin(t)).toFloat(), (cy + x * sin(t) + y * cos(t)).toFloat())
        return Quad(p(-hw, -hh), p(hw, -hh), p(hw, hh), p(-hw, hh))
    }

    @Test fun aPageIsFoundWhereItWasDrawn() {
        val page = Quad(Pt(120f, 60f), Pt(520f, 80f), Pt(500f, 440f), Pt(140f, 420f))
        near(page, Vision.page(frame(640, 480, page)), 12f)
    }

    @Test fun aTiltedReceiptIsFound() {
        // A long receipt, turned twenty degrees, as a hand holds a phone over it.
        val page = turned(320f, 240f, 90f, 200f, 20.0)
        near(page, Vision.page(frame(640, 480, page)), 12f)
    }

    @Test fun aFrameWithNoPageHasNone() {
        assertNull(Vision.page(frame(640, 480, null)))
        // A sliver is no page.
        val sliver = Quad(Pt(10f, 10f), Pt(40f, 10f), Pt(40f, 30f), Pt(10f, 30f))
        assertNull(Vision.page(frame(640, 480, sliver)))
    }

    @Test fun cornersAreOrderedClockwiseFromTheTopLeft() {
        val q = Quad.ordered(listOf(Pt(10f, 90f), Pt(90f, 10f), Pt(10f, 10f), Pt(90f, 90f)))
        assertEquals(Quad(Pt(10f, 10f), Pt(90f, 10f), Pt(90f, 90f), Pt(10f, 90f)), q)
        assertTrue(q.convex)
        assertFalse(Quad(Pt(0f, 0f), Pt(10f, 10f), Pt(10f, 0f), Pt(0f, 10f)).convex)
    }

    @Test fun aQuadTurnsWithItsFrame() {
        // A frame 100 by 50; a quarter turn clockwise makes it 50 by 100.
        val q = Quad(Pt(10f, 5f), Pt(60f, 5f), Pt(60f, 25f), Pt(10f, 25f))
        val r = q.rotated(1, 100, 50)
        assertEquals(Quad(Pt(25f, 10f), Pt(45f, 10f), Pt(45f, 60f), Pt(25f, 60f)), r)
        assertEquals(q, q.rotated(4, 100, 50))
    }

    @Test fun aPageIsSteadyOnlyOnceItHasHeldStill() {
        val s = Steady(holdMs = 1000, tolerance = 0.02f)
        val q = Quad(Pt(100f, 100f), Pt(500f, 100f), Pt(500f, 400f), Pt(100f, 400f))
        val shaken = q.toward(q.scaled(1.2f), 1f)
        assertFalse(s.tell(q, 0, 800f))
        assertFalse(s.tell(q, 500, 800f))
        assertTrue("a second still", s.tell(q, 1000, 800f))
        assertFalse("moved", s.tell(shaken, 1100, 800f))
        assertFalse(s.tell(shaken, 1900, 800f))
        assertTrue(s.tell(shaken, 2100, 800f))
        assertFalse("lost", s.tell(null, 2200, 800f))
        assertFalse(s.tell(shaken, 2300, 800f))
    }

    @Test fun sharpTextIsSharperThanBlurred() {
        val page = Quad(Pt(80f, 40f), Pt(560f, 40f), Pt(560f, 440f), Pt(80f, 440f))
        val sharp = frame(640, 480, page)
        val px = IntArray(sharp.w * sharp.h) { sharp.px[it].toInt() and 0xff }
        var soft = px
        repeat(4) { soft = Vision.blur(soft, sharp.w, sharp.h) }
        val blurred = Luma(sharp.w, sharp.h, ByteArray(soft.size) { soft[it].toByte() })
        val a = Vision.sharpness(sharp)
        val b = Vision.sharpness(blurred)
        assertTrue("sharp $a over blurred $b", a > 4 * b)
    }

    @Test fun aPageMadeFlatLandsOnItsRectangle() {
        val q = Quad(Pt(20f, 10f), Pt(80f, 20f), Pt(70f, 90f), Pt(10f, 80f))
        val m = Warp.homography(q, 60, 80)
        for ((rect, corner) in listOf(0.0 to 0.0, 60.0 to 0.0, 60.0 to 80.0, 0.0 to 80.0).zip(q.corners)) {
            val (u, v) = Warp.map(m, rect.first, rect.second)
            assertEquals(corner.x.toDouble(), u, 1e-6)
            assertEquals(corner.y.toDouble(), v, 1e-6)
        }
        // A square of white in a black frame, flattened, is white throughout.
        val w = 100
        val src = IntArray(w * w) { i -> if (inside(q, (i % w) + 0.5f, (i / w) + 0.5f)) -1 else 0xff000000.toInt() }
        val flat = Warp.flatten(src, w, w, q, 30, 40)
        val white = flat.count { (it and 0xffffff) == 0xffffff }
        assertTrue("$white of ${flat.size} white", white > flat.size * 0.95)
    }
}
