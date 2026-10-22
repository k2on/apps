// The listening session's ends that are not harken's player: Home's last
// played (`lastPlayed`) and the lock screen's player (`RemotePlayer`), whose
// every command is a transition — its volume too, which is what the phone's
// buttons and the system's volume panel reach while the sound is on a
// speaker. The picker and the scrub are the player's screens, tested in Rust
// (`harken/ffi/tests/screens.rs`). The session is the domain's, run by a
// harken server in the process (`Hall`, `harken/ffi`), and the devices real
// peers of the kernel.
//
// Each was falsified once by breaking what it holds; the comment beside
// each says how, and what failed.
package dev.harken.android

import android.os.Looper
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi
import dev.harken.android.onboarding.ArkSession
import androidx.media3.common.DeviceInfo
import dev.harken.android.gen.Clock
import dev.harken.android.gen.Equip
import dev.harken.android.gen.Harken
import dev.harken.android.gen.Output
import dev.harken.ffi.Hall
import dev.harken.screens.Ask
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class ListenTests {
    // For layoutlib's Looper, which the lock screen's player is built on.
    @get:Rule val paparazzi = Paparazzi(deviceConfig = DeviceConfig.PIXEL_5, theme = "android:Theme.Material.NoActionBar")

    private val queue = listOf(queuedTrack("a", 1), queuedTrack("b", 2), queuedTrack("c", 3))

    // A device's end of the session, as the app has it: a `Listen` over a session with no thread
    // of its own, so a press is previewed and published before it returns.
    private fun listen(hall: Hall, peer: Harken, session: String, name: String): Listen =
        Listen(ArkSession.inline(peer, "https://music.example.com", testLogin("alice", session)), name).also { hall.carry() }

    /**
     * Home's last played is the session's track while there is a session with one — what is
     * playing or paused on whichever device, and where — and otherwise the newest play in the log,
     * which is what a phone offline, or one whose session is idle, has to say.
     *
     * Falsified by preferring the log's play whenever there was one: Home said "Air" while the
     * session was playing "b" on the laptop.
     */
    @Test
    fun homesLastPlayedIsTheSessionsElseTheLogs() {
        val hall = Hall()
        val phone = hall.device("phone", true)
        val laptop = hall.device("laptop", true, "computer")
        val air = Played(queue[0].id, "Air", "Handel", "Water Music", 100_000, "music/air.mp3", 1, "#album/Water%20Music")
        val r = Presser("phone")
        val heard = { Heard(me = "phone", joined = r.joined(phone), listening = r.session(phone)) }
        val noArt: (String) -> String = { "" }

        val fromLog = lastPlayed(heard(), air, emptyList(), noArt, "https://m", "Phone")!!
        assertEquals("an idle session: the log's", "Air", fromLog.track.title)
        assertEquals(Page.ForAlbum("Water Music"), fromLog.from)
        assertFalse(fromLog.live)

        val lr = Presser("laptop")
        assertNull(lr.press(laptop, Press.Start(queue, 1, 0, "#album/Water%20Music")))
        hall.carry()
        val live = lastPlayed(heard(), air, emptyList(), noArt, "https://m", "Phone")!!
        assertEquals("the session's track", "b", live.track.title)
        assertTrue(live.live)
        assertEquals("and where it plays", "Playing on laptop", live.on)
        assertEquals(Page.ForAlbum("Water Music"), live.from)

        assertNull(lr.press(laptop, Press.Pause))
        hall.carry()
        assertNull("paused: no device to name", lastPlayed(heard(), air, emptyList(), noArt, "https://m", "Phone")!!.on)

        hall.drop(phone)
        assertEquals("offline: the log's again", "Air", lastPlayed(heard(), air, emptyList(), noArt, "https://m", "Phone")!!.track.title)
        assertNull("nothing anywhere: nothing", lastPlayed(heard(), null, emptyList(), noArt, "https://m", "Phone"))
    }

    /**
     * The lock screen's player while the sound is elsewhere: the session drawn — its track, its
     * queue, playing or not — and every command a transition: pause, a seek, next and previous, a
     * track of the queue, a queue set from outside, play next, and a headset's stop, which is a
     * pause. With nothing queued it is empty: no notification.
     *
     * Falsified by `Forward.seek` taking a seek within the track for a jump to it: the session
     * went back to the start of the track, playing, where the lock screen asked for 45 s, paused.
     * And by a stop that was answered with nothing: the session went on playing.
     */
    @Test
    fun theLockScreensPlayerForwardsEveryCommandAsATransition() {
        val hall = Hall()
        val phonePeer = hall.device("phone", true)
        val laptopPeer = hall.device("laptop", true, "computer")
        val phone = listen(hall, phonePeer, "phone", "Phone")
        val laptop = Presser("laptop")
        val player = RemotePlayer(Looper.myLooper() ?: Looper.getMainLooper())
        player.listen = phone
        player.show(phone.heard.value)
        assertEquals("nothing queued: empty, so no notification", 0, player.mediaItemCount)
        assertEquals(androidx.media3.common.Player.STATE_IDLE, player.playbackState)

        assertNull(laptop.press(laptopPeer, Press.Start(queue, 0, 30_000, "#album/Water%20Music")))
        hall.carry()
        phone.poke()
        player.show(phone.heard.value)
        assertEquals("the session's queue", 3, player.mediaItemCount)
        assertEquals("b", queue[1].title)
        assertTrue("playing, on the laptop", player.playWhenReady)
        assertTrue(player.currentPosition >= 30_000)

        fun session() = hall.state().active!!

        player.pause()
        hall.carry()
        assertFalse("pause is a transition", session().clock.playing)
        assertTrue("and the sound stayed where it was", hall.state().outputs("laptop"))
        player.show(phone.heard.value)
        player.seekTo(45_000)
        hall.carry()
        assertEquals("a seek is a seek", Clock.Paused(45_000), session().clock)
        player.play()
        hall.carry()
        assertTrue("play is play, on the laptop", session().clock.playing && hall.state().outputs("laptop"))
        player.show(phone.heard.value)
        player.seekToNext()
        hall.carry()
        assertEquals("next is a skip", 1L, session().at)
        player.show(phone.heard.value)
        player.seekToNextMediaItem()
        hall.carry()
        assertEquals(2L, session().at)
        player.show(phone.heard.value)
        player.seekTo(0, 0)
        hall.carry()
        assertEquals("a track of the queue is a jump", 0L, session().at)
        player.show(phone.heard.value)
        player.stop()
        hall.carry()
        assertFalse("a headset's stop is a pause", session().clock.playing)
        assertTrue("and never forgets the queue", hall.state().active != null)

        player.show(phone.heard.value)
        val d = queuedTrack("d", 4)
        player.setMediaItems(listOf(Items.of(d, "https://m", "", "#library")), 0, 0)
        hall.carry()
        assertEquals("a queue set from outside is a track pressed", listOf("d"), session().queue.map { it.title })
        assertEquals("#library", session().from)
        assertTrue("and, nothing sounding, plays here", hall.state().outputs("phone"))
        assertTrue(session().output is Output.Device)
    }

    // Three speakers of one family in the house — the kitchen at 40, the bedroom at 60, the study at
    // 20 — each saying what it can do as the bridge says it for them.
    private fun house(hall: Hall) {
        for ((id, level) in listOf("kitchen" to 40, "bedroom" to 60, "study" to 20)) {
            val p = hall.device(id, true, "speaker")
            p.equip(Equip(true, "sonos", false, level.toLong()))
            hall.carry()
        }
    }

    /**
     * The phone's buttons, while the sound is on a speaker: the session's player says it plays
     * remotely, 0 to 100, at the speaker's level, so the media session hands the buttons and the
     * system's panel to it — a press up or down is `nudge_volume` by five, the panel's slider
     * `set_volume`, its mute `set_mute`; while the speaker leads a group, the group's level and the
     * group's transitions. A car's own knob is the car's: nothing it asks reaches a speaker. And
     * with the sound on this phone it plays locally, so the buttons are the phone's own stream.
     *
     * Falsified by a player that said it played remotely whatever the output: with nothing playing,
     * the buttons were a remote's ("nothing plays: the buttons are the phone's"). And by a group's
     * buttons being the output's alone: the player stood at the kitchen's 70 where the group's 65
     * was wanted.
     */
    @Test
    fun theButtonsMoveTheSpeakerTheSoundIsOn() {
        val hall = Hall()
        val phonePeer = hall.device("phone", true)
        val phone = listen(hall, phonePeer, "phone", "Phone")
        // The phone has a volume of its own, as it says once it joins.
        phonePeer.equip(Equip(true, null, false, 50.toLong()))
        house(hall)
        val player = RemotePlayer(Looper.myLooper() ?: Looper.getMainLooper())
        player.listen = phone
        fun show() {
            hall.carry()
            phone.poke()
            player.show(phone.heard.value)
        }
        fun kitchen() = hall.state().devices.getValue("kitchen")
        fun bedroomOf() = hall.state().devices.getValue("bedroom")

        show()
        assertEquals("nothing plays: the buttons are the phone's", DeviceInfo.PLAYBACK_TYPE_LOCAL, player.deviceInfo.playbackType)
        phone.press(Press.Start(queue, 0, 0, ""))
        phone.ask(Ask.Pick("kitchen"))
        show()
        assertEquals("the sound on the kitchen: remote", DeviceInfo.PLAYBACK_TYPE_REMOTE, player.deviceInfo.playbackType)
        assertEquals(100, player.deviceInfo.maxVolume)
        assertEquals("at the kitchen's level", 40, player.deviceVolume)

        player.increaseDeviceVolume(0)
        show()
        assertEquals("a press up is a nudge of five", 45L, kitchen().volume)
        player.decreaseDeviceVolume(0)
        player.decreaseDeviceVolume(0)
        show()
        assertEquals(35L, kitchen().volume)
        assertEquals(35, player.deviceVolume)
        player.setDeviceVolume(70, 0)
        show()
        assertEquals("the panel's slider is a level", 70L, kitchen().volume)
        player.setDeviceMuted(true, 0)
        show()
        assertTrue("and its mute a mute", kitchen().muted && player.isDeviceMuted)
        assertEquals("the bedroom untouched", 60L, bedroomOf().volume)

        // The bedroom joins: the buttons are the group's.
        phone.ask(Ask.RowJoin("bedroom"))
        show()
        assertEquals("bedroom follows the kitchen", "kitchen", bedroomOf().follows)
        assertEquals("the group's level, the mean of its members'", 65, player.deviceVolume)
        assertFalse("not every member is muted", player.isDeviceMuted)
        player.increaseDeviceVolume(0)
        show()
        assertEquals("a press up is the group's nudge", 70, hall.state().groupLevel("kitchen"))
        assertTrue("the bedroom moved with it", bedroomOf().volume > 60)
        assertFalse("and moving a volume unmutes", kitchen().muted)

        // In a car, the knob is the car's.
        val before = hall.state().groupLevel("kitchen")
        player.fromCar = { true }
        player.increaseDeviceVolume(0)
        player.setDeviceVolume(10, 0)
        show()
        assertEquals("nothing a car asks of a volume reaches a speaker", before, hall.state().groupLevel("kitchen"))
        player.fromCar = { false }

        // The sound here: the buttons are the phone's own stream again.
        phone.ask(Ask.Pick("phone"))
        show()
        assertEquals("with the sound here, the buttons are the phone's", DeviceInfo.PLAYBACK_TYPE_LOCAL, player.deviceInfo.playbackType)
    }
}
