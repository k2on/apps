// Covers, fetched once and shrunk before anything draws them — the
// desktop's `covers.rs`, on a phone, and the `CoverLoader` `ark-ui`'s
// `Cover` draws from.
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
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.Dp
import dev.harken.android.ui.Art
import dev.harken.android.ui.CoverLoader
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.util.Collections
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.launch

class Covers(context: Context, private val server: String) : CoverLoader {
    private val dir = folder(context)
    private val density = context.resources.displayMetrics.density
    private val missing: MutableSet<String> = Collections.synchronizedSet(HashSet())
    // One fetch and one decode per URL, however many rows ask at once. The rows of one album all
    // miss the memory cache in the same frame, and each used to read the file and decode it again
    // — and every cover in the app waited on one mutex while the first was fetched.
    private val loading = ConcurrentHashMap<String, Deferred<ImageBitmap?>>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /** How many times a cover was read off the disk (or fetched), and decoded: for the tests. */
    internal val reads = AtomicInteger()
    internal val decodes = AtomicInteger()

    /** How many covers on screen went looking for their picture, rather than finding it decoded: for the tests. */
    internal val launches = AtomicInteger()

    override fun launched() {
        launches.incrementAndGet()
    }
    private val memory =
        object : LruCache<String, ImageBitmap>(MEMORY_BYTES) {
            override fun sizeOf(key: String, value: ImageBitmap): Int = value.width * value.height * 4
        }

    /** The URL a record's `art` is at, or null when it has none. */
    override fun urlOf(art: String): String? = if (art.isBlank()) null else Media.url(server, art)

    /** A cover already decoded, without waiting. */
    override fun cached(url: String): ImageBitmap? = memory.get(url)

    /**
     * Start decoding the covers of a list that has just arrived, `side` across each, so the rows
     * that draw them find them decoded. A row that has to go looking draws the derived square and
     * the picture lands after it: a fling through a fresh list of search results brings rows to the
     * middle of the screen before their covers have been read.
     */
    fun warm(arts: List<Pair<String, Dp>>) {
        for ((art, side) in arts) {
            val url = urlOf(art) ?: continue
            if (memory.get(url) != null || url in missing || loading.containsKey(url)) continue
            warming.incrementAndGet()
            scope.launch {
                try {
                    load(url, side)
                } finally {
                    warming.decrementAndGet()
                }
            }
        }
    }

    /** How many covers [warm] is still decoding: for the tests. */
    internal val warming = AtomicInteger()

    /** The cover at `url`, decoded to about `side` (and never past [BOUND] pixels), or null. */
    override suspend fun load(url: String, side: Dp): ImageBitmap? {
        memory.get(url)?.let { return it }
        if (url in missing) return null
        val want = minOf(BOUND, (side.value * density).toInt().coerceAtLeast(64))
        // Started only once it is the one in the map, so its `finally` cannot run before it is
        // there and leave a finished one behind.
        val shared =
            loading.computeIfAbsent(url) {
                scope.async(start = CoroutineStart.LAZY) {
                    try {
                        // Asked again here, where it is the one in the map: a row that missed
                        // both before the last load ended must not start another.
                        memory.get(url)?.let { return@async it }
                        if (url in missing) return@async null
                        val bytes = onDisk(url) ?: return@async null.also { missing.add(url) }
                        decodes.incrementAndGet()
                        val bitmap = decode(bytes, want)?.asImageBitmap()
                        if (bitmap == null) missing.add(url) else memory.put(url, bitmap)
                        bitmap
                    } finally {
                        loading.remove(url)
                    }
                }
            }
        shared.start()
        return shared.await()
    }

    // The encoded original: from the disk, or fetched and written there.
    private fun onDisk(url: String): ByteArray? {
        reads.incrementAndGet()
        return original(dir, url)
    }

    companion object {
        /**
         * The encoded original of the cover at `url`, kept under `dir` by its URL's FNV-1a: from
         * the disk, or fetched and written there — the screens' cache and the car's
         * (CoverProvider.kt) one and the same. Null when it cannot be had.
         */
        fun original(dir: File, url: String): ByteArray? {
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

        /** Where the covers' originals are kept. */
        fun folder(context: Context): File = File(context.cacheDir, "covers")

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
