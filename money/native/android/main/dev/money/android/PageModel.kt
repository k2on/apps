// The scanner's eyes: DocAligner's corner model in money's library
// (money/ffi/src/page.rs), handed a camera frame shrunk to the square it
// looks at, in colour. It answers where the page's four corners are, or
// that it sees none; Scanner.kt runs it on a thread of its own.
package dev.money.android

import android.media.Image
import dev.money.ffi.moneyPageCorners

object Model {
    /** The side of the square the model looks at. */
    const val SIDE = 256

    /** Whether the library has the model: false once a call to it has failed. */
    @Volatile
    var works = true
        private set

    /** The page in `rgb` (SIDE by SIDE, three bytes a pixel), in a frame `w` by `h`, or none. */
    fun page(rgb: ByteArray, w: Int, h: Int): Quad? =
        try {
            moneyPageCorners(rgb)?.let { c ->
                fun at(i: Int) = Pt(c[2 * i] * w, c[2 * i + 1] * h)
                Quad(at(0), at(1), at(2), at(3))
            }
        } catch (e: Throwable) {
            works = false
            null
        }
}

/**
 * A YUV_420_888 frame shrunk to `side` by `side`, RGB, by the nearest pixel: what the model is given
 * (it squeezes every picture to its square, as it was trained).
 */
fun square(image: Image, side: Int): ByteArray {
    val (w, h) = image.width to image.height
    val (yp, up, vp) = Triple(image.planes[0], image.planes[1], image.planes[2])
    val (yb, ub, vb) = Triple(yp.buffer, up.buffer, vp.buffer)
    val out = ByteArray(side * side * 3)
    for (j in 0 until side) {
        val sy = j * h / side
        for (i in 0 until side) {
            val sx = i * w / side
            val y = yb.get(sy * yp.rowStride + sx * yp.pixelStride).toInt() and 0xff
            val u = (ub.get((sy / 2) * up.rowStride + (sx / 2) * up.pixelStride).toInt() and 0xff) - 128
            val v = (vb.get((sy / 2) * vp.rowStride + (sx / 2) * vp.pixelStride).toInt() and 0xff) - 128
            val at = (j * side + i) * 3
            out[at] = clamp(y + 1.402f * v)
            out[at + 1] = clamp(y - 0.344f * u - 0.714f * v)
            out[at + 2] = clamp(y + 1.772f * u)
        }
    }
    return out
}

private fun clamp(v: Float): Byte = v.toInt().coerceIn(0, 255).toByte()
