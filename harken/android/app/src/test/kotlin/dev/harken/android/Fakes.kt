// What the app's tests play with: a track of a test's queue, a phone's
// media stream, and a player that does what it is told and moves only when a
// test says so. The session they play in is a real one — a `Hall`, harken's
// server in the process (`harken/ffi`), its devices real peers of the kernel
// (TestPeers.kt). The reconciler's own tests are Rust's (`harken/ffi/tests/
// player.rs`, `harken/player/tests`).
package dev.harken.android

import dev.arkdb.ffi.Id
import dev.harken.android.gen.Track as Queued
import dev.harken.ffi.Loudness
import dev.harken.ffi.PhoneSound
import dev.harken.ffi.SoundState
import org.json.JSONArray

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

/**
 * A player that does what it is told, and moves only when a test says so — with a volume, given a
 * stream: what harken's reconciler drives in a test where ExoPlayer would be.
 */
class FakeSound(val stream: FakeStream? = null) : PhoneSound {
    private val level = stream?.let(::StreamLevel)

    var queue: List<Queued> = emptyList()
    var index: Int = -1
    var playing: Boolean = false
    var buffering: Boolean = false
    var positionMs: Long = 0
    var ended: Boolean = false
    var from: String = ""
    var stalled = false
    var loads = 0

    override fun state(): SoundState = SoundState(queue.map { it.id.hex }, index.toLong(), playing, buffering, positionMs, ended, from)

    override fun takeStalled(): Boolean = stalled.also { stalled = false }

    override fun tracks(): String = queue.joinToString(",", "[", "]") { it.json() }

    override fun load(queue: String, at: Long, positionMs: Long, playing: Boolean, from: String) {
        this.queue = JSONArray(queue).let { a -> List(a.length()) { Queued.of(a.get(it)) } }
        index = at.toInt()
        this.positionMs = positionMs
        this.playing = playing
        this.from = from
        ended = false
        loads++
    }

    override fun jump(at: Long, positionMs: Long) {
        index = at.toInt()
        this.positionMs = positionMs
        ended = false
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
    }

    override fun wakeIn(ms: Long) {}

    override fun volume(): Loudness? = level?.read()?.let { (l, m) -> Loudness(l.toLong(), m) }

    override fun setVolume(level: Long, muted: Boolean) {
        this.level?.apply(level.toInt(), muted)
    }
}
