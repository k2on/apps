// The session is the process's, not the activity's (Kept.kt,
// `onboarding/Sessions.kt`): the activity going — its screens
// disposed, its hold let go — while the phone is the account's output leaves
// the phone in the session, as the output, playing, and another device sees
// nothing move. Over the process's `Sessions` with a session that is a real
// peer in a harken server in the process (`Hall`), whose socket goes when the
// session closes, the real `HarkenApp` composed and disposed, and the media
// service's player a stand-in.
package dev.harken.android

import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.ComposeView
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import app.cash.paparazzi.Snapshot
import app.cash.paparazzi.SnapshotHandler
import dev.harken.android.onboarding.ArkSession
import dev.harken.android.onboarding.Sessions
import dev.harken.android.gen.Output
import dev.harken.ffi.Hall
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class KeptTests {
    @get:Rule
    val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar", snapshotHandler = Discard)

    /** The frame is drawn and thrown away: what is asserted is the session. */
    private object Discard : SnapshotHandler {
        override fun newFrameHandler(snapshot: Snapshot, frameCount: Int, fps: Int): SnapshotHandler.FrameHandler =
            Proxy.newProxyInstance(SnapshotHandler::class.java.classLoader, arrayOf(SnapshotHandler.FrameHandler::class.java)) { _, _, _ -> null }
                as SnapshotHandler.FrameHandler

        override fun close() {}
    }

    private val queue = listOf(queuedTrack("a", 1), queuedTrack("b", 2), queuedTrack("c", 3))
    private val server = "https://music.example.com"

    /**
     * The activity destroyed while the phone is the output: the screens disposed and the app's hold
     * let go, the media service's hold kept. The phone stays in the session, as its output, its
     * player playing and still reconciled — a pause on the laptop reaches it — and the laptop never
     * sees it away or paused. Then the service goes too, nothing holding the session: it closes, as
     * it always did, and the laptop sees the phone away.
     *
     * Falsified twice. A release that closed the session whatever else held it: "the phone left
     * the session with the activity" — the laptop saw it away, paused. And the app's screens
     * closing the listening session on their way out, as they did: "the player is no longer
     * reconciled" — a pause on the laptop left the phone playing.
     */
    @Test
    fun theActivityGoingLeavesThePhoneInTheSessionPlaying() {
        val hall = Hall()
        val phone = hall.device("phone", true)
        val laptop = hall.device("laptop", true, "computer")
        val login = testLogin("alice", "phone")
        var opened = 0
        // The process's sessions, opening the phone's peer: its socket goes when the session closes.
        val sessions =
            Sessions(open = { s, l, _ ->
                opened++
                ArkSession.inline(phone, s, l).also { it.kept<AutoCloseable>("socket") { AutoCloseable { if (phone.linked) hall.drop(phone) } } }
            })

        // The app opens, the media service binds: one session, held twice.
        val app = sessions.hold(server, login, "app")
        val service = sessions.hold(server, login, "playback")
        assertSame("one session for both", app.session, service.session)
        assertEquals(1, opened)
        val session = app.session
        val kept = Kept.of(session, "Phone")
        assertSame("one library and one listening, whoever asks", kept, Kept.of(session, "Phone"))
        val sound = FakeSound()
        kept.listen.attach(sound)

        // The screens, as the activity has them: composed over the session, then gone with it.
        var playback: Playback? = null
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
                HarkenApp(session, playbackOf = { c, s -> Playback(c, s, connect = false).also { playback = it } })
            }
        }

        // The phone plays: a track pressed in the app, the session hands it the sound.
        kept.listen.press(Press.Start(queue, 0, 0, "#album/Water%20Music"))
        hall.carry()
        kept.listen.poke()
        hall.carry()
        assertTrue("the phone is the output", hall.state().outputs("phone"))
        assertTrue("and it plays", sound.playing && hall.state().active!!.clock.playing)

        // The activity goes: its composition disposed — a snapshot draws it and lets it go, as a
        // window that closes does — and its hold let go.
        paparazzi.snapshot(view)
        view.disposeComposition()
        assertTrue("the screens were composed", playback != null)
        app.release()
        hall.carry()

        assertTrue("the phone left the session with the activity", phone.linked)
        assertFalse("the session is open", session.isClosed)
        assertEquals("the service holds it still", listOf("playback"), sessions.holders(session))
        val seen = Remote("laptop", "Laptop").session(laptop)!!
        val output = seen.active!!.output
        assertTrue("the laptop sees the phone as the output, here: $output", output is Output.Device && output.id == "phone")
        assertTrue("and playing", seen.active!!.clock.playing)
        assertTrue("the phone's player plays on", sound.playing)

        // Still reconciled: a pause on the laptop reaches the phone's player.
        assertEquals(null, Remote("laptop", "Laptop").press(laptop, Press.Pause, null))
        hall.carry()
        kept.listen.poke()
        assertFalse("the player is no longer reconciled", sound.playing)

        // The service goes too: nothing holds it, and it closes as it always did.
        service.release()
        assertTrue("closed with its last hold", session.isClosed)
        assertFalse("its socket went with it", phone.linked)
        val after = hall.state().active!!.output
        assertTrue("the laptop sees the phone away: $after", after is Output.Away)
        phone.peer.close()
        laptop.peer.close()
    }
}
