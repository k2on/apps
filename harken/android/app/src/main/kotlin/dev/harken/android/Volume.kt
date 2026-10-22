// Volume, and speakers joined, as the listening session holds them
// (docs/plan-live.md "Volume and groups"): each device's level, 0 to 100,
// and its mute are its entry in `devices`, and a group is who follows whom.
// Like the rest of the session it is state — anybody moves a volume, the
// device obeys it, and what was done on the device itself is said back as
// its own transition. Kept free of Android, as the reconciler is, so that a
// test reads every rule with real peers and a stand-in stream.
//
// What is in this file is the phone's own media stream as the session's 0
// to 100, and the throttle a dragged slider sends through. What is playing's
// volume, what a control moves and the picker's rows are harken's player's
// screens (`harken/player/src/screens.rs`, read as `dev.harken.screens`);
// what becomes of the phone's own entry — `equip`, the stream followed, a
// hand on the buttons said — is its reconciler's.
package dev.harken.android

import kotlin.math.roundToInt

/** How far a volume button moves a volume: one press, five of a hundred, as the desktop's `+` and `-`. */
const val NUDGE = 5

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

/** The desktop's and the relay's interval: a dragged control sends at most twenty times a second. */
const val THROTTLE_MS: Long = 50

/**
 * At most one send per [windowMs], trailing edge, the last value always — what a dragged slider's
 * ordered calls go through, since the client never holds an ordered call (its order is its point):
 * the desktop's 50 ms (`harken_ffi::THROTTLE_MS`). The caller owns the clock and the timer: it sends
 * what [offer] hands back now, and asks [take] again at [due].
 */
class Throttle<T>(private val windowMs: Long = THROTTLE_MS) {
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
class Drag<K>(
    private val clock: () -> Long,
    private val later: (Long, () -> Unit) -> Unit,
    private val send: (K, Int) -> Unit,
) {
    private val throttle = Throttle<Pair<K, Int>>()
    private var armed = false

    /** `of` — a slider, by whatever names it — dragged to `level`. */
    fun to(of: K, level: Int) {
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
