// The reconciler (Remote.kt) against harken's own session, as the desktop's
// tests hold `harken/iced/src/listening.rs`: real Kotlin peers, a server in
// the test (`Hall`) that runs the module's transitions and hooks, and a
// stand-in player. The six the desktop has — the output obeying and nobody
// else sounding, a track's end said once, a play recorded once by the output
// alone, both branches of coming back, and a device unlinked being its own
// session — and the phone's own: the player moving on by itself, a stall,
// a play counted offline, a press of the session, and its own volume.
//
// Each was falsified once by breaking what it holds; the comment beside
// each says how, and what failed.
package dev.harken.android

import dev.arkdb.client.Peer
import dev.harken.android.listening.Clock
import dev.harken.android.listening.Output
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RemoteTests {
    private val queue = listOf(queuedTrack("a", 1), queuedTrack("b", 2))

    private class Device(val hall: Hall, val peer: Peer, session: String, stream: FakeStream? = null) {
        val remote = Remote(session, session)
        val sound = FakeSound(stream)

        fun reconcile() {
            remote.reconcile(peer, sound)
            hall.carry()
        }

        fun call(transition: String, input: Map<String, dev.arkdb.Value>) {
            assertNull("$transition was refused", remote.call(peer, transition, input))
            hall.carry()
        }

        fun press(p: Press) {
            assertNull("$p was refused", remote.press(peer, p, sound))
            hall.carry()
        }

        fun session() = remote.session(peer)
    }

    private fun Hall.on(session: String, canPlay: Boolean, kind: String = "phone", stream: FakeStream? = null) =
        Device(this, device(session, canPlay, kind), session, stream)

    /**
     * The one rule this phone lives by: the output's player is made to agree with the session, and
     * every other device is silent. A press on the desktop — a remote, which cannot play — pauses
     * the phone, through nothing but the session; and a hand-off quiets it.
     *
     * Falsified by a reconcile that left a device that is not the output alone: the phone went on
     * playing after the sound moved to the laptop.
     */
    @Test
    fun theOutputObeysTheSessionAndNobodyElseMakesASound() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        val desk = hall.on("desktop", false, "computer")
        phone.call("start", Inputs.start(queue, 0, 0, "#album/Water%20Music"))
        phone.reconcile()
        assertEquals("the phone loaded what it pressed", queue, phone.sound.queue)
        assertTrue(phone.sound.playing)
        assertEquals(0, phone.sound.index)
        desk.reconcile()
        assertFalse("the desktop makes no sound", desk.sound.playing)
        assertEquals("and loads nothing", 0, desk.sound.loads)
        assertTrue(desk.remote.elsewhere(desk.session()!!))

        desk.call("pause", Inputs.none)
        phone.reconcile()
        assertFalse("a pause pressed on the desktop is a pause on the phone", phone.sound.playing)
        desk.call("seek", Inputs.seek(40_000))
        phone.reconcile()
        assertEquals("and a seek is a seek", 40_000L, phone.sound.positionMs)
        desk.call("play", Inputs.none)
        phone.reconcile()
        assertTrue("a play pressed on the desktop plays the phone", phone.sound.playing)

        val laptop = hall.on("laptop", true, "computer")
        desk.call("transfer", Inputs.transfer("laptop"))
        phone.reconcile()
        laptop.reconcile()
        assertFalse("the phone lost the sound and is silent", phone.sound.playing)
        assertTrue(laptop.sound.playing && laptop.sound.queue == queue)
        val s = laptop.session()!!
        assertTrue("the laptop said it had started", s.outputs("laptop") && !s.movingTo("laptop"))
        assertEquals("the phone loaded once", 1, phone.sound.loads)
    }

    /**
     * The phone's player holds the queue and moves on through it by itself: that is the end of a
     * track, said once as `advance`, and the player is not dragged back while it lands — and the
     * next track is not loaded again. The last one ending leaves the session paused at its start,
     * said once however often a player still at its end is reconciled; play there plays it again.
     *
     * Falsified by treating the player's own move as somebody else's: the reconciler jumped it
     * back, and the session was still on the first track. And by saying an end whenever the player
     * sits at one, neither remembering which end was said nor asking whether the session still
     * plays: the last track's end was said three times. Either alone holds it — the end's preview
     * pauses the session before the next reconcile — so each was taken away alone first, and passed.
     */
    @Test
    fun aTrackThatEndsIsSaidOnceAndTheNextOnePlays() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        phone.call("start", Inputs.start(queue, 0, 0, ""))
        phone.reconcile()
        phone.sound.finish()
        val calls = phone.remote.stats.calls
        phone.reconcile()
        phone.reconcile()
        assertEquals("one track on", 1L, phone.session()!!.active!!.at)
        assertEquals("said once", calls + 1, phone.remote.stats.calls)
        assertEquals("the player was not loaded again", 1, phone.sound.loads)
        assertEquals("nor dragged back", 0, phone.sound.jumps)
        assertTrue(phone.sound.playing)

        phone.sound.finish()
        val before = phone.remote.stats.calls
        repeat(3) { phone.reconcile() }
        assertEquals("the end of the queue, said once", before + 1, phone.remote.stats.calls)
        val a = phone.session()!!.active!!
        assertEquals(1L, a.at)
        assertEquals(Clock.Paused(0), a.clock)
        assertFalse(phone.sound.playing)
        phone.call("play", Inputs.none)
        phone.reconcile()
        assertTrue("and played again from its start", phone.sound.playing)
        assertEquals(0L, phone.sound.positionMs)
    }

    /**
     * The phone is right about where it is. ExoPlayer a second behind a clock nobody touched — it
     * started a moment after `started` was said, or its buffer ran dry — is followed: the session is
     * told where the player is (`report_position`), so every bar draws what is heard, and the player
     * is not sent a second ahead, which was a second of the track skipped. Within the bound nothing
     * is said; filling a buffer, nothing either; a seek on another device is the session moving, and
     * the player goes there.
     *
     * Falsified by the old rule, the player always sent where the clock counts: "not sent ahead to
     * where the clock counts" failed, the player sought a second on over what it had not played.
     */
    @Test
    fun theSessionFollowsThePhoneAndThePhoneFollowsASeek() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        val desk = hall.on("desktop", false, "computer")
        phone.call("start", Inputs.start(queue, 0, 40_000, ""))
        phone.reconcile()
        val loaded = phone.sound.positionMs
        assertTrue("loaded where it was pressed: $loaded", loaded in 40_000L until 40_100L)
        fun counted(): Long = phone.session()!!.active!!.position(phone.peer.serverNow())

        // A second on the clock, and the player has played none of it.
        Thread.sleep(1_000)
        hall.now = System.currentTimeMillis()
        val seeks = phone.sound.seeks
        phone.reconcile()
        assertEquals("not sent ahead to where the clock counts", loaded, phone.sound.positionMs)
        assertEquals(seeks, phone.sound.seeks)
        val at = counted()
        assertTrue("the session is where the player is: $at", kotlin.math.abs(at - loaded) <= Remote.SYNC_MS)
        val calls = phone.remote.stats.calls
        phone.reconcile()
        assertEquals("and says it once", calls, phone.remote.stats.calls)

        // Within the bound, nothing either way; filling a buffer, nothing either.
        phone.sound.positionMs = counted() - Remote.SYNC_MS / 2
        phone.reconcile()
        assertEquals(calls, phone.remote.stats.calls)
        assertEquals(seeks, phone.sound.seeks)
        phone.sound.positionMs = counted() - 3_000
        phone.sound.buffering = true
        phone.reconcile()
        assertEquals(calls, phone.remote.stats.calls)
        assertEquals(seeks, phone.sound.seeks)
        phone.sound.buffering = false
        phone.sound.positionMs = counted()

        // A seek on the desktop is the session moving: the player goes.
        hall.now = System.currentTimeMillis()
        desk.call("seek", Inputs.seek(100_000))
        phone.reconcile()
        assertTrue("sought where the session is: ${phone.sound.positionMs}", kotlin.math.abs(phone.sound.positionMs - 100_000) <= Remote.SYNC_MS)
        assertEquals("and nothing said back", calls, phone.remote.stats.calls)
    }

    /**
     * A play counts once it has played thirty seconds or half the track, once per load of a track,
     * and only on the device making the sound — and offline too, as a pending intent the server
     * gets later. Short of the threshold the reconciler asks to be woken when it will be reached,
     * since nothing here has a clock.
     *
     * Falsified by counting on every reconcile past the threshold: the second one authored a
     * second play. And by counting from the session's clock on every device part of it: the
     * laptop, watching, recorded one too. (Counting the laptop's own player first passed: a device
     * that is not the output has nothing loaded to count, which is a reason, not the rule.)
     */
    @Test
    fun aPlayIsRecordedOnceByTheOutputAlone() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        val laptop = hall.on("laptop", true, "computer")
        phone.call("start", Inputs.start(queue, 0, 29_000, "#album/Water%20Music"))
        phone.reconcile()
        val pending = phone.peer.pendingLen
        assertEquals("not yet: half of a hundred seconds is fifty, thirty is first", pending, phone.peer.pendingLen)
        assertEquals("a wake at the threshold", true, (phone.sound.wake ?: 0) in 900L..1_100L)
        phone.sound.positionMs = 30_500
        phone.call("seek", Inputs.seek(30_500))
        phone.reconcile()
        assertEquals("thirty seconds in: a play", pending + 1, phone.peer.pendingLen)
        phone.reconcile()
        assertEquals("once per play", pending + 1, phone.peer.pendingLen)
        laptop.sound.positionMs = 40_000
        laptop.reconcile()
        assertEquals("a device watching records nothing", 0L, laptop.remote.stats.plays)
        val recent = phone.peer.query("recently_played", emptyMap()).asList()
        assertEquals(1, recent.size)
        assertEquals("#album/Water%20Music", recent[0].field("from").asText())

        // Offline the phone is its own session, and a play is the log's all the same.
        hall.drop(phone.peer)
        phone.sound.jump(1, 0)
        phone.reconcile()
        phone.sound.positionMs = 31_000
        phone.reconcile()
        assertEquals("a play made offline, pending", pending + 2, phone.peer.pendingLen)
        assertEquals(2, phone.peer.query("recently_played", emptyMap()).asList().size)
    }

    /**
     * The decided rule for coming back, the first way: something is playing elsewhere when the
     * phone's link returns, so its own sound — it went on playing while it was away — stops, and
     * it says nothing.
     *
     * Falsified by a reconcile that says `start` whenever it comes back playing: the phone took the
     * sound from the laptop.
     */
    @Test
    fun comingBackToASessionSoundingElsewhereStopsThisPhone() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        val laptop = hall.on("laptop", true, "computer")
        phone.call("start", Inputs.start(queue, 0, 0, ""))
        phone.reconcile()
        laptop.reconcile()

        hall.drop(phone.peer)
        phone.reconcile()
        assertTrue("offline, its own session: it plays on", phone.sound.playing)
        laptop.call("skip", Inputs.skip(1))
        laptop.reconcile()
        assertTrue("the laptop took the sound: the phone that had it is away", laptop.sound.playing)

        hall.relink(phone.peer)
        phone.reconcile()
        assertFalse("something is playing elsewhere: this phone's own sound stops", phone.sound.playing)
        val s = phone.session()!!
        assertTrue("and the laptop plays on", s.outputs("laptop") && s.sounding)
        assertEquals(0L, phone.remote.stats.cameBackPlaying)
    }

    /**
     * …and the other way: nothing is sounding when it comes back, so what it was doing on its own
     * becomes the session — its queue, its track and where it had got to, on this phone — and its
     * own sound goes on without a reload.
     *
     * Falsified by a reconcile that only ever stops its sound on coming back: the session stayed
     * paused and away, and the phone went silent.
     */
    @Test
    fun comingBackPlayingToAQuietSessionMakesItThisOne() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        phone.call("start", Inputs.start(queue, 0, 0, ""))
        phone.reconcile()

        hall.drop(phone.peer)
        // On its own it moved to the second track and on into it.
        phone.sound.jump(1, 12_000)
        phone.sound.from = "#playlist/Favorites"
        phone.reconcile()
        hall.relink(phone.peer)
        phone.reconcile()
        phone.reconcile()
        assertTrue("its own sound goes on", phone.sound.playing)
        val s = phone.session()!!
        val a = s.active!!
        assertTrue(s.outputs("phone") && s.sounding)
        assertEquals(1L, a.at)
        assertEquals("#playlist/Favorites", a.from)
        assertTrue(a.clock is Clock.Playing && (a.clock as Clock.Playing).atMs == 12_000L)
        assertEquals(1L, phone.remote.stats.cameBackPlaying)
        assertEquals("not loaded again", 1, phone.sound.loads)
    }

    /**
     * Unlinked, nothing is called: a call made then would be said on the next connection, long after
     * it meant anything; and the stale session, which says playing, is not obeyed.
     *
     * Falsified by `joined` not asking whether the peer is linked: the offline reconcile drove the
     * player from the stale session and resumed it.
     */
    @Test
    fun aPhoneUnlinkedIsItsOwnSession() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        phone.call("start", Inputs.start(queue, 0, 0, ""))
        phone.reconcile()
        hall.drop(phone.peer)
        phone.sound.pause()
        val calls = phone.remote.stats.calls
        phone.reconcile()
        assertFalse("the stale session is not obeyed", phone.sound.playing)
        assertNull(phone.session())
        assertEquals("and nothing is called", calls, phone.remote.stats.calls)
        assertEquals(NOT_PART, phone.remote.press(phone.peer, Press.Pause, phone.sound))
    }

    /**
     * The player stopping by itself — focus taken by a call, the headphones out — is the session
     * pausing there, said once; the phone does not fight it by resuming.
     *
     * Falsified by not asking the player whether it stalled: the reconciler resumed it, and the
     * session went on saying playing.
     */
    @Test
    fun aStallIsSaidAndNotFought() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        phone.call("start", Inputs.start(queue, 0, 5_000, ""))
        phone.reconcile()
        phone.sound.playing = false
        phone.sound.stalled = true
        phone.sound.positionMs = 7_000
        phone.reconcile()
        phone.reconcile()
        assertFalse("not resumed", phone.sound.playing)
        assertEquals(Clock.Paused(7_000), phone.session()!!.active!!.clock)
    }

    /**
     * A press is a transition, and the three that need reading are read the desktop's way: the one
     * button pauses what sounds and otherwise plays; with nothing queued it plays what this phone
     * had; "play next" starts the queue again with the track after this one, where it was.
     *
     * Falsified by a toggle that always said `play`: the sounding session did not pause.
     */
    @Test
    fun aPressIsATransition() {
        val hall = Hall()
        val phone = hall.on("phone", true)
        val laptop = hall.on("laptop", true, "computer")
        phone.sound.own(queue, 1, 0, "#library")
        phone.press(Press.Play)
        assertEquals("nothing queued: this phone's own, pressed again", 1L, phone.session()!!.active!!.at)
        laptop.press(Press.Toggle)
        assertFalse("the one button pauses what sounds", phone.session()!!.sounding)
        laptop.press(Press.Toggle)
        assertTrue("and plays what does not", phone.session()!!.sounding)
        val c = queuedTrack("c", 3)
        laptop.press(Press.Next(c))
        val a = phone.session()!!.active!!
        assertEquals(listOf("a", "b", "c"), a.queue.map { it.title })
        assertEquals(1L, a.at)
        assertTrue(phone.session()!!.outputs("phone"))
        laptop.press(Press.Scrub(42_000))
        assertEquals("a scrub reaches the phone", 42_000L, phone.session()!!.scrub["laptop"])
        assertNotNull(Heard(me = "phone", joined = true, listening = phone.session()).scrubbing)
        assertEquals(42_000L, Heard(me = "phone", joined = true, listening = phone.session()).scrubbing)
        assertNull("its own slot is not another's", Heard(me = "laptop", joined = true, listening = phone.session()).scrubbing)
        assertTrue(phone.session()!!.active!!.output is Output.Device)
    }

    /**
     * The phone's own volume is its entry in the session: said once a connection (`equip`) — a
     * volume anybody can move, no family, at its media stream's level — and followed whoever moves
     * it; the stream moved by anything else — the buttons, the system's panel — is said once, as
     * `report_volume`. Never what the phone set itself: a stream has fifteen steps, so the 30 the
     * desktop asks for is the fifth, and the 33 that reads back is where the phone left it, not a
     * hand.
     *
     * Falsified by a follow that compared the stream with the level the session asked for rather
     * than with what it read after its own write: the phone said 33 back, and the desktop's 30
     * became 33 ("and the 33 it reads is not said back"). And by an equip said at every reconcile
     * rather than once a connection: "once a connection, not once a reconcile".
     */
    @Test
    fun thePhonesVolumeIsItsEntryAndOnlyAHandIsSaid() {
        val hall = Hall()
        val stream = FakeStream(index = 9)
        val phone = hall.on("phone", true, stream = stream)
        val desk = hall.on("desktop", false, "computer")
        fun me() = hall.state().devices.getValue("phone")
        assertFalse("nothing has a volume until it says so", me().canVolume)
        phone.reconcile()
        assertTrue("said once it joined", me().canVolume)
        assertEquals("at its stream's level: the ninth of fifteen steps", 60L, me().volume)
        assertNull("and no family: a phone joins no group", me().family)
        val calls = phone.remote.stats.calls
        repeat(3) { phone.reconcile() }
        assertEquals("once a connection, not once a reconcile", calls, phone.remote.stats.calls)
        assertEquals("and nothing written to the stream", 0, stream.writes)

        desk.call("set_volume", Volumes.call(Level.Device("phone"), Change.To(30))!!.second)
        phone.reconcile()
        assertEquals("a level set on the desktop is the stream's nearest step", 5, stream.index)
        repeat(3) { phone.reconcile() }
        assertEquals("and the 33 it reads is not said back", 30L, me().volume)
        assertEquals(calls, phone.remote.stats.calls)
        desk.call("set_mute", Volumes.call(Level.Device("phone"), Change.Mute(true))!!.second)
        phone.reconcile()
        assertTrue("a mute is a mute", stream.muted)
        assertEquals("the step left where it was", 5, stream.index)

        // The buttons, while the sound is here: the stream moved by a hand.
        stream.index = 12
        stream.muted = false
        repeat(3) { phone.reconcile() }
        assertEquals("said once", calls + 1, phone.remote.stats.calls)
        assertEquals(80L, me().volume)
        assertFalse(me().muted)
        assertEquals("and the desktop hears it", 80L, desk.session()!!.devices.getValue("phone").volume)
        assertEquals("not fought", 12, stream.index)

        // A new connection is a room that has never heard what this phone can do.
        hall.drop(phone.peer)
        hall.relink(phone.peer)
        assertFalse(me().canVolume)
        phone.reconcile()
        assertTrue("said again on the new connection", me().canVolume)
        assertEquals(80L, me().volume)
        assertEquals(calls + 2, phone.remote.stats.calls)
    }
}
