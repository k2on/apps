// What the reconciler's tests play with: a track of a test's queue, a
// phone's media stream, and a player that does what it is told and moves
// only when a test says so. The session they play in is a real one — a
// `Hall`, harken's server in the process (`harken/ffi`), its devices real
// peers of the kernel (TestPeers.kt) — so what is tested is the reconciler
// against the session the domain defines.
package dev.harken.android

import dev.arkdb.ffi.Id
import dev.harken.android.gen.Track as Queued

/** A track of a test's queue: `n` names it, and every one is a hundred seconds long. */
fun queuedTrack(title: String, n: Int, ms: Long = 100_000): Queued {
    return Queued("Water Music", "Handel", ms, "music/$title.mp3", Id("%032x".format(n)), title)
}

/**
 * A phone's media stream as `AudioManager` keeps it, in a test: fifteen steps, as most phones have,
 * and a mute; every write counted, so a test can say nothing was written.
 */
class FakeStream(override var index: Int = 7, override val max: Int = 15) : Stream {
    override val min: Int = 0
    override var muted: Boolean = false
    var writes = 0

    override fun set(index: Int) {
        this.index = index.coerceIn(min, max)
        writes++
    }

    override fun mute(muted: Boolean) {
        this.muted = muted
        writes++
    }
}

/** A player that does what it is told, and moves only when a test says so — with a volume, given a stream. */
class FakeSound(val stream: FakeStream? = null) : Sound {
    private val level = stream?.let(::StreamLevel)

    override var queue: List<Queued> = emptyList()
    override var index: Int = -1
    override var playing: Boolean = false
    override var buffering: Boolean = false
    override var positionMs: Long = 0
    override var ended: Boolean = false
    override var from: String = ""
    var stalled = false
    var loads = 0
    var jumps = 0
    var seeks = 0
    var wake: Long? = null

    override fun takeStalled(): Boolean = stalled.also { stalled = false }

    override fun load(queue: List<Queued>, at: Int, positionMs: Long, playing: Boolean, from: String) {
        this.queue = queue
        index = at
        this.positionMs = positionMs
        this.playing = playing
        this.from = from
        ended = false
        loads++
    }

    override fun jump(at: Int, positionMs: Long) {
        index = at
        this.positionMs = positionMs
        ended = false
        jumps++
    }

    override fun resume() {
        playing = true
        ended = false
    }

    override fun pause() {
        playing = false
    }

    override fun seek(ms: Long) {
        positionMs = ms
        ended = false
        seeks++
    }

    override fun wakeIn(ms: Long) {
        wake = ms
    }

    override fun volume(): Pair<Int, Boolean>? = level?.read()

    override fun setVolume(level: Int, muted: Boolean) {
        this.level?.apply(level, muted)
    }

    /** The track played to its end, and the player went on to the next as ExoPlayer does — or, at the last, stopped there. */
    fun finish() {
        if (index + 1 < queue.size) {
            index++
            positionMs = 0
        } else {
            ended = true
            positionMs = queue[index].durationMs
        }
    }

    /** Played this phone's own queue, as a press did while it was alone. */
    fun own(queue: List<Queued>, at: Int, positionMs: Long, from: String) {
        load(queue, at, positionMs, true, from)
    }
}
