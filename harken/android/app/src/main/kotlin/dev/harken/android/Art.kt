// A record's picture: the cover the server has for it, and the desktop's
// derived square until it arrives or when there is none
// (`rust/arkui/src/art.rs`).
//
// The square is drawn the desktop's way: the name's tint from the first
// stop of its pair, a diagonal from full strength to a third, and a note
// knocked back on it. Which pair a name picks is FNV-1a over UTF-16 code
// units, the same function the desktop and the old phone client walked,
// so one album is one square on every screen. It is not a placeholder to
// be replaced by a spinner: it is right on its own, and a cover that does
// arrive simply appears over it.
package dev.harken.android

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp

object Art {
    /** FNV-1a over UTF-16 code units: `arkui::art::hash`, which both clients pin. */
    fun hash(text: String): Int {
        var h = 0x811c9dc5.toInt()
        for (unit in text) {
            h = h xor unit.code
            h *= 0x01000193
        }
        return h
    }

    /** The pair `seed` draws as, out of `set`. */
    fun <T> pick(set: List<T>, seed: String): T = set[(hash(seed).toLong() and 0xffffffffL).rem(set.size).toInt()]
}

/** The derived square, filling its box: a gradient in the name's tint and a note on it. */
@Composable
fun Square(seed: String, modifier: Modifier = Modifier) {
    val tint = Art.pick(LocalHarken.current.art, seed).first
    Canvas(modifier) {
        drawRect(Brush.linearGradient(listOf(tint, tint.copy(alpha = 0.33f)), Offset.Zero, Offset(size.width, size.height)))
        // The desktop's note, in its 100-unit box: a head, a stem and a flag.
        val u = size.width / 100f
        val ink = tint.copy(alpha = 0.35f)
        drawCircle(ink, radius = 8 * u, center = Offset(44 * u, 62 * u))
        drawRect(ink, topLeft = Offset(50 * u, 26 * u), size = Size(4 * u, 36 * u))
        val flag =
            Path().apply {
                moveTo(50 * u, 26 * u)
                lineTo(68 * u, 32 * u)
                lineTo(68 * u, 40 * u)
                lineTo(50 * u, 34 * u)
                close()
            }
        drawPath(flag, ink)
    }
}

/**
 * A record's picture, `side` across: the server's cover at `art` (a path under the media root, or a
 * whole URL), over the square derived from `seed`. Round for a person.
 */
@Composable
fun Cover(seed: String, art: String, side: Dp, modifier: Modifier = Modifier, round: Boolean = false, corner: Dp = Space.corner) {
    val shape = if (round) CircleShape else RoundedCornerShape(corner)
    Box(modifier.clip(shape)) {
        Square(seed, Modifier.fillMaxSize())
        val covers = LocalCovers.current
        val url = covers?.urlOf(art)
        if (covers != null && url != null) {
            var bitmap by remember(url) { mutableStateOf<ImageBitmap?>(covers.cached(url)) }
            LaunchedEffect(url) { if (bitmap == null) bitmap = covers.load(url, side) }
            bitmap?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
        }
    }
}
