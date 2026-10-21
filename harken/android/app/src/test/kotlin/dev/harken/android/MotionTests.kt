// The motion, driven the way a person drives it, on Paparazzi's frame clock:
// a page opened from its card grows out of it, the back gesture seeks it
// back toward the card and a cancelled one puts it back, a page pulled down
// from its top goes back and one pulled a little does not, and a tab
// arrives with no frame of anything in between. The player's bar is dragged
// up under the finger and settles to the nearer end, a sideways drag on it
// skips and leaves it where it is, and the full player, collapsed, takes no
// touch. Pages are the real `HarkenApp` over a peer holding the demo's
// songs; the player is `Chrome` over a page that counts its taps.
//
// With HARKEN_MOTION_FRAMES set to a directory, the frames worth looking at
// are written there as PNGs: a page opening at 0, 25, 50, 75 and 100% of
// the way, the back gesture at 0.3 and 0.6, the player part-way up.
package dev.harken.android

import dev.harken.android.gen.AddSong
import dev.harken.android.gen.Harken
import dev.harken.android.gen.CreatePlaylist
import android.view.MotionEvent
import android.view.View
import androidx.activity.BackEventCompat
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.harken.android.onboarding.ArkSession
import dev.harken.android.ui.Origin
import dev.harken.android.ui.PageMotion
import dev.harken.android.ui.Springs
import java.io.File
import java.lang.reflect.Proxy
import kotlin.math.abs
import kotlinx.coroutines.launch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

private const val FPS = 60

class MotionTests {
    private val shots = Shots()

    @get:Rule
    val paparazzi =
        Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true, snapshotHandler = shots)

    /**
     * The frames a test asks for, and nothing else. `keep` names the next frame drawn; while `series`
     * is set, every frame is written under it with `meta`'s number, for picking afterwards.
     */
    private class Shots : SnapshotHandler {
        val out: File? = System.getenv("HARKEN_MOTION_FRAMES")?.let(::File)
        var keep: String? = null
        var series: String? = null
        var meta: () -> Float = { 0f }
        val written = mutableListOf<Pair<String, Float>>()
        private var n = 0

        override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
            Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, method, args ->
                if (method.name == "handle") {
                    val image = args!![0]
                    keep?.let { name ->
                        keep = null
                        write(image, name)
                    }
                    series?.let { s ->
                        val v = meta()
                        val name = "%s-%03d".format(s, n++)
                        written += name to v
                        write(image, name)
                    }
                }
                null
            } as SnapshotHandler.FrameHandler

        private fun write(image: Any, name: String) {
            val dir = out ?: return
            dir.mkdirs()
            Class.forName("javax.imageio.ImageIO")
                .getMethod("write", Class.forName("java.awt.image.RenderedImage"), String::class.java, File::class.java)
                .invoke(null, image, "png", File(dir, "$name.png"))
        }

        /** Of a series, the frames nearest each of `at`, renamed `<as>-<percent>`; the rest deleted. */
        fun pick(series: String, at: List<Float>, `as`: String) {
            val dir = out ?: return
            val mine = written.filter { it.first.startsWith("$series-") }
            for (t in at) {
                // The nearest, and of equals the latest: a series may start where it ends.
                val best = mine.asReversed().minByOrNull { abs(it.second - t) } ?: continue
                File(dir, "${best.first}.png").copyTo(File(dir, "$`as`-${(t * 100).toInt()}.png"), overwrite = true)
            }
            for ((name, _) in mine) File(dir, "$name.png").delete()
        }

        override fun close() {}
    }

    /** The demo's songs, authored through `add_song` as the server's scanner would. */
    private fun demo(): Harken {
        val peer = alone()
        for (t in Demo.tracks) {
            val d = Demo.details.getValue(t.id)
            peer.addSong(
                AddSong(
                    title = t.title,
                    artist = t.creator,
                    album = d.album,
                    durationMs = t.durationMs,
                    file = t.file,
                    track = d.track,
                    part = d.part,
                    catalogue = "",
                    performer = d.performer,
                    bpm = 0,
                    albumArt = "",
                    artistArt = "",
                    disc = 1,
                    workTitle = "",
                    movementNo = 0,
                ),
            )
        }
        return peer
    }

    private var clock = 0L

    private suspend fun frame() {
        clock = withFrameNanos { it }
    }

    private suspend fun frames(ms: Long) {
        if (clock == 0L) clock = withFrameNanos { it }
        val target = clock + ms * 1_000_000
        while (clock < target) clock = withFrameNanos { it }
    }

    private fun root(view: ComposeView): SemanticsNode = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.rootSemanticsNode

    private fun all(n: SemanticsNode): Sequence<SemanticsNode> = sequenceOf(n) + n.children.asSequence().flatMap { all(it) }

    private fun texts(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.Text)?.map { it.text }.orEmpty()

    private fun has(view: ComposeView, text: String): Boolean = all(root(view)).any { text in texts(it) }

    /** A tap, as a click action: what the semantics tree says the clickable with `text` on it does. */
    private fun click(view: ComposeView, text: String) {
        val node = all(root(view)).first { text in texts(it) && it.config.getOrNull(SemanticsActions.OnClick) != null }
        node.config[SemanticsActions.OnClick].action!!.invoke()
    }

    private fun described(view: ComposeView, what: String): List<SemanticsNode> =
        all(root(view)).filter { it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(what) == true }.toList()

    // A finger: events at the frame clock's time, so the velocity a drag ends with is the one it had.
    private var downAt = 0L

    private fun touch(view: View, action: Int, at: Offset) {
        val now = clock / 1_000_000
        if (action == MotionEvent.ACTION_DOWN) downAt = now
        val e = MotionEvent.obtain(downAt, now, action, at.x, at.y, 0)
        view.dispatchTouchEvent(e)
        e.recycle()
    }

    /** Down at `from`, along to `to` in `steps` frames, held still for a moment if `rest`, and up. */
    private suspend fun drag(view: View, from: Offset, to: Offset, steps: Int, rest: Boolean = true, during: () -> Unit = {}) {
        touch(view, MotionEvent.ACTION_DOWN, from)
        frame()
        for (i in 1..steps) {
            touch(view, MotionEvent.ACTION_MOVE, from + (to - from) * (i.toFloat() / steps))
            frame()
            during()
        }
        // Still for a moment: let go with nothing left of the drag's speed.
        if (rest) repeat(8) {
            touch(view, MotionEvent.ACTION_MOVE, to)
            frame()
            during()
        }
        touch(view, MotionEvent.ACTION_UP, to)
        frame()
    }

    @Composable
    private fun Host(back: OnBackPressedDispatcher, content: @Composable () -> Unit) {
        val owner = LocalLifecycleOwner.current
        val dispatcher =
            remember {
                object : OnBackPressedDispatcherOwner {
                    override val onBackPressedDispatcher = back
                    override val lifecycle = owner.lifecycle
                }
            }
        val store = remember { object : ViewModelStoreOwner { override val viewModelStore = ViewModelStore() } }
        CompositionLocalProvider(LocalViewModelStoreOwner provides store, LocalOnBackPressedDispatcherOwner provides dispatcher) { content() }
    }

    /** What the last song's menu to open can do. */
    private var menu: MenuActions? = null

    /**
     * The real app over the demo's songs, running `test`. `threaded`, its session reads on a thread
     * of its own, as a phone's does — so a page's rows arrive a moment after the page — rather than
     * at once on the frame clock's.
     */
    private fun app(test: (ComposeView, PageMotion, OnBackPressedDispatcher) -> suspend () -> Unit, ms: Long, start: LibraryTab? = null, threaded: Boolean = false) {
        val peer = demo()
        val login = testLogin("library", roles = listOf("library"))
        val session = if (threaded) ArkSession.over(peer, "https://music.example.com", login) else ArkSession.inline(peer, "https://music.example.com", login)
        val motion = PageMotion(Tab.routes)
        val back = OnBackPressedDispatcher()
        var done = false
        try {
            val view = ComposeView(paparazzi.context)
            val script = test(view, motion, back)
            view.setContent {
                Host(back) { HarkenApp(session, playbackOf = { c, s -> Playback(c, s, connect = false) }, start = start, motion = motion, onMenu = { menu = it }) }
                LaunchedEffect(Unit) {
                    frames(800)
                    script()
                    done = true
                }
            }
            paparazzi.gif(view, "motion", 0L, ms, FPS)
        } finally {
            session.close()
            peer.peer.close()
        }
        assertTrue("the script ran to its end", done)
    }

    private val start = BackEventCompat(0f, 1200f, 0f, BackEventCompat.EDGE_LEFT)

    private fun at(p: Float) = BackEventCompat(1080f * p, 1200f, p, BackEventCompat.EDGE_LEFT)

    /** The frames from now until `done` holds, as (ms since now, what `of` read): a curve to print. */
    private suspend fun curve(of: () -> Float, max: Long = 1_000L, done: (Float) -> Boolean = { false }): List<Pair<Long, Float>> {
        val t0 = clock
        val seen = mutableListOf(0L to of())
        while (clock - t0 < max * 1_000_000) {
            frame()
            seen += (clock - t0) / 1_000_000 to of()
            if (done(seen.last().second)) break
        }
        return seen
    }

    private fun table(name: String, c: List<Pair<Long, Float>>) = println("$name: " + c.joinToString(" ") { (t, v) -> "%d:%.4f".format(t, v) })

    @Test
    fun aPageGrowsOutOfItsCardAndTheBackGesturePreviewsIt() {
        var opening = emptyList<Pair<Long, Float>>()
        var opened = false
        var at30 = -1f
        var at90 = -1f
        var drawnUnder = false
        var afterCancel = -1f
        var keptAfterCancel = false
        var shrink = emptyList<Pair<Long, Float>>()
        var releasedAt = -1f
        var closedFully = false
        var gone = false
        app(
            { view, motion, back ->
                {
                    shots.meta = { motion.drawnAt }
                    shots.keep = "page-from"
                    frame()
                    shots.series = "open"
                    click(view, "Brandenburg Concertos")
                    opening = curve({ motion.drawnAt }, 600)
                    shots.series = null
                    frames(200)
                    opened = has(view, "ALBUM")

                    // The edge swipe: the page shrinks toward the limit, with Home under it.
                    back.dispatchOnBackStarted(start)
                    for (r in listOf(0.2f, 0.3f, 0.5f, 0.9f)) {
                        back.dispatchOnBackProgressed(at(r))
                        frames(100)
                        if (r == 0.3f) at30 = motion.previewDrawn
                        if (r == 0.9f) at90 = motion.previewDrawn
                        if (r != 0.3f) shots.keep = "page-back-${(r * 100).toInt()}"
                        frame()
                    }
                    drawnUnder = has(view, "Recently added")
                    back.dispatchOnBackCancelled()
                    frames(600)
                    afterCancel = motion.previewDrawn
                    keptAfterCancel = has(view, "ALBUM")

                    // Through: past the threshold, the page goes the rest of the way into its card.
                    back.dispatchOnBackStarted(start)
                    back.dispatchOnBackProgressed(at(0.5f))
                    frames(150)
                    releasedAt = motion.previewDrawn
                    shots.meta = { motion.drawnAt }
                    shots.series = "shrink"
                    back.onBackPressed()
                    shrink = curve({ motion.drawnAt }, 700) { it <= 0f }
                    closedFully = shrink.any { it.second <= 0.01f }
                    frames(300)
                    shots.series = null
                    gone = !has(view, "ALBUM") && has(view, "Recently added")
                }
            },
            8_000L,
        )
        shots.pick("open", listOf(0f, 0.25f, 0.5f, 0.75f, 1f), "page-open")
        shots.pick("shrink", listOf(0.75f, 0.5f, 0.25f, 0.05f), "page-back-done")
        table("curve page-open (ms:shown)", opening)
        table("curve page-back-done (ms:shown)", shrink)
        println("back preview at 0.3: $at30, at 0.9: $at90 (limit ${Springs.PREVIEW}); after cancel: $afterCancel; released at $releasedAt")
        assertTrue("the page opened", opened)
        assertTrue("it grew through the in-between: $opening", opening.any { it.second > 0.05f && it.second < 0.95f })
        assertEquals("and ended open", 1f, opening.last().second, 0f)
        // From the first frame the page is drawn moving: the navigation composes it a frame or two after the tap.
        val began = opening.first { it.second < 1f }.first
        val settled = opening.first { it.first > began && it.second >= 0.995f }.first - began
        assertTrue("open to within half a percent in about 250–320 ms: $settled", settled in 200L..340L)
        // The gesture previews: within the limit, and stiffer as it goes.
        assertTrue("at 0.3: $at30", at30 > 0f && at30 <= Springs.PREVIEW)
        assertTrue("at 0.9: $at90, only a little past 0.3's $at30", at90 <= Springs.PREVIEW && at90 > at30 && at90 - at30 < 0.15f * at30)
        assertTrue("and the page it goes back to is there under it", drawnUnder)
        assertEquals("a cancelled gesture springs back", 0f, afterCancel, 0f)
        assertTrue("and keeps the page", keptAfterCancel)
        assertTrue("released mid-preview: $releasedAt", releasedAt > 0f)
        assertTrue("let go past the threshold, it closes all the way into its card: $shrink", closedFully)
        assertTrue("and goes back", gone)
    }

    @Test
    fun aPagePulledDownFromItsTopGoesBack() {
        var scrolledNotPulled = true
        var pulled = 0f
        var gone = false
        var smallPull = 0f
        var kept = false
        var restored = -1f
        app(
            { view, motion, back ->
                {
                    val w = view.width.toFloat()
                    val h = view.height.toFloat()
                    click(view, "Brandenburg Concertos")
                    frames(600)
                    // Scrolled down a way: a drag down scrolls the list back before it pulls anything.
                    drag(view, Offset(w / 2, h * 0.7f), Offset(w / 2, h * 0.4f), 10)
                    frames(300)
                    drag(view, Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.45f), 6) { if (motion.previewDrawn > 0f || motion.drawnAt < 1f) scrolledNotPulled = false }
                    frames(300)
                    // Then from the top: the page previews under the finger, and far enough goes back.
                    drag(view, Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.62f), 10) { pulled = maxOf(pulled, motion.previewDrawn) }
                    frames(700)
                    gone = !has(view, "ALBUM")
                    // A little way, and let go: it springs back.
                    if (gone) click(view, "Brandenburg Concertos")
                    frames(600)
                    drag(view, Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.35f), 4) { smallPull = maxOf(smallPull, motion.previewDrawn) }
                    frames(600)
                    kept = has(view, "ALBUM")
                    restored = motion.previewDrawn
                }
            },
            9_000L,
        )
        println("pull: scrolled, not pulled $scrolledNotPulled; pulled to $pulled; small pull to $smallPull, restored to $restored")
        assertTrue("a drag down a scrolled list scrolls it and pulls nothing", scrolledNotPulled)
        assertTrue("a pull from the top previews the page: $pulled", pulled > 0f && pulled <= Springs.PREVIEW)
        assertTrue("and far enough goes back", gone)
        assertTrue("a short pull moved it too: $smallPull", smallPull > 0f)
        assertTrue("and let go, keeps the page", kept)
        assertEquals("sprung back", 0f, restored, 0f)
    }

    @Test
    fun aTabArrivesWithNoFrameInBetween() {
        var moving = -1
        var homeGone = -1
        var libraryShown = false
        var opening = -1
        app(
            { view, motion, _ ->
                {
                    motion.movingFrames = 0
                    click(view, "Library")
                    var n = 0
                    while (has(view, "Recently added") && n < 10) {
                        frame()
                        n++
                    }
                    homeGone = n
                    libraryShown = has(view, "Playlists")
                    frames(600)
                    // Between the other roots too, and back.
                    click(view, "Search")
                    frames(300)
                    click(view, "Library")
                    frames(300)
                    click(view, "Home")
                    frames(300)
                    click(view, "Library")
                    frames(300)
                    moving = motion.movingFrames + motion.tabFrames
                    // A page does move, so the count is not empty for want of counting.
                    click(view, "Albums")
                    frames(200)
                    click(view, "Brandenburg Concertos")
                    frames(600)
                    opening = motion.movingFrames
                }
            },
            6_000L,
        )
        println("tab: Home was gone from the tree after $homeGone frames, none of them drawn")
        assertEquals("frames drawn mid-move switching tabs", 0, moving)
        assertTrue("Home went: $homeGone", homeGone in 0..9)
        assertTrue("Library came", libraryShown)
        assertTrue("frames drawn mid-move opening a page: $opening", opening > 0)
    }

    /** The tab bar's item for `tab`, and whether it is the one lit. */
    private fun lit(view: ComposeView, tab: String): Boolean =
        all(root(view)).first { tab in texts(it) && it.config.getOrNull(SemanticsActions.OnClick) != null }.config.getOrNull(SemanticsProperties.Selected) == true

    @Test
    fun aTabKeepsItsOwnStack() {
        var opened = false
        var onLibrary = false
        var backOnFirstFrame = false
        var moving = -1
        var stillThere = false
        app(
            { view, motion, _ ->
                {
                    click(view, "Brandenburg Concertos")
                    frames(600)
                    opened = has(view, "ALBUM")
                    click(view, "Library")
                    frames(300)
                    onLibrary = !has(view, "ALBUM") && lit(view, "Library")
                    motion.movingFrames = 0
                    click(view, "Home")
                    frame()
                    backOnFirstFrame = has(view, "ALBUM")
                    frames(600)
                    moving = motion.movingFrames
                    stillThere = has(view, "ALBUM") && lit(view, "Home")
                }
            },
            4_000L,
        )
        println("tabs: Home's album back on the first frame $backOnFirstFrame, $moving frames drawn mid-move")
        assertTrue("the album opened", opened)
        assertTrue("Library is its own", onLibrary)
        assertTrue("Home's album is there again on the first frame", backOnFirstFrame)
        assertEquals("with no frame of a move", 0, moving)
        assertTrue("and stays", stillThere)
    }

    @Test
    fun reselectingATabOnAPageGoesBackToItsRoot() {
        var shrink = emptyList<Pair<Long, Float>>()
        var atRoot = false
        app(
            { view, motion, _ ->
                {
                    click(view, "Brandenburg Concertos")
                    frames(600)
                    motion.movingFrames = 0
                    click(view, "Home")
                    shrink = curve({ motion.drawnAt }, 700) { it <= 0f }
                    frames(300)
                    atRoot = !has(view, "ALBUM") && has(view, "Recently added") && lit(view, "Home")
                }
            },
            3_000L,
        )
        table("curve reselect (ms:shown)", shrink)
        assertTrue("the page shrank back through the in-between: $shrink", shrink.any { it.second > 0.05f && it.second < 0.95f })
        assertTrue("into its card", shrink.any { it.second <= 0.01f })
        assertTrue("and Home's root is shown", atRoot)
    }

    @Test
    fun reselectingATabAtItsRootScrollsToTheTop() {
        var down: Pair<Int, Int>? = null
        var top: Pair<Int, Int>? = null
        app(
            { view, _, _ ->
                {
                    // Every song, at the Library's root: far down it, then the Library tapped again.
                    val st = listWith(view) { n -> "Alla Hornpipe" in texts(n) }!!
                    st.scrollToItem(st.layoutInfo.totalItemsCount - 1)
                    frames(300)
                    down = st.firstVisibleItemIndex to st.firstVisibleItemScrollOffset
                    click(view, "Library")
                    frames(800)
                    top = st.firstVisibleItemIndex to st.firstVisibleItemScrollOffset
                }
            },
            3_000L,
            start = LibraryTab.Songs,
        )
        println("reselect at the root: scrolled to $down, then $top")
        assertTrue("scrolled down: $down", (down?.first ?: 0) > 0)
        assertEquals("and back at the top", 0 to 0, top)
    }

    @Test
    fun backAtTheLibrarysRootPreviewsAndLandsOnHome() {
        var taken = false
        var at30 = -1f
        var at90 = -1f
        var homeUnder = false
        var cancelled = -1f
        var keptLibrary = false
        var landed = false
        var gone = emptyList<Pair<Long, Float>>()
        app(
            { view, motion, back ->
                {
                    click(view, "Library")
                    frames(300)
                    taken = back.hasEnabledCallbacks()
                    // A preview, cancelled: it springs back, and the Library stays.
                    back.dispatchOnBackStarted(start)
                    back.dispatchOnBackProgressed(at(0.5f))
                    frames(150)
                    back.dispatchOnBackCancelled()
                    frames(500)
                    cancelled = motion.tabDrawnScale
                    keptLibrary = lit(view, "Library") && !motion.tabBacking
                    // Then through.
                    back.dispatchOnBackStarted(start)
                    for (r in listOf(0.3f, 0.6f, 0.9f)) {
                        back.dispatchOnBackProgressed(at(r))
                        frames(100)
                        if (r == 0.3f) at30 = motion.tabDrawnScale
                        if (r == 0.9f) at90 = motion.tabDrawnScale
                        shots.keep = "tab-back-${(r * 100).toInt()}"
                        frame()
                    }
                    homeUnder = has(view, "Recently added")
                    shots.meta = { 1f - motion.tabDrawnScale }
                    shots.series = "tab-back-done"
                    back.onBackPressed()
                    gone = curve({ motion.tabDrawnScale }, 500) { !motion.tabBacking }
                    shots.series = null
                    frames(300)
                    landed = lit(view, "Home") && has(view, "Recently added") && !motion.tabBacking
                }
            },
            4_000L,
        )
        shots.pick("tab-back-done", listOf(0.11f, 0.13f, 0.15f), "tab-back-done")
        table("curve tab-back-done (ms:scale)", gone)
        println("tab back: taken $taken; scale ${at30} at 0.3, ${at90} at 0.9; cancelled back to $cancelled; landed on Home $landed")
        assertTrue("back at the Library's root is taken", taken)
        assertEquals("a cancel springs back", 1f, cancelled, 0f)
        assertTrue("and keeps the Library", keptLibrary)
        assertTrue("at 0.3 it has shrunk: $at30", at30 < 1f && at30 > 0.9f)
        // The preview's limit: at most a tenth of its size, all but reached by 0.9.
        assertTrue("at 0.9 it is at about 0.9: $at90", at90 >= 0.9f - 1e-4f && at90 < 0.901f && at90 < at30)
        assertTrue("with Home under it", homeUnder)
        assertTrue("let go, it went: $gone", gone.isNotEmpty() && gone.last().first <= 300)
        assertTrue("and Home is the tab", landed)
    }

    @Test
    fun atHomesRootNoBackIsTaken() {
        var atHome = true
        var onPage = false
        app(
            { view, _, back ->
                {
                    atHome = back.hasEnabledCallbacks()
                    click(view, "Brandenburg Concertos")
                    frames(600)
                    onPage = back.hasEnabledCallbacks()
                }
            },
            2_500L,
        )
        assertFalse("at Home's root nothing takes back: the system's own runs", atHome)
        assertTrue("on a page, back is taken", onPage)
    }

    /** The vertical list with a node `marker` holds somewhere under it, as its state: the screen keeps it to itself. */
    private fun listWith(view: ComposeView, marker: (SemanticsNode) -> Boolean): LazyListState? {
        val root = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.unmergedRootSemanticsNode
        val node = all(root).firstOrNull { n -> n.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) != null && all(n).any(marker) } ?: return null
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
        return null
    }

    /** A row's menu button, of the rows on screen. */
    private fun more(view: ComposeView): SemanticsNode =
        all(root(view)).first { n -> n.config.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith("More for ") } == true && n.boundsInRoot.top > 0f }

    /** Back, and the place the list `marker` finds is at on the first frame it is there again, and once settled. */
    private suspend fun backTo(view: ComposeView, b: OnBackPressedDispatcher, marker: (SemanticsNode) -> Boolean): Pair<Pair<Int, Int>?, Pair<Int, Int>?> {
        b.onBackPressed()
        var first: Pair<Int, Int>? = null
        for (i in 0 until 60) {
            frame()
            val st = listWith(view, marker) ?: continue
            first = st.firstVisibleItemIndex to st.firstVisibleItemScrollOffset
            break
        }
        frames(700)
        val after = listWith(view, marker)?.let { it.firstVisibleItemIndex to it.firstVisibleItemScrollOffset }
        return first to after
    }

    @Test
    fun theSearchKeepsItsPlaceAcrossAPage() {
        var left: Pair<Int, Int>? = null
        var opened = false
        var back: Pair<Pair<Int, Int>?, Pair<Int, Int>?>? = null
        val field = { n: SemanticsNode -> n.config.getOrNull(SemanticsActions.SetText) != null }
        // The results' last heading: still there scrolled down, where the field is not.
        val results = { n: SemanticsNode -> "Artists" in texts(n) }
        app(
            { view, _, b ->
                {
                    click(view, "Search")
                    frames(300)
                    all(root(view)).first(field).config[SemanticsActions.SetText].action!!.invoke(AnnotatedString("Bach"))
                    frames(600)
                    val st = listWith(view, field)!!
                    // Down to the albums, past every song.
                    st.scrollToItem(st.layoutInfo.totalItemsCount - 1)
                    frames(300)
                    left = st.firstVisibleItemIndex to st.firstVisibleItemScrollOffset
                    // To an album, from a row's menu (a card's tap also notes the search on the
                    // phone, which layoutlib's preferences cannot hold).
                    more(view).config[SemanticsActions.OnClick].action!!.invoke()
                    frames(300)
                    menu!!.goTo(Page.ForAlbum("Brandenburg Concertos"))
                    frames(700)
                    opened = has(view, "ALBUM")
                    back = backTo(view, b, results)
                }
            },
            5_000L,
        )
        println("search: left at $left; back at ${back?.first} on its first frame, ${back?.second} settled")
        assertTrue("scrolled down: $left", (left?.first ?: 0) > 3)
        assertTrue("the album opened", opened)
        assertEquals("on its first frame back, where it was left", left, back?.first)
        assertEquals("and still there", left, back?.second)
    }

    @Test
    fun aPageKeepsItsPlaceAcrossAPageItOpened() {
        var left: Pair<Int, Int>? = null
        var opened = false
        var back: Pair<Pair<Int, Int>?, Pair<Int, Int>?>? = null
        // A part's heading: the album page draws them, and the composer's page, with the same songs, does not.
        val album = { n: SemanticsNode -> texts(n).any { it.startsWith("Suite No.") } }
        app(
            { view, _, b ->
                {
                    click(view, "Water Music")
                    frames(700)
                    val st = listWith(view, album)!!
                    st.scrollToItem(8, 40)
                    frames(300)
                    left = st.firstVisibleItemIndex to st.firstVisibleItemScrollOffset
                    // A row's menu, to the album's composer.
                    more(view).config[SemanticsActions.OnClick].action!!.invoke()
                    frames(300)
                    menu!!.goTo(Page.ForArtist("George Frideric Handel"))
                    frames(700)
                    opened = has(view, "ARTIST")
                    back = backTo(view, b, album)
                }
            },
            5_000L,
            start = LibraryTab.Albums,
            threaded = true,
        )
        println("album: left at $left; back at ${back?.first} on its first frame, ${back?.second} settled")
        assertTrue("scrolled down: $left", (left?.first ?: 0) >= 4)
        assertTrue("the artist opened", opened)
        assertEquals("on its first frame back, where it was left", left, back?.first)
        assertEquals("and still there", left, back?.second)
    }

    /** The row with `title` on screen, where its menu button is, and its own bounds. */
    private fun row(view: ComposeView, title: String): Pair<SemanticsNode, androidx.compose.ui.geometry.Rect> {
        val more = described(view, "More for $title").first()
        val row = all(root(view)).first { title in texts(it) && it.config.getOrNull(SemanticsActions.OnClick) != null }
        return more to row.boundsInRoot
    }

    @Test
    fun aMenuOpensItsPageOutOfTheRow() {
        var album: Origin? = null
        var albumExpected: androidx.compose.ui.geometry.Rect? = null
        var albumRose = -1
        var artist: Origin? = null
        var artistExpected: androidx.compose.ui.geometry.Rect? = null
        var settings: Origin? = Origin(androidx.compose.ui.geometry.Rect.Zero, 0f)
        var settingsRose = -1
        val d = paparazzi.context.resources.displayMetrics.density
        app(
            { view, motion, b ->
                {
                    // Every song: a row's menu, then its album, out of the row's cover.
                    val (more, bounds) = row(view, "Alla Hornpipe")
                    albumExpected = androidx.compose.ui.geometry.Rect(bounds.left + 16 * d, bounds.top + (bounds.height - 48 * d) / 2, bounds.left + 64 * d, bounds.top + (bounds.height + 48 * d) / 2)
                    more.config[SemanticsActions.OnClick].action!!.invoke()
                    frames(300)
                    motion.rose = 0
                    shots.meta = { motion.drawnAt }
                    shots.series = "menu-album"
                    // The sheet is a window of its own, which layoutlib does not raise: its entry, by hand.
                    menu!!.goTo(Page.ForAlbum("Water Music"))
                    frames(500)
                    shots.series = null
                    album = motion.taken
                    albumRose = motion.rose
                    b.onBackPressed()
                    frames(600)
                    // Its artist, out of the row itself.
                    val (more2, bounds2) = row(view, "Alla Hornpipe")
                    artistExpected = bounds2
                    more2.config[SemanticsActions.OnClick].action!!.invoke()
                    frames(300)
                    shots.series = "menu-artist"
                    menu!!.goTo(Page.ForArtist("George Frideric Handel"))
                    frames(500)
                    shots.series = null
                    artist = motion.taken
                    b.onBackPressed()
                    frames(600)
                    // Settings has nothing to grow from: it rises.
                    click(view, "Home")
                    frames(300)
                    motion.rose = 0
                    shots.series = "rise"
                    described(view, "Settings").first().config[SemanticsActions.OnClick].action!!.invoke()
                    frames(500)
                    shots.series = null
                    settings = motion.taken
                    settingsRose = motion.rose
                }
            },
            8_000L,
            start = LibraryTab.Songs,
        )
        shots.pick("menu-album", listOf(0.05f, 0.25f, 0.5f, 0.75f, 1f), "menu-album")
        shots.pick("menu-artist", listOf(0.05f, 0.25f, 0.5f, 0.75f, 1f), "menu-artist")
        shots.pick("rise", listOf(0.05f, 0.25f, 0.5f, 0.75f, 1f), "rise")
        println("menu: album grew from $album (the cover $albumExpected), artist from $artist (the row $artistExpected), settings from $settings rising $settingsRose frames")
        assertTrue("the album grew out of the row's cover: $album against $albumExpected", album != null && album!!.art && near(album!!.bounds, albumExpected!!))
        assertEquals("and did not rise", 0, albumRose)
        assertTrue("the artist grew out of the row: $artist against $artistExpected", artist != null && !artist!!.art && near(artist!!.bounds, artistExpected!!))
        assertEquals("settings had nothing to grow from", null, settings)
        assertTrue("so it rose: $settingsRose", settingsRose > 0)
    }

    private fun near(a: androidx.compose.ui.geometry.Rect, b: androidx.compose.ui.geometry.Rect) =
        abs(a.left - b.left) <= 1f && abs(a.top - b.top) <= 1f && abs(a.right - b.right) <= 1f && abs(a.bottom - b.bottom) <= 1f

    // The player -------------------------------------------------------------

    /** What the player's controls were asked to do. */
    private class Asked(val opening: () -> Unit) : Controls {
        var toggles = 0
        var nexts = 0
        var previouses = 0
        var mores = 0
        var opens = 0

        override fun open() {
            opens++
            opening()
        }

        override fun more() {
            mores++
        }

        override fun toggle() {
            toggles++
        }

        override fun next() {
            nexts++
        }

        override fun previous() {
            previouses++
        }
    }

    private val playing =
        PlayerState(now = Demo.playing(Demo.tracks[0]), playing = true, durationMs = Demo.tracks[0].durationMs, upNext = Demo.tracks.subList(1, 5).map(Demo::playing))

    private val playerBack = OnBackPressedDispatcher()

    private fun player(test: (ComposeView, () -> PlayerSheet, Asked, () -> Int) -> suspend () -> Unit, ms: Long) {
        var sheet: PlayerSheet? = null
        var taps = 0
        var done = false
        val view = ComposeView(paparazzi.context)
        lateinit var asked: Asked
        view.setContent {
            Host(playerBack) {
                HarkenTheme(dark = false) {
                    val s = rememberPlayerSheet().also { sheet = it }
                    val scope = rememberCoroutineScope()
                    asked = remember { Asked { scope.launch { s.open() } } }
                    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
                        Chrome(Tab.Home, playing, asked, onTab = {}, sheet = s) { Box(Modifier.fillMaxSize().clickable { taps++ }) }
                    }
                }
            }
            LaunchedEffect(Unit) {
                frames(500)
                test(view, { sheet!! }, asked) { taps }()
                done = true
            }
        }
        paparazzi.gif(view, "player", 0L, ms, FPS)
        assertTrue("the script ran to its end", done)
    }

    // Where the bar is: above the tab bar, which is 80 dp with no system bar under it.
    private fun bar(view: View): Offset {
        val d = view.resources.displayMetrics.density
        return Offset(view.width * 0.45f, view.height - (80f + 4f + 29f) * d)
    }

    @Test
    fun theBarFollowsTheFingerAndSettlesToTheNearerEnd() {
        var short = -1f
        var shortSettled = -1f
        var far = -1f
        var farSettled = -1f
        var down = -1f
        player(
            { view, sheet, _, _ ->
                {
                    val b = bar(view)
                    val travel = sheet().travel
                    drag(view, b, b - Offset(0f, travel * 0.3f), 10) { short = sheet().p }
                    frames(800)
                    shortSettled = sheet().p
                    drag(view, b, b - Offset(0f, travel * 0.7f), 10) { far = sheet().p }
                    frames(800)
                    farSettled = sheet().p
                    // And down from anywhere on the full player.
                    drag(view, Offset(view.width / 2f, view.height * 0.3f), Offset(view.width / 2f, view.height * 0.9f), 10)
                    frames(800)
                    down = sheet().p
                }
            },
            6_000L,
        )
        println("bar: held at $short settled to $shortSettled; held at $far settled to $farSettled; dragged down to $down")
        assertTrue("held part-way: $short", short > 0.2f && short < 0.4f)
        assertEquals("short of half, back to the bar", 0f, shortSettled, 0f)
        assertTrue("held most of the way: $far", far > 0.6f && far < 0.8f)
        assertEquals("past half, up to full", 1f, farSettled, 0f)
        assertEquals("dragged down, back to the bar", 0f, down, 0f)
    }

    @Test
    fun aSidewaysDragOnTheBarSkipsAndStaysDown() {
        var highest = 0f
        var nexts = -1
        var previouses = -1
        player(
            { view, sheet, asked, _ ->
                {
                    val b = bar(view)
                    val right = Offset(view.width * 0.7f, b.y)
                    val left = Offset(view.width * 0.2f, b.y)
                    drag(view, right, left, 6, rest = false) { highest = maxOf(highest, sheet().p) }
                    frames(600)
                    nexts = asked.nexts
                    drag(view, left, right, 6, rest = false) { highest = maxOf(highest, sheet().p) }
                    frames(600)
                    previouses = asked.previouses
                }
            },
            4_000L,
        )
        assertEquals("left skipped ahead", 1, nexts)
        assertEquals("right went back", 1, previouses)
        assertEquals("and the player did not move up", 0f, highest, 0f)
    }

    @Test
    fun theCollapsedPlayerTakesNoTouch() {
        var where = Offset.Zero
        var composedCollapsed = true
        var toggles = -1
        var taps = -1
        var openToggles = -1
        var barOpens = -1
        player(
            { view, sheet, asked, pageTaps ->
                {
                    val s = sheet()
                    s.open()
                    frames(300)
                    // The full player's menu, at its top: there, open.
                    where = described(view, "More").single().boundsInRoot.center
                    touch(view, MotionEvent.ACTION_DOWN, where)
                    frame()
                    touch(view, MotionEvent.ACTION_UP, where)
                    frames(200)
                    openToggles = asked.mores
                    s.close()
                    frames(500)
                    composedCollapsed = described(view, "Close the player").isNotEmpty()
                    // The same place, collapsed: the page's.
                    touch(view, MotionEvent.ACTION_DOWN, where)
                    frame()
                    touch(view, MotionEvent.ACTION_UP, where)
                    frames(200)
                    toggles = asked.mores - openToggles
                    taps = pageTaps()
                    // And the bar, where the full face's controls would lie over it: the bar's.
                    val before = asked.opens
                    touch(view, MotionEvent.ACTION_DOWN, bar(view))
                    frame()
                    touch(view, MotionEvent.ACTION_UP, bar(view))
                    frames(200)
                    barOpens = asked.opens - before
                }
            },
            4_000L,
        )
        assertEquals("open, the menu button answers", 1, openToggles)
        assertEquals("collapsed, a tap where its button was does not reach it", 0, toggles)
        assertEquals("it reaches the page", 1, taps)
        assertEquals("and a tap on the bar opens it", 1, barOpens)
        assertFalse("and the full player is not there at all", composedCollapsed)
    }

    @Test
    fun theBarOpensOnATapAlongTheSamePath() {
        val seen = mutableListOf<Float>()
        player(
            { view, sheet, _, _ ->
                {
                    val b = bar(view)
                    shots.meta = { sheet().p }
                    shots.series = "player"
                    touch(view, MotionEvent.ACTION_DOWN, b)
                    frame()
                    touch(view, MotionEvent.ACTION_UP, b)
                    repeat(30) {
                        frame()
                        seen += sheet().p
                    }
                    shots.series = null
                }
            },
            3_000L,
        )
        shots.pick("player", listOf(0f, 0.25f, 0.5f, 0.75f, 1f), "player-open")
        println("tap: p went $seen")
        assertTrue("through the in-between: $seen", seen.any { it > 0.1f && it < 0.9f })
        assertEquals("to full", 1f, seen.last(), 0.001f)
    }

    /** When `c` first reads `at` or less, in ms: when a settle toward 0 lands. */
    private fun landed(c: List<Pair<Long, Float>>, at: Float = 0f): Long = c.firstOrNull { it.second <= at }?.first ?: Long.MAX_VALUE

    @Test
    fun aSlowReleaseLandsWithinTheCapAndAFlickSooner() {
        var slow = emptyList<Pair<Long, Float>>()
        var fast = emptyList<Pair<Long, Float>>()
        var slowFrom = -1f
        var fastFrom = -1f
        player(
            { view, sheet, _, _ ->
                {
                    val s = sheet()
                    s.open()
                    frames(500)
                    val from = Offset(view.width / 2f, view.height * 0.3f)
                    val to = from + Offset(0f, s.travel * 0.6f)
                    // Drawn down to 60% of the way to the bar, held still, and let go: the nearer end.
                    shots.meta = { sheet().p }
                    shots.series = "settle-slow"
                    drag(view, from, to, 20) { slowFrom = sheet().p }
                    slow = curve({ sheet().p }, 1_000) { it <= 0f }
                    shots.series = null
                    frames(300)
                    s.open()
                    frames(500)
                    // The same place, flicked: let go still moving.
                    shots.series = "settle-fast"
                    drag(view, from, to, 4, rest = false) { fastFrom = sheet().p }
                    fast = curve({ sheet().p }, 1_000) { it <= 0f }
                    shots.series = null
                }
            },
            5_000L,
        )
        shots.pick("settle-slow", listOf(0.4f, 0.3f, 0.2f, 0.1f, 0f), "player-settle-slow")
        shots.pick("settle-fast", listOf(0.4f, 0.2f, 0f), "player-settle-fast")
        table("curve player-slow-release (ms:p)", slow)
        table("curve player-fast-flick (ms:p)", fast)
        val a = landed(slow)
        val b = landed(fast)
        println("settle: a slow release from p=$slowFrom landed in $a ms, a flick from p=$fastFrom in $b ms")
        assertTrue("let go at about 60% closed: $slowFrom, $fastFrom", slowFrom in 0.3f..0.5f && fastFrom in 0.3f..0.5f)
        // A frame of the drag's own release before the curve starts, at 60 fps.
        assertTrue("a slow release lands within the cap: $a ms", a + 17 <= Springs.CAP_MS)
        assertTrue("a flick lands sooner: $b ms against $a", b < a)
    }

    @Test
    fun thePlayersBackGesturePreviewsAndSaturates() {
        var at30 = -1f
        var at90 = -1f
        var cancelled = -1f
        var finished = -1f
        player(
            { _, sheet, _, _ ->
                {
                    val s = sheet()
                    s.open()
                    frames(500)
                    playerBack.dispatchOnBackStarted(start)
                    playerBack.dispatchOnBackProgressed(at(0.3f))
                    frames(100)
                    at30 = 1f - s.p
                    playerBack.dispatchOnBackProgressed(at(0.9f))
                    frames(100)
                    at90 = 1f - s.p
                    playerBack.dispatchOnBackCancelled()
                    frames(500)
                    cancelled = s.p
                    playerBack.dispatchOnBackStarted(start)
                    playerBack.dispatchOnBackProgressed(at(0.5f))
                    frames(100)
                    playerBack.onBackPressed()
                    frames(500)
                    finished = s.p
                }
            },
            3_000L,
        )
        println("player back: previewed ${at30} at 0.3, ${at90} at 0.9; cancelled to $cancelled; finished at $finished")
        assertTrue("at 0.3, within the limit: $at30", at30 > 0f && at30 <= Springs.PREVIEW + 0.001f)
        assertTrue("at 0.9, only a little further: $at90", at90 <= Springs.PREVIEW + 0.001f && at90 - at30 < 0.15f * at30)
        assertEquals("a cancel puts it back up", 1f, cancelled, 0f)
        assertEquals("a finished one takes it to the bar", 0f, finished, 0f)
    }
}
