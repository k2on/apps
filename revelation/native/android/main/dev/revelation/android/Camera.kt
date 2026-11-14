// The camera: two numbers over the document, and nothing else in the app
// knows about the screen's geometry beyond them.
//
// `y` is the document coordinate at the screen's vertical centre and
// `scale` how many screen dp one document unit is (1 is reading size). The
// document is one-dimensional, so the column is always horizontally
// centred and there is no horizontal pan:
//
//     screenY = (docY − y) · scale + H/2
//
// Screen coordinates here are dp; the view multiplies by its density. Zoom
// is bounded by `scaleMin`, where the whole document fits with a little
// air, and `scaleMax`, reading size. `y` is clamped so the document never
// leaves the screen, and when the whole of it fits it is centred.
package dev.revelation.android

import kotlin.math.min

class Camera(
    /** The screen's height in dp. */
    var viewHeight: Double,
    /** The document's height in document units. */
    var docHeight: Double,
    /** The air above and below the document when it is zoomed all the way out, in dp: at least the header's height. */
    var pad: Double = 32.0,
) {
    var y: Double = viewHeight / 2
    var scale: Double = 1.0

    val scaleMax: Double get() = 1.0
    val scaleMin: Double get() = min(scaleMax, (viewHeight - 2 * pad) / docHeight)

    /** The visible height of document, `H / scale`: the fly-to's `w`. */
    val visibleHeight: Double get() = viewHeight / scale

    fun screenY(docY: Double): Double = (docY - y) * scale + viewHeight / 2

    fun docY(screenY: Double): Double = y + (screenY - viewHeight / 2) / scale

    /** The document at the screen's top and bottom edges. */
    val docTop: Double get() = docY(0.0)
    val docBottom: Double get() = docY(viewHeight)

    /** The range `y` may take at [s]: one point, the document's middle, when the document fits. */
    fun minY(s: Double = scale): Double = if (docHeight * s <= viewHeight) docHeight / 2 else viewHeight / (2 * s)

    fun maxY(s: Double = scale): Double = if (docHeight * s <= viewHeight) docHeight / 2 else docHeight - viewHeight / (2 * s)

    fun clampY(yy: Double, s: Double = scale): Double = yy.coerceIn(minY(s), maxY(s))

    fun clampScale(s: Double): Double = s.coerceIn(scaleMin, scaleMax)

    /** Puts the camera at ([y], [scale]) within its bounds. */
    fun set(y: Double, scale: Double) {
        this.scale = clampScale(scale)
        this.y = clampY(y, this.scale)
    }

    /** Puts the camera at [scale] as given, a rubber band past the bounds, and y within the bounds at it. */
    fun place(y: Double, scale: Double) {
        this.scale = scale
        this.y = clampY(y, scale)
    }

    /**
     * Moves the document by [dyScreen] dp on screen (a finger moving down by `d` moves the document
     * down, so the camera up). Returns what could not be moved because an end was reached, in dp on
     * screen, positive past the top: what the edge effect is pulled by.
     */
    fun dragBy(dyScreen: Double): Double {
        val wanted = y - dyScreen / scale
        val clamped = clampY(wanted)
        y = clamped
        return (clamped - wanted) * scale
    }

    /**
     * A pinch step: the scale becomes [newScale] (not clamped: the gesture rubber-bands it), the
     * document point that was under [focus] stays under it, and the focus's own move from
     * [previousFocus] pans. Both in screen dp.
     */
    fun pinch(previousFocus: Double, focus: Double, newScale: Double) {
        val anchored = docY(previousFocus)
        scale = newScale
        y = clampY(anchored - (focus - viewHeight / 2) / newScale)
    }

    /** The camera that zooms to [newScale] keeping the document under [focus] (screen dp) still. */
    fun zoomedAbout(focus: Double, newScale: Double): Pair<Double, Double> {
        val s = clampScale(newScale)
        return Pair(clampY(docY(focus) - (focus - viewHeight / 2) / s, s), s)
    }

    companion object {
        /**
         * A rubber band for a scale pinched past [lo] or [hi]: within the bounds it is [raw]; past
         * them the overshoot is pulled in ever harder, approaching a factor of [give] beyond the bound
         * and never reaching it, so a pinch past reading size gives a little and springs back.
         */
        fun rubber(raw: Double, lo: Double, hi: Double, give: Double = 1.25): Double {
            if (raw > hi) {
                val over = raw / hi - 1
                val limit = give - 1
                return hi * (1 + limit * over / (over + limit))
            }
            if (raw < lo) {
                val over = lo / raw - 1
                val limit = give - 1
                return lo / (1 + limit * over / (over + limit))
            }
            return raw
        }
    }
}
