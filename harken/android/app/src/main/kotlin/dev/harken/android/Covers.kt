// Covers, fetched once and shrunk before anything draws them — the
// desktop's `covers.rs`, on a phone.
//
// Two caches: the encoded original on disk, under the app's cache
// directory, keyed by the URL's FNV-1a (a cache: the system may sweep it,
// and losing it costs a fetch); and decoded bitmaps in memory, bounded by
// bytes. The decode is bounded too: `inSampleSize` is chosen from the
// image's own bounds so a 3000-pixel scan becomes a few hundred pixels
// before it is ever a bitmap — a cover is drawn at most a few hundred
// pixels across, and a full-size one is tens of megabytes. A failure is
// remembered for the session and its reason dropped: a cover is decoration,
// and the derived square under it is already right.
//
// No dependency: `HttpURLConnection` and `BitmapFactory`, which the
// platform has. `/media` needs no token (`harken/server`), which is also
// what lets the notification's own loader fetch the same URL.
package dev.harken.android

import android.content.Context
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.Dp
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.util.Collections
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

class Covers(context: Context, private val server: String) {
    private val dir = File(context.cacheDir, "covers")
    private val density = context.resources.displayMetrics.density
    private val missing: MutableSet<String> = Collections.synchronizedSet(HashSet())
    private val fetching = Mutex()
    private val memory =
        object : LruCache<String, ImageBitmap>(MEMORY_BYTES) {
            override fun sizeOf(key: String, value: ImageBitmap): Int = value.width * value.height * 4
        }

    /** The URL a record's `art` is at, or null when it has none. */
    fun urlOf(art: String): String? = if (art.isBlank()) null else Media.url(server, art)

    /** A cover already decoded, without waiting. */
    fun cached(url: String): ImageBitmap? = memory.get(url)

    /** The cover at `url`, decoded to about `side` (and never past [BOUND] pixels), or null. */
    suspend fun load(url: String, side: Dp): ImageBitmap? {
        memory.get(url)?.let { return it }
        if (url in missing) return null
        return withContext(Dispatchers.IO) {
            val bytes = fetching.withLock { onDisk(url) } ?: return@withContext null.also { missing.add(url) }
            val want = minOf(BOUND, (side.value * density).toInt().coerceAtLeast(64))
            val bitmap = decode(bytes, want)?.asImageBitmap()
            if (bitmap == null) missing.add(url) else memory.put(url, bitmap)
            bitmap
        }
    }

    // The encoded original: from the disk, or fetched and written there.
    private fun onDisk(url: String): ByteArray? {
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

    companion object {
        /** The most pixels a cover is decoded to across: the full player's, at a phone's density. */
        const val BOUND = 768

        /** What the decoded covers may hold in memory. */
        const val MEMORY_BYTES = 32 * 1024 * 1024

        /** Decode `bytes` with the largest power-of-two shrink that keeps it at least `want` across. */
        fun decode(bytes: ByteArray, want: Int): android.graphics.Bitmap? {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
            val opts = BitmapFactory.Options().apply { inSampleSize = sampleSize(minOf(bounds.outWidth, bounds.outHeight), want) }
            return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
        }

        /** The largest power of two that leaves `shortest` at least `want`. */
        fun sampleSize(shortest: Int, want: Int): Int {
            var n = 1
            while (shortest / (n * 2) >= want) n *= 2
            return n
        }
    }
}

/** The covers of the session, or none — in a screenshot, where only the squares are drawn. */
val LocalCovers = staticCompositionLocalOf<Covers?> { null }
