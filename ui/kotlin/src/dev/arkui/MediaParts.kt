// The parts a screen of covers and sounds is laid out with (ui/docs/primitives.md, "What Harken
// Lays Out"), as `ark_app::view` names them: something pressed as one (`Pressable`), a cover over
// its derived square (`Artwork`), tiles in as many columns as fit (`ArtworkGrid`), and where a
// sound is, counted from its clock as it is drawn (`ClockScrubber`, `ClockHairline`). Their twins
// are SwiftUI's MediaParts.swift, iced's `arkui::view` and arkui-web's parts.
package dev.arkui

import android.content.Context
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.semantics
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
import kotlinx.coroutines.delay

/** How a pressed thing answers (ui/docs/motion.md, Press Feedback). */
public enum class PressKind {
    /** A cover, a tile, a glyph: it gives under the finger ([Press.Give]). */
    Give,

    /** A row the page's width: lit under the finger ([Press.Highlight]). */
    Highlight,
}

/** What is laid out in [content], pressed as one: its press as the finger lands, said as [label], its menu when held. */
@Composable
public fun Pressable(label: String, press: PressKind, menu: List<MenuAction> = emptyList(), onTap: () -> Unit, content: @Composable () -> Unit) {
    var open by remember { mutableStateOf(false) }
    val offered = menu.filter { it.shown }
    val kind = if (press == PressKind.Give) Press.Give else Press.Highlight
    val tap =
        if (offered.isEmpty()) Modifier.pressable(onClick = onTap, role = Role.Button, press = kind)
        else Modifier.pressable(onClick = onTap, onLongClick = { open = true }, role = Role.Button, press = kind)
    // A row is the width of its list; a tile in a grid its cell's ([ArtworkGrid]).
    val cell = LocalGridCell.current
    val width =
        when {
            press == PressKind.Highlight -> Modifier.fillMaxWidth()
            cell != null -> Modifier.width(cell)
            else -> Modifier
        }
    Box(width.then(tap).semantics(mergeDescendants = true) { contentDescription = label }) {
        androidx.compose.runtime.CompositionLocalProvider(LocalGridCell provides null) { Column { content() } }
        MenuOf(offered, open) { open = false }
    }
}

/** How large a cover is drawn: `[layout]`'s `art-*`, or as wide as given. */
public enum class ArtSize(public val side: Dp?) {
    Row(LayoutTokens.ART_ROW.dp),
    Shelf(LayoutTokens.ART_SHELF.dp),
    Header(LayoutTokens.ART_HEADER.dp),
    Fill(null),
}

/** As wide as a cover of [size], what it holds kept to it. */
public fun Modifier.artWidth(size: ArtSize): Modifier =
    when (val side = size.side) {
        null -> fillMaxWidth()
        else -> width(side)
    }

/**
 * A cover: the picture at [url] over the square [seed] derives (shown while it comes, or when there
 * is none), at [size], round for a person — ArkUI's [Cover] over the app's [CoverLoader], or
 * [UrlCovers] when it gives none. A screenshot waits for it to come or fail.
 */
@Composable
public fun Artwork(url: String, seed: String, label: String, size: ArtSize, round: Boolean) {
    val context = LocalContext.current
    val loader = LocalCoverLoader.current ?: remember(context) { UrlCovers.of(context) }
    val described = Modifier.semantics { contentDescription = label }
    val side = size.side
    if (side != null) {
        androidx.compose.runtime.CompositionLocalProvider(LocalCoverLoader provides loader) {
            Cover(seed, url, side, described.size(side), round = round)
        }
    } else {
        BoxWithConstraints(described.fillMaxWidth().aspectRatio(1f)) {
            androidx.compose.runtime.CompositionLocalProvider(LocalCoverLoader provides loader) {
                Cover(seed, url, maxWidth, Modifier.fillMaxWidth().aspectRatio(1f), round = round)
            }
        }
    }
}

/** Tiles in as many columns as fit at least `art-shelf` wide, at the page's margins. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
public fun ArtworkGrid(content: @Composable () -> Unit) {
    // A list's row holds it (a page is a LazyColumn), so it lays its tiles out in rows of as
    // many as fit rather than nesting a lazy grid in a lazy list.
    BoxWithConstraints(Modifier.fillMaxWidth().padding(horizontal = Space.page)) {
        val least = LayoutTokens.ART_SHELF.dp
        val columns = maxOf(1, ((maxWidth + Space.m) / (least + Space.m)).toInt())
        val each = (maxWidth - Space.m * (columns - 1)) / columns
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Space.m),
            verticalArrangement = Arrangement.spacedBy(Space.l),
            maxItemsInEachRow = columns,
        ) {
            androidx.compose.runtime.CompositionLocalProvider(LocalGridCell provides each) { content() }
        }
    }
}

/** How wide a tile of an [ArtworkGrid] is, where it is in one. */
public val LocalGridCell: androidx.compose.runtime.ProvidableCompositionLocal<Dp?> =
    androidx.compose.runtime.staticCompositionLocalOf { null }

/** Milliseconds as `m:ss`, or `h:mm:ss` from an hour: ark_format's `clock`. */
internal fun clockWords(ms: Long): String {
    val secs = maxOf(ms, 0L) / 1000
    val h = secs / 3600
    return if (h == 0L) "%d:%02d".format(secs / 60, secs % 60) else "%d:%02d:%02d".format(h, secs / 60 % 60, secs % 60)
}

/** The device's clock while [playing] (and not in a screenshot), read again every quarter second. */
@Composable
private fun ticking(playing: Boolean): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(playing) {
        now = System.currentTimeMillis()
        while (playing && !Shots.on) {
            delay(250)
            now = System.currentTimeMillis()
        }
    }
    return now
}

private fun clockAt(at: Long, since: Long, playing: Boolean, length: Long, now: Long): Long =
    (if (playing) at + maxOf(now - since, 0L) else at).coerceIn(0L, maxOf(length, 1L))

/**
 * Where a sound is, which a finger drags: ArkUI's [Scrubber] over its clock, moving as it plays;
 * let go, [onSeek] is sent once with where. Meeting either end under the finger is a selection's
 * haptic.
 */
@Composable
public fun ClockScrubber(at: Long, since: Long, playing: Boolean, length: Long, onSeek: (Long) -> Unit) {
    val now = clockAt(at, since, playing, length, ticking(playing))
    val whole = maxOf(length, 1L).toFloat()
    var end by remember { mutableStateOf(0) }
    SensoryFeedback(Feedback.Selection, end.takeIf { it != 0 })
    Column(Modifier.fillMaxWidth()) {
        Scrubber(
            value = now / whole,
            onValue = { v -> end = if (v <= 0f) -1 else if (v >= 1f) 1 else 0 },
            onDone = { v ->
                end = 0
                onSeek((v * whole).toLong())
            },
            left = { clockWords((it * whole).toLong()) },
            right = { clockWords(length) },
        )
    }
}

/** How far along a sound is, a hairline in the accent, moving as it plays. */
@Composable
public fun ClockHairline(at: Long, since: Long, playing: Boolean, length: Long) {
    val now = clockAt(at, since, playing, length, ticking(playing))
    val fraction = now.toFloat() / maxOf(length, 1L)
    val fill = MaterialTheme.colorScheme.primary
    // Drawn, not laid out: the fill's width read in the draw.
    Box(
        Modifier.fillMaxWidth()
            .height(2.dp)
            .background(MaterialTheme.colorScheme.outlineVariant)
            .drawBehind { drawRect(fill, size = Size(size.width * fraction, size.height)) }
            .semantics { progressBarRangeInfo = ProgressBarRangeInfo(fraction, 0f..1f) })
}

/**
 * Covers by their URLs, for an app that gives [Cover] no loader of its own: the encoded original
 * on the disk under the app's cache (by the URL's FNV-1a; a cache the system may sweep), decoded
 * once for each size it is drawn at with the largest power-of-two shrink that keeps it that size,
 * and kept in memory by bytes. One fetch and one decode per URL and size however many rows ask at
 * once; one that cannot be had is remembered for the launch (a cover is decoration, and the square
 * under it is already right). `HttpURLConnection` and `BitmapFactory`: no dependency.
 */
public class UrlCovers private constructor(context: Context) : CoverLoader {
    private val dir = File(context.cacheDir, "covers")
    private val density = context.resources.displayMetrics.density
    private val missing: MutableSet<String> = Collections.synchronizedSet(HashSet())
    private val loading = ConcurrentHashMap<String, Deferred<ImageBitmap?>>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val memory =
        object : LruCache<String, ImageBitmap>(MEMORY_BYTES) {
            override fun sizeOf(key: String, value: ImageBitmap): Int = value.width * value.height * 4
        }

    override fun urlOf(art: String): String? = art.ifBlank { null }

    override fun cached(url: String, side: Dp): ImageBitmap? = memory.get("${pixels(side)} $url")

    private fun pixels(side: Dp): Int = minOf(BOUND, (side.value * density).toInt().coerceAtLeast(64))

    override suspend fun load(url: String, side: Dp): ImageBitmap? {
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

/**
 * How loud, as `volume` lays it out: [VolumeSlider] over a level 0 to 100, its loudspeaker sending
 * [onMute]; the level follows the finger and [onChange] is sent once, with where it was let go, as
 * the scrubber's seek is. A level from elsewhere is sprung to ([VolumeSlider]).
 */
@Composable
public fun VolumePart(level: Long, muted: Boolean, about: String, onChange: (Long) -> Unit, onMute: () -> Unit) {
    var held by remember { mutableStateOf<Int?>(null) }
    VolumeSlider(
        level.coerceIn(0L, 100L).toInt(),
        muted,
        onLevel = { held = it },
        onDone = {
            held?.let { onChange(it.toLong()) }
            held = null
        },
        onMute = onMute,
        modifier = Modifier.fillMaxWidth(),
        about = about,
    )
}
