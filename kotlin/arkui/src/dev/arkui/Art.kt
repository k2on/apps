// A picture: the one an app has for a thing, and `rust/arkui`'s derived
// square until it arrives or when there is none (`rust/arkui/src/art.rs`).
//
// The square is drawn the desktop's way: the name's tint from the first
// stop of its pair, a diagonal from full strength to a third, and a note
// knocked back on it. Which pair a name picks is FNV-1a over UTF-16 code
// units, the same function the desktop and the old phone client walked,
// so one album is one square on every screen. It is not a placeholder to
// be replaced by a spinner: it is right on its own, and a cover that does
// arrive simply appears over it.
//
// Where a picture comes from is the app's: a `CoverLoader` it provides
// (`LocalCoverLoader`), which says where a thing's art is, hands back one
// already decoded at once, and loads the rest. With none — a screenshot —
// only the squares are drawn.
package dev.arkui

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Shader
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp

public object Art {
    /** FNV-1a over UTF-16 code units: `arkui::art::hash`, which both clients pin. */
    public fun hash(text: String): Int {
        var h = 0x811c9dc5.toInt()
        for (unit in text) {
            h = h xor unit.code
            h *= 0x01000193
        }
        return h
    }

    /** The pair `seed` draws as, out of `set`. */
    public fun <T> pick(set: List<T>, seed: String): T =
        set[(hash(seed).toLong() and 0xffffffffL).rem(set.size).toInt()]
}

/**
 * The derived square as a bitmap `side` pixels across, drawn as [Square] draws it: for a picture
 * something outside the app's own screens loads by a URI — a car's browse list, say — where there
 * is no composition to draw into. `set` is the app's gradients, as [ArkTheme.art] has them.
 */
public fun squareBitmap(seed: String, set: List<Pair<Color, Color>>, side: Int): Bitmap {
    val tint = Art.pick(set, seed).first
    val bitmap = Bitmap.createBitmap(side, side, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bitmap)
    val s = side.toFloat()
    val full = tint.toArgb()
    val third = tint.copy(alpha = 0.33f).toArgb()
    canvas.drawRect(
        0f,
        0f,
        s,
        s,
        Paint().apply { shader = LinearGradient(0f, 0f, s, s, full, third, Shader.TileMode.CLAMP) })
    // The desktop's note, in its 100-unit box: a head, a stem and a flag.
    val u = s / 100f
    val ink = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = tint.copy(alpha = 0.35f).toArgb() }
    canvas.drawCircle(44 * u, 62 * u, 8 * u, ink)
    canvas.drawRect(50 * u, 26 * u, 54 * u, 62 * u, ink)
    val flag =
        android.graphics.Path().apply {
            moveTo(50 * u, 26 * u)
            lineTo(68 * u, 32 * u)
            lineTo(68 * u, 40 * u)
            lineTo(50 * u, 34 * u)
            close()
        }
    canvas.drawPath(flag, ink)
    return bitmap
}

/**
 * The derived square, filling its box: a gradient in the name's tint and a note on it. What it
 * draws with is made when its size or its tint changes, and kept: a row redrawn as a list moves
 * allocates nothing for it.
 */
@Composable
public fun Square(seed: String, modifier: Modifier = Modifier) {
    val tint = Art.pick(LocalArkTheme.current.art, seed).first
    Spacer(
        modifier.drawWithCache {
            val gradient =
                Brush.linearGradient(
                    listOf(tint, tint.copy(alpha = 0.33f)),
                    Offset.Zero,
                    Offset(size.width, size.height))
            // The desktop's note, in its 100-unit box: a head, a stem and a flag.
            val u = size.width / 100f
            val ink = tint.copy(alpha = 0.35f)
            val flag =
                Path().apply {
                    moveTo(50 * u, 26 * u)
                    lineTo(68 * u, 32 * u)
                    lineTo(68 * u, 40 * u)
                    lineTo(50 * u, 34 * u)
                    close()
                }
            onDrawBehind {
                drawRect(gradient)
                drawCircle(ink, radius = 8 * u, center = Offset(44 * u, 62 * u))
                drawRect(ink, topLeft = Offset(50 * u, 26 * u), size = Size(4 * u, 36 * u))
                drawPath(flag, ink)
            }
        })
}

/**
 * A picture, `side` across: the cover the app's [CoverLoader] has at `art` (for harken, a path
 * under the media root, or a whole URL), over the square derived from `seed`. Round for a person.
 */
@Composable
public fun Cover(
    seed: String,
    art: String,
    side: Dp,
    modifier: Modifier = Modifier,
    round: Boolean = false,
    corner: Dp = Space.corner
) {
    val shape = if (round) CircleShape else RoundedCornerShape(corner)
    Box(modifier.clip(shape)) {
        Square(seed, Modifier.fillMaxSize())
        val covers = LocalCoverLoader.current
        val url = remember(covers, art) { covers?.urlOf(art) }
        if (covers != null && url != null) {
            // A cover already decoded is drawn at once, and nothing is launched for it: a fling
            // brings several rows in a frame, and a coroutine each to find what was already there
            // was a cost per row.
            val cached = remember(url) { covers.cached(url) }
            if (cached != null) {
                Image(
                    cached,
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize())
            } else {
                var bitmap by remember(url) { mutableStateOf<ImageBitmap?>(null) }
                LaunchedEffect(url) {
                    covers.launched()
                    bitmap = covers.load(url, side)
                }
                bitmap?.let {
                    Image(
                        it,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize())
                }
            }
        }
    }
}

/**
 * Where an app's pictures come from, for [Cover]: an image library's, or one of the app's own. A
 * cover already decoded is drawn in the frame its row enters, so [cached] is asked first and must
 * not wait; [load] is asked only for what it did not have.
 */
public interface CoverLoader {
    /** Where the picture named `art` is, or null when there is none. */
    public fun urlOf(art: String): String?

    /** The picture at `url` if it is already decoded, without waiting. */
    public fun cached(url: String): ImageBitmap?

    /** The picture at `url`, decoded to about `side` across, or null when there is none. */
    public suspend fun load(url: String, side: Dp): ImageBitmap?

    /**
     * Told each time a cover on screen went looking for its picture rather than finding it decoded.
     */
    public fun launched() {}
}

/** The app's pictures, or none — in a screenshot, where only the squares are drawn. */
public val LocalCoverLoader: ProvidableCompositionLocal<CoverLoader?> = staticCompositionLocalOf {
    null
}
