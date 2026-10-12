// The player: Media3's ExoPlayer inside a `MediaSessionService`, and the
// screens' end of it, a `MediaController`.
//
// The service owns the sound, so it outlives every screen and the activity
// itself: leaving the app does not stop the music, and the notification,
// the lock screen and a Bluetooth headset's buttons are the media
// session's, which Media3 draws and answers. What it plays is
// `/media/<file>` from the session's server (`Media.url`), streamed with
// range requests, which is what makes seeking work. Notifications are the
// one permission the onboarding asks for, because they are what the
// notification and its controls need on Android 13 and later; without it
// the music still plays.
//
// The screens never touch the ExoPlayer. [Playback] connects a controller,
// hands it a queue — the list a track was started from, the album or the
// playlist or the search results, snapshotted, so somebody else's edit
// arriving does not redirect what is playing — and reads back what the
// player says into one [PlayerState] the mini player and the full player
// both draw.
package dev.harken.android

import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaController
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** The service: one ExoPlayer and the media session around it. */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        val player =
            ExoPlayer.Builder(this)
                .setAudioAttributes(
                    AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
                    /* handleAudioFocus= */ true)
                .setHandleAudioBecomingNoisy(true)
                .setWakeMode(C.WAKE_MODE_NETWORK)
                .build()
        val open =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        session = MediaSession.Builder(this, player).setSessionActivity(open).setCallback(Callback).build()
    }

    // A controller in another process — a watch, a car — hands items without
    // their URI; the request metadata carries it.
    private object Callback : MediaSession.Callback {
        override fun onAddMediaItems(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> =
            Futures.immediateFuture(
                mediaItems
                    .map { item ->
                        if (item.localConfiguration != null) item
                        else item.requestMetadata.mediaUri?.let { item.buildUpon().setUri(it).build() } ?: item
                    }
                    .toMutableList())
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    // Swiped away from the recents while nothing plays: nothing to keep.
    override fun onTaskRemoved(rootIntent: Intent?) {
        val p = session?.player
        if (p == null || !p.playWhenReady || p.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }
}

/** What is playing, as a screen draws it. */
data class Playing(val id: String, val title: String, val artist: String, val album: String, val art: String)

enum class Repeat {
    Off,
    All,
    One,
}

/** The player as the screens see it. */
data class PlayerState(
    val now: Playing? = null,
    val playing: Boolean = false,
    val buffering: Boolean = false,
    val positionMs: Long = 0,
    val durationMs: Long = 0,
    val shuffle: Boolean = false,
    val repeat: Repeat = Repeat.Off,
    /** What plays after this one, in the order it will play — shuffled, when shuffle is on. */
    val upNext: List<Playing> = emptyList(),
)

/** The screens' end: a controller of the service's session, and what it says as [PlayerState]. */
class Playback(private val context: Context, private val server: String) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow(PlayerState())
    val state: StateFlow<PlayerState> = _state.asStateFlow()
    private var controller: MediaController? = null
    private var ticking: Job? = null

    /** Called with the track and the page it was started from, whenever a new one starts. */
    var onStart: (Playing) -> Unit = {}

    private val future: ListenableFuture<MediaController> =
        MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync()

    init {
        future.addListener(
            {
                val c = runCatching { future.get() }.getOrNull() ?: return@addListener
                controller = c
                c.addListener(
                    object : Player.Listener {
                        override fun onEvents(player: Player, events: Player.Events) = read()

                        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                            mediaItem?.let { onStart(playing(it)) }
                        }
                    })
                read()
            },
            MoreExecutors.directExecutor())
    }

    // What the player says, as one value; and while it plays, the position
    // twice a second, since nothing else moves the scrubber.
    private fun read() {
        val c = controller ?: return
        val tl = c.currentTimeline
        val next = ArrayList<Playing>()
        if (!tl.isEmpty && c.currentMediaItemIndex != C.INDEX_UNSET) {
            var i = tl.getNextWindowIndex(c.currentMediaItemIndex, Player.REPEAT_MODE_OFF, c.shuffleModeEnabled)
            while (i != C.INDEX_UNSET && next.size < UP_NEXT) {
                next.add(playing(c.getMediaItemAt(i)))
                i = tl.getNextWindowIndex(i, Player.REPEAT_MODE_OFF, c.shuffleModeEnabled)
            }
        }
        _state.value =
            PlayerState(
                now = c.currentMediaItem?.let(::playing),
                playing = c.isPlaying,
                buffering = c.playbackState == Player.STATE_BUFFERING,
                positionMs = c.currentPosition.coerceAtLeast(0),
                durationMs = c.duration.takeIf { it != C.TIME_UNSET }?.coerceAtLeast(0) ?: 0,
                shuffle = c.shuffleModeEnabled,
                repeat =
                    when (c.repeatMode) {
                        Player.REPEAT_MODE_ALL -> Repeat.All
                        Player.REPEAT_MODE_ONE -> Repeat.One
                        else -> Repeat.Off
                    },
                upNext = next,
            )
        if (c.isPlaying && ticking?.isActive != true) {
            ticking =
                scope.launch {
                    while (isActive) {
                        delay(500)
                        val now = controller ?: break
                        _state.value = _state.value.copy(positionMs = now.currentPosition.coerceAtLeast(0))
                        if (!now.isPlaying) break
                    }
                }
        }
    }

    private fun item(t: Track, album: Album?): MediaItem {
        val uri = Uri.parse(Media.url(server, t.file))
        val art = album?.art?.takeIf { it.isNotEmpty() }?.let { Uri.parse(Media.url(server, it)) }
        return MediaItem.Builder()
            .setMediaId(t.id.hex)
            .setUri(uri)
            .setRequestMetadata(MediaItem.RequestMetadata.Builder().setMediaUri(uri).build())
            .setMediaMetadata(
                MediaMetadata.Builder()
                    .setTitle(t.title)
                    .setArtist(t.creator)
                    .setAlbumTitle(album?.name)
                    .setArtworkUri(art)
                    .setIsPlayable(true)
                    .setIsBrowsable(false)
                    .build())
            .build()
    }

    private fun playing(m: MediaItem): Playing =
        Playing(
            id = m.mediaId,
            title = m.mediaMetadata.title?.toString() ?: "",
            artist = m.mediaMetadata.artist?.toString() ?: "",
            album = m.mediaMetadata.albumTitle?.toString() ?: "",
            art = m.mediaMetadata.artworkUri?.toString() ?: "",
        )

    /** Play `queue` from `at`: the list the track was started from, with each track's album. */
    fun play(queue: List<Track>, at: Int, albumOf: (Track) -> Album?, shuffled: Boolean = false) {
        val c = controller ?: return
        if (queue.isEmpty()) return
        c.shuffleModeEnabled = shuffled
        c.setMediaItems(queue.map { item(it, albumOf(it)) }, at.coerceIn(0, queue.size - 1), 0)
        c.prepare()
        c.play()
    }

    /** Put `track` right after what is playing; or play it, when nothing is. */
    fun playNext(track: Track, album: Album?) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) return play(listOf(track), 0, { album })
        c.addMediaItem(c.currentMediaItemIndex + 1, item(track, album))
    }

    fun toggle() {
        val c = controller ?: return
        if (c.isPlaying) c.pause() else c.play()
    }

    fun next() = controller?.seekToNext() ?: Unit

    fun previous() = controller?.seekToPrevious() ?: Unit

    fun seek(ms: Long) {
        controller?.seekTo(ms)
        _state.value = _state.value.copy(positionMs = ms)
    }

    fun shuffle() {
        val c = controller ?: return
        c.shuffleModeEnabled = !c.shuffleModeEnabled
    }

    /** Off, all, one, off. */
    fun repeat() {
        val c = controller ?: return
        c.repeatMode =
            when (c.repeatMode) {
                Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
                Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
                else -> Player.REPEAT_MODE_OFF
            }
    }

    /** Jump to the `n`th of [PlayerState.upNext]. */
    fun skipAhead(n: Int) {
        val c = controller ?: return
        var i = c.currentMediaItemIndex
        for (step in 0..n) {
            if (i == C.INDEX_UNSET) break
            i = c.currentTimeline.getNextWindowIndex(i, Player.REPEAT_MODE_OFF, c.shuffleModeEnabled)
        }
        if (i != C.INDEX_UNSET) c.seekTo(i, 0)
    }

    /** Let go of the controller; the service, and the music, go on. */
    fun close() {
        ticking?.cancel()
        MediaController.releaseFuture(future)
        controller = null
    }

    private companion object {
        /** How much of what is to come the queue sheet shows. */
        const val UP_NEXT = 100
    }
}
