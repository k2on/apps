// This phone's end of the account's listening session: a reconciler, the
// Kotlin twin of the desktop's (`harken/iced/src/listening.rs`), and kept
// free of Android so that it can be driven off a phone with a stand-in
// player.
//
// The session is the domain's live state (`harken_domain::listening`): three
// volatile tables the server's second machine holds (§1.9), one session per
// account, on the same `/sync` socket as the log — what `harken_listening`
// hands back is the server's confirmed state with this phone's own calls
// previewed on top, read here through the types the domain generates
// (`gen/Harken.kt`). **The state is the instruction**: nothing here is told
// what to do. Every button on every device is a transition ([Remote.press]);
// the device the session names as its output makes its player agree
// ([Remote.reconcile]); every other device is silent and draws it.
//
// What the output says back is only what happens to it: `started` once a
// hand-off is loaded, `advance` once per end of a track, `stalled` when the
// sound stopped by itself (the player says so: focus lost, the headphones
// out, a stream that failed — no timer here). No heartbeat: a playing clock
// carries the time it started from, and every device counts the position
// for itself against its own clock — an entry carries the clock of whoever
// authored it.
//
// Where the desktop loads one track at a time, the phone's player holds the
// whole queue, so that it plays on through it when the socket goes: a track
// ending is then the player moving on by itself, which is said as `advance`
// and waited for, never fought.
//
// **A phone unlinked is its own session**, as the desktop is: nothing is
// called while the socket is down or before this phone stands in the
// session on a new one (its `on_join` has landed). Plays are the exception,
// because they are the log's: one is authored once a track has played
// thirty seconds or half its length, once per load, by the device making
// the sound — and offline it is a pending intent like any other. Coming
// back, the decided rule: sounding somewhere else, this phone's own sound
// stops and it is a controller; nothing sounding and this phone was playing
// on its own, its queue, track and position become the session — a
// `start`, which hands it the sound.
package dev.harken.android

import dev.arkdb.ffi.Id
import dev.arkdb.ffi.Refusal
import dev.harken.android.gen.Active
import dev.harken.android.gen.Advance
import dev.harken.android.gen.At
import dev.harken.android.gen.Clock
import dev.harken.android.gen.Enqueue
import dev.harken.android.gen.Harken
import dev.harken.android.gen.Jump
import dev.harken.android.gen.Member
import dev.harken.android.gen.Move
import dev.harken.android.gen.Output
import dev.harken.android.gen.Present
import dev.harken.android.gen.RecordPlay
import dev.harken.android.gen.Remove
import dev.harken.android.gen.Report
import dev.harken.android.gen.Scrub
import dev.harken.android.gen.Seek
import dev.harken.android.gen.Skip
import dev.harken.android.gen.Start
import dev.harken.android.gen.Track as Queued
import dev.harken.android.gen.Transfer
import dev.harken.ffi.harkenListening
import kotlin.math.abs
import org.json.JSONObject

/** What a press is answered with while this phone is its own session: the player's, then. */
const val NOT_PART = "not part of the session"

/**
 * The account's listening session as this phone holds it (`harken_listening`): every device
 * standing in it, by id; where each one's scrubber is being dragged; and what is queued, if anything.
 */
data class Listening(
    val devices: Map<String, Present> = emptyMap(),
    val scrub: Map<String, Long> = emptyMap(),
    val active: Active? = null,
) {
    companion object {
        fun parse(json: String): Listening {
            val o = JSONObject(json)
            val ds = o.getJSONArray("devices")
            val sc = o.getJSONObject("scrub")
            val session = o.opt("session")
            return Listening(
                devices = List(ds.length()) { Present.of(ds.get(it)) }.associateBy { it.id },
                scrub = sc.keys().asSequence().associateWith { sc.getLong(it) },
                active = if (session == null || session == JSONObject.NULL) null else Active.of(session),
            )
        }
    }
}

// The session read: the generated types say what is there, these what it means.

/** How far into the track at `now`. */
fun Clock.position(now: Long): Long =
    when (this) {
        is Clock.Paused -> atMs
        is Clock.Playing -> atMs + maxOf(now - since, 0)
    }

val Clock.playing: Boolean
    get() = this is Clock.Playing

/**
 * Where the session stands at `now`: the clock's position — except while the sound is on its way to
 * a device (`moving`), when it stands where it was handed over until that device says it has
 * started. The domain's `standing`, read the same way: what the bar draws, and where a phone the
 * sound is handed to loads the track.
 */
fun Active.position(now: Long): Long =
    if ((output as? Output.Device)?.moving == true) {
        when (val c = clock) {
            is Clock.Paused -> c.atMs
            is Clock.Playing -> c.atMs
        }
    } else clock.position(now)

/** Whether its position is moving: playing, and not waiting on a hand-off. */
val Active.counting: Boolean
    get() = clock.playing && (output as? Output.Device)?.moving != true

/** The device an output names, here or not; none for nowhere. */
val Output.id: String?
    get() =
        when (this) {
            is Output.Device -> id
            is Output.Away -> id
            Output.Nowhere -> null
        }

val Active.now: Queued
    get() = queue[at.toInt().coerceIn(0, queue.size - 1)]

/** Whether `id` is the output and is here. */
fun Listening.outputs(id: String): Boolean = (active?.output as? Output.Device)?.id == id

/** Something is making a sound: a clock running on an output that is here. */
val Listening.sounding: Boolean
    get() = active?.let { it.output is Output.Device && it.clock.playing } == true

/** The sound is on its way to `id`, which has not said it has started. */
fun Listening.movingTo(id: String): Boolean = (active?.output as? Output.Device)?.let { it.id == id && it.moving } == true

/** The inputs, as the transitions take them. */
object Inputs {
    fun start(queue: List<Queued>, at: Int, positionMs: Long, from: String): Start = Start(at.toLong(), from, maxOf(positionMs, 0), queue)

    fun seek(ms: Long): Seek = Seek(maxOf(ms, 0))

    fun skip(by: Int): Skip = Skip(by.toLong())

    fun jump(index: Int): Jump = Jump(index.toLong())

    fun transfer(to: String): Transfer = Transfer(to)

    /** Tracks after the one playing (`next`), or at the end of the queue. */
    fun enqueue(tracks: List<Queued>, next: Boolean): Enqueue = Enqueue(next, tracks)

    fun remove(index: Int): Remove = Remove(index.toLong())

    fun move(from: Int, to: Int): Move = Move(from.toLong(), to.toLong())

    /** `started` and `stalled`. */
    fun at(atMs: Long): At = At(maxOf(atMs, 0))

    /** `report_position`: where the output is, measured against the clock that started playing at `since`. */
    fun reportPosition(atMs: Long, since: Long): Report = Report(maxOf(atMs, 0), since)

    fun advance(fromTrack: Int): Advance = Advance(fromTrack.toLong())

    fun scrub(atMs: Long?): Scrub = Scrub(atMs?.let { maxOf(it, 0) })

    fun recordPlay(t: Queued, from: String): RecordPlay = RecordPlay(t.id, t.title, t.creator, t.album, maxOf(t.durationMs, 0), t.file, from)

    /**
     * How long a track has to have played before it counts as a play: thirty seconds or half its
     * length, whichever is first (`host::counts_after`).
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

    /**
     * Where its volume is, 0 to 100, and whether it is muted: null where it has none anybody can set
     * — the phone's media stream ([StreamLevel]), as the session reads it.
     */
    fun volume(): Pair<Int, Boolean>? = null

    /** Put its volume there, without a panel; nothing where it has none. */
    fun setVolume(level: Int, muted: Boolean) {}
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

    /** Put these after what is playing — the session's `enqueue`, the track playing playing on. */
    data class Next(val tracks: List<Queued>) : Press() {
        constructor(track: Queued) : this(listOf(track))
    }

    /** Put these at the end of the queue. */
    data class Append(val tracks: List<Queued>) : Press()

    /** Take the track at this place out of the queue. */
    data class Remove(val index: Int) : Press()

    /** Move the track at `from` to `to`. */
    data class Move(val from: Int, val to: Int) : Press()

    /** Where this phone's scrubber is being dragged to, or none: ephemeral, and throttled. */
    data class Scrub(val atMs: Long?) : Press()

    /** The sound to another device. */
    data class Transfer(val to: String) : Press()

    /**
     * The sound to this device, if it is anywhere else; nothing if it is here, on its way here,
     * or there is nothing to move. What a car asks before it plays (Car.kt).
     */
    object Here : Press()

    /** A volume moved: a slider, a mute, a volume button, the system's panel. */
    data class Loud(val level: Level, val change: Change) : Press()

    /** A speaker joins the group that is playing. */
    data class Join(val device: String) : Press()

    /** A speaker leaves whatever group it is in, and goes quiet. */
    data class Unjoin(val device: String) : Press()
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

    /** The connection this phone said what it can do on (`equip`): below the peer's epoch, not yet. */
    private var equipped = 0L

    /**
     * The phone's own volume in the session, as this reconciler last left it: the entry it last put
     * the stream at — or said, or found at `equip` — and what the stream read after. A stream that
     * reads otherwise than [seen] was moved by something else.
     */
    private var wanted: Pair<Int, Boolean>? = null
    private var seen: Pair<Int, Boolean>? = null

    /**
     * The track and the clock the player was last made to agree with: a difference with the clock
     * still this one is the player's, and the session is told; with another, the session moved, and
     * the player is.
     */
    private var against: Triple<Long, Id, Clock>? = null

    /** When this phone last said where its player is, by the server's clock as it reads it. */
    private var reported: Long? = null

    /** Every load of a track is a play: the track loaded, this load's number, and the last counted. */
    private var loaded: Pair<Int, Id>? = null
    private var play = 0L
    private var counted = 0L

    var stats = RemoteStats()
        private set

    /** The session as this phone holds it, linked or not. */
    fun held(h: Harken): Listening = runCatching { Listening.parse(harkenListening(h.peer)) }.getOrDefault(Listening())

    /** Whether this phone is part of the session now: linked, as itself, and standing in it (its `on_join` landed). */
    fun joined(h: Harken): Boolean {
        val st = h.peer.status()
        return st.linked && st.session == me && held(h).devices.containsKey(me)
    }

    /** The session, when there is one to be part of; null is this phone being its own session. */
    fun session(h: Harken): Listening? = if (joined(h)) held(h) else null

    // A transition, as `press` hands it to `call`.
    private fun call(f: Harken.() -> Unit): Harken.() -> Unit = f

    /** Whether the sound belongs to another device — here or not — which is when this phone is silent. */
    fun elsewhere(s: Listening): Boolean = s.active?.output?.id?.let { it != me } == true

    /**
     * Call a transition. A refusal — the preview's, here — is counted and handed back; the
     * server's arrive later ([refusals]).
     */
    fun call(h: Harken, transition: String, f: Harken.() -> Unit): String? {
        stats = stats.copy(calls = stats.calls + 1)
        return try {
            h.f()
            null
        } catch (e: Refusal) {
            val why = e.why
            stats = stats.copy(refused = stats.refused + 1, lastRefusal = "$transition: $why")
            why
        }
    }

    /** What the server refused of this phone's calls since the last ask. */
    fun refusals(h: Harken): List<String> {
        val out = h.peer.takeLiveRefusals()
        if (out.isNotEmpty()) stats = stats.copy(refused = stats.refused + out.size, lastRefusal = out.last())
        return out
    }

    /**
     * A press, as a transition — or, unlinked, nothing: the caller does it to the player, which is
     * this phone's own session. `local` is that player, whose queue a press plays when the session
     * has none. Null when it was taken; the refusal, in a sentence, when it was not.
     */
    fun press(h: Harken, press: Press, local: Sound?): String? {
        val s = session(h) ?: return NOT_PART
        val a = s.active
        val (transition, f) =
            when (press) {
                Press.Toggle ->
                    when {
                        s.sounding -> "pause" to call { pause() }
                        a != null -> "play" to call { play() }
                        else -> startLocal(local) ?: return "nothing to play"
                    }
                Press.Play -> if (a != null) "play" to call { play() } else startLocal(local) ?: return "nothing to play"
                Press.Pause -> "pause" to call { pause() }
                is Press.Skip -> "skip" to call { skip(Inputs.skip(if (press.by < 0) -1 else 1)) }
                is Press.Seek -> "seek" to call { seek(Inputs.seek(press.ms)) }
                is Press.Jump -> "jump" to call { jump(Inputs.jump(press.index)) }
                is Press.Start -> "start" to call { start(Inputs.start(press.queue, press.at, press.positionMs, press.from)) }
                is Press.Scrub -> "scrub" to call { scrub(Inputs.scrub(press.atMs)) }
                is Press.Transfer -> "transfer" to call { transfer(Inputs.transfer(press.to)) }
                is Press.Loud -> Volumes.call(press.level, press.change) ?: return NOT_PART
                is Press.Join -> "join" to call { join(Member(press.device)) }
                is Press.Unjoin -> "unjoin" to call { unjoin(Member(press.device)) }
                Press.Here -> if (a == null || s.outputs(me)) return null else "transfer" to call { transfer(Inputs.transfer(me)) }
                // The session's own edits of its queue: the track playing plays on. Into a session
                // with nothing queued, `enqueue` starts it, as a track pressed does.
                is Press.Next -> "enqueue" to call { enqueue(Inputs.enqueue(press.tracks, true)) }
                is Press.Append -> "enqueue" to call { enqueue(Inputs.enqueue(press.tracks, false)) }
                is Press.Remove -> "remove" to call { remove(Inputs.remove(press.index)) }
                is Press.Move -> "move" to call { move(Inputs.move(press.from, press.to)) }
            }
        return call(h, transition, f)
    }

    // Nothing queued in the session: what this phone last had, pressed again.
    private fun startLocal(local: Sound?): Pair<String, Harken.() -> Unit>? {
        val l = local ?: return null
        if (l.queue.isEmpty()) return null
        val input = Inputs.start(l.queue, l.index.coerceIn(0, l.queue.size - 1), 0, l.from)
        return "start" to call { start(input) }
    }

    /**
     * Make this phone agree with the session, and say what happened to it. Called after every
     * change the peer reports and every event the player raises.
     */
    fun reconcile(h: Harken, sound: Sound) {
        if (!joined(h)) {
            // Its own session. Whether it was playing on its own is what matters when it comes
            // back; and a play is the log's, so it counts here too.
            alonePlaying = sound.playing
            count(h, sound, sound.queue.getOrNull(sound.index), sound.from)
            return
        }
        val s = session(h) ?: return
        val epoch = h.peer.status().epoch.toLong()
        equip(h, sound, s, epoch)
        if (met != epoch) {
            // The first sight of the session on this connection: the rule for coming back.
            met = epoch
            if (alonePlaying && sound.queue.isNotEmpty()) {
                alonePlaying = false
                if (!(s.sounding && !s.outputs(me))) {
                    val at = sound.index.coerceIn(0, sound.queue.size - 1)
                    val input = Inputs.start(sound.queue, at, sound.positionMs, sound.from)
                    if (call(h, "start") { start(input) } == null)
                        stats = stats.copy(cameBackPlaying = stats.cameBackPlaying + 1)
                    // Its own sound goes on: the session it has just made says what it is doing.
                    expect = at
                    return
                }
                // Somebody else is playing: this phone is a controller of it now, and its own
                // sound stops below.
            }
        }
        val now = session(h) ?: s
        follow(h, sound, now)
        val a = now.active
        if (a != null && now.outputs(me)) {
            drive(h, sound, a)
            return
        }
        // Not the output: silent. Losing the sound is not something this phone does; it is told,
        // and this is the one place that can notice.
        if (sound.playing) sound.pause()
        startedFor = null
    }

    // What this phone can do, said once a connection: a volume anybody can move, at the level its
    // stream is at, and no family — a phone joins no group. Its entry starts as whatever `on_join`
    // assumed, which is nothing; a player with no volume says nothing and stays that.
    private fun equip(h: Harken, sound: Sound, s: Listening, epoch: Long) {
        if (equipped == epoch || s.devices[me] == null) return
        val now = sound.volume() ?: return
        if (call(h, "equip") { equip(Volumes.equip(true, null, now.first, now.second)) } != null) return
        equipped = epoch
        wanted = now
        seen = now
    }

    /**
     * The stream at this phone's own entry in the session, whoever moved it there — and the stream
     * moved by anything else, said. Moved is read against what the stream read after this phone's
     * own last write, so its own write is never said back: the session asks for 30, the stream
     * takes its nearest step and reads 27, and 27 is where this phone left it, not a hand.
     */
    private fun follow(h: Harken, sound: Sound, s: Listening) {
        val mine = s.devices[me]?.takeIf { it.canVolume } ?: return
        val now = sound.volume() ?: return
        val want = mine.volume.toInt() to mine.muted
        val was = seen
        if (was != null && now != was) {
            // Moved, and not by this: the buttons, the system's panel, another app.
            seen = now
            wanted = now
            if (now != want) call(h, "report_volume") { reportVolume(Volumes.report(now.first, now.second)) }
            return
        }
        if (want != wanted) {
            sound.setVolume(want.first, want.second)
            wanted = want
            seen = sound.volume()
        } else if (was == null) seen = now
    }

    // This phone is the output: its player made to agree with the session, and what happened to
    // it said.
    private fun drive(h: Harken, sound: Sound, a: Active) {
        val now = System.currentTimeMillis()
        val at = a.at.toInt()
        val track = a.now
        val here = a.at to track.id
        val want = a.position(now)
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
                    call(h, "advance") { advance(Inputs.advance(at)) }
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
            val input = Inputs.at(sound.positionMs)
            call(h, "started") { started(input) }
            return
        }
        if (!moving) startedFor = null
        // The last track ended: said once. Once said, a player still at its end is reconciled like
        // any other — played again from the start if somebody presses play.
        if (sound.ended && a.clock.playing && advancedFor != here) {
            advancedFor = here
            call(h, "advance") { advance(Inputs.advance(at)) }
            return
        }
        // It stopped by itself: the session is told, and pauses there.
        if (sound.takeStalled() && a.clock.playing) {
            val input = Inputs.at(sound.positionMs)
            call(h, "stalled") { stalled(input) }
            return
        }
        when {
            a.clock.playing && !sound.playing -> sound.resume()
            !a.clock.playing && sound.playing -> sound.pause()
        }
        // Somewhere else in the track than the session counts, by more than the bound. If the
        // session moved since the player was last made to agree — a seek on another device — the
        // player goes where it is now; if it is the player that came apart from a clock nobody
        // touched — ExoPlayer starting a moment after `started` was said, a buffer that ran dry —
        // the player is right about where it is, and the session is told (`report_position`), at
        // most once a second, against that clock. Filling a buffer, neither.
        val clock = a.clock
        val moved = against != Triple(a.at, track.id, clock)
        against = Triple(a.at, track.id, clock)
        if (!sound.buffering && abs(sound.positionMs - want) > SYNC_MS) {
            when {
                !moved && clock is Clock.Playing && sound.playing -> {
                    val last = reported
                    if (last == null || now - last >= REPORT_MS) {
                        reported = now
                        val input = Inputs.reportPosition(sound.positionMs, clock.since)
                        call(h, "report_position") { reportPosition(input) }
                    }
                }
                !moved && clock is Clock.Playing -> {}
                else -> sound.seek(want)
            }
        }
        count(h, sound, track, a.from)
    }

    // A play, once it has played long enough, once per load of a track — and, short of that, a
    // wake at the moment it will have.
    private fun count(h: Harken, sound: Sound, track: Queued?, from: String) {
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
            h.recordPlay(Inputs.recordPlay(track, from))
            stats = stats.copy(plays = stats.plays + 1)
        } catch (e: Refusal) {
            stats = stats.copy(lastRefusal = "record_play: ${e.why}")
        }
    }

    companion object {
        /**
         * How far the player may be from where the session counts it before one is put right: the
         * domain's `read::SYNC_MS`, the bound every output is held to — the browser's element and
         * the bridge's speakers too.
         */
        const val SYNC_MS = 600L

        /** How often, at most, a player that stays apart from the clock is said again. */
        const val REPORT_MS = 1_000L
    }
}

/** One row of the picker: a device of this account — or the group that plays — and what it can be asked. */
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
    /**
     * Where the sound is — the output and every speaker following it — or, while it is nowhere, this
     * phone, where the next press plays.
     */
    val ticked: Boolean,
    val me: Boolean,
    /** Its volume, where anybody can move it: a row with none draws no slider. */
    val volume: Int? = null,
    val muted: Boolean = false,
    /** What its button does: join it to the group that plays (true), take it out (false), or nothing. */
    val join: Boolean? = null,
    /** The row for the group as a whole, "Kitchen + 1", its volume every member's. */
    val group: Boolean = false,
) {
    /** What its volume controls move. */
    val level: Level
        get() = if (group) Level.Group else Level.Device(id)
}

/**
 * The picker, as the desktop's (23c40a0, and "Volume and groups"): no row that stops it everywhere;
 * this phone the default; a slider and a mute on every row with a volume; join and unjoin on a
 * speaker that can; and the group that plays first, as a whole.
 */
object Picker {
    /**
     * A row per device of the account, by name — and the output that has gone, which is the one
     * row most worth drawing, drawn and not selectable. While the output leads a group, the group
     * comes first, named for its leader and how many follow it.
     */
    fun rows(s: Listening, me: String): List<DeviceRow> {
        val output = s.active?.output
        val ticked = output?.id ?: me
        val leader = s.leader
        val rows =
            s.devices.values
                .map { d ->
                    val follows = leader != null && d.follows == leader
                    DeviceRow(
                        d.id,
                        d.name,
                        d.kind,
                        d.canPlay,
                        away = false,
                        moving = s.movingTo(d.id),
                        ticked = d.id == ticked || follows,
                        me = d.id == me,
                        volume = if (d.canVolume) d.volume.toInt() else null,
                        muted = d.muted,
                        join = if (follows) false else if (s.joinable(d.id)) true else null,
                    )
                }
                .toMutableList()
        if (output is Output.Away && rows.none { it.id == output.id })
            rows.add(DeviceRow(output.id, output.name, "computer", takeable = false, away = true, moving = false, ticked = true, me = output.id == me))
        val sorted = rows.sortedWith(compareBy<DeviceRow> { it.name }.thenBy { it.id })
        if (leader == null || !s.grouped) return sorted
        val head = s.devices[leader]
        val all =
            DeviceRow(
                id = "",
                name = "${head?.name ?: leader} + ${s.followers(leader).size}",
                kind = head?.kind ?: "speaker",
                takeable = false,
                away = false,
                moving = false,
                ticked = false,
                me = false,
                volume = s.groupLevel(leader),
                muted = s.groupMuted(leader),
                group = true,
            )
        return listOf(all) + sorted
    }

    /**
     * Where the cursor starts: on the output — not the group's row, nor a speaker following it — or on
     * this phone, so that a tap straight away changes nothing.
     */
    fun cursor(rows: List<DeviceRow>): Int = rows.indexOfFirst { it.ticked && !it.group && it.join != false }.takeIf { it >= 0 } ?: 0

    /** What picking `row` asks: a hand-off, or nothing — the device that has it, or one that cannot. */
    fun pick(row: DeviceRow, s: Listening?): Press? {
        if (row.group || !row.takeable || s?.active == null) return null
        if (s.outputs(row.id) && !s.movingTo(row.id)) return null
        return Press.Transfer(row.id)
    }

    /** What a row's slider, let go or dragged to `level`, asks. */
    fun level(row: DeviceRow, level: Int): Press? = row.volume?.let { Press.Loud(row.level, Change.To(level)) }

    /** What a row's mute asks: the other way from how it is. */
    fun mute(row: DeviceRow): Press? = row.volume?.let { Press.Loud(row.level, Change.Mute(!row.muted)) }

    /** What a row's join button asks: join, leave, or nothing. */
    fun join(row: DeviceRow): Press? =
        when (row.join) {
            true -> Press.Join(row.id)
            false -> Press.Unjoin(row.id)
            null -> null
        }
}
