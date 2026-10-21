// The account's listening session as the phone's screens read it, and what
// they ask of it. The reconciler that makes this phone's player agree with
// the session — and says what happened to it — is harken's one reconciler,
// in Rust (`harken/player`, run here through `dev.harken.ffi.Player`, the
// same rules a Home Assistant speaker is held to); what is here is what the
// screens, the picker and the lock screen draw and press.
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

/** Whether `id` is the output and is here. */
fun Listening.outputs(id: String): Boolean = (active?.output as? Output.Device)?.id == id

/** Something is making a sound: a clock running on an output that is here. */
val Listening.sounding: Boolean
    get() = active?.let { it.output is Output.Device && it.clock.playing } == true

/** The sound is on its way to `id`, which has not said it has started. */
fun Listening.movingTo(id: String): Boolean = (active?.output as? Output.Device)?.let { it.id == id && it.moving } == true

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

    /** Where this phone's scrubber is being dragged to, or none: ephemeral, and throttled. */
    data class Scrub(val atMs: Long?) : Press()

    /** The sound to another device. */
    data class Transfer(val to: String) : Press()

    /** The sound to this device, if it is anywhere else. What a car asks before it plays (Car.kt). */
    object Here : Press()

    /** A volume moved: a slider, a mute, a volume button, the system's panel. */
    data class Loud(val level: Level, val change: Change) : Press()

    /** A speaker joins the group that is playing. */
    data class Join(val device: String) : Press()

    /** A speaker leaves whatever group it is in, and goes quiet. */
    data class Unjoin(val device: String) : Press()

    /** The press as harken's player reads it (`harken_player::Press`, externally tagged JSON). */
    fun json(): String {
        fun tracks(ts: List<Queued>) = JSONArray(ts.map { JSONObject(it.json()) })
        fun of(name: String, vararg fields: Pair<String, Any?>) =
            JSONObject().put(name, JSONObject().apply { for ((k, v) in fields) put(k, v ?: JSONObject.NULL) }).toString()
        return when (this) {
            Play -> "\"Play\""
            Pause -> "\"Pause\""
            Toggle -> "\"Toggle\""
            Here -> "\"Here\""
            is Skip -> of("Skip", "by" to by)
            is Seek -> of("Seek", "ms" to ms)
            is Jump -> of("Jump", "index" to index)
            is Start -> of("Start", "queue" to tracks(queue), "at" to at, "position_ms" to positionMs, "from" to from)
            is Next -> of("Next", "tracks" to tracks(tracks))
            is Append -> of("Append", "tracks" to tracks(tracks))
            is Remove -> of("Remove", "index" to index)
            is Move -> of("Move", "from" to from, "to" to to)
            is Scrub -> of("Scrub", "at_ms" to atMs)
            is Transfer -> of("Transfer", "to" to to)
            is Join -> of("Join", "device" to device)
            is Unjoin -> of("Unjoin", "device" to device)
            is Loud -> {
                val l =
                    when (level) {
                        Level.Group -> "Group"
                        Level.Here -> "Here"
                        is Level.Device -> JSONObject().put("Device", JSONObject().put("id", level.id))
                    }
                val c =
                    when (change) {
                        is Change.To -> JSONObject().put("To", JSONObject().put("level", change.level))
                        is Change.By -> JSONObject().put("By", JSONObject().put("delta", change.delta))
                        is Change.Mute -> JSONObject().put("Mute", JSONObject().put("muted", change.muted))
                    }
                of("Loud", "level" to l, "change" to c)
            }
        }
    }
}

/** Every number this phone's player holds that the peer does not, for the diagnostics. */
data class RemoteStats(
    /** Transitions called, and refused by the preview or the server. */
    val calls: Long = 0,
    val refused: Long = 0,
    /** `record_play`s authored. */
    val plays: Long = 0,
    /** `start`s said coming back with something of its own playing. */
    val cameBackPlaying: Long = 0,
    val lastRefusal: String? = null,
) {
    companion object {
        fun of(o: JSONObject?): RemoteStats =
            if (o == null) RemoteStats()
            else RemoteStats(o.optLong("calls"), o.optLong("refused"), o.optLong("plays"), o.optLong("came_back"), o.optString("last_refusal").takeIf { o.has("last_refusal") && !o.isNull("last_refusal") })
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
 * The picker, as the desktop's: no row that stops it everywhere; this phone the default; a slider and
 * a mute on every row with a volume; join and unjoin on a speaker that can; and the group that plays
 * first, as a whole.
 */
object Picker {
    /**
     * A row per device of the account, by name — and the output that has gone, drawn and not
     * selectable. While the output leads a group, the group comes first, named for its leader and how
     * many follow it.
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

    /** Where the cursor starts: on the output — not the group's row, nor a follower — or on this phone. */
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
