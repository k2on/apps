// ark-media-player on Android (modules/ark-media-player/README.md): the `NativeMediaPlayer`
// interface UniFFI writes, over Media3's ExoPlayer inside a media session service
// (ArkMediaService.kt) — so the sound outlives the activity, the notification and its controls,
// the lock screen and a headset's buttons are the media session's, the audio focus is asked for
// and given back, and the headphones out pause it (becoming noisy). An app that lists
// `media-player` in its modules.toml compiles this file in and hands one to its bridge, which
// registers it as the port (`ark_media_player::phone::port`).
//
// One player a process: [ArkMedia] holds the ExoPlayer and what the system is shown, and the
// service's media session shows [ArkMedia.shown], a player that draws them and does nothing it
// is asked — every command of the system's (play, pause, a skip, a seek, a speed) is said to the
// app as a `Remote`, and the app decides. Every call returns at once and is done on the main
// thread; every event is told from it.
//
// The picture is always one: the artwork from its URI, or — with none — ark-ui's derived square of
// the app's `placeholder` seed, in the palette the app gave ([ArkMedia.squares]), handed to the
// media session as the picture's bytes. Never an empty tile.
package dev.arkui.modules

import android.content.ComponentName
import android.content.Context
import android.graphics.Bitmap
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.annotation.OptIn
import androidx.compose.ui.graphics.Color
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import dev.arkui.ArkTheme
import dev.arkui.squareBitmap
import java.io.ByteArrayOutputStream
import uniffi.ark_media_player.BrowseItem
import uniffi.ark_media_player.MediaControls
import uniffi.ark_media_player.MediaEvent
import uniffi.ark_media_player.MediaListener
import uniffi.ark_media_player.NativeMediaPlayer
import uniffi.ark_media_player.NowPlaying
import uniffi.ark_media_player.RemoteCommand
import uniffi.ark_media_player.RouteChange

/**
 * The phone's player. [service] is the media session service the app declares — ArkMediaService,
 * or the app's own subclass of it — which a controller binds so the session lives while the app
 * does, and which the system starts in the foreground while it plays; none from inside that
 * service, which is alive already.
 */
@OptIn(UnstableApi::class)
class PhoneMediaPlayer(
    context: Context,
    service: Class<out ArkMediaService>? = ArkMediaService::class.java,
) : NativeMediaPlayer {
    private val app = context.applicationContext
    private val main = Handler(Looper.getMainLooper())

    init {
        main.post {
            ArkMedia.start(app)
            // Bound for as long as the process lives: the service and its session with it.
            if (service != null) MediaController.Builder(app, SessionToken(app, ComponentName(app, service))).buildAsync()
        }
    }

    override fun load(url: String, atMs: Long) {
        main.post { ArkMedia.load(url, atMs) }
    }

    override fun play() {
        main.post { ArkMedia.exo?.play() }
    }

    override fun pause() {
        main.post { ArkMedia.exo?.pause() }
    }

    override fun seek(atMs: Long) {
        main.post { ArkMedia.seek(atMs) }
    }

    override fun setRate(rate: Float) {
        main.post { ArkMedia.exo?.playbackParameters = PlaybackParameters(if (rate > 0) rate else 1f) }
    }

    override fun setVolume(volume: Float) {
        main.post { ArkMedia.exo?.volume = volume.coerceIn(0f, 1f) }
    }

    override fun stop() {
        main.post { ArkMedia.unload() }
    }

    override fun setNowPlaying(info: NowPlaying?) {
        main.post { ArkMedia.show(info) }
    }

    override fun setControls(controls: MediaControls) {
        main.post { ArkMedia.offer(controls) }
    }

    override fun setChildren(parent: String, items: List<BrowseItem>) {
        main.post { ArkMedia.children(parent, items) }
    }

    override fun setListener(listener: MediaListener?) {
        main.post { ArkMedia.listener = listener }
    }
}

/** The process's one player and what the system is shown of it: the main thread's only. */
@OptIn(UnstableApi::class)
object ArkMedia {
    var exo: ExoPlayer? = null
        private set

    /** What the media session shows. */
    var shown: ShownPlayer? = null
        private set

    var listener: MediaListener? = null
    internal var info: NowPlaying? = null
    internal var controls = MediaControls(false, false, 0u, 0u, false, false, false)

    /**
     * The derived square's gradients the placeholder is drawn in: the app's, for the dark side, as
     * the notification and the lock screen draw it over a dark shade. Ark-ui's neutral ones until
     * the app sets its own.
     */
    var squares: List<Pair<Color, Color>> = ArkTheme.NeutralDark

    /** The placeholder last drawn, by its seed and palette: drawn once, not at every state. */
    private var drawn: Triple<String, List<Pair<Color, Color>>, ByteArray>? = null

    /** The derived square of `seed`, as the picture's bytes (PNG). */
    private fun placeholder(seed: String): ByteArray {
        drawn?.takeIf { it.first == seed && it.second == squares }?.let { return it.third }
        val out = ByteArrayOutputStream()
        squareBitmap(seed, squares, PLACEHOLDER_PX).compress(Bitmap.CompressFormat.PNG, 100, out)
        return out.toByteArray().also { drawn = Triple(seed, squares, it) }
    }

    /** Whether the ExoPlayer has a sound of the app's: else what is shown is the app's [info] alone. */
    internal var loaded = false
    private var told = false
    private val main = Handler(Looper.getMainLooper())

    /** The browse tree as the app gave it, by parent, and who waits for a parent it has not. */
    internal val tree = HashMap<String, List<BrowseItem>>()
    internal val waiting = HashMap<String, MutableList<(List<BrowseItem>) -> Unit>>()

    internal fun tell(e: MediaEvent) {
        listener?.heard(e)
    }

    /** The ExoPlayer and the player the session shows, made once. */
    fun start(context: Context) {
        if (exo != null) return
        val app = context.applicationContext
        val p =
            ExoPlayer.Builder(app)
                .setAudioAttributes(
                    AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
                    /* handleAudioFocus= */ true)
                .setHandleAudioBecomingNoisy(true)
                .setWakeMode(C.WAKE_MODE_NETWORK)
                .build()
        p.addListener(Heard)
        exo = p
        shown = ShownPlayer(Looper.getMainLooper())
        // Headphones in: said (out is ExoPlayer's becoming-noisy, which pauses it).
        app.getSystemService(AudioManager::class.java)?.registerAudioDeviceCallback(
            object : AudioDeviceCallback() {
                override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) {
                    if (exo != null && added.any { it.isSink }) tell(MediaEvent.Route(RouteChange.PLUGGED))
                }
            },
            main)
    }

    fun load(url: String, atMs: Long) {
        val p = exo ?: return
        told = false
        loaded = true
        p.setMediaItem(item(url), atMs.coerceAtLeast(0))
        p.playWhenReady = false
        p.prepare()
        shown?.changed()
    }

    fun seek(atMs: Long) {
        val p = exo ?: return
        p.seekTo(atMs.coerceAtLeast(0))
        tell(MediaEvent.Position(p.currentPosition))
    }

    fun unload() {
        exo?.run {
            stop()
            clearMediaItems()
        }
        loaded = false
        shown?.changed()
    }

    /** The sound at `url`, with what the system shows of it. */
    private fun item(url: String): MediaItem =
        MediaItem.Builder().setMediaId(url).setUri(url).setMediaMetadata(metadata(info)).build()

    internal fun metadata(i: NowPlaying?): MediaMetadata {
        val b = MediaMetadata.Builder().setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC).setIsPlayable(true).setIsBrowsable(false)
        if (i == null) return b.build()
        b.setTitle(i.title).setArtist(i.artist).setAlbumTitle(i.album).setDurationMs(i.durationMs.takeIf { it > 0 })
        val art = i.artwork
        val seed = i.placeholder
        when {
            art != null -> b.setArtworkUri(Uri.parse(art))
            seed != null -> b.setArtworkData(placeholder(seed), MediaMetadata.PICTURE_TYPE_FRONT_COVER)
        }
        return b.build()
    }

    fun show(i: NowPlaying?) {
        info = i
        // The playing item's words and picture again: the same sound, so it plays on.
        val p = exo
        val now = p?.currentMediaItem
        if (p != null && loaded && now != null) p.replaceMediaItem(p.currentMediaItemIndex, now.buildUpon().setMediaMetadata(metadata(i)).build())
        shown?.changed()
    }

    fun offer(c: MediaControls) {
        controls = c
        shown?.changed()
    }

    fun children(parent: String, items: List<BrowseItem>) {
        tree[parent] = items
        waiting.remove(parent)?.forEach { it(items) }
    }

    /** What the ExoPlayer did, told as the app hears it. */
    private object Heard : Player.Listener {
        private val ticking = Runnable { tick() }

        private fun tick() {
            val p = exo ?: return
            if (!p.isPlaying) return
            tell(MediaEvent.Position(p.currentPosition))
            main.postDelayed(ticking, TICK_MS)
        }

        override fun onPlaybackStateChanged(state: Int) {
            val p = exo ?: return
            when (state) {
                Player.STATE_READY ->
                    if (!told) {
                        told = true
                        tell(MediaEvent.Loaded(p.duration.takeIf { it != C.TIME_UNSET } ?: 0))
                    }
                Player.STATE_BUFFERING -> if (p.playWhenReady) tell(MediaEvent.Waiting)
                Player.STATE_ENDED -> tell(MediaEvent.Ended)
            }
            shown?.changed()
        }

        override fun onIsPlayingChanged(isPlaying: Boolean) {
            val p = exo ?: return
            if (isPlaying) {
                tell(MediaEvent.Playing)
                main.removeCallbacks(ticking)
                main.postDelayed(ticking, TICK_MS)
            } else {
                main.removeCallbacks(ticking)
                if (p.playbackState != Player.STATE_ENDED) tell(MediaEvent.Paused)
            }
            shown?.changed()
        }

        // The system pausing it: the focus lost for good, or the headphones out.
        override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
            if (playWhenReady) return
            when (reason) {
                Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_FOCUS_LOSS -> tell(MediaEvent.Interrupted)
                Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_BECOMING_NOISY -> tell(MediaEvent.Route(RouteChange.UNPLUGGED))
            }
        }

        // The focus lost for a moment (a call, a navigation prompt): held, then given back, and the
        // player goes on by itself.
        private var suppressed = false

        override fun onPlaybackSuppressionReasonChanged(reason: Int) {
            val p = exo ?: return
            val now = reason != Player.PLAYBACK_SUPPRESSION_REASON_NONE
            if (now == suppressed) return
            suppressed = now
            if (now) tell(MediaEvent.Interrupted) else if (p.playWhenReady) tell(MediaEvent.Resumed)
        }

        override fun onPlayerError(error: PlaybackException) = tell(MediaEvent.Failed(error.message ?: error.errorCodeName))
    }

    private const val TICK_MS = 500L

    /** How many pixels across the placeholder is drawn: a lock screen's cover. */
    private const val PLACEHOLDER_PX = 512
}

/**
 * The player the media session shows: the ExoPlayer's sound with the app's words, or — nothing
 * loaded — the app's [NowPlaying] alone, a sound made elsewhere; and the controls the app offers.
 * Nothing it is asked is done: each command is said to the app as a `Remote`, a car's choice as a
 * `PlayItem`, what a car or a watch adds to the queue (while the app offers it) as an `Add`.
 */
@OptIn(UnstableApi::class)
class ShownPlayer(looper: Looper) : SimpleBasePlayer(looper) {
    fun changed() = invalidateState()

    override fun getState(): State {
        val c = ArkMedia.controls
        val commands =
            Player.Commands.Builder()
                .addAll(
                    Player.COMMAND_PLAY_PAUSE,
                    Player.COMMAND_PREPARE,
                    Player.COMMAND_STOP,
                    Player.COMMAND_SET_MEDIA_ITEM,
                    Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                    Player.COMMAND_GET_TIMELINE,
                    Player.COMMAND_GET_METADATA,
                    Player.COMMAND_RELEASE,
                )
                .apply {
                    if (c.canSeek) addAll(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM, Player.COMMAND_SEEK_TO_DEFAULT_POSITION)
                    if (c.hasNext) addAll(Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM)
                    if (c.hasPrevious) addAll(Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
                    if (c.skipForwardS > 0u) add(Player.COMMAND_SEEK_FORWARD)
                    if (c.skipBackS > 0u) add(Player.COMMAND_SEEK_BACK)
                    if (c.canRate) add(Player.COMMAND_SET_SPEED_AND_PITCH)
                    if (c.canAdd) add(Player.COMMAND_CHANGE_MEDIA_ITEMS)
                }
                .build()
        val b = State.Builder().setAvailableCommands(commands)
        if (c.skipForwardS > 0u) b.setSeekForwardIncrementMs(c.skipForwardS.toLong() * 1000)
        if (c.skipBackS > 0u) b.setSeekBackIncrementMs(c.skipBackS.toLong() * 1000)
        val info = ArkMedia.info ?: return b.setPlaybackState(Player.STATE_IDLE).setPlaylist(emptyList()).build()
        val p = ArkMedia.exo
        val own = p != null && ArkMedia.loaded
        val item =
            MediaItemData.Builder("now")
                .setMediaItem(MediaItem.Builder().setMediaId("now").setMediaMetadata(ArkMedia.metadata(info)).build())
                .setDurationUs(
                    (if (own && p!!.duration != C.TIME_UNSET) p.duration else info.durationMs).takeIf { it > 0 }?.let { it * 1000 } ?: C.TIME_UNSET)
                .setIsSeekable(c.canSeek)
                .build()
        b.setPlaylist(listOf(item)).setCurrentMediaItemIndex(0)
        if (own) {
            val e = p!!
            return b
                .setContentPositionMs { e.currentPosition }
                .setPlayWhenReady(e.playWhenReady, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
                .setPlaybackState(if (e.playbackState == Player.STATE_IDLE) Player.STATE_BUFFERING else e.playbackState)
                .setPlaybackParameters(e.playbackParameters)
                .setIsLoading(e.isLoading)
                .build()
        }
        // A sound made elsewhere, drawn as the app says it: counted on while it plays.
        val playing = info.rate > 0f
        return b
            .setContentPositionMs(
                if (playing) PositionSupplier.getExtrapolating(info.positionMs, info.rate) else PositionSupplier.getConstant(info.positionMs))
            .setPlayWhenReady(playing, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
            .setPlaybackState(Player.STATE_READY)
            .build()
    }

    private fun ask(c: RemoteCommand): ListenableFuture<*> {
        ArkMedia.tell(MediaEvent.Remote(c))
        return Futures.immediateVoidFuture()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> =
        ask(if (playWhenReady) RemoteCommand.Play else RemoteCommand.Pause)

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> =
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> ask(RemoteCommand.Next)
            Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> ask(RemoteCommand.Previous)
            Player.COMMAND_SEEK_FORWARD -> ask(RemoteCommand.SkipForward(ArkMedia.controls.skipForwardS))
            Player.COMMAND_SEEK_BACK -> ask(RemoteCommand.SkipBack(ArkMedia.controls.skipBackS))
            else -> ask(RemoteCommand.SeekTo(if (positionMs == C.TIME_UNSET) 0 else positionMs))
        }

    override fun handleSetPlaybackParameters(playbackParameters: PlaybackParameters): ListenableFuture<*> =
        ask(RemoteCommand.Rate(playbackParameters.speed))

    // A headset's stop, the notification's dismissal: the app's to say what it means.
    override fun handleStop(): ListenableFuture<*> = ask(RemoteCommand.Stop)

    // A car's or a watch's choice: the item it chose, by the app's id.
    override fun handleSetMediaItems(mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<*> {
        val id = mediaItems.getOrNull(startIndex.coerceAtLeast(0))?.mediaId ?: mediaItems.firstOrNull()?.mediaId
        if (!id.isNullOrEmpty()) ArkMedia.tell(MediaEvent.PlayItem(id))
        return Futures.immediateVoidFuture()
    }

    // A car's or a watch's additions, by the app's ids: before the one item shown is after what
    // plays, anywhere past it the end — as a controller asks for "play next" and "add to queue".
    override fun handleAddMediaItems(index: Int, mediaItems: MutableList<MediaItem>): ListenableFuture<*> {
        val ids = mediaItems.map { it.mediaId }.filter { it.isNotEmpty() }
        if (ids.isNotEmpty()) ArkMedia.tell(MediaEvent.Add(ids, index < mediaItemCount))
        return Futures.immediateVoidFuture()
    }

    // The queue is the app's: one item shown, nothing of it taken out, moved or replaced here.
    override fun handleRemoveMediaItems(fromIndex: Int, toIndex: Int): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleMoveMediaItems(fromIndex: Int, toIndex: Int, newIndex: Int): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleReplaceMediaItems(fromIndex: Int, toIndex: Int, mediaItems: MutableList<MediaItem>): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleRelease(): ListenableFuture<*> = Futures.immediateVoidFuture()
}
