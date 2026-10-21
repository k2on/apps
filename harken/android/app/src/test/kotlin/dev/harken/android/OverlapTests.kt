// The same container composed twice for a moment is not two containers
// (docs/plan-android.md, "Where a destination's state lives"). Home's rows
// are items of a lazy list keyed by their index, so a row going from above
// its shelves — the offline line when the link opens, the last played card
// when there is nothing to say any more — moves every shelf up one slot: the
// slot that drew the heading above a shelf draws the shelf now, and the
// recomposer applies it before the slot that drew the shelf lets go. The
// shelf's state is remembered there before it is forgotten here, and
// `DestinationState.attach` threw for that on a phone, a few seconds after
// launch and again on coming back to the app — what the link does each time.
//
// Home as the app draws it — the demo's library, the session's track as the
// last played card, the playlists and albums, its rows read from flows
// collected with the lifecycle as `HarkenApp` collects them — on
// Paparazzi's frame clock, in a destination, through a launch with the link
// waiting, the link opening, the shelf scrolled out of the list and back,
// the app in the background while the link drops, and back. Every frame,
// each name is drawn by one state, the shelf at the place it was left, and
// Home is one composition. And the guard is still there for what it is for:
// two containers under one name, a frame later, throw where `Strict` is on
// and are said once and lived with where it is off.
package dev.harken.android

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.platform.ComposeView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.harken.android.ui.Destination
import dev.harken.android.ui.DestinationState
import dev.harken.android.ui.Destinations
import dev.harken.android.ui.Strict
import dev.harken.android.ui.rememberDestinationListState
import java.lang.reflect.Proxy
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.rules.TestRule
import org.junit.runner.Description
import org.junit.runners.model.Statement

class OverlapTests {
    val paparazzi =
        Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", useDeviceResolution = true, snapshotHandler = Discard)

    /**
     * What a test expects thrown. A throw from a frame callback is caught by layoutlib and reported
     * by Paparazzi when the test is over, so it is looked for around Paparazzi's rule.
     */
    private class Expect : TestRule {
        var thrown: String? = null
        var seen = false

        override fun apply(base: Statement, description: Description): Statement =
            object : Statement() {
                override fun evaluate() {
                    try {
                        base.evaluate()
                    } catch (e: Throwable) {
                        val x = thrown
                        if (x == null || e.message?.contains(x) != true) throw e
                        seen = true
                    }
                    thrown?.let { if (!seen) throw AssertionError("nothing threw: $it") }
                }
            }
    }

    private val expect = Expect()

    @get:Rule val rules: RuleChain = RuleChain.outerRule(expect).around(paparazzi)

    private object Discard : SnapshotHandler {
        override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
            Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, _, _ -> null }
                as SnapshotHandler.FrameHandler

        override fun close() {}
    }

    private var clock = 0L

    private suspend fun frame() {
        clock = withFrameNanos { it }
    }

    private suspend fun frames(ms: Long, each: () -> Unit = {}) {
        if (clock == 0L) clock = withFrameNanos { it }
        val target = clock + ms * 1_000_000
        while (clock < target) {
            clock = withFrameNanos { it }
            each()
        }
    }

    /** A lifecycle a test moves: the activity's, stopped and started again. */
    private class Activity : LifecycleOwner {
        val registry = LifecycleRegistry.createUnsafe(this).also { it.currentState = Lifecycle.State.RESUMED }
        override val lifecycle: Lifecycle
            get() = registry
    }

    /**
     * `content` in a destination named `name`, under `activity`'s lifecycle, running `script`;
     * what was thrown, if anything was, comes back — from the frame, or from the recomposer.
     */
    private fun run(name: String, activity: Activity, ms: Long, content: @Composable () -> Unit, script: suspend (DestinationState) -> Unit): Throwable? {
        var state: DestinationState? = null
        var done = false
        var thrown: Throwable? = null
        val handler = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { _, e -> if (thrown == null) thrown = e }
        Destinations.sink = { if (it.name == name) state = it }
        try {
            val view = ComposeView(paparazzi.context)
            view.setContent {
                val store = remember { object : ViewModelStoreOwner { override val viewModelStore = ViewModelStore() } }
                CompositionLocalProvider(LocalViewModelStoreOwner provides store, LocalLifecycleOwner provides activity) {
                    Destination(store, name, content)
                }
                LaunchedEffect(Unit) {
                    frames(500)
                    script(state!!)
                    done = true
                }
            }
            paparazzi.gif(view, "overlap", 0L, ms, 60)
        } catch (e: Throwable) {
            if (thrown == null) thrown = e
        } finally {
            Destinations.sink = null
            Thread.setDefaultUncaughtExceptionHandler(handler)
        }
        if (thrown == null) assertTrue("the script ran to its end", done)
        return thrown
    }

    private fun place(s: Any?): List<Int> = (s as LazyListState).let { listOf(it.firstVisibleItemIndex, it.firstVisibleItemScrollOffset) }

    /**
     * Home through what the link does to it on a phone. Launched, the link is waiting — the session
     * starts there — so Home draws the offline line above the last played card; it opens, and the
     * line goes. Then the shelf is scrolled out of the list and back, the app goes to the
     * background, the link drops while it is there, and the app comes back to the line and to the
     * link opening again. Every frame: one state per name, the shelf at (1, 23), one composition.
     *
     * On the code before this, the link opening threw what the phone did — "two containers of home
     * are both kept as "shelf:recent": give each its own name" — from `attach`, out of the
     * recomposer's apply.
     */
    @Test
    fun homeLosingTheOfflineLineKeepsItsShelvesWhereTheyWere() {
        val live = LastPlayed(Demo.playing(Demo.tracks[17]), Page.ForAlbum("The Four Seasons"), live = true, on = "Playing on Phone")
        val link = MutableStateFlow("waiting")
        val activity = Activity()
        val problems = mutableListOf<String>()
        val thrown =
            run(Route.Home.pattern, activity, 6_000L, {
                val l by link.collectAsStateWithLifecycle()
                val offline = if (l == "waiting" || l == "offline") "music.example.com" else null
                HomeScreen(HomeData(true, live, Demo.tracks, Demo.playlists, Demo.albums, offline), Demo::albumOf, Idle)
            }) { home ->
                val shelf = home.drawn("shelf:recent") as LazyListState
                shelf.scrollToItem(1, 23)
                frames(200)
                fun look(at: String) {
                    if (home.composed != 1) problems += "$at: Home is ${home.composed} compositions"
                    val s = home.drawn("shelf:recent")
                    if (s != null && place(s) != listOf(1, 23)) problems += "$at: the shelf is at ${place(s)}"
                }
                // The link opens, a few seconds after launch.
                link.value = "open"
                frames(500) { look("the link opened") }
                // The shelf out of the list and back.
                (home.drawn("home") as LazyListState).scrollToItem(9)
                frames(300) { look("scrolled down") }
                (home.drawn("home") as LazyListState).scrollToItem(0)
                frames(300) { look("scrolled back") }
                // The background, the link dropping there, and back.
                activity.registry.currentState = Lifecycle.State.CREATED
                frames(200) { look("in the background") }
                link.value = "waiting"
                frames(200) { look("the link dropped in the background") }
                activity.registry.currentState = Lifecycle.State.RESUMED
                frames(300) { look("back, the link waiting") }
                link.value = "open"
                frames(500) { look("back, the link open") }
                if (home.drawn("shelf:recent") == null) problems += "the shelf is not drawn at the end"
                if (home.drawnKeys.any { '#' in it }) problems += "a name was taken for a second container: ${home.drawnKeys}"
            }
        assertEquals("nothing thrown", null, thrown?.toString())
        assertTrue(problems.joinToString("\n"), problems.isEmpty())
    }

    /**
     * The last played card going — a session that ends with no play of this person's in the log —
     * moves every shelf up two slots, and comes to the same.
     */
    @Test
    fun homeLosingTheLastPlayedCardKeepsItsShelvesWhereTheyWere() {
        val last = MutableStateFlow<LastPlayed?>(LastPlayed(Demo.playing(Demo.tracks[17]), Page.ForAlbum("The Four Seasons"), live = true))
        val problems = mutableListOf<String>()
        val thrown =
            run(Route.Home.pattern, Activity(), 3_000L, {
                val l by last.collectAsStateWithLifecycle()
                HomeScreen(HomeData(true, l, Demo.tracks, Demo.playlists, Demo.albums, null), Demo::albumOf, Idle)
            }) { home ->
                (home.drawn("shelf:recent") as LazyListState).scrollToItem(1, 23)
                // As far along as it goes: the demo's playlists are a short shelf.
                (home.drawn("shelf:playlists") as LazyListState).scrollToItem(2, 7)
                frames(200)
                val lists = place(home.drawn("shelf:playlists"))
                if (lists == listOf(0, 0)) problems += "the playlists' shelf did not scroll"
                last.value = null
                frames(500) {
                    val r = home.drawn("shelf:recent")
                    val p = home.drawn("shelf:playlists")
                    if (r != null && place(r) != listOf(1, 23)) problems += "the recent shelf is at ${place(r)}"
                    if (p != null && place(p) != lists) problems += "the playlists' shelf is at ${place(p)}, not $lists"
                }
            }
        assertEquals("nothing thrown", null, thrown?.toString())
        assertTrue(problems.joinToString("\n"), problems.isEmpty())
    }

    // Two rows of one destination, both kept as "same": the mistake the guard is for.
    @Composable
    private fun TwoUnderOneName() {
        Column {
            for (row in 1..2) {
                LazyRow(state = rememberDestinationListState("same")) { items(40) { Text("row $row, card $it   ") } }
            }
        }
    }

    /**
     * Where `Strict` is on — every unit test, a debuggable build — two containers under one name
     * still throw, a frame after the second arrives. Falsified by `mistake` not throwing: nothing
     * was thrown.
     */
    @Test
    fun twoContainersUnderOneNameThrowWhereStrictIsOn() {
        assertTrue("the tests run strict", Strict.on)
        expect.thrown = "two containers of strict are both kept as \"same\": give each its own name"
        val thrown = run("strict", Activity(), 1_500L, { TwoUnderOneName() }) { frames(500) }
        if (thrown?.message?.contains(expect.thrown!!) == true) expect.seen = true
    }

    /**
     * Where it is off — a release — the same mistake is said once, and both go on: the second has
     * a name of its own, each keeps its own place, and nothing is thrown. Falsified by a release
     * that throws as a test does: the script never ran; and by leaving the second under the first's
     * name: one name drawn, and the first row's place lost to the second's.
     */
    @Test
    fun twoContainersUnderOneNameAreSaidOnceAndLivedWithWhereStrictIsOff() {
        val said = mutableListOf<String>()
        val was = Strict.on
        val say = Strict.say
        Strict.on = false
        Strict.say = { said += it }
        var names: Set<String> = emptySet()
        var first: List<Int>? = null
        var second: List<Int>? = null
        val thrown =
            try {
                run("release", Activity(), 2_500L, { TwoUnderOneName() }) { d ->
                    frames(300)
                    names = d.drawnKeys
                    (d.drawn("same") as LazyListState).scrollToItem(5)
                    (d.drawn("same#2") as LazyListState?)?.scrollToItem(9)
                    frames(300)
                    first = d.drawn("same")?.let(::place)
                    second = d.drawn("same#2")?.let(::place)
                    frames(300)
                }
            } finally {
                Strict.on = was
                Strict.say = say
            }
        assertEquals("nothing thrown", null, thrown?.toString())
        assertEquals("said once", listOf("two containers of release are both kept as \"same\": give each its own name"), said)
        assertEquals("each drawn under a name of its own", setOf("same", "same#2"), names)
        assertEquals("the first where it was put", listOf(5, 0), first)
        assertEquals("the second where it was put", listOf(9, 0), second)
    }
}
