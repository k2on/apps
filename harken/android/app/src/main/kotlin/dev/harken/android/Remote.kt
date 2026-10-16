// This phone's end of the account's listening session: a reconciler, the
// Kotlin twin of the desktop's (`harken/iced/src/listening.rs`), and kept
// free of Android so that a test drives it with real peers and a stand-in
// player.
//
// The session is the domain's live state (`harken_domain::listening`), held
// by the server, one per account, on the same `/sync` socket as the log —
// what `Peer.live("listening")` hands back is the server's confirmed state
// with this phone's own calls replayed on top, read here through the types
// `arkc gen kotlin` prints of the module (`listening/Listening.kt`). **The
// state is the instruction**: nothing here is told what to do. Every
// button on every device is a transition ([Remote.press]); the device the
// session names as its output makes its player agree ([Remote.reconcile]);
// every other device is silent and draws it.
//
// What the output says back is only what happens to it: `started` once a
// hand-off is loaded, `advance` once per end of a track, `stalled` when the
// sound stopped by itself (the player says so: focus lost, the headphones
// out, a stream that failed — no timer here). No heartbeat: a playing clock
// carries the server's time it started from, and every device counts the
// position for itself against `Peer.serverNow`.
//
// Where the desktop loads one track at a time, the phone's player holds the
// whole queue, so that it plays on through it when the socket goes: a track
// ending is then the player moving on by itself, which is said as `advance`
// and waited for, never fought.
//
// **A phone unlinked is its own session**, as the desktop is: nothing is
// called while the socket is down or before the server's snapshot has come
// on a new one. Plays are the exception, because they are the log's: one is
// authored once a track has played thirty seconds or half its length, once
// per load, by the device making the sound — and offline it is a pending
// intent like any other. Coming back, the decided rule: sounding somewhere
// else, this phone's own sound stops and it is a controller; nothing
// sounding and this phone was playing on its own, its queue, track and
// position become the session — a `start`, which hands it the sound.
package dev.harken.android

import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.client.LiveStatus
import dev.arkdb.client.Peer
import dev.arkdb.client.PeerError
import dev.harken.android.listening.Clock
import dev.harken.android.listening.Listening
import dev.harken.android.listening.Output
import dev.harken.android.listening.Session
import dev.harken.android.listening.Track as Queued
import kotlin.math.abs

/** The live state's name. */
const val LISTENING = "listening"

/** What a press is answered with while this phone is its own session: the player's, then. */
const val NOT_PART = "not part of the session"

// The session read: the generated types say what is there, these what it means.

/** How far into the track at `now`, the server's clock. */
fun Clock.position(now: Long): Long =
    when (this) {
        is Clock.Paused -> atMs
        is Clock.Playing -> atMs + maxOf(now - since, 0)
    }

val Clock.playing: Boolean
    get() = this is Clock.Playing

/** The device an output names, here or not; none for nowhere. */
val Output.id: String?
    get() =
        when (this) {
            is Output.Device -> id
            is Output.Away -> id
            Output.Nowhere -> null
        }

/** The queue and where in it, when anything is queued. */
val Listening.active: Session.Active?
    get() = session as? Session.Active

val Session.Active.now: Queued
    get() = queue[at.toInt().coerceIn(0, queue.size - 1)]

/** Whether `id` is the output and is here. */
fun Listening.outputs(id: String): Boolean = (active?.output as? Output.Device)?.id == id

/** Something is making a sound: a clock running on an output that is here. */
val Listening.sounding: Boolean
    get() = active?.let { it.output is Output.Device && it.clock.playing } == true

/** The sound is on its way to `id`, which has not said it has started. */
fun Listening.movingTo(id: String): Boolean = (active?.output as? Output.Device)?.let { it.id == id && it.moving } == true

/** The inputs, as the transitions take them (`read::input`). */
object Inputs {
    val none: Map<String, Value> = emptyMap()

    fun start(queue: List<Queued>, at: Int, positionMs: Long, from: String): Map<String, Value> =
        mapOf(
            "at" to Value.int(at.toLong()),
            "from" to Value.text(from),
            "position" to Value.int(maxOf(positionMs, 0)),
            "queue" to Value.list(queue.map { it.toValue() }),
        )

    fun seek(ms: Long): Map<String, Value> = mapOf("ms" to Value.int(maxOf(ms, 0)))

    fun skip(by: Int): Map<String, Value> = mapOf("by" to Value.int(by.toLong()))

    fun jump(index: Int): Map<String, Value> = mapOf("index" to Value.int(index.toLong()))

    fun transfer(to: String): Map<String, Value> = mapOf("to" to Value.text(to))

    /** `started` and `stalled`. */
    fun at(atMs: Long): Map<String, Value> = mapOf("at_ms" to Value.int(maxOf(atMs, 0)))

    fun advance(fromTrack: Int): Map<String, Value> = mapOf("from_track" to Value.int(fromTrack.toLong()))

    fun scrub(atMs: Long?): Map<String, Value> = mapOf("at_ms" to Value.opt(atMs?.let { Value.int(maxOf(it, 0)) }))

    fun recordPlay(t: Queued, from: String): Map<String, Value> =
        mapOf(
            "album" to Value.text(t.album),
            "creator" to Value.text(t.creator),
            "duration_ms" to Value.int(maxOf(t.durationMs, 0)),
            "file" to Value.text(t.file),
            "from" to Value.text(from),
            "title" to Value.text(t.title),
            "track" to Value.id(t.id),
        )

    /**
     * How long a track has to have played before it counts as a play: thirty seconds or half its
     * length, whichever is first (`read::counts_after`).
     */
    fun countsAfter(durationMs: Long): Long = if (durationMs <= 0) 30_000 else minOf(durationMs / 2, 30_000)
}

/** What sounds, as the reconciler sees it: the phone's ExoPlayer, or a test's. */
interface Sound {
    /** The queue loaded, in order: what this phone plays through on its own. */
    val queue: List<Queued>

    /** Where in it, or -1 with nothing loaded. */
    val index: Int

    /** Asked to play — Media3's `playWhenReady` — whether or not a buffer is still filling. */
    val playing: Boolean

    /** Filling a buffer: where it is cannot be compared with where it should be. */
    val buffering: Boolean

    val positionMs: Long

    /** At the end of the last track of its queue. */
    val ended: Boolean

    /** Where the queue loaded was taken from (`#album/…`), as whoever loaded it said. */
    val from: String

    /** Whether the player stopped by itself since the last ask: a stall, said once. */
    fun takeStalled(): Boolean

    /** Load this queue, at this track and point, playing or not. */
    fun load(queue: List<Queued>, at: Int, positionMs: Long, playing: Boolean, from: String)

    /** Another track of the queue loaded, at this point. */
    fun jump(at: Int, positionMs: Long)

    fun resume()

    fun pause()

    fun seek(ms: Long)

    /** Reconcile again in `ms`: the moment a play would count. Nothing here has a clock. */
    fun wakeIn(ms: Long) {}
}

/** What a button, a key, a row or a lock screen asks of the session — or, alone, of the player. */
sealed class Press {
    object Play : Press()

    object Pause : Press()

    /** The one button that is both: pause what sounds, else play. */
    object Toggle : Press()

    data class Skip(val by: Int) : Press()

    data class Seek(val ms: Long) : Press()

    /** A track of the queue, from its start. */
    data class Jump(val index: Int) : Press()

    /** A track pressed: this queue, from here. */
    data class Start(val queue: List<Queued>, val at: Int, val positionMs: Long, val from: String) : Press()

    /** Put `track` after what is playing: the queue started again, with it next, where it was. */
    data class Next(val track: Queued) : Press()

    /** Where this phone's scrubber is being dragged to, or none: ephemeral, and throttled. */
    data class Scrub(val atMs: Long?) : Press()

    /** The sound to another device. */
    data class Transfer(val to: String) : Press()
}

/** Every number this end of the session holds that the peer does not, for the diagnostics. */
data class RemoteStats(
    /** Transitions called, and refused by the preview or the server. */
    val calls: Long = 0,
    val refused: Long = 0,
    /** `record_play`s authored. */
    val plays: Long = 0,
    /** `start`s said coming back with something of its own playing. */
    val cameBackPlaying: Long = 0,
    val lastRefusal: String? = null,
)

/**
 * This phone's end of the account's listening session. Touched on the peer's thread only, as the
 * peer is.
 */
class Remote(
    /** This device: its login's session id, which is what a `hello` with an empty id declares. */
    val me: String,
    /** What this phone is called in somebody else's picker. */
    val name: String,
) {
    /** The connection whose snapshot this phone has met: below the peer's epoch, its own session. */
    private var met = 0L

    /** The player was playing while this phone was its own session. */
    private var alonePlaying = false

    /** The hand-off `started` was said for: the track it was loaded on. */
    private var startedFor: Pair<Long, Id>? = null

    /** The end of a track `advance` was said for. */
    private var advancedFor: Pair<Long, Id>? = null

    /** Where this reconciler last put the player in its queue: a move it did not make is the player's own. */
    private var expect = -1

    /** Every load of a track is a play: the track loaded, this load's number, and the last counted. */
    private var loaded: Pair<Int, Id>? = null
    private var play = 0L
    private var counted = 0L

    var stats = RemoteStats()
        private set

    /** Whether this phone is part of the session now: linked, as itself, and the server's snapshot met. */
    fun joined(peer: Peer): Boolean = peer.linked && peer.ctx.session == me && peer.liveStatus(LISTENING)?.synced == true

    /** The session, when there is one to be part of; null is this phone being its own session. */
    fun session(peer: Peer): Listening? {
        if (!joined(peer)) return null
        return peer.live(LISTENING)?.let { v -> runCatching { Listening.of(v) }.getOrNull() }
    }

    /** The live instance's numbers, for the diagnostics. */
    fun status(peer: Peer): LiveStatus? = peer.liveStatus(LISTENING)

    /** Whether the sound belongs to another device — here or not — which is when this phone is silent. */
    fun elsewhere(s: Listening): Boolean = s.active?.output?.id?.let { it != me } == true

    /**
     * Call a transition. A refusal — the preview's, here — is counted and handed back; the
     * server's arrive later ([refusals]).
     */
    fun call(peer: Peer, transition: String, input: Map<String, Value>): String? {
        stats = stats.copy(calls = stats.calls + 1)
        return try {
            if (transition == "scrub") peer.callThrottled(LISTENING, transition, input) else peer.call(LISTENING, transition, input)
            null
        } catch (e: PeerError) {
            val why = e.message ?: "refused"
            stats = stats.copy(refused = stats.refused + 1, lastRefusal = "$transition: $why")
            why
        }
    }

    /** What the server refused of this phone's calls since the last ask. */
    fun refusals(peer: Peer): List<String> {
        val out = peer.takeLiveRefusals().map { it.reason }
        if (out.isNotEmpty()) stats = stats.copy(refused = stats.refused + out.size, lastRefusal = out.last())
        return out
    }

    /**
     * A press, as a transition — or, unlinked, nothing: the caller does it to the player, which is
     * this phone's own session. `local` is that player, whose queue a press plays when the session
     * has none. Null when it was taken; the refusal, in a sentence, when it was not.
     */
    fun press(peer: Peer, press: Press, local: Sound?): String? {
        val s = session(peer) ?: return NOT_PART
        val a = s.active
        val (transition, input) =
            when (press) {
                Press.Toggle ->
                    when {
                        s.sounding -> "pause" to Inputs.none
                        a != null -> "play" to Inputs.none
                        else -> startLocal(local) ?: return "nothing to play"
                    }
                Press.Play -> if (a != null) "play" to Inputs.none else startLocal(local) ?: return "nothing to play"
                Press.Pause -> "pause" to Inputs.none
                is Press.Skip -> "skip" to Inputs.skip(if (press.by < 0) -1 else 1)
                is Press.Seek -> "seek" to Inputs.seek(press.ms)
                is Press.Jump -> "jump" to Inputs.jump(press.index)
                is Press.Start -> "start" to Inputs.start(press.queue, press.at, press.positionMs, press.from)
                is Press.Scrub -> "scrub" to Inputs.scrub(press.atMs)
                is Press.Transfer -> "transfer" to Inputs.transfer(press.to)
                is Press.Next -> {
                    if (a == null) return call(peer, "start", Inputs.start(listOf(press.track), 0, 0, ""))
                    // There is no insert: the queue is started again with it next, where it was —
                    // and, if it was paused, paused again, in order, so nothing sounds.
                    val at = a.at.toInt()
                    val queue = a.queue.take(at + 1) + press.track + a.queue.drop(at + 1)
                    val why = call(peer, "start", Inputs.start(queue, at, a.clock.position(peer.serverNow()), a.from))
                    if (why != null || a.clock.playing) return why
                    "pause" to Inputs.none
                }
            }
        return call(peer, transition, input)
    }

    // Nothing queued in the session: what this phone last had, pressed again.
    private fun startLocal(local: Sound?): Pair<String, Map<String, Value>>? {
        val l = local ?: return null
        if (l.queue.isEmpty()) return null
        return "start" to Inputs.start(l.queue, l.index.coerceIn(0, l.queue.size - 1), 0, l.from)
    }

    /**
     * Make this phone agree with the session, and say what happened to it. Called after every
     * change the peer reports and every event the player raises.
     */
    fun reconcile(peer: Peer, sound: Sound) {
        if (!joined(peer)) {
            // Its own session. Whether it was playing on its own is what matters when it comes
            // back; and a play is the log's, so it counts here too.
            alonePlaying = sound.playing
            count(peer, sound, sound.queue.getOrNull(sound.index), sound.from)
            return
        }
        val s = session(peer) ?: return
        if (met != peer.epoch) {
            // The first sight of the session on this connection: the rule for coming back.
            met = peer.epoch
            if (alonePlaying && sound.queue.isNotEmpty()) {
                alonePlaying = false
                if (!(s.sounding && !s.outputs(me))) {
                    val at = sound.index.coerceIn(0, sound.queue.size - 1)
                    if (call(peer, "start", Inputs.start(sound.queue, at, sound.positionMs, sound.from)) == null)
                        stats = stats.copy(cameBackPlaying = stats.cameBackPlaying + 1)
                    // Its own sound goes on: the session it has just made says what it is doing.
                    expect = at
                    return
                }
                // Somebody else is playing: this phone is a controller of it now, and its own
                // sound stops below.
            }
        }
        val now = session(peer) ?: s
        val a = now.active
        if (a != null && now.outputs(me)) {
            drive(peer, sound, a)
            return
        }
        // Not the output: silent. Losing the sound is not something this phone does; it is told,
        // and this is the one place that can notice.
        if (sound.playing) sound.pause()
        startedFor = null
    }

    // This phone is the output: its player made to agree with the session, and what happened to
    // it said.
    private fun drive(peer: Peer, sound: Sound, a: Session.Active) {
        val now = peer.serverNow()
        val at = a.at.toInt()
        val track = a.now
        val here = a.at to track.id
        val want = a.clock.position(now)
        if (sound.queue.map { it.id } != a.queue.map { it.id }) {
            // Another queue: the session's, loaded as this phone's own, so that going offline
            // mid-track leaves it playing what it was.
            sound.load(a.queue, at, want, a.clock.playing, a.from)
            expect = at
            advancedFor = null
        } else if (sound.index != at) {
            when {
                // The end it said has not landed yet: wait for it rather than fight the player.
                advancedFor == here -> return
                // The player moved on by itself: the track the session is at ended.
                sound.index == expect + 1 && expect == at && a.clock.playing -> {
                    advancedFor = here
                    call(peer, "advance", Inputs.advance(at))
                    return
                }
                // Somebody moved the session: a skip, a jump, a track pressed.
                else -> {
                    sound.jump(at, want)
                    expect = at
                    advancedFor = null
                }
            }
        } else {
            expect = at
        }
        // The hand-off has landed when the track is loaded: said once.
        val moving = (a.output as? Output.Device)?.moving == true
        if (moving && startedFor != here) {
            startedFor = here
            call(peer, "started", Inputs.at(sound.positionMs))
            return
        }
        if (!moving) startedFor = null
        // The last track ended: said once. Once said, a player still at its end is reconciled like
        // any other — played again from the start if somebody presses play.
        if (sound.ended && a.clock.playing && advancedFor != here) {
            advancedFor = here
            call(peer, "advance", Inputs.advance(at))
            return
        }
        // It stopped by itself: the session is told, and pauses there.
        if (sound.takeStalled() && a.clock.playing) {
            call(peer, "stalled", Inputs.at(sound.positionMs))
            return
        }
        when {
            a.clock.playing && !sound.playing -> sound.resume()
            !a.clock.playing && sound.playing -> sound.pause()
        }
        // Somewhere else in the track than the session counts: a seek made on another device.
        if (!sound.buffering && abs(sound.positionMs - want) > DRIFT_MS) sound.seek(want)
        count(peer, sound, track, a.from)
    }

    // A play, once it has played long enough, once per load of a track — and, short of that, a
    // wake at the moment it will have.
    private fun count(peer: Peer, sound: Sound, track: Queued?, from: String) {
        if (track == null) return
        val current = sound.index to track.id
        if (loaded != current) {
            loaded = current
            play += 1
        }
        if (counted == play || !sound.playing) return
        val due = Inputs.countsAfter(track.durationMs) - sound.positionMs
        if (due > 0) return sound.wakeIn(due)
        counted = play
        try {
            peer.mutate("record_play", Inputs.recordPlay(track, from))
            stats = stats.copy(plays = stats.plays + 1)
        } catch (e: PeerError) {
            stats = stats.copy(lastRefusal = "record_play: ${e.message}")
        }
    }

    companion object {
        /**
         * How far the player may be from where the session counts it before the output seeks to
         * agree: a seek on another device moves the clock by more than this, and a player's own
         * jitter never does.
         */
        const val DRIFT_MS = 1_500L
    }
}

/** One row of the picker: a device of this account, and what it can be asked. */
data class DeviceRow(
    val id: String,
    val name: String,
    val kind: String,
    /** Here and able to make a sound: a row that can be picked. */
    val takeable: Boolean,
    /** The device the sound still belongs to, whose socket has gone. */
    val away: Boolean,
    /** The sound is on its way to it. */
    val moving: Boolean,
    /** Where the sound is — or, while it is nowhere, this phone, where the next press plays. */
    val ticked: Boolean,
    val me: Boolean,
)

/** The picker, as the desktop's (23c40a0): no row that stops it everywhere; this phone the default. */
object Picker {
    /**
     * A row per device of the account, by name — and the output that has gone, which is the one
     * row most worth drawing, drawn and not selectable.
     */
    fun rows(s: Listening, me: String): List<DeviceRow> {
        val output = s.active?.output
        val ticked = output?.id ?: me
        val rows =
            s.devices.values.map { d ->
                DeviceRow(d.id, d.name, d.kind, d.canPlay, away = false, moving = s.movingTo(d.id), ticked = d.id == ticked, me = d.id == me)
            }.toMutableList()
        if (output is Output.Away && rows.none { it.id == output.id })
            rows.add(DeviceRow(output.id, output.name, "computer", takeable = false, away = true, moving = false, ticked = true, me = output.id == me))
        return rows.sortedWith(compareBy<DeviceRow> { it.name }.thenBy { it.id })
    }

    /** Where the cursor starts: on the output, or on this phone, so that a tap straight away changes nothing. */
    fun cursor(rows: List<DeviceRow>): Int = rows.indexOfFirst { it.ticked }.takeIf { it >= 0 } ?: 0

    /** What picking `row` asks: a hand-off, or nothing — the device that has it, or one that cannot. */
    fun pick(row: DeviceRow, s: Listening?): Press? {
        if (!row.takeable || s?.active == null) return null
        if (s.outputs(row.id) && !s.movingTo(row.id)) return null
        return Press.Transfer(row.id)
    }
}
