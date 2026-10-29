// A fast fling through every song, on the JVM: what one row entering the
// screen costs to compose and measure, and what a frame of rows costs to
// draw. The real `HarkenApp` over a peer holding two thousand songs authored
// through `add_song`, two hundred albums each with a cover already on the
// disk, on Paparazzi's frame clock — the list moved three rows a frame, the
// speed of a hard fling, six times from the top: four to warm the JIT and
// the covers' memory cache, then two reported. Timed in this thread's CPU
// time, so the rest of the machine does not move the numbers much, and
// allocations counted by HotSpot. These are HotSpot and layoutlib numbers:
// a relative proxy, to compare one build of a row with another, and not a
// phone's frame times. Run only when asked: HARKEN_FLING_BENCH set, to a
// number of songs or to anything for 2000.
package dev.harken.android

import dev.harken.android.gen.AddSong
import dev.harken.android.gen.CreatePlaylist
import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.harken.android.onboarding.ArkSession
import dev.harken.android.ui.Art
import dev.harken.android.ui.Cover
import dev.harken.android.ui.LocalCoverLoader
import dev.harken.android.ui.Recompositions
import java.io.File
import java.lang.reflect.Proxy
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test

class FlingTests {
    @get:Rule
    val paparazzi =
        Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true, snapshotHandler = Discard)

    /** The frames are rendered and thrown away: the counts are the result. */
    private object Discard : SnapshotHandler {
        override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
            Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, _, _ -> null }
                as SnapshotHandler.FrameHandler

        override fun close() {}
    }

    private fun song(i: Int): AddSong =
        AddSong(
            title = "Song number $i, a title of some length",
            artist = "Artist ${i / 40}",
            album = "Album ${i / 10}",
            durationMs = 120_000L + 997L * i,
            file = "music/$i.flac",
            track = (i % 10 + 1).toLong(),
            part = "",
            catalogue = "",
            performer = "",
            bpm = 0,
            albumArt = "covers/${i / 10}.png",
            artistArt = "",
            disc = 1,
            workTitle = "",
            movementNo = 0,
            artistBanner = "",
            released = 0,
            folder = "",
        )

    private val counts = ConcurrentHashMap<String, Int>()
    private var clock = 0L

    private suspend fun frame() {
        clock = withFrameNanos { it }
    }

    private suspend fun frames(ms: Long) {
        if (clock == 0L) clock = withFrameNanos { it }
        val target = clock + ms * 1_000_000
        while (clock < target) clock = withFrameNanos { it }
    }

    // HotSpot's own counts for this thread: the bytes it has allocated and the CPU it has used.
    // Reached by reflection: a unit test compiles against android.jar, which has no
    // java.lang.management.
    private val threads = Class.forName("java.lang.management.ManagementFactory").getMethod("getThreadMXBean").invoke(null)
    private val allocatedBytes = Class.forName("com.sun.management.ThreadMXBean").getMethod("getCurrentThreadAllocatedBytes")
    private val cpuTime = Class.forName("java.lang.management.ThreadMXBean").getMethod("getCurrentThreadCpuTime")

    private fun allocated(): Long = allocatedBytes.invoke(threads) as Long

    private fun cpu(): Long = cpuTime.invoke(threads) as Long

    /**
     * The state of the one vertically scrolling list on screen. The screen keeps it to itself, so it
     * is found where its scrolling modifier holds it: the semantics' own scroll actions animate, and
     * a fling is measured a frame at a time.
     */
    private fun list(view: ComposeView): LazyListState {
        val root = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.unmergedRootSemanticsNode
        fun find(n: SemanticsNode): SemanticsNode? {
            if (n.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) != null) return n
            for (c in n.children) find(c)?.let { return it }
            return null
        }
        val node = find(root) ?: error("no vertical list on screen")
        for (info in node.layoutInfo.getModifierInfo()) {
            var c: Class<*>? = info.modifier.javaClass
            while (c != null) {
                for (f in c.declaredFields) {
                    if (f.type.isPrimitive) continue
                    f.isAccessible = true
                    (f.get(info.modifier) as? LazyListState)?.let { return it }
                }
                c = c.superclass
            }
        }
        error("no LazyListState under the list")
    }

    /** What one pass of the fling cost, summed over its frames. */
    private class Pass(val frames: Int, val rows: Int, val scrollNs: Long, val scrollBytes: Long, val drawNs: Long, val drawBytes: Long) {
        override fun toString(): String =
            "rows entered ${"%.2f".format(rows.toDouble() / frames)}/frame; " +
                "compose+measure ${"%.1f".format(scrollNs / 1e3 / rows)} µs and ${scrollBytes / rows} B per entering row; " +
                "draw ${"%.3f".format(drawNs / 1e6 / frames)} ms and ${drawBytes / frames} B per frame"
    }

    @Test
    fun aFastFlingThroughEverySong() {
        val asked = System.getenv("HARKEN_FLING_BENCH")
        assumeTrue("set HARKEN_FLING_BENCH to run the fling's measurement", asked != null)
        val n = asked!!.toIntOrNull() ?: 2000
        val server = "https://music.example.com"
        val peer = alone()
        repeat(n) { peer.addSong(song(it)) }
        // Every album's cover on the disk already, where `Covers` keeps the originals.
        val png = java.util.Base64.getDecoder().decode(PNG)
        val dirs = setOf(File(paparazzi.context.cacheDir, "covers"), File(paparazzi.context.applicationContext.cacheDir, "covers"))
        for (d in dirs) {
            d.mkdirs()
            for (a in 0..(n - 1) / 10) File(d, "%08x".format(Art.hash(Media.url(server, "covers/$a.png")))).writeBytes(png)
        }
        val session = ArkSession.inline(peer, server, testLogin("library", roles = listOf("library")))
        val rowsPerFrame = 3
        val passes = mutableListOf<Pass>()
        // Each frame drawn once more, on a software canvas, timed: layoutlib's own render is not
        // the app's, and a canvas that is not hardware runs every layer's drawing again rather than
        // replaying what it recorded.
        val canvas = Canvas(Bitmap.createBitmap(1080, 2340, Bitmap.Config.ARGB_8888))
        Recompositions.sink = { what -> counts.merge(what, 1, Int::plus) }
        val view = ComposeView(paparazzi.context)
        try {
            view.setContent {
                val owner = LocalLifecycleOwner.current
                val back =
                    object : OnBackPressedDispatcherOwner {
                        override val onBackPressedDispatcher = OnBackPressedDispatcher()
                        override val lifecycle = owner.lifecycle
                    }
                val store = remember { object : ViewModelStoreOwner { override val viewModelStore = ViewModelStore() } }
                CompositionLocalProvider(LocalViewModelStoreOwner provides store, LocalOnBackPressedDispatcherOwner provides back) {
                    HarkenApp(session, playbackOf = { c, s -> Playback(c, s, connect = false) }, start = LibraryTab.Songs)
                }
                LaunchedEffect(Unit) {
                    frames(500)
                    val step = 64 * view.resources.displayMetrics.density * rowsPerFrame
                    val length = n / rowsPerFrame - 8
                    repeat(6) {
                        val state = list(view)
                        state.scrollToItem(0)
                        frames(300)
                        counts.clear()
                        var scrollNs = 0L
                        var scrollBytes = 0L
                        var drawNs = 0L
                        var drawBytes = 0L
                        frame()
                        repeat(length) {
                            // The scroll composes, measures and places the rows it brings in, there
                            // and then; the frame after it draws them.
                            val a0 = allocated()
                            val t0 = cpu()
                            state.dispatchRawDelta(step)
                            scrollNs += cpu() - t0
                            scrollBytes += allocated() - a0
                            frame()
                            val d0 = allocated()
                            val t1 = cpu()
                            view.draw(canvas)
                            drawNs += cpu() - t1
                            drawBytes += allocated() - d0
                        }
                        passes += Pass(length, counts["TrackRow"] ?: 0, scrollNs, scrollBytes, drawNs, drawBytes)
                    }
                }
            }
            paparazzi.gif(view, "fling", 0L, (6 * (n / rowsPerFrame + 20) + 60) * 1000L / 60, 60)
        } finally {
            Recompositions.sink = null
            session.close()
            peer.peer.close()
            for (d in dirs) d.deleteRecursively()
        }
        passes.forEachIndexed { i, p -> println("fling over $n songs, pass ${i + 1}${if (i < 4) " (warming)" else ""}: $p") }
        assertEquals("passes", 6, passes.size)
        assertTrue("rows entered while flinging", passes.all { it.rows > 0 })
    }

    /**
     * Search results arrive with their covers already being decoded: a fling straight through a fresh
     * search, from its first row to its last, sends no row looking for its cover. A row that has to
     * look draws the derived square and its picture lands after it — on a phone, a read and a decode
     * of a real cover later, which a fast fling brings to the middle of the screen.
     */
    @Test
    fun searchResultsArriveWithTheirCoversDecoded() {
        val n = 300
        val server = "https://music.example.com"
        val peer = alone()
        repeat(n) { peer.addSong(song(it)) }
        val png = java.util.Base64.getDecoder().decode(PNG)
        val dirs = setOf(File(paparazzi.context.cacheDir, "covers"), File(paparazzi.context.applicationContext.cacheDir, "covers"))
        for (d in dirs) {
            d.mkdirs()
            for (a in 0..(n - 1) / 10) File(d, "%08x".format(Art.hash(Media.url(server, "covers/$a.png")))).writeBytes(png)
        }
        val session = ArkSession.inline(peer, server, testLogin("library", roles = listOf("library")))
        var covers: Covers? = null
        var looked = -1
        var rows = -1
        var reached = false
        Recompositions.sink = { what -> counts.merge(what, 1, Int::plus) }
        val view = ComposeView(paparazzi.context)
        try {
            view.setContent {
                val owner = LocalLifecycleOwner.current
                val back =
                    remember {
                        object : OnBackPressedDispatcherOwner {
                            override val onBackPressedDispatcher = OnBackPressedDispatcher()
                            override val lifecycle = owner.lifecycle
                        }
                    }
                val store = remember { object : ViewModelStoreOwner { override val viewModelStore = ViewModelStore() } }
                CompositionLocalProvider(LocalViewModelStoreOwner provides store, LocalOnBackPressedDispatcherOwner provides back) {
                    HarkenApp(session, playbackOf = { c, s -> Playback(c, s, connect = false) }, coversOf = { c, s -> Covers(c, s).also { covers = it } })
                }
                LaunchedEffect(Unit) {
                    frames(500)
                    val root = (view.getChildAt(0) as ViewRootForTest).semanticsOwner
                    fun all(x: SemanticsNode): Sequence<SemanticsNode> = sequenceOf(x) + x.children.asSequence().flatMap { all(it) }
                    all(root.rootSemanticsNode).first { it.config.getOrNull(SemanticsProperties.Text)?.any { t -> t.text == "Search" } == true && it.config.getOrNull(SemanticsActions.OnClick) != null }
                        .config[SemanticsActions.OnClick].action!!.invoke()
                    frames(300)
                    all(root.unmergedRootSemanticsNode).first { it.config.getOrNull(SemanticsActions.SetText) != null }.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString("1"))
                    // The answer, and as long as it takes to decode what it draws: real time, on the
                    // covers' own threads.
                    frames(300)
                    var waited = 0
                    while (covers!!.warming.get() > 0 && waited++ < 400) {
                        Thread.sleep(5)
                        frame()
                    }
                    val before = covers!!.launches.get()
                    counts.clear()
                    val state = list(view)
                    val step = 64 * view.resources.displayMetrics.density * 3
                    var i = 0
                    while (state.canScrollForward && i++ < 200) {
                        state.dispatchRawDelta(step)
                        frame()
                    }
                    reached = !state.canScrollForward
                    looked = covers!!.launches.get() - before
                    rows = counts["TrackRow"] ?: 0
                }
            }
            paparazzi.gif(view, "search", 0L, 6_000L, 60)
        } finally {
            Recompositions.sink = null
            session.close()
            peer.peer.close()
            for (d in dirs) d.deleteRecursively()
        }
        println("search fling: $rows rows entered, $looked went looking for their cover")
        assertTrue("flung to the end", reached)
        assertTrue("rows entered: $rows", rows > 20)
        assertEquals("rows that went looking for a cover", 0, looked)
    }

    /**
     * A cover already decoded is drawn without a coroutine to find it: five rows of an album whose
     * cover is in memory launch nothing, and three of one whose cover is only on the disk launch
     * one each, which share one read.
     */
    @Test
    fun aCoverAlreadyDecodedLaunchesNothing() {
        val server = "https://music.example.com"
        val covers = Covers(paparazzi.context, server)
        val dir = File(paparazzi.context.cacheDir, "covers")
        dir.mkdirs()
        val png = java.util.Base64.getDecoder().decode(PNG)
        val files = listOf("covers/in-memory.png", "covers/on-disk.png").map { File(dir, "%08x".format(Art.hash(covers.urlOf(it)!!))) }
        try {
            for (f in files) f.writeBytes(png)
            val decoded = runBlocking { covers.load(covers.urlOf("covers/in-memory.png")!!, 48.dp) }
            assertNotNull("the cover decoded", decoded)
            assertEquals(1, covers.reads.get())
            val view = ComposeView(paparazzi.context)
            view.setContent {
                CompositionLocalProvider(LocalCoverLoader provides covers) {
                    Column {
                        repeat(5) { Cover("one", "covers/in-memory.png", 48.dp, Modifier.size(48.dp)) }
                        repeat(3) { Cover("two", "covers/on-disk.png", 48.dp, Modifier.size(48.dp)) }
                    }
                }
            }
            paparazzi.gif(view, "covers", 0L, 500L, 20)
            assertEquals("rows that went looking for a cover", 3, covers.launches.get())
            assertEquals("disk reads", 2, covers.reads.get())
        } finally {
            for (f in files) f.delete()
        }
    }

    private companion object {
        /** A 64×64 PNG, one colour, eight bits a channel: one layoutlib's decoder reads. */
        const val PNG =
            "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAZUlEQVR42u3QQREAAAQAMMHklEkrcjh7rMCiK+ezECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAu5bKKKSaDEgnvMAAAAASUVORK5CYII="
    }
}
