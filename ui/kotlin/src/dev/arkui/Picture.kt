// The picture primitive and the grid (ui/docs/primitives.md): a picture by its URL that is exactly
// the frame it is given, fetched, decoded for its size and kept (`ArkPicture`, `image(..).fill()`),
// the measures a frame is fixed by (`ArkMeasure`), and cells in as many equal columns as fit
// (`ArkGrid`, `grid(..)`). Their twins are SwiftUI's Primitives.swift (`ArkPicture`, `LazyVGrid`),
// iced's `arkui::view::layout` and arkui-web's layout.
package dev.arkui

import android.content.Context
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.util.Collections
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async

/** A primitive's fixed measures (`Measure`): `[layout]`'s `art-*`. */
public object ArkMeasure {
    public val artRow: Dp = LayoutTokens.ART_ROW.dp
    public val artShelf: Dp = LayoutTokens.ART_SHELF.dp
    public val artHeader: Dp = LayoutTokens.ART_HEADER.dp
    public val actionsWidth: Dp = LayoutTokens.ACTIONS_WIDTH.dp

    /** The side a filling picture with no measure (a grid's, the player's) is decoded for. */
    internal val fill: Dp = 384.dp
}

/**
 * A picture by its URL that is exactly the box it is given ([modifier]'s; `image(..).fill()`): the
 * picture covers it and is cut to it, so it never sizes anything by its own aspect. Nothing is
 * drawn until it has come, so what is under it shows (a cover's derived square). Decoded for
 * [decode] across (the most a filling picture is drawn at when null) by [UrlCovers]; a screenshot waits for it to come or fail.
 */
@Composable
public fun ArkPicture(url: String, label: String, decode: Dp?, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val covers = remember(context) { UrlCovers.of(context) }
    val side = decode ?: ArkMeasure.fill
    val said =
        if (label.isEmpty()) Modifier
        else
            Modifier.semantics {
                contentDescription = label
                role = Role.Image
            }
    Box(modifier.clipToBounds().then(said)) {
        val at = url.ifBlank { null }
        if (at != null) {
            // One already decoded is drawn at once, and nothing is launched for it: a fling
            // brings several rows in a frame.
            val cached = remember(at, side) { covers.cached(at, side) }
            var bitmap by remember(at, side) { mutableStateOf(cached) }
            if (cached == null) {
                LaunchedEffect(at, side) { bitmap = covers.load(at, side) }
            }
            bitmap?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
        }
    }
}

/**
 * Its content in as many equal columns as fit at least [min] wide, [across] apart, its rows [down]
 * apart (`grid(..)`): each cell is measured a column's width exactly, and a row is as tall as its
 * tallest. A page is a lazy list, so the grid lays out its cells in rows rather than nesting a lazy
 * grid in it.
 */
@Composable
public fun ArkGrid(min: Dp, across: Dp, down: Dp, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Layout(content, modifier) { measurables, constraints ->
        val gap = across.roundToPx()
        val line = down.roundToPx()
        val least = min.roundToPx()
        val width = if (constraints.hasBoundedWidth) constraints.maxWidth else least
        val columns = maxOf(1, (width + gap) / (least + gap))
        val each = maxOf(0, (width - gap * (columns - 1)) / columns)
        val cells = measurables.map { it.measure(Constraints(minWidth = each, maxWidth = each)) }
        val rows = cells.chunked(columns)
        val heights = rows.map { r -> r.maxOf { it.height } }
        val height = heights.sum() + line * maxOf(0, rows.size - 1)
        layout(width, height) {
            var y = 0
            rows.forEachIndexed { i, r ->
                r.forEachIndexed { j, c -> c.placeRelative(j * (each + gap), y) }
                y += heights[i] + line
            }
        }
    }
}

/**
 * Pictures by their URLs, for [ArkPicture]: the encoded original
 * on the disk under the app's cache (by the URL's FNV-1a; a cache the system may sweep), decoded
 * once for each size it is drawn at with the largest power-of-two shrink that keeps it that size,
 * and kept in memory by bytes. One fetch and one decode per URL and size however many rows ask at
 * once; one that cannot be had is remembered for the launch (a cover is decoration, and the square
 * under it is already right). `HttpURLConnection` and `BitmapFactory`: no dependency.
 */
public class UrlCovers private constructor(context: Context) {
    private val dir = File(context.cacheDir, "covers")
    private val density = context.resources.displayMetrics.density
    private val missing: MutableSet<String> = Collections.synchronizedSet(HashSet())
    private val loading = ConcurrentHashMap<String, Deferred<ImageBitmap?>>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val memory =
        object : LruCache<String, ImageBitmap>(MEMORY_BYTES) {
            override fun sizeOf(key: String, value: ImageBitmap): Int = value.width * value.height * 4
        }

    /** The picture at `url` if it is already decoded for `side`, without waiting. */
    public fun cached(url: String, side: Dp): ImageBitmap? = memory.get("${pixels(side)} $url")

    private fun pixels(side: Dp): Int = minOf(BOUND, (side.value * density).toInt().coerceAtLeast(64))

    /** The picture at `url`, decoded to about `side` across, or null when there is none. */
    public suspend fun load(url: String, side: Dp): ImageBitmap? {
        val want = pixels(side)
        val key = "$want $url"
        memory.get(key)?.let { return it }
        if (url in missing) return null
        val shared =
            loading.computeIfAbsent(key) {
                scope.async(start = CoroutineStart.LAZY) {
                    // A screenshot waits for the cover (ui/docs/shots.md).
                    Ready.waiting("cover", true)
                    try {
                        memory.get(key)?.let { return@async it }
                        val bytes = original(url) ?: return@async null.also { missing.add(url) }
                        val bitmap = decode(bytes, want)?.asImageBitmap()
                        if (bitmap == null) missing.add(url) else memory.put(key, bitmap)
                        bitmap
                    } finally {
                        loading.remove(key)
                        Ready.waiting("cover", false)
                    }
                }
            }
        shared.start()
        return shared.await()
    }

    private fun original(url: String): ByteArray? {
        val file = File(dir, "%08x".format(Art.hash(url)))
        if (file.exists()) return file.readBytes()
        val bytes =
            try {
                val c = URI(url).toURL().openConnection() as HttpURLConnection
                c.connectTimeout = 10_000
                c.readTimeout = 20_000
                try {
                    if (c.responseCode / 100 != 2) return null
                    c.inputStream.use { it.readBytes() }
                } finally {
                    c.disconnect()
                }
            } catch (e: Exception) {
                return null
            }
        dir.mkdirs()
        val tmp = File(dir, file.name + ".tmp")
        tmp.writeBytes(bytes)
        tmp.renameTo(file)
        return bytes
    }

    public companion object {
        /** The most pixels a cover is decoded to across: the full player's, at a phone's density. */
        public const val BOUND: Int = 768
        private const val MEMORY_BYTES = 32 * 1024 * 1024

        @Volatile private var shared: UrlCovers? = null

        /** The process's one. */
        public fun of(context: Context): UrlCovers =
            shared ?: synchronized(this) { shared ?: UrlCovers(context.applicationContext).also { shared = it } }

        private fun decode(bytes: ByteArray, want: Int): android.graphics.Bitmap? {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
            var n = 1
            while (minOf(bounds.outWidth, bounds.outHeight) / (n * 2) >= want) n *= 2
            return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = n })
        }
    }
}

