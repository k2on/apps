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
import dev.arkdb.Value
import dev.arkdb.android.ArkSession
import dev.arkdb.client.Account
import dev.arkdb.client.Autos
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import java.io.File
import java.lang.reflect.Proxy
import kotlin.math.abs
import kotlinx.coroutines.launch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

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

    private fun harken(): ByteArray {
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            File(dir, "domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
            File(dir, "harken/domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
            dir = dir.parentFile
        }
        error("no harken.ark above ${File("").absolutePath}")
    }

    /** The demo's songs, authored through `add_song` as the server's scanner would. */
    private fun demo(): Peer {
        val peer = Peer.open(Domain.fromBytes(harken()), MemoryStorage(), Options.alone("library").copy(autos = Autos.Seeded(5)))
        peer.setRoles(listOf("library"))
        for (t in Demo.tracks) {
            val d = Demo.details.getValue(t.id)
            peer.mutate(
                "add_song",
                mapOf(
                    "title" to Value.text(t.title),
                    "artist" to Value.text(t.creator),
                    "album" to Value.text(d.album),
                    "duration_ms" to Value.int(t.durationMs),
                    "file" to Value.text(t.file),
                    "track" to Value.int(d.track),
                    "part" to Value.text(d.part),
                    "catalogue" to Value.text(""),
                    "performer" to Value.text(d.performer),
                    "bpm" to Value.int(0),
                    "album_art" to Value.text(""),
                    "artist_art" to Value.text(""),
                    "disc" to Value.int(1),
                    "work_title" to Value.text(""),
                    "movement_no" to Value.int(0),
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
        val login = Login("t", "s", Account("library"), 0)
        val session = if (threaded) ArkSession.over(peer, "https://music.example.com", login) else ArkSession.inline(peer, "https://music.example.com", login)
        val motion = PageMotion()
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
            paparazzi.gif(view, "motion", 0L, ms, 30)
        } finally {
            session.close()
            peer.close()
        }
        assertTrue("the script ran to its end", done)
    }

    private val start = BackEventCompat(0f, 1200f, 0f, BackEventCompat.EDGE_LEFT)

    private fun at(p: Float) = BackEventCompat(1080f * p, 1200f, p, BackEventCompat.EDGE_LEFT)

    @Test
    fun aPageGrowsOutOfItsCardAndTheBackGestureSeeksIt() {
        val grew = mutableListOf<Float>()
        var opened = false
        var seeked30 = -1f
        var seeked60 = -1f
        var afterCancel = -1f
        var keptAfterCancel = false
        var gone = false
        app(
            { view, motion, back ->
                {
                    shots.meta = { motion.drawnAt }
                    shots.keep = "page-from"
                    frame()
                    shots.series = "open"
                    click(view, "Brandenburg Concertos")
                    repeat(18) {
                        frame()
                        grew += motion.drawnAt
                    }
                    shots.series = null
                    frames(300)
                    opened = has(view, "ALBUM")

                    back.dispatchOnBackStarted(start)
                    back.dispatchOnBackProgressed(at(0.3f))
                    frames(200)
                    seeked30 = motion.drawnAt
                    shots.keep = "page-back-30"
                    frames(100)
                    back.dispatchOnBackProgressed(at(0.6f))
                    frames(200)
                    seeked60 = motion.drawnAt
                    shots.keep = "page-back-60"
                    frames(100)
                    back.dispatchOnBackCancelled()
                    frames(PAGE_MS * 2L)
                    afterCancel = motion.drawnAt
                    keptAfterCancel = has(view, "ALBUM")

                    // And through: past the threshold, the page goes back into its card.
                    shots.series = "shrink"
                    back.dispatchOnBackStarted(start)
                    back.dispatchOnBackProgressed(at(0.4f))
                    frames(150)
                    back.onBackPressed()
                    frames(PAGE_MS + 300L)
                    shots.series = null
                    gone = !has(view, "ALBUM") && has(view, "Recently added")
                }
            },
            8_000L,
        )
        shots.pick("open", listOf(0f, 0.25f, 0.5f, 0.75f, 1f), "page-open")
        shots.pick("shrink", listOf(0.5f), "page-shrink")
        println("open: drawn at $grew; back at 0.3: $seeked30, at 0.6: $seeked60, after cancel: $afterCancel")
        assertTrue("the page opened", opened)
        assertTrue("it grew through the in-between: $grew", grew.any { it > 0.05f && it < 0.95f })
        assertEquals("and ended open", 1f, grew.last(), 0f)
        // The gesture seeks the move: part-way at 0.3, further at 0.6, and back to open when let go.
        assertTrue("seeked at 0.3: $seeked30", seeked30 > 0f && seeked30 < 1f)
        assertTrue("further at 0.6: $seeked60 < $seeked30", seeked60 < seeked30)
        assertEquals("a cancelled gesture puts the page back", 1f, afterCancel, 0f)
        assertTrue("and keeps it", keptAfterCancel)
        assertTrue("a finished gesture goes back", gone)
    }

    @Test
    fun aPagePulledDownFromItsTopGoesBack() {
        var scrolledNotPulled = true
        var pulled = 1f
        var gone = false
        var smallPull = 1f
        var kept = false
        var restored = -1f
        app(
            { view, motion, back ->
                {
                    val w = view.width.toFloat()
                    val h = view.height.toFloat()
                    click(view, "Brandenburg Concertos")
                    frames(PAGE_MS + 300L)
                    // Scrolled down a way: a drag down scrolls the list back before it pulls anything.
                    drag(view, Offset(w / 2, h * 0.7f), Offset(w / 2, h * 0.4f), 10)
                    frames(300)
                    drag(view, Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.45f), 6) { if (motion.drawnAt < 1f) scrolledNotPulled = false }
                    frames(300)
                    // Then from the top: the page shrinks under the finger, and far enough goes back.
                    drag(view, Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.62f), 10) { pulled = minOf(pulled, motion.drawnAt) }
                    frames(PAGE_MS + 300L)
                    gone = !has(view, "ALBUM")
                    // A little way, and let go: it springs back.
                    if (gone) click(view, "Brandenburg Concertos")
                    frames(PAGE_MS + 300L)
                    drag(view, Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.35f), 4) { smallPull = minOf(smallPull, motion.drawnAt) }
                    frames(PAGE_MS * 2L)
                    kept = has(view, "ALBUM")
                    restored = motion.drawnAt
                }
            },
            9_000L,
        )
        println("pull: scrolled, not pulled $scrolledNotPulled; pulled to $pulled; small pull to $smallPull, restored to $restored")
        assertTrue("a drag down a scrolled list scrolls it and pulls nothing", scrolledNotPulled)
        assertTrue("a pull from the top shrinks the page: $pulled", pulled < 1f)
        assertTrue("and far enough goes back", gone)
        assertTrue("a short pull moved it too: $smallPull", smallPull < 1f)
        assertTrue("and let go, keeps the page", kept)
        assertEquals("open again", 1f, restored, 0f)
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
                    frames(PAGE_MS + 200L)
                    moving = motion.movingFrames
                    // A page does move, so the count is not empty for want of counting.
                    click(view, "Albums")
                    frames(200)
                    click(view, "Brandenburg Concertos")
                    frames(PAGE_MS + 200L)
                    opening = motion.movingFrames
                }
            },
            4_000L,
        )
        println("tab: Home was gone from the tree after $homeGone frames, none of them drawn")
        assertEquals("frames drawn mid-move switching tabs", 0, moving)
        assertTrue("Home went: $homeGone", homeGone in 0..9)
        assertTrue("Library came", libraryShown)
        assertTrue("frames drawn mid-move opening a page: $opening", opening > 0)
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

    private fun player(test: (ComposeView, () -> PlayerSheet, Asked, () -> Int) -> suspend () -> Unit, ms: Long) {
        var sheet: PlayerSheet? = null
        var taps = 0
        var done = false
        val view = ComposeView(paparazzi.context)
        lateinit var asked: Asked
        view.setContent {
            Host(OnBackPressedDispatcher()) {
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
        paparazzi.gif(view, "player", 0L, ms, 30)
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
}
