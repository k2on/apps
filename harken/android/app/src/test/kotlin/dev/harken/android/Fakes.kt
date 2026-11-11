// What the app's tests play with: a track of a test's queue, a phone's
// media stream, and a media player (ark-media-player's `NativeMediaPlayer`)
// that does what it is told at once and says so. The session they play in is a real one — a `Hall`, harken's
// server in the process (`harken/ffi`), its devices real peers of the kernel
// (TestPeers.kt). The reconciler's own tests are Rust's (`harken/ffi/tests/
// player.rs`, `harken/player/tests`).
package dev.harken.android

import dev.arkdb.ffi.Id
import dev.harken.android.gen.Track as Queued
import uniffi.ark_media_player.BrowseItem
import uniffi.ark_media_player.MediaControls
import uniffi.ark_media_player.MediaEvent
import uniffi.ark_media_player.MediaListener
import uniffi.ark_media_player.NativeMediaPlayer
import uniffi.ark_media_player.NowPlaying

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
 * A media player that does what it is told at once and says so, as the ExoPlayer would a moment
 * later: what harken's reconciler drives in a test where the module's `PhoneMediaPlayer` would be.
 */
class FakeMedia : NativeMediaPlayer {
    @Volatile var url: String? = null
    @Volatile var playing: Boolean = false
    @Volatile var positionMs: Long = 0
    @Volatile var loads = 0
    @Volatile var shown: NowPlaying? = null
    @Volatile private var listener: MediaListener? = null

    private fun tell(e: MediaEvent) {
        listener?.heard(e)
    }

    override fun load(url: String, atMs: Long) {
        this.url = url
        positionMs = atMs
        playing = false
        loads++
        tell(MediaEvent.Loaded(100_000))
    }

    override fun play() {
        playing = true
        tell(MediaEvent.Playing)
    }

    override fun pause() {
        playing = false
        tell(MediaEvent.Paused)
    }

    override fun seek(atMs: Long) {
        positionMs = atMs
        tell(MediaEvent.Position(atMs))
    }

    override fun setRate(rate: Float) {}

    override fun setVolume(volume: Float) {}

    override fun stop() {
        url = null
        playing = false
    }

    override fun setNowPlaying(info: NowPlaying?) {
        shown = info
    }

    override fun setControls(controls: MediaControls) {}

    override fun setChildren(parent: String, items: List<BrowseItem>) {}

    override fun setListener(listener: MediaListener?) {
        this.listener = listener
    }
}
