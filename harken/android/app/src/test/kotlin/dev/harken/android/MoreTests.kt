// The full player's More, whatever is playing: a song, a podcast's
// episode, a sermon, or something the library cannot place. Episodes are not
// among the library's songs (`library` leaves them out; they are their
// show's, `show`), and More looked for what plays among the songs alone, so
// for an episode it found nothing and did nothing. Here the episodes are a
// real kernel's, authored as the server's feeds author them; the player is
// the real `FullPlayer`, its More pressed through the semantics tree on
// Paparazzi's frame clock, and the menu it opens the real `TrackMenu`, as
// `HarkenApp` raises it (`Menu.playing`, `Shows.of`).
package dev.harken.android

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.harken.android.gen.AddEpisode
import dev.harken.android.gen.AddShow
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class MoreTests {
    @get:Rule
    val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", snapshotHandler = Discard)

    /** The frames are drawn and thrown away: what is asserted is what the menu offers. */
    private object Discard : SnapshotHandler {
        override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
            Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, _, _ -> null }
                as SnapshotHandler.FrameHandler

        override fun close() {}
    }

    private fun episode(show: String, title: String, at: Long) = AddEpisode(show, title, 1_800_000L, ".feeds/$title.mp3", title, at, "", "", "", 0, 0)

    private fun root(view: ComposeView): SemanticsNode = (view.getChildAt(0) as ViewRootForTest).semanticsOwner.rootSemanticsNode

    private fun all(n: SemanticsNode): Sequence<SemanticsNode> = sequenceOf(n) + n.children.asSequence().flatMap { all(it) }

    private fun texts(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.Text)?.map { it.text }.orEmpty()

    private fun press(view: ComposeView, node: (SemanticsNode) -> Boolean) {
        all(root(view)).first { node(it) && it.config.getOrNull(SemanticsActions.OnClick) != null }.config[SemanticsActions.OnClick].action!!.invoke()
    }

    /** The player's More, pressed. */
    private fun more(view: ComposeView) = press(view) { it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains("More") == true }

    /** A row of the menu, pressed. */
    private fun choose(view: ComposeView, label: String) = press(view) { label in texts(it) }


    /** What a menu offered, and where it went: read by `menu` once `script` has run. */
    private class Seen {
        var opened: Track? = null
        var went: Page? = null
        var said: Set<String> = emptySet()

        /** What went wrong in the script, if anything: a throw on the frame clock is otherwise lost. */
        var failed: Throwable? = null

        fun offered(label: String): Boolean = label in said
    }

    /**
     * The full player playing `now`, its More pressed, and what it opened: the menu the app raises
     * for it, over the player. `script` runs once the menu is up.
     */
    private fun menu(now: Playing, durationMs: Long, songs: List<Track>, shows: List<Show>, script: (ComposeView) -> Unit = {}): Seen {
        val seen = Seen()
        var done = false
        val view = ComposeView(paparazzi.context)
        view.setContent {
            HarkenTheme(dark = false) {
                var open by remember { mutableStateOf<Track?>(null) }
                val controls =
                    remember {
                        object : Controls {
                            // As HarkenApp's More: what plays, as a row, whatever it is.
                            override fun more() {
                                open = Menu.playing(now, durationMs, songs, shows)
                            }
                        }
                    }
                val actions =
                    remember {
                        object : MenuActions {
                            override fun goTo(page: Page) {
                                seen.went = page
                            }
                        }
                    }
                Column(Modifier.fillMaxSize()) {
                    Box(Modifier.weight(1f)) { FullPlayer(PlayerState(now = now, playing = true, durationMs = durationMs), controls) }
                    open?.let { TrackMenu(it, null, null, actions, Shows.of(it, shows)) }
                }
                LaunchedEffect(Unit) {
                    try {
                        repeat(30) { withFrameNanos {} }
                        more(view)
                        repeat(30) { withFrameNanos {} }
                        seen.opened = open
                        seen.said = all(root(view)).flatMap { texts(it).asSequence() }.toSet()
                        script(view)
                        repeat(10) { withFrameNanos {} }
                    } catch (e: Throwable) {
                        seen.failed = e
                    }
                    done = true
                }
            }
        }
        paparazzi.gif(view, "more", 0L, 2_000L, 60)
        seen.failed?.let { throw AssertionError("the script failed; the menu said ${seen.said}", it) }
        assertTrue("the script ran to its end", done)
        return seen
    }

    /**
     * An episode playing, More pressed: a menu opens — the episode's — with Go to Show, which goes
     * to its show; Mark as Played; and the queue's rows. And the same for a sermon.
     *
     * Falsified by More as it was, finding what plays among the songs alone: nothing opened.
     */
    @Test
    fun moreOnAnEpisodeOpensItsMenuWithGoToShow() = episodeMenu("podcast/full-time-nix", "podcast")

    @Test
    fun moreOnASermonOpensItsMenuWithGoToShow() = episodeMenu("sermon/gospel-in-life", "sermon")

    private fun episodeMenu(key: String, kind: String) {
        val h = alone()
        try {
            h.addSong(song(1))
            h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
            h.addShow(AddShow("sermon", "Gospel in Life", "Tim Keller", "Sermons.", "", "https://podcast.gospelinlife.com/feed.xml", ""))
            h.addEpisode(episode("podcast/full-time-nix", "Bump", 1))
            h.addEpisode(episode("sermon/gospel-in-life", "Living Grace", 2))
            val shows = Phone.once({ h.shows() }, Show::of)
            val songs = Phone.once({ Phone.library(h, null) }, Track::of)
            run {
                val ep = Phone.once({ Phone.show(h, key) }, Track::of).single()
                assertTrue("an episode is not among the songs", songs.none { it.id == ep.id })
                // As the player has it: the queue's, with no album, and its show's name for its maker.
                val now = Playing(ep.id.hex, ep.title, ep.creator, "", "", ep.file)
                val seen = menu(now, ep.durationMs, songs, shows) { view -> choose(view, "Go to Show") }
                val opened = seen.opened
                assertTrue("More opened a menu for the $kind", opened != null)
                assertEquals("the $kind's own", ep.id, opened!!.id)
                assertEquals("of its show's kind", kind, opened.kind)
                assertEquals("its file, to queue it by", ep.file, opened.file)
                for (row in listOf("Go to Show", "Mark as Played", "Play Next", "Add to Queue")) assertTrue("the $kind's menu offers $row", seen.offered(row))
                assertFalse("an episode has no artist to go to", seen.offered("Go to Artist"))
                assertEquals("Go to Show goes to its show", Page.ForShow(key), seen.went)
            }
        } finally {
            h.peer.close()
        }
    }

    /** A song plays: its own row, from the songs, and its menu as its row's — its artist, no show. */
    @Test
    fun moreOnASongOpensItsRowsMenu() {
        val hornpipe = Demo.tracks[0]
        val seen = menu(Demo.playing(hornpipe).copy(file = hornpipe.file), hornpipe.durationMs, Demo.tracks, Demo.shows)
        assertEquals(hornpipe, seen.opened)
        assertTrue(seen.offered("Go to Artist"))
        assertFalse(seen.offered("Go to Show"))
    }

    /**
     * Something the library cannot place — not among the songs, of no show it has — still opens a
     * menu, made of what the player says of it: never a dead tap.
     */
    @Test
    fun moreOnWhatTheLibraryCannotPlaceStillOpensAMenu() {
        val now = Playing("%032x".format(4242), "A Stray Recording", "Nobody Known", "", "", "music/stray.flac")
        val seen = menu(now, 61_000, Demo.tracks, Demo.shows)
        val opened = seen.opened
        assertTrue("a menu opened", opened != null)
        assertEquals("A Stray Recording", opened!!.title)
        assertEquals("music/stray.flac", opened.file)
        assertNull("of no show", Shows.of(opened, Demo.shows))
        assertTrue("its title heads it", seen.offered("A Stray Recording"))
        assertTrue(seen.offered("Play Next"))
    }
}
