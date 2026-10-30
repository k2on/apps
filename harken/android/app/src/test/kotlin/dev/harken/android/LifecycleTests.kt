// What a destination drawn but not in front can tell: its content sees the
// lifecycle of where it is (`LocalLifecycleOwner`), so a video, an
// animation clock, anything that should only run in front, follows RESUMED
// (docs/plan-android.md, "Where a destination's state lives"). The probe is
// each destination's own: the lifecycle its content was composed under,
// which `ark-ui`'s `Destination` records — a NavBackStackEntry's, under a
// tab's own. And the registry the NavHosts are built from, read back from
// the graphs.
package dev.harken.android

import androidx.compose.foundation.lazy.LazyListState
import androidx.lifecycle.Lifecycle
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.arkui.routes
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class LifecycleTests {
    @get:Rule
    val paparazzi =
        Paparazzi(
            deviceConfig = SHORT,
            theme = "android:Theme.Material.NoActionBar",
            useDeviceResolution = true,
            snapshotHandler =
                object : SnapshotHandler {
                    override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
                        Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, _, _ -> null }
                            as SnapshotHandler.FrameHandler

                    override fun close() {}
                },
        )

    @Test
    fun aTabUnderTheBackPreviewIsStartedAndResumesWhenItLands() {
        val seen = mutableListOf<String>()
        val problems = mutableListOf<String>()
        var cancelledComposed = -1
        var cancelledHome: Lifecycle.State? = null
        var cancelledLibrary: Lifecycle.State? = null
        var landedHome: Lifecycle.State? = null
        var landedLibrary: Lifecycle.State? = null
        var enteredBefore = -1
        var enteredAfter = -1
        var listBefore: Any? = null
        var listAfter: Any? = null
        Driver(paparazzi).run(6_000L) {
            val home = state(Route.Home.pattern)!!
            (home.drawn("home") as LazyListState).scrollToItem(3, 20)
            frames(200)
            click("Library")
            frames(300)
            val library = state(Route.Library.pattern)!!
            fun note(when_: String) = seen.add("$when_: Home ${home.lifecycle?.currentState} (composed ${home.composed}), Library ${library.lifecycle?.currentState}")

            // Previewed, and cancelled.
            back.dispatchOnBackStarted(start)
            back.dispatchOnBackProgressed(at(0.3f))
            frame()
            frame()
            for (r in listOf(0.3f, 0.6f, 0.9f)) {
                back.dispatchOnBackProgressed(at(r))
                repeat(6) {
                    frame()
                    if (home.composed != 1) problems += "at $r Home is not drawn under the preview"
                    if (home.lifecycle?.currentState != Lifecycle.State.STARTED) problems += "at $r Home under the preview is ${home.lifecycle?.currentState}"
                    if (library.lifecycle?.currentState == Lifecycle.State.RESUMED) problems += "at $r the Library leaving is RESUMED"
                }
                note("preview $r")
            }
            back.dispatchOnBackCancelled()
            frames(600)
            note("cancelled")
            cancelledComposed = home.composed
            cancelledHome = home.lifecycle?.currentState
            cancelledLibrary = library.lifecycle?.currentState

            // Previewed again, and let go.
            back.dispatchOnBackStarted(start)
            back.dispatchOnBackProgressed(at(0.3f))
            frame()
            frame()
            back.dispatchOnBackProgressed(at(0.9f))
            repeat(6) {
                frame()
                if (library.lifecycle?.currentState == Lifecycle.State.RESUMED) problems += "the Library leaving is RESUMED"
            }
            enteredBefore = home.entered
            listBefore = home.drawn("home")
            back.onBackPressed()
            frames(800)
            note("landed")
            enteredAfter = home.entered
            listAfter = home.drawn("home")
            landedHome = home.lifecycle?.currentState
            landedLibrary = library.lifecycle?.currentState
        }
        println("tab lifecycles:\n" + seen.joinToString("\n"))
        assertTrue(problems.joinToString("\n"), problems.isEmpty())
        assertEquals("cancelled, Home is not drawn", 0, cancelledComposed)
        assertEquals("cancelled, Home is no further on than created", Lifecycle.State.CREATED, cancelledHome)
        assertEquals("cancelled, the Library is in front again", Lifecycle.State.RESUMED, cancelledLibrary)
        assertEquals("landed, Home is in front", Lifecycle.State.RESUMED, landedHome)
        assertEquals("landed, the Library is not drawn", Lifecycle.State.CREATED, landedLibrary)
        assertEquals("landed, Home is the composition that was under the preview, not a new one", enteredBefore, enteredAfter)
        assertSame("…its list the same list", listBefore, listAfter)
    }

    @Test
    fun aPageUnderTheBackPreviewIsStartedAsTheNavHostSays() {
        var under: Lifecycle.State? = null
        var leaving: Lifecycle.State? = null
        var cancelledUnder: Lifecycle.State? = null
        var cancelledTop: Lifecycle.State? = null
        var landed: Lifecycle.State? = null
        Driver(paparazzi).run(5_000L) {
            val home = state(Route.Home.pattern)!!
            navs.getValue(Tab.Home).navigate(Page.ForAlbum(Demo.key("Water Music")).route)
            frames(700)
            val album = state(Route.Album.pattern)!!
            back.dispatchOnBackStarted(start)
            back.dispatchOnBackProgressed(at(0.5f))
            frames(200)
            under = home.lifecycle?.currentState
            leaving = album.lifecycle?.currentState
            back.dispatchOnBackCancelled()
            frames(800)
            cancelledUnder = home.lifecycle?.currentState
            cancelledTop = album.lifecycle?.currentState
            back.dispatchOnBackStarted(start)
            back.dispatchOnBackProgressed(at(0.5f))
            frames(150)
            back.onBackPressed()
            frames(800)
            landed = home.lifecycle?.currentState
        }
        println("page lifecycles: under $under, leaving $leaving; cancelled $cancelledUnder / $cancelledTop; landed $landed")
        assertEquals("Home under the page's preview", Lifecycle.State.STARTED, under)
        assertEquals("the page leaving", Lifecycle.State.STARTED, leaving)
        assertEquals("cancelled, Home not drawn", Lifecycle.State.CREATED, cancelledUnder)
        assertEquals("cancelled, the page in front", Lifecycle.State.RESUMED, cancelledTop)
        assertEquals("landed, Home in front", Lifecycle.State.RESUMED, landed)
    }

    @Test
    fun withThePlayerDownHomeIsInFront() {
        var down: Lifecycle.State? = null
        Driver(paparazzi).run(2_000L) { down = state(Route.Home.pattern)!!.lifecycle?.currentState }
        assertEquals("with the player down, Home is in front", Lifecycle.State.RESUMED, down)
    }

    @Test
    fun withThePlayerHalfUpHomeIsBehindIt() {
        var up: Lifecycle.State? = null
        Driver(paparazzi).run(2_000L, sheet = { d -> PlayerSheet.held(0.5f, d) }) { up = state(Route.Home.pattern)!!.lifecycle?.currentState }
        assertEquals("with it half up, Home is behind it", Lifecycle.State.STARTED, up)
    }

    @Test
    fun theRegistryIsTheGraph() {
        val graphs = mutableMapOf<Tab, Set<String>>()
        Driver(paparazzi).run(3_000L) {
            // A tab's graph is set by its NavHost, which is composed once the tab is shown.
            for (t in Tab.entries) {
                click(t.title)
                frames(300)
            }
            for ((t, n) in navs) graphs[t] = n.graph.routes()
        }
        for (t in Tab.entries) {
            assertEquals("$t's graph is the routes `Route` lists for it", Route.entries.filter { it.on(t) }.map { it.pattern }.toSet(), graphs[t])
        }
        // And every route is a restoration case.
        assertEquals(Route.entries.toSet(), Case.all.map { it.route }.toSet())
    }
}
