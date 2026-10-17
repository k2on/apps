// Volume, and speakers joined, as the listening session holds them
// (docs/plan-live.md "Volume and groups"): each device's level, 0 to 100,
// and its mute are its entry in `devices`, and a group is who follows whom.
// Like the rest of the session it is state — anybody moves a volume, the
// device obeys it, and what was done on the device itself is said back as
// its own transition. Kept free of Android, as the reconciler is, so that a
// test reads every rule with real peers and a stand-in stream.
//
// What is in this file is what the phone reads of it — the output's volume
// (the group's while the output leads one), the transitions a control calls,
// and the phone's own media stream as the session's 0 to 100 — and the
// throttle a dragged slider sends through. What the reconciler does with the
// phone's own entry is Remote.kt's; the picker's rows are there too.
package dev.harken.android

import dev.arkdb.Value
import dev.harken.android.listening.Listening
import dev.harken.android.listening.ListeningDevices
import dev.harken.android.listening.Output
import kotlin.math.roundToInt

/** How far a volume button moves a volume: one press, five of a hundred, as the desktop's `+` and `-`. */
const val NUDGE = 5

/** What a volume control moves. */
sealed class Level {
    /** The output's group, every member with a volume in proportion; a group of one is its leader. */
    object Group : Level()

    /** One device of the session. */
    data class Device(val id: String) : Level()

    /** This phone's own media stream, while it is its own session: moved here, and in no session. */
    object Here : Level()
}

/** How a volume control moves what it moves. */
sealed class Change {
    data class To(val level: Int) : Change()

    /** What a volume button sends: it knows which way, and not where to. */
    data class By(val delta: Int) : Change()

    data class Mute(val muted: Boolean) : Change()
}

/**
 * Whatever is playing's volume, as the full player's slider and the system's volume panel draw it:
 * what a control moves ([of]), where it is, and whether it is another device's — the phone a
 * remote for it, its buttons moving that device — or this phone's own.
 */
data class Volume(val of: Level, val level: Int, val muted: Boolean, val remote: Boolean)

/** The device the sound is on, where it is here: the leader of whatever group plays. */
val Listening.leader: String?
    get() = (active?.output as? Output.Device)?.id

/** Who follows `leader`, by id. */
fun Listening.followers(leader: String): List<ListeningDevices> = devices.values.filter { it.follows == leader }.sortedBy { it.id }

/** The group `leader` leads: it, then whoever follows it. */
fun Listening.group(leader: String): List<ListeningDevices> = listOfNotNull(devices[leader]) + followers(leader)

/** Whether the output leads a group: something follows it. */
val Listening.grouped: Boolean
    get() = leader?.let { followers(it).isNotEmpty() } == true

/**
 * The level of the group `leader` leads, as the domain reads it: the mean of its members' that have
 * a volume, rounded down. Null for a group in which nothing has one.
 */
fun Listening.groupLevel(leader: String): Int? {
    val levels = group(leader).filter { it.canVolume }.map { it.volume }
    return if (levels.isEmpty()) null else (levels.sum() / levels.size).toInt()
}

/** Whether every member of `leader`'s group with a volume is muted. */
fun Listening.groupMuted(leader: String): Boolean {
    val members = group(leader).filter { it.canVolume }
    return members.isNotEmpty() && members.all { it.muted }
}

/**
 * Whether `id` can be joined to the output's group now: the domain's `joinable`, read the same way —
 * a speaker that is here, can play, and is of the leader's family, which both have; not the leader,
 * and a leader that follows nobody.
 */
fun Listening.joinable(id: String): Boolean {
    val l = leader?.let { devices[it] } ?: return false
    val d = devices[id] ?: return false
    return l.follows == null && d.id != l.id && d.canPlay && d.family != null && d.family == l.family
}

/**
 * What is playing's volume: the output's group, where the output leads one; the output's own, where
 * it has one; and, while the sound is nowhere — or on this phone — this phone's, which is where the
 * next press plays. Null where nothing it could move has a volume. `remote` is the sound on another
 * device: what makes the phone's buttons that device's.
 */
fun Listening.volume(me: String): Volume? {
    val out = leader
    if (out != null) {
        val remote = out != me
        if (grouped) return groupLevel(out)?.let { Volume(Level.Group, it, groupMuted(out), remote) }
        return devices[out]?.takeIf { it.canVolume }?.let { Volume(Level.Device(out), it.volume.toInt(), it.muted, remote) }
    }
    return devices[me]?.takeIf { it.canVolume }?.let { Volume(Level.Device(me), it.volume.toInt(), it.muted, remote = false) }
}

/** The transitions a volume control calls, and what joining a speaker calls. */
object Volumes {
    fun equip(canVolume: Boolean, family: String?, level: Int, muted: Boolean): Map<String, Value> =
        mapOf(
            "can_volume" to Value.bool(canVolume),
            "family" to Value.opt(family?.let { Value.text(it) }),
            "muted" to Value.bool(muted),
            "volume" to Value.int(level.coerceIn(0, 100).toLong()),
        )

    fun report(level: Int, muted: Boolean): Map<String, Value> =
        mapOf("level" to Value.int(level.coerceIn(0, 100).toLong()), "muted" to Value.bool(muted))

    fun member(device: String): Map<String, Value> = mapOf("device" to Value.text(device))

    /** The transition `change` of `level` is, and its input: none for this phone's own stream. */
    fun call(level: Level, change: Change): Pair<String, Map<String, Value>>? =
        when (level) {
            Level.Here -> null
            Level.Group ->
                when (change) {
                    is Change.To -> "set_group_volume" to mapOf("level" to Value.int(change.level.coerceIn(0, 100).toLong()))
                    is Change.By -> "nudge_group_volume" to mapOf("delta" to Value.int(change.delta.coerceIn(-100, 100).toLong()))
                    is Change.Mute -> "set_group_mute" to mapOf("muted" to Value.bool(change.muted))
                }
            is Level.Device ->
                when (change) {
                    is Change.To -> "set_volume" to mapOf("device" to Value.text(level.id), "level" to Value.int(change.level.coerceIn(0, 100).toLong()))
                    is Change.By -> "nudge_volume" to mapOf("delta" to Value.int(change.delta.coerceIn(-100, 100).toLong()), "device" to Value.text(level.id))
                    is Change.Mute -> "set_mute" to mapOf("device" to Value.text(level.id), "muted" to Value.bool(change.muted))
                }
        }
}

/**
 * The phone's media stream, as Android's `AudioManager` has it — `STREAM_MUSIC`, an index between
 * two ends and a mute — or a test's. Every write is without a flag: no panel is raised by a volume
 * another device set.
 */
interface Stream {
    val min: Int
    val max: Int
    val index: Int
    val muted: Boolean

    fun set(index: Int)

    fun mute(muted: Boolean)
}

/**
 * A [Stream] read and set as the session's 0 to 100. A stream has fewer steps than a hundred —
 * fifteen, on most phones — so a level set comes back as the nearest step: 30 is set as the fifth of
 * fifteen and read as 27, and 27 is the fifth again. Which is why the reconciler compares what it
 * reads with what it read after its own last write, never with the level it asked for.
 */
class StreamLevel(private val stream: Stream) {
    private val span: Int
        get() = maxOf(stream.max - stream.min, 1)

    /** The stream's step as a level. */
    fun level(index: Int): Int = ((index - stream.min) * 100.0 / span).roundToInt().coerceIn(0, 100)

    /** A level as the stream's nearest step. */
    fun index(level: Int): Int = stream.min + (level.coerceIn(0, 100) * span / 100.0).roundToInt()

    /** Where it is: its level, and its mute. */
    fun read(): Pair<Int, Boolean> = level(stream.index) to stream.muted

    /** Put it at `level`, then muted or not — the mute last, since setting a step can unmute it. */
    fun apply(level: Int, muted: Boolean) {
        val i = index(level)
        if (i != stream.index) stream.set(i)
        if (muted != stream.muted) stream.mute(muted)
    }
}

/**
 * At most one send per [windowMs], trailing edge, the last value always — what a dragged slider's
 * ordered calls go through, since the client never holds an ordered call (its order is its point):
 * the desktop's 50 ms (`ark_client::THROTTLE_MS`). The caller owns the clock and the timer: it sends
 * what [offer] hands back now, and asks [take] again at [due].
 */
class Throttle<T>(private val windowMs: Long = dev.arkdb.client.THROTTLE_MS) {
    private var sent = Long.MIN_VALUE / 2
    private var held: T? = null

    /** `v` at `now`: handed back if it goes now, else held, replacing whatever was. */
    fun offer(now: Long, v: T): T? {
        if (now >= sent + windowMs) {
            sent = now
            held = null
            return v
        }
        held = v
        return null
    }

    /** What is held, still. */
    fun held(): T? = held

    /** When what is held is due; null with nothing held. */
    fun due(): Long? = if (held == null) null else sent + windowMs

    /** What is held, if it is due at `now` — or at once, `force`d by letting go. */
    fun take(now: Long, force: Boolean = false): T? {
        val h = held ?: return null
        if (!force && now < sent + windowMs) return null
        held = null
        sent = now
        return h
    }
}

/**
 * A volume slider dragged: each level sent through a [Throttle] as the transition it is — a speaker
 * has to move while the finger does, which a value sent only on release would not do — the last
 * always, by `later` at the window's end, or at once when it is let go. Another slider dragged while
 * one is held sends the held one first. The caller owns the clock and the timer, so that a test
 * moves both.
 */
class Drag(
    private val clock: () -> Long,
    private val later: (Long, () -> Unit) -> Unit,
    private val send: (Level, Int) -> Unit,
) {
    private val throttle = Throttle<Pair<Level, Int>>()
    private var armed = false

    /** `of` dragged to `level`. */
    fun to(of: Level, level: Int) {
        val held = throttle.held()
        if (held != null && held.first != of) done()
        val now = throttle.offer(clock(), of to level)
        if (now != null) send(now.first, now.second) else arm()
    }

    /** Let go: whatever is held goes now. */
    fun done() {
        throttle.take(clock(), force = true)?.let { send(it.first, it.second) }
    }

    private fun arm() {
        if (armed) return
        val due = throttle.due() ?: return
        armed = true
        later(maxOf(due - clock(), 0)) {
            armed = false
            throttle.take(clock())?.let { send(it.first, it.second) }
            arm()
        }
    }
}
