// The fly-to: the smooth zoom-and-pan of van Wijk and Nuijten ("Smooth and
// efficient zooming and panning", 2003), the path d3's interpolateZoom and
// a map's camera animate on, in one dimension.
//
// A view is (y, w): the document at the screen's centre and the height of
// document the screen shows, w = H / scale. Between two views the camera
// zooms out exactly as far as the distance needs — across a chapter
// barely, from Genesis to Revelation all the way to the line — travels,
// and zooms back in, along one hyperbolic path with no seam between the
// three. ρ = √2 is the paper's and d3's constant: how much the path prefers
// zooming out to panning. This follows d3's interpolateZoom line for line,
// with the second axis gone.
package dev.revelation.android

import kotlin.math.abs
import kotlin.math.cosh
import kotlin.math.exp
import kotlin.math.ln
import kotlin.math.sinh
import kotlin.math.sqrt
import kotlin.math.tanh

class FlyTo(
    val y0: Double,
    val w0: Double,
    val y1: Double,
    val w1: Double,
    val rho: Double = sqrt(2.0),
) {
    private val dy = y1 - y0
    private val d1 = abs(dy)
    private val degenerate = dy * dy < EPSILON2
    private val r0: Double
    private val r1: Double

    /** The path's length in the paper's units: what the duration grows with. */
    val length: Double

    init {
        if (degenerate) {
            r0 = 0.0
            r1 = 0.0
            length = abs(ln(w1 / w0) / rho)
        } else {
            val rho2 = rho * rho
            val rho4 = rho2 * rho2
            val d2 = dy * dy
            val b0 = (w1 * w1 - w0 * w0 + rho4 * d2) / (2 * w0 * rho2 * d1)
            val b1 = (w1 * w1 - w0 * w0 - rho4 * d2) / (2 * w1 * rho2 * d1)
            r0 = ln(sqrt(b0 * b0 + 1) - b0)
            r1 = ln(sqrt(b1 * b1 + 1) - b1)
            length = (r1 - r0) / rho
        }
    }

    /** How long the move takes: growing with the path, about 300 ms for a chapter and at most 1.2 s across the book. */
    val durationMs: Long get() = (length * MS_PER_UNIT).toLong().coerceIn(MIN_MS, MAX_MS)

    /** The view at [t] in [0, 1] of the path (not eased: [ease] is applied by the caller over time). */
    fun at(t: Double): View {
        if (t <= 0) return View(y0, w0)
        if (t >= 1) return View(y1, w1)
        if (degenerate) {
            // No distance to travel: the zoom alone, at a constant rate on a log scale.
            return View(y0 + t * dy, w0 * exp(ln(w1 / w0) * t))
        }
        val s = t * length
        val coshr0 = cosh(r0)
        val u = w0 / (rho * rho * d1) * (coshr0 * tanh(rho * s + r0) - sinh(r0))
        return View(y0 + u * dy, w0 * coshr0 / cosh(rho * s + r0))
    }

    data class View(val y: Double, val w: Double)

    companion object {
        private const val EPSILON2 = 1e-12
        const val MIN_MS = 300L
        const val MAX_MS = 1200L
        private const val MS_PER_UNIT = 100.0

        /** Ease-in-out (cubic), so the move starts and lands without a jolt. */
        fun ease(t: Double): Double {
            val x = t.coerceIn(0.0, 1.0) * 2
            return if (x <= 1) x * x * x / 2 else ((x - 2) * (x - 2) * (x - 2) + 2) / 2
        }
    }
}
