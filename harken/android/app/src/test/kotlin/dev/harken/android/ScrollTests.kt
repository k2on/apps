// While a track plays, the player publishes its position twice a second, and
// that must move the progress under the mini player and nothing else: not
// the app's top, not the screen, not the mini player around it, no row of
// the list on screen. At rest — nothing playing, nothing arriving — nothing
// is composed again at all (`docs/plan-reactive.md`). The real `HarkenApp` —
// its library over a peer holding sixty songs authored through `add_song`,
// its wiring, its screens — on Paparazzi's frame clock, with the position
// ticked through `Playback.showPosition` where the media session would, over
// a session with no thread of its own so the peer and the frame clock cannot
// race. And the covers: the rows of one album on screen read the file and
// decode it once between them.
package dev.harken.android

import dev.harken.android.gen.AddSong
import dev.harken.android.gen.CreatePlaylist
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.platform.ComposeView
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
import dev.harken.android.ui.Recompositions
import java.io.File
import java.lang.reflect.Proxy
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class ScrollTests {
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
            title = "Song $i",
            artist = "Artist ${i / 10}",
            album = "Album ${i / 5}",
            durationMs = 180_000,
            file = "music/$i.flac",
            track = (i % 5 + 1).toLong(),
            part = "",
            catalogue = "",
            performer = "",
            bpm = 0,
            albumArt = "",
            artistArt = "",
            disc = 1,
            workTitle = "",
            movementNo = 0,
        )

    private val counts = ConcurrentHashMap<String, Int>()

    private fun count(what: String): Int = counts[what] ?: 0

    private var clock = 0L


    private suspend fun frames(ms: Long) {
        if (clock == 0L) clock = withFrameNanos { it }
        val target = clock + ms * 1_000_000
        while (clock < target) clock = withFrameNanos { it }
    }

    @Test
    fun thePositionTickingRecomposesNoRow() {
        val peer = alone()
        repeat(60) { peer.addSong(song(it)) }
        // No thread of its own: the library is read on this one, as the screen composes, so the
        // peer and the frame clock cannot race.
        val session = ArkSession.inline(peer, "https://music.example.com", testLogin("library", roles = listOf("library")))
        var playback: Playback? = null
        var rowsDrawn = -1
        var rowsDuring = -1
        var progressDuring = -1
        var atRest: Map<String, Int>? = null
        var during: Map<String, Int>? = null
        Recompositions.sink = { what -> counts.merge(what, 1, Int::plus) }
        try {
            val view = ComposeView(paparazzi.context)
            view.setContent {
                val owner = LocalLifecycleOwner.current
                val back =
                    object : OnBackPressedDispatcherOwner {
                        override val onBackPressedDispatcher = OnBackPressedDispatcher()
                        override val lifecycle = owner.lifecycle
                    }
                val store = remember { object : ViewModelStoreOwner { override val viewModelStore = ViewModelStore() } }
                CompositionLocalProvider(LocalViewModelStoreOwner provides store, LocalOnBackPressedDispatcherOwner provides back) {
                    HarkenApp(session, playbackOf = { c, s -> Playback(c, s, connect = false).also { playback = it } }, start = LibraryTab.Songs)
                }
                LaunchedEffect(Unit) {
                    frames(300)
                    rowsDrawn = count("TrackRow")
                    // A second at rest: nothing playing, nothing arriving.
                    counts.clear()
                    frames(1_000)
                    atRest = HashMap(counts)
                    val p = playback!!
                    p.show(PlayerState(now = Demo.playing(Demo.tracks[0]), playing = true, durationMs = 180_000))
                    p.showPosition(1_000)
                    frames(300)
                    counts.clear()
                    // Four of the player's ticks, half a second apart.
                    repeat(4) { k ->
                        p.showPosition(1_500L + 500L * k)
                        frames(500)
                    }
                    during = HashMap(counts)
                    rowsDuring = count("TrackRow")
                    progressDuring = count("Progress")
                }
            }
            paparazzi.gif(view, "ticks", 0L, 5_000L, 20)
        } finally {
            Recompositions.sink = null
            session.close()
            peer.peer.close()
        }
        println("ticks: at rest for a second $atRest; over four position ticks $during")
        assertTrue("rows were drawn before the ticks: $rowsDrawn", rowsDrawn > 0)
        assertEquals("composed at rest", emptyMap<String, Int>(), atRest)
        // The ticks did reach the screen: what shows the position was composed for each.
        assertTrue("the progress followed the position: $progressDuring", progressDuring >= 4)
        assertEquals("rows recomposed while the position ticked", 0, rowsDuring)
        // And nothing above it: per tick, the app's top, its frame, the screen and the mini player
        // around the hairline are each composed 0 times.
        for (what in listOf("HarkenApp", "Chrome", "LibraryScreen", "MiniPlayer", "CoverCard"))
            assertEquals("$what recomposed while the position ticked ($during)", 0, during!![what] ?: 0)
    }

    /** Eleven rows of one album ask for its cover at once: one read off the disk, one decode. */
    @Test
    fun oneAlbumsRowsShareOneReadAndOneDecode() {
        val server = "https://music.example.com"
        val covers = Covers(paparazzi.context, server)
        val url = covers.urlOf("covers/one.png")!!
        val dir = File(paparazzi.context.cacheDir, "covers")
        dir.mkdirs()
        val file = File(dir, "%08x".format(Art.hash(url)))
        file.writeBytes(java.util.Base64.getDecoder().decode(PNG))
        try {
            runBlocking { (1..11).map { async { covers.load(url, 56.dp) } }.awaitAll() }
            assertEquals("disk reads", 1, covers.reads.get())
            assertEquals("decodes", 1, covers.decodes.get())
        } finally {
            file.delete()
        }
    }

    private companion object {
        /** A 64×64 PNG, one colour. */
        const val PNG =
            "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAmElEQVR4nO3QMREAIBDAsBeGTjThCmRkoEP2XufsdX82OkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAK0BOkBrgA7QGqADtAboAO0BKKKSaMNRZVkAAAAASUVORK5CYII="
    }
}
