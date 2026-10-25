// Every destination comes back where it was left, on its first frame, however
// it comes back — and is where it was on every frame it is drawn on the way,
// the preview of a back included (docs/plan-android.md, "Where a
// destination's state lives"). Parametrized over `Route`, the list each
// tab's NavHost is built from, so a route added there is a case here: the
// `when` in `reach` does not compile without one. The library's views and
// the Library's four lists are each a case of their own.
//
// A case: the real `HarkenApp` over the demo's songs and a few more (enough
// albums, artists and playlists that every list and every shelf scrolls),
// on Paparazzi's frame clock at a short phone's size, so even Settings
// scrolls. The destination is reached, every scrolling container it draws
// is scrolled somewhere other than its top — read back from the
// destination's own places (`ark-ui`'s `DestinationState`), after a check
// that every scrolling node on screen draws a state the destination keeps —
// and then it goes through each transition:
//
// - the tab switched away from and back to;
// - a page opened from it and gone back from by the page's back arrow, by
//   the edge's back gesture (its preview drawn over it at 0.3 and 0.9), and
//   by a pull down from the page's top;
// - the tab back preview from a tab that is not the first (r = 0.3, then
//   0.9), cancelled and then let go: the destination is then either the tab
//   drawn under it (Home's stack) or the tab leaving (Search's, the
//   Library's);
// - its tab reselected from a page above it (a tab's root only: from a page
//   above a page, reselect goes past it);
// - its shelves scrolled out of the list and back;
// - the process killed: everything saved, the composition and every
//   ViewModel thrown away, and the app composed again from what was saved.
//
// The assertion is the same each time: every frame the destination is
// drawn on, from the first, has every container it had at the place it had
// — no frame at the top, no frame anywhere else.
package dev.harken.android

import dev.harken.android.gen.AddSong
import dev.harken.android.gen.Harken
import dev.harken.android.gen.CreatePlaylist
import dev.harken.android.gen.Recordings
import dev.harken.android.gen.Works
import dev.arkdb.ffi.Id
import android.view.MotionEvent
import android.view.View
import androidx.activity.BackEventCompat
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.LocalSaveableStateRegistry
import androidx.compose.runtime.saveable.SaveableStateRegistry
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
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
import androidx.navigation.NavHostController
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.harken.android.onboarding.ArkSession
import dev.harken.android.ui.DestinationState
import dev.harken.android.ui.Destinations
import dev.harken.android.ui.PageMotion
import dev.harken.android.ui.routes
import java.io.File
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.Parameterized

/** A destination as a case: a route, and for the Library's root which of its lists. */
data class Case(val route: Route, val view: LibraryTab? = null) {
    override fun toString(): String = if (view == null) route.name else "${route.name}-${view.name}"

    companion object {
        /** Every route, and the Library's root once per list. */
        val all: List<Case> = Route.entries.flatMap { r -> if (r == Route.Library) LibraryTab.entries.map { Case(r, it) } else listOf(Case(r)) }
    }
}

/** The library the restoration tests are drawn over: the demo's, and enough more that everything scrolls. */
object Fixture {
    /** What is added beside the demo's songs: 24 albums of two songs, by six artists. */
    val extra: List<Triple<String, String, String>> =
        (0 until 48).map { i -> Triple("Piece ${i + 1}", "Artist ${(i / 2) % 6}", "Album %02d".format(i / 2 + 1)) }

    /** The playlists made, the first with thirty songs on it. */
    val playlists: List<String> = (1..16).map { "List %02d".format(it) }

    /** The artist with four albums: a shelf on their page that scrolls. */
    const val ARTIST = "Artist 0"

    /** The first playlist, with thirty songs on it, as the last [peer] made it. */
    var first: Id? = null
        private set

    /** Water Music, recorded eight times, and its fullest performance: a work's page and a recording's that scroll. */
    var work: String = ""
        private set

    var recording: String = ""
        private set

    private fun song(title: String, artist: String, album: String, ms: Long, track: Long, file: String, part: String = "", performer: String = "", workTitle: String = ""): AddSong =
        AddSong(
            title = title,
            artist = artist,
            album = album,
            durationMs = ms,
            file = file,
            track = track,
            part = part,
            catalogue = "",
            performer = performer,
            bpm = 0,
            albumArt = "",
            artistArt = "",
            disc = 1,
            workTitle = workTitle,
            movementNo = 0,
            artistBanner = "",
        )

    fun peer(): Harken {
        val peer = alone()
        for (t in Demo.tracks) {
            val d = Demo.details.getValue(t.id)
            peer.addSong(song(t.title, t.creator, d.album, t.durationMs, d.track, t.file, d.part, d.performer))
        }
        for ((i, e) in extra.withIndex()) peer.addSong(song(e.first, e.second, e.third, 120_000L + i, (i % 2 + 1).toLong(), "music/extra/$i.mp3"))
        // Six more performances of Water Music's first suite, each its own record.
        for (o in 1..6) for (t in 31L..33L) peer.addSong(song("Movement $t", "George Frideric Handel", "Water Music $o", 150_000L, t, "music/water/$o/$t.mp3", "Suite No. 1 in F major", "Orchestra $o", "Water Music"))
        for (n in playlists) peer.createPlaylist(CreatePlaylist(n))
        val list = Phone.once({ peer.playlists() }) { it }.first { it.name == playlists[0] }
        val songs = Phone.once({ Phone.library(peer, null) }, Track::of)
        for (t in songs.take(30)) Phone.addToPlaylist(peer, list.id, t.id)
        first = list.id
        work = Phone.once({ peer.works(Works("George Frideric Handel")) }) { it }.first { it.title == "Water Music" }.id
        recording = Phone.once({ peer.recordings(Recordings(work)) }) { it }.first().id
        return peer
    }
}

/** The frames are rendered and thrown away: what the destinations say is the result. */
private object Discard : SnapshotHandler {
    override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
        Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, _, _ -> null }
            as SnapshotHandler.FrameHandler

    override fun close() {}
}

/** A short phone: Settings scrolls too. */
val SHORT = DeviceConfig.PIXEL_5.copy(screenHeight = 1400)

/**
 * The app driven on the frame clock, with the destinations it draws in view: what a case and the
 * lifecycle tests share.
 */
class Driver(private val paparazzi: Paparazzi) {
    val states = mutableListOf<DestinationState>()
    var navs: Map<Tab, NavHostController> = emptyMap()
    lateinit var view: ComposeView
    lateinit var back: OnBackPressedDispatcher
    lateinit var motion: PageMotion
    private var clock = 0L

    /** Kill the process: save, throw the composition and every ViewModel away, compose again. */
    var kill: suspend () -> Unit = {}

    suspend fun frame() {
        clock = withFrameNanos { it }
    }

    suspend fun frames(ms: Long) {
        if (clock == 0L) clock = withFrameNanos { it }
        val target = clock + ms * 1_000_000
        while (clock < target) clock = withFrameNanos { it }
    }

    /** The destination drawn as `name` most recently entered. */
    fun state(name: String): DestinationState? = states.lastOrNull { it.name == name }

    private fun root(): SemanticsNode = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.rootSemanticsNode

    private fun unmerged(): SemanticsNode = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.unmergedRootSemanticsNode

    fun all(n: SemanticsNode): Sequence<SemanticsNode> = sequenceOf(n) + n.children.asSequence().flatMap { all(it) }

    fun texts(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.Text)?.map { it.text }.orEmpty()

    fun has(text: String): Boolean = all(root()).any { text in texts(it) }

    fun click(text: String) {
        val node = all(root()).first { text in texts(it) && it.config.getOrNull(SemanticsActions.OnClick) != null }
        node.config[SemanticsActions.OnClick].action!!.invoke()
    }

    fun clickDescribed(what: String) {
        val node =
            all(root()).first { n ->
                n.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(what) == true && n.config.getOrNull(SemanticsActions.OnClick) != null
            }
        node.config[SemanticsActions.OnClick].action!!.invoke()
    }

    fun type(text: String) {
        all(root()).first { it.config.getOrNull(SemanticsActions.SetText) != null }.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(text))
    }

    /** Every scrolling node on screen, and the state it scrolls by. */
    fun scrollers(): List<Pair<SemanticsNode, Any?>> =
        all(unmerged())
            .filter { it.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) != null || it.config.getOrNull(SemanticsProperties.HorizontalScrollAxisRange) != null }
            .map { it to stateOf(it) }
            .toList()

    private fun stateOf(node: SemanticsNode): Any? {
        for (info in node.layoutInfo.getModifierInfo()) {
            var c: Class<*>? = info.modifier.javaClass
            while (c != null) {
                for (f in c.declaredFields) {
                    if (f.type.isPrimitive) continue
                    f.isAccessible = true
                    val v = f.get(info.modifier)
                    if (v is LazyListState || v is LazyGridState || v is ScrollState) return v
                }
                c = c.superclass
            }
        }
        return null
    }

    private var downAt = 0L

    private fun touch(action: Int, at: Offset) {
        val now = clock / 1_000_000
        if (action == MotionEvent.ACTION_DOWN) downAt = now
        val e = MotionEvent.obtain(downAt, now, action, at.x, at.y, 0)
        (view as View).dispatchTouchEvent(e)
        e.recycle()
    }

    /** A finger down at `from`, along to `to` in `steps` frames, held a moment, and up; `during` each frame. */
    suspend fun drag(from: Offset, to: Offset, steps: Int, during: suspend () -> Unit = {}) {
        touch(MotionEvent.ACTION_DOWN, from)
        frame()
        for (i in 1..steps) {
            touch(MotionEvent.ACTION_MOVE, from + (to - from) * (i.toFloat() / steps))
            frame()
            during()
        }
        repeat(8) {
            touch(MotionEvent.ACTION_MOVE, to)
            frame()
            during()
        }
        touch(MotionEvent.ACTION_UP, to)
        frame()
        during()
    }

    val start = BackEventCompat(0f, 900f, 0f, BackEventCompat.EDGE_LEFT)

    fun at(p: Float) = BackEventCompat(1080f * p, 900f, p, BackEventCompat.EDGE_LEFT)

    @Composable
    private fun Host(store: ViewModelStoreOwner, content: @Composable () -> Unit) {
        val owner = LocalLifecycleOwner.current
        val dispatcher =
            remember {
                object : OnBackPressedDispatcherOwner {
                    override val onBackPressedDispatcher = back
                    override val lifecycle = owner.lifecycle
                }
            }
        CompositionLocalProvider(LocalViewModelStoreOwner provides store, LocalOnBackPressedDispatcherOwner provides dispatcher) { content() }
    }

    /**
     * The app, from `start`, over the fixture's library, running `script`; `held` a player sheet
     * drawn part-way up, for a test of what is behind it.
     */
    fun run(ms: Long, start: LibraryTab? = null, sheet: ((androidx.compose.ui.unit.Density) -> PlayerSheet)? = null, script: suspend Driver.() -> Unit) {
        val peer = Fixture.peer()
        val login = testLogin("library", roles = listOf("library"))
        val session = ArkSession.inline(peer, "https://music.example.com", login)
        back = OnBackPressedDispatcher()
        var done = false
        Destinations.sink = { states += it }
        try {
            view = ComposeView(paparazzi.context)
            var saved: Map<String, List<Any?>>? = null
            var save: () -> Map<String, List<Any?>> = { emptyMap() }
            // Not a `key`: a key is part of where every saveable value is kept, so a new one
            // would find nothing of what the old one saved. The same call, gone and back.
            var alive by mutableStateOf(true)
            kill = {
                saved = save()
                alive = false
                frame()
                // What the process kept with the session goes with the process: the library is
                // read again, as a launch reads it.
                Kept.drop(session)
                alive = true
                frame()
            }
            view.setContent {
                if (alive) {
                    val registry = remember { SaveableStateRegistry(saved) { true } }
                    save = { registry.performSave() }
                    val store = remember { object : ViewModelStoreOwner { override val viewModelStore = ViewModelStore() } }
                    val m = remember { PageMotion(Tab.routes).also { motion = it } }
                    CompositionLocalProvider(LocalSaveableStateRegistry provides registry) {
                        Host(store) {
                            val density = androidx.compose.ui.platform.LocalDensity.current
                            val s = sheet?.let { f -> remember { f(density) } } ?: rememberPlayerSheet()
                            HarkenApp(session, playbackOf = { c, sv -> Playback(c, sv, connect = false) }, start = start, motion = m, sheet = s, onNavs = { navs = it })
                        }
                    }
                }
                LaunchedEffect(Unit) {
                    frames(800)
                    script()
                    done = true
                }
            }
            paparazzi.gif(view, "restore", 0L, ms, 60)
        } finally {
            Destinations.sink = null
            session.close()
            peer.peer.close()
        }
        assertTrue("the script ran to its end", done)
    }
}

@RunWith(Parameterized::class)
class RestorationTests(private val case: Case) {
    @get:Rule val paparazzi = Paparazzi(deviceConfig = SHORT, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true, snapshotHandler = Discard)

    companion object {
        @JvmStatic @Parameterized.Parameters(name = "{0}") fun cases(): List<Case> = Case.all

        /** What each case went through, for the matrix the docs print. */
        val ran = java.util.concurrent.ConcurrentHashMap<String, List<String>>()
    }

    /** The tab whose stack the destination is on: its own for a root, Home for a page. */
    private val tab: Tab = case.route.tab ?: Tab.Home

    private val name = case.route.pattern

    /** Where on the stack it is put, and how. */
    private fun Driver.reach() {
        when (case.route) {
            Route.Home -> {}
            Route.Search -> {
                click("Search")
            }
            Route.Library -> {}
            Route.Album -> navs.getValue(tab).navigate(Page.ForAlbum("Water Music").route)
            Route.Artist -> navs.getValue(tab).navigate(Page.ForArtist(Fixture.ARTIST).route)
            Route.Playlist -> navs.getValue(tab).navigate(Page.ForPlaylist(Fixture.first!!).route)
            Route.Work -> navs.getValue(tab).navigate(Page.ForWork(Fixture.work).route)
            Route.Recording -> navs.getValue(tab).navigate(Page.ForRecording(Fixture.recording).route)
            Route.Settings -> navs.getValue(tab).navigate(Route.Settings.pattern)
            Route.Diagnostics -> navs.getValue(tab).navigate(Route.Diagnostics.pattern)
        }
    }

    /**
     * Every container the destination draws, somewhere other than its top; the shelves first. The
     * names of those with nowhere to go — everything already on screen — come back.
     */
    private suspend fun Driver.scroll(d: DestinationState): Set<String> {
        val fits = mutableSetOf<String>()
        val seen = mutableSetOf<String>()
        // Again for whatever scrolling brought into view: a shelf below the fold.
        while (true) {
        val keys = (d.drawnKeys - seen).sortedBy { if (it.startsWith("shelf:") || it == "chips") 0 else 1 }
        if (keys.isEmpty()) break
        seen += keys
        for (k in keys) {
            when (val s = d.drawn(k)) {
                is LazyListState -> {
                    if (!s.canScrollForward) fits += k
                    else if (k.startsWith("shelf:")) s.scrollToItem(1, 23)
                    // Search's results end in its two shelves: down to them, so they are drawn.
                    else if (case.route == Route.Search) s.scrollToItem(s.layoutInfo.totalItemsCount - 5, 31)
                    else s.scrollToItem(2, 31)
                }
                is LazyGridState -> if (!s.canScrollForward) fits += k else s.scrollToItem(2, 31)
                is ScrollState -> if (s.maxValue == 0) fits += k else s.scrollTo(minOf(137, s.maxValue))
            }
            frames(100)
        }
        frames(300)
        }
        return fits
    }

    private val problems = mutableListOf<String>()
    private val done = mutableListOf<String>()

    /** The places drawn now, of the keys drawn: what every frame is held to. */
    private fun drawn(d: DestinationState): Map<String, List<Int>> = d.drawnKeys.associateWith { d.places().getValue(it) }

    /**
     * Each frame for `ms` (and the one `act` takes), that the destination — when it is drawn —
     * draws every container it had, where it had it. `drawnOn` counts the frames it was drawn on;
     * `must` says it has to be drawn on at least one.
     */
    private suspend fun Driver.holds(what: String, before: Map<String, List<Int>>, ms: Long = 500, must: Boolean = true, act: suspend Driver.() -> Unit = {}) {
        var drawnOn = 0
        var first: Map<String, List<Int>>? = null
        suspend fun look() {
            val d = target() ?: return
            if (d.composed == 0 || d.drawnKeys.isEmpty()) return
            val now = drawn(d)
            drawnOn++
            if (first == null) first = now
            // Every container that was drawn, drawn where it was; one scrolled into view since may be drawn too.
            if (before.any { (k, p) -> now[k] != p }) problems += "$case, $what, frame $drawnOn: $now, left at $before"
        }
        act()
        look()
        val t0 = System.nanoTime()
        var waited = 0L
        while (waited < ms) {
            frame()
            look()
            waited += 16
        }
        if (must && drawnOn == 0) problems += "$case, $what: never drawn"
        done += what
        println("$case $what: drawn on $drawnOn frames, first ${first ?: "-"}")
        if (System.nanoTime() - t0 < 0) error("clock")
    }

    @Test
    fun comesBackWhereItWasLeft() {
        Driver(paparazzi).run(30_000L, start = case.view) {
            reach()
            frames(700)
            if (case.route == Route.Search) {
                type("a")
                frames(700)
            }
            val d = state(name)
            assertTrue("$case is drawn", d != null && d.composed == 1)
            d!!
            target = { d }
            // Every scrolling node on screen scrolls by a state this destination keeps.
            for ((node, s) in scrollers()) {
                assertTrue("$case: a scrolling node with no state of its own (${node.config})", s != null)
                assertTrue("$case: a scrolling node whose state the destination does not keep: $s", d.drawnKeys.any { d.drawn(it) === s })
            }
            val fits = scroll(d)
            val left = drawn(d)
            println("$case: left at $left; with nowhere to go: $fits")
            assertTrue("$case scrolled somewhere: $left", left.isNotEmpty() && left.all { (k, p) -> k in fits || p.any { it != 0 } } && fits.size < left.size)

            // The tab, switched away from and back to.
            val other = if (tab == Tab.Search) "Library" else "Search"
            click(other)
            frames(300)
            holds("tab switched back", left) { click(tab.title) }

            // A page opened from it, and back: by the arrow, by the edge, by a pull.
            val page = Page.ForAlbum("The Four Seasons").route
            for (how in listOf("arrow", "edge", "pull")) {
                navs.getValue(tab).navigate(page)
                frames(700)
                when (how) {
                    "arrow" -> holds("page, back by its arrow", left, 700) { clickDescribed("Back") }
                    "edge" -> {
                        back.dispatchOnBackStarted(start)
                        holds("page, the edge's preview at 0.3", left, 200, must = true) { back.dispatchOnBackProgressed(at(0.3f)) }
                        holds("page, the edge's preview at 0.9", left, 200) { back.dispatchOnBackProgressed(at(0.9f)) }
                        holds("page, back by the edge", left, 700) { back.onBackPressed() }
                    }
                    "pull" -> {
                        val w = view.width.toFloat()
                        val h = view.height.toFloat()
                        holds("page, back by a pull", left, 700) { drag(Offset(w / 2, h * 0.3f), Offset(w / 2, h * 0.7f), 10) }
                    }
                }
                frames(200)
            }

            // The tab back preview, from a tab that is not the first.
            if (tab == Tab.Home) {
                click("Library")
                frames(300)
                back.dispatchOnBackStarted(start)
                holds("under the tab back preview, 0.3", left, 300) { back.dispatchOnBackProgressed(at(0.3f)) }
                holds("under the tab back preview, 0.9", left, 300) { back.dispatchOnBackProgressed(at(0.9f)) }
                holds("the tab back preview cancelled", left, 600, must = false) { back.dispatchOnBackCancelled() }
                if (d.composed != 0) problems += "$case: still drawn once the preview that drew it is cancelled"
                back.dispatchOnBackStarted(start)
                holds("under the tab back preview again, 0.3", left, 200) { back.dispatchOnBackProgressed(at(0.3f)) }
                holds("under the tab back preview again, 0.9", left, 200) { back.dispatchOnBackProgressed(at(0.9f)) }
                // What lands is the composition that was under the preview, not a new one.
                val entered = d.entered
                val list = d.drawnKeys.associateWith { d.drawn(it) }
                holds("the tab back preview landed", left, 800) { back.onBackPressed() }
                if (d.entered != entered || list.any { (k, v) -> d.drawn(k) !== v }) problems += "$case: composed again from scratch when the tab back landed"
            } else {
                back.dispatchOnBackStarted(start)
                holds("leaving in the tab back preview, 0.3", left, 300) { back.dispatchOnBackProgressed(at(0.3f)) }
                holds("leaving in the tab back preview, 0.9", left, 300) { back.dispatchOnBackProgressed(at(0.9f)) }
                holds("the tab back preview cancelled", left, 600) { back.dispatchOnBackCancelled() }
                back.dispatchOnBackStarted(start)
                back.dispatchOnBackProgressed(at(0.3f))
                frames(100)
                back.dispatchOnBackProgressed(at(0.9f))
                frames(100)
                back.onBackPressed()
                frames(600)
                holds("its tab chosen again, after the tab back preview landed", left) { click(tab.title) }
            }

            // Reselected, from a page above it: a root only.
            if (case.route.tab != null) {
                navs.getValue(tab).navigate(page)
                frames(700)
                holds("its tab reselected from a page", left, 800) { click(tab.title) }
            }

            // Its shelves, out of its list and back.
            val main = d.drawnKeys.firstOrNull { !it.startsWith("shelf:") && it != "chips" && d.drawn(it) is LazyListState }
            if (left.keys.any { it.startsWith("shelf:") } && main != null) {
                val s = d.drawn(main) as LazyListState
                val at = s.firstVisibleItemIndex to s.firstVisibleItemScrollOffset
                s.scrollToItem(s.layoutInfo.totalItemsCount - 1)
                frames(300)
                holds("its shelves, scrolled out of its list and back", left) { s.scrollToItem(at.first, at.second) }
            }

            // The process killed, and the app composed again from what it saved: the destination is
            // a new one, of the same name, its places read back from what was saved.
            states.clear()
            target = { state(name) }
            holds("the process killed", left, 800) { kill() }
        }
        ran[case.toString()] = done.toList()
        println("$case: ${done.size} transitions: ${done.joinToString("; ")}")
        assertTrue(problems.joinToString("\n"), problems.isEmpty())
    }

    /** The destination the frames are read from. */
    private var target: () -> DestinationState? = { null }
}
