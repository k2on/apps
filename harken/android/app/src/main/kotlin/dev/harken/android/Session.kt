// The account's listening session as the phone's screens read it, and what
// they ask of it. The reconciler that makes this phone's player agree with
// the session — and says what happened to it — is harken's one reconciler,
// in Rust (`harken/player`, run here through `dev.harken.ffi.Player`, the
// same rules a Home Assistant speaker is held to), and so are the rows the
// player, the picker and the diagnostics draw and what their controls ask
// (`harken/player/src/screens.rs`); what is here is the session as the lock
// screen, a car and Home read it, and the presses that carry tracks.
//
// The session is the domain's live state (`harken_domain::listening`), read
// through the types the domain generates (`gen/Harken.kt`). **The state is
// the instruction**: every button on every device is a transition ([Press]),
// and only the device the session names as its output makes a sound.
package dev.harken.android

import dev.harken.android.gen.Active
import dev.harken.android.gen.Clock
import dev.harken.android.gen.Output
import dev.harken.android.gen.Present
import dev.harken.android.gen.Track as Queued
import org.json.JSONArray
import org.json.JSONObject

/** What a press is answered with while this phone is its own session: the player's, then. */
const val NOT_PART = "not part of the session"

/**
 * The account's listening session as this phone holds it: every device standing in it, by id; where
 * each one's scrubber is being dragged; and what is queued, if anything.
 */
data class Listening(
    val devices: Map<String, Present> = emptyMap(),
    val scrub: Map<String, Long> = emptyMap(),
    val active: Active? = null,
) {
    companion object {
        fun parse(json: String): Listening = of(JSONObject(json))

        fun of(o: JSONObject): Listening {
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
 * a device (`moving`), when it stands where it was handed over. The domain's `standing`.
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

/** Something is making a sound: a clock running on an output that is here. */
val Listening.sounding: Boolean
    get() = active?.let { it.output is Output.Device && it.clock.playing } == true

/** What a button, a key, a row or a lock screen asks of the session — or, alone, of the player. */
sealed class Press {
    object Play : Press()

    object Pause : Press()

    data class Skip(val by: Int) : Press()

    data class Seek(val ms: Long) : Press()

    /** A track of the queue, from its start. */
    data class Jump(val index: Int) : Press()

    /** A track pressed: this queue, from here. */
    data class Start(val queue: List<Queued>, val at: Int, val positionMs: Long, val from: String) : Press()

    /** Put these after what is playing, the track playing playing on. */
    data class Next(val tracks: List<Queued>) : Press() {
        constructor(track: Queued) : this(listOf(track))
    }

    /** Put these at the end of the queue. */
    data class Append(val tracks: List<Queued>) : Press()

    /** Take the track at this place out of the queue. */
    data class Remove(val index: Int) : Press()

    /** Move the track at `from` to `to`. */
    data class Move(val from: Int, val to: Int) : Press()

    /** The sound to this device, if it is anywhere else. What a car asks before it plays (Car.kt). */
    object Here : Press()

    /** The press as harken's player reads it (`harken_player::Press`, externally tagged JSON). */
    fun json(): String {
        fun tracks(ts: List<Queued>) = JSONArray(ts.map { JSONObject(it.json()) })
        fun of(name: String, vararg fields: Pair<String, Any?>) =
            JSONObject().put(name, JSONObject().apply { for ((k, v) in fields) put(k, v ?: JSONObject.NULL) }).toString()
        return when (this) {
            Play -> "\"Play\""
            Pause -> "\"Pause\""
            Here -> "\"Here\""
            is Skip -> of("Skip", "by" to by)
            is Seek -> of("Seek", "ms" to ms)
            is Jump -> of("Jump", "index" to index)
            is Start -> of("Start", "queue" to tracks(queue), "at" to at, "position_ms" to positionMs, "from" to from)
            is Next -> of("Next", "tracks" to tracks(tracks))
            is Append -> of("Append", "tracks" to tracks(tracks))
            is Remove -> of("Remove", "index" to index)
            is Move -> of("Move", "from" to from, "to" to to)
        }
    }
}
