// The player: Media3's ExoPlayer inside a `MediaSessionService`, the
// account's listening session around it, and the screens' end of both.
//
// The service owns the sound, so it outlives every screen and the activity
// itself: leaving the app does not stop the music, and the notification,
// the lock screen and a Bluetooth headset's buttons are the media
// session's, which Media3 draws and answers. **It holds the app's session
// for as long as it lives** (Kept.kt) — which Android ties to its playing or
// being bound — so the activity going is not the phone leaving the
// account's listening session: the socket, the device and the sound stay.
// With no activity at all, it opens the session itself from the login the
// onboarding remembered.
//
// **It is a media library** (`MediaLibraryService`), which is what a car
// browses (Car.kt): the tree, search, voice and resumption are read from the
// replica, and what a car chooses is handed to whichever player the media
// session has — the session's, as a press, or this phone's own — the same
// way as anything else that sets a queue. One rule is the car's alone: a
// choice made in it plays on this phone, whatever is sounding elsewhere
// ([RemotePlayer.fromCar]); a car's covers are `content://` URIs
// (CoverProvider.kt), granted to it as it is handed them. What it plays is
// `/media/<file>` from the session's server (`Media.url`), streamed with
// range requests, which is what makes seeking work. Notifications are the
// one permission the onboarding asks for, because they are what the
// notification and its controls need on Android 13 and later; without it
// the music still plays.
//
// **Two players, one media session** (docs/plan-live.md phase 3). While this
// phone is part of the account's listening session the media session's
// player is a [RemotePlayer]: the session drawn — its track, playing or not,
// a position counted from its clock against the server's — whichever device
// is making the sound, and every button pressed on it a transition. So the
// lock screen is a remote for the session when the sound is on the laptop,
// and the same remote when it is here; with nothing queued anywhere it is
// empty and there is no notification. The ExoPlayer is then the
// reconciler's alone (Remote.kt, through [ExoSound]): it plays exactly when
// the session names this phone its output. Unlinked — no session, or the
// socket down — the media session's player is the ExoPlayer itself, this
// phone's own, as it always was.
//
// The screens never touch the ExoPlayer. [Playback] is their end: while the
// phone is part of the session it draws the session ([Heard]) and every
// button is a press of it (`Listen.press`); alone it connects a controller,
// hands it a queue — the list a track was started from, snapshotted, so
// somebody else's edit arriving does not redirect what is playing — and
// reads back what the player says. One [PlayerState] either way, which the
// mini player and the full player both draw.
//
// **The position is its own flow** ([Playback.position]), apart from the
// state: it is the one thing that moves while nothing else does, twice a
// second while a track plays, and only the hairline under the mini player
// and the full player's scrubber read it — so a tick recomposes those two
// and nothing above them, not the app's top and not a screen. The ticker
// runs only while something plays — here, or anywhere in the session; paused,
// stopped or with nothing loaded, nothing here runs (`docs/plan-reactive.md`).
// What another device's scrubber is being dragged to is a flow of its own
// ([Playback.scrubbing]), read by the scrubber alone.
//
// **The volume keys are whatever is playing's** (docs/plan-live.md "Volume
// and groups"), by Android's own mechanism for a remote, the way Cast does
// it: while the sound is on another device that has a volume, the session's
// player says it plays remotely (`DeviceInfo.PLAYBACK_TYPE_REMOTE`, 0 to 100)
// at that device's level — its group's, while it leads one — so the media
// session hands the buttons and the system's volume panel to the player
// rather than to the phone's own stream, and each is a transition: a press
// `nudge_volume` (or `nudge_group_volume`) by [NUDGE], the panel's slider
// `set_volume`, its mute `set_mute`. While the sound is here, or nowhere, it
// plays locally and the buttons move the phone's media stream, as they
// always did — which the phone, a device of the session with a volume, says
// as `report_volume` (Remote.kt). A car's own knob is the car's: a volume a
// car's controller asks for is never sent to a speaker.
package dev.harken.android

import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.database.ContentObserver
import android.media.AudioManager
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.DeviceInfo
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaController
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionError
import androidx.media3.session.SessionToken
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.SettableFuture
import dev.arkdb.Id
import dev.arkdb.android.ArkSession
import dev.arkdb.android.ArkSessions
import dev.arkdb.android.Hold
import dev.arkdb.android.Sessions
import dev.harken.android.listening.Output
import dev.harken.android.listening.Track as Queued
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/** The service: one ExoPlayer, the session's player beside it, and the media library session around them. */
@OptIn(UnstableApi::class)
class PlaybackService : MediaLibraryService() {
    private var session: MediaLibrarySession? = null
    private lateinit var exo: ExoPlayer
    private lateinit var sound: ExoSound
    private lateinit var remote: RemotePlayer
    private val scope = MainScope()
    private lateinit var sessions: Sessions

    /** The app's session, held for as long as this service lives; none while nobody is signed in. */
    private var hold: Hold? = null

    /** Stop ringing the reconciler when the phone's media stream moves. */
    private var unwatch: () -> Unit = {}

    override fun onCreate() {
        super.onCreate()
        exo =
            ExoPlayer.Builder(this)
                .setAudioAttributes(
                    AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
                    /* handleAudioFocus= */ true)
                .setHandleAudioBecomingNoisy(true)
                .setWakeMode(C.WAKE_MODE_NETWORK)
                .build()
        sound = ExoSound(exo, Handler(mainLooper))
        // The phone's own volume, a device's of the session: its media stream, which the
        // reconciler follows and reports — rung whenever a volume setting moves, since what moved
        // it is read from the stream itself.
        val stream = MusicStream(this)
        sound.stream = StreamLevel(stream)
        unwatch = stream.watch { Listen.current.value?.poke() }
        remote = RemotePlayer(mainLooper)
        val open =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val made = MediaLibrarySession.Builder(this, exo, Browsing()).setSessionActivity(open).build()
        session = made
        // A choice in a car plays here: the media session says which controller is asking.
        remote.fromCar = { made.controllerForCurrentRequest?.let { isCar(made, it) } == true }
        // Streaming needs the server: a stream that cannot be fetched is said, in a sentence, to
        // whatever is showing the player — a car's screen most of all — rather than a silence.
        exo.addListener(
            object : Player.Listener {
                override fun onPlayerError(error: PlaybackException) {
                    if (error.errorCode / 1000 != 2) return
                    val host = Uri.parse(hold?.session?.server ?: "").host ?: "the server"
                    made.sendError(SessionError(SessionError.ERROR_IO, "Harken can't reach $host. Your library is on this phone; playing it needs the server."))
                }
            })
        // Whichever session the app has open, followed; none, and the ExoPlayer is this phone's own.
        scope.launch { Listen.current.collectLatest { follow(it) } }
        // The app's session, held: the one the activity has open, or — with none, and a login
        // remembered — one opened here.
        sessions = Kept.sessions(this)
        scope.launch { sessions.current.collect { adopt(it) } }
        ensure()
    }

    // Hold the session the process has open, if this service does not already; let go of one
    // that was closed for everyone.
    private fun adopt(s: ArkSession?) {
        val h = hold
        if (s == null || s.isClosed) {
            if (h != null && h.session.isClosed) hold = null
            return
        }
        if (h?.session === s) return
        h?.release()
        hold = sessions.hold(s.server, s.login, "playback")
        Kept.of(s, Device.name(this))
    }

    /** A session to play from and browse: held, or opened from the remembered login; none signed out. */
    private fun ensure(): ArkSession? {
        hold?.session?.takeIf { !it.isClosed }?.let { return it }
        sessions.current.value?.takeIf { !it.isClosed }?.let {
            adopt(it)
            return it
        }
        val (server, login) = ArkSessions.remembered(this) ?: return null
        val h = sessions.hold(server, login, "playback")
        hold = h
        Kept.of(h.session, Device.name(this))
        return h.session
    }

    private suspend fun follow(l: Listen?) {
        if (l == null) {
            sound.listen = null
            remote.listen = null
            use(exo)
            return
        }
        sound.listen = l
        remote.listen = l
        l.attach(sound)
        try {
            l.heard.collect { h ->
                remote.show(h)
                use(if (h.joined) remote else exo)
            }
        } finally {
            l.detach(sound)
            sound.listen = null
        }
    }

    // The media session's player, swapped only when it changes: what the lock screen, the
    // notification and a headset reach.
    private fun use(p: Player) {
        val s = session ?: return
        if (s.player !== p) s.player = p
    }

    private fun isCar(s: MediaSession, c: MediaSession.ControllerInfo): Boolean = s.isAutomotiveController(c) || s.isAutoCompanionController(c)

    /** What the car reads, over the session held; none while nobody is signed in. */
    private fun car(): Car? {
        val s = ensure() ?: return null
        val c = cars
        if (c != null && c.first === s) return c.second
        return Car(Kept.of(s, Device.name(this)), s.server).also {
            it.artPx = artPx
            cars = s to it
        }
    }

    private var cars: Pair<ArkSession, Car>? = null

    /** How many tabs the car draws, and how big its pictures are: as it said at its root. */
    private var tabs = Car.TABS
    private var artPx = CarArt.SIZE

    /** What the last searches found, by their words: a car asks for the results after the search. */
    private val searched = LinkedHashMap<String, List<MediaItem>>()

    // A suspended read, as the future Media3 waits on; on the main thread, as the callbacks are.
    private fun <T> later(f: suspend () -> T): ListenableFuture<T> {
        val out = SettableFuture.create<T>()
        scope.launch {
            try {
                out.set(f())
            } catch (e: Exception) {
                out.setException(e)
            }
        }
        return out
    }

    // A car reads only the pictures it was handed: each granted to it as it is.
    private fun grant(c: MediaSession.ControllerInfo, items: List<MediaItem>) {
        val to = c.packageName
        if (to == packageName || to == MediaSession.ControllerInfo.LEGACY_CONTROLLER_PACKAGE_NAME) return
        for (i in items) {
            val uri = i.mediaMetadata.artworkUri ?: continue
            if (uri.authority != CarArt.AUTHORITY) continue
            try {
                grantUriPermission(to, uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
            } catch (e: SecurityException) {
                // A package this phone cannot name: it draws no picture.
            }
        }
    }

    private val signIn = SessionError(SessionError.ERROR_SESSION_AUTHENTICATION_EXPIRED, "Sign in to Harken on your phone.")

    private fun <T : Any> signedOut(): LibraryResult<T> = LibraryResult.ofError<T>(signIn)

    // A pick as the media session hands it to its player: every track with its URL, its cover
    // and the page it came from.
    private fun items(s: ArkSession, p: Pick): MediaSession.MediaItemsWithStartPosition {
        val art = Kept.of(s, Device.name(this)).library.artOf
        return MediaSession.MediaItemsWithStartPosition(p.queue.map { Items.of(it, s.server, art(it.album), p.from) }, p.at, p.positionMs)
    }

    /**
     * The media library: the car's tree, its search, voice, resumption — and, as before, a
     * controller in another process handing items without their URI, the request metadata
     * carrying it.
     */
    private inner class Browsing : MediaLibrarySession.Callback {
        override fun onGetLibraryRoot(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> {
            val extras = params?.extras
            tabs = extras?.getInt(MediaConstants.EXTRAS_KEY_ROOT_CHILDREN_LIMIT, Car.TABS)?.takeIf { it > 0 } ?: Car.TABS
            artPx = extras?.getInt(MediaConstants.EXTRAS_KEY_MEDIA_ART_SIZE_PIXELS, CarArt.SIZE)?.takeIf { it > 0 } ?: CarArt.SIZE
            cars?.second?.artPx = artPx
            // The session opened now, if it can be, so the tabs it is about to ask for are read.
            ensure()
            return Futures.immediateFuture(LibraryResult.ofItem(Car.rootItem(), MediaLibraryService.LibraryParams.Builder().setExtras(Car.rootExtras()).build()))
        }

        override fun onGetChildren(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
            val car = car() ?: return Futures.immediateFuture(signedOut())
            return later {
                val list = car.children(parentId, page, pageSize, tabs) ?: return@later LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
                grant(browser, list)
                LibraryResult.ofItemList(list, params)
            }
        }

        override fun onGetItem(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> {
            val car = car() ?: return Futures.immediateFuture(signedOut())
            return later {
                val item = car.item(mediaId) ?: return@later LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
                grant(browser, listOf(item))
                LibraryResult.ofItem(item, null)
            }
        }

        override fun onSearch(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<Void>> {
            val car = car() ?: return Futures.immediateFuture(signedOut())
            scope.launch {
                val found = car.search(query)
                searched[query] = found
                while (searched.size > 8) searched.remove(searched.keys.first())
                session.notifySearchResultChanged(browser, query, found.size, params)
            }
            return Futures.immediateFuture(LibraryResult.ofVoid())
        }

        override fun onGetSearchResult(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, page: Int, pageSize: Int, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
            val car = car() ?: return Futures.immediateFuture(signedOut())
            return later {
                val all = searched[query] ?: car.search(query)
                val from = page.toLong() * pageSize
                val list = if (page < 0 || pageSize <= 0 || from >= all.size) emptyList() else all.subList(from.toInt(), minOf(all.size.toLong(), from + pageSize).toInt())
                grant(browser, list)
                LibraryResult.ofItemList(list, params)
            }
        }

        // One item a car or a voice asks to play: a browse id, the words of a search, or a track
        // with its URI. What it plays is the list the track was in, from it.
        override fun onSetMediaItems(mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
            val one = mediaItems.singleOrNull()
            if (one == null || one.localConfiguration != null || one.requestMetadata.mediaUri != null || Items.queued(one) != null)
                return Futures.immediateFuture(MediaSession.MediaItemsWithStartPosition(withUris(mediaItems), startIndex, startPositionMs))
            val s = ensure() ?: return Futures.immediateFailedFuture(UnsupportedOperationException(signIn.message))
            val car = car() ?: return Futures.immediateFailedFuture(UnsupportedOperationException(signIn.message))
            return later {
                val query = one.requestMetadata.searchQuery
                val pick = if (query != null) car.voice(query, one.requestMetadata.extras) else car.resolve(one.mediaId)
                if (pick == null) {
                    mediaSession.sendError(controller, SessionError(SessionError.ERROR_BAD_VALUE, if (query != null) "Nothing in your library matches \"$query\"." else "That isn't in your library any more."))
                    throw UnsupportedOperationException("nothing to play")
                }
                items(s, pick)
            }
        }

        override fun onAddMediaItems(mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>): ListenableFuture<MutableList<MediaItem>> {
            if (mediaItems.all { it.localConfiguration != null || Items.queued(it) != null }) return Futures.immediateFuture(withUris(mediaItems))
            val s = ensure() ?: return Futures.immediateFuture(withUris(mediaItems))
            val car = car() ?: return Futures.immediateFuture(withUris(mediaItems))
            return later {
                val out = ArrayList<MediaItem>()
                for (m in mediaItems) {
                    if (m.localConfiguration != null || Items.queued(m) != null) {
                        out.addAll(withUris(listOf(m)))
                        continue
                    }
                    val q = m.requestMetadata.searchQuery
                    val pick = (if (q != null) car.voice(q, m.requestMetadata.extras) else car.resolve(m.mediaId)) ?: continue
                    // A track added is that track; a search's words, everything they found.
                    val p = if (q != null) pick else Pick(listOf(pick.queue[pick.at]), 0, 0, pick.from)
                    out.addAll(items(s, p).mediaItems)
                }
                out
            }
        }

        // Play pressed with nothing loaded — a car starting, a headset's button: carrying on.
        override fun onPlaybackResumption(mediaSession: MediaSession, controller: MediaSession.ControllerInfo): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
            val s = ensure() ?: return Futures.immediateFailedFuture(UnsupportedOperationException(signIn.message))
            val car = car() ?: return Futures.immediateFailedFuture(UnsupportedOperationException(signIn.message))
            return later { car.resume()?.let { items(s, it) } ?: throw UnsupportedOperationException("nothing to resume") }
        }
    }

    // A controller in another process — a watch — hands items without their URI; the request
    // metadata carries it.
    private fun withUris(items: List<MediaItem>): MutableList<MediaItem> =
        items
            .map { item ->
                if (item.localConfiguration != null) item
                else item.requestMetadata.mediaUri?.let { item.buildUpon().setUri(it).build() } ?: item
            }
            .toMutableList()

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? = session

    // Swiped away from the recents while nothing plays: nothing to keep.
    override fun onTaskRemoved(rootIntent: Intent?) {
        val p = session?.player
        if (p == null || !p.playWhenReady || p.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        scope.cancel()
        unwatch()
        Listen.current.value?.detach(sound)
        session?.run {
            release()
        }
        exo.release()
        remote.release()
        session = null
        // Nothing plays and nothing is bound: the session closes, unless the activity holds it.
        hold?.release()
        hold = null
        super.onDestroy()
    }
}

/** How a track crosses a media item and back: the id, the words, and the path and length as extras. */
object Items {
    private const val FILE = "harken.file"
    private const val DURATION = "harken.duration_ms"
    private const val FROM = "harken.from"

    /**
     * `t` as a media item for `server`, with its cover's path and the route it was played from. A
     * track whose album has no cover carries the derived square, as a URI the app answers itself
     * (CoverProvider.kt): what the notification and a car draw, where the screens draw the square.
     */
    fun of(t: Queued, server: String, art: String, from: String): MediaItem {
        val uri = Uri.parse(Media.url(server, t.file))
        val extras = Bundle().apply {
            putString(FILE, t.file)
            putLong(DURATION, t.durationMs)
            putString(FROM, from)
        }
        return MediaItem.Builder()
            .setMediaId(t.id.hex)
            .setUri(uri)
            .setRequestMetadata(MediaItem.RequestMetadata.Builder().setMediaUri(uri).build())
            .setMediaMetadata(
                MediaMetadata.Builder()
                    .setTitle(t.title)
                    .setArtist(t.creator)
                    .setAlbumTitle(t.album)
                    .setArtworkUri(art.takeIf { it.isNotEmpty() }?.let { Uri.parse(Media.url(server, it)) } ?: CarArt.uri(null, t.album.ifEmpty { t.title }))
                    .setIsPlayable(true)
                    .setIsBrowsable(false)
                    .setExtras(extras)
                    .build())
            .build()
    }

    /** The track a media item carries, if it is one of ours. */
    fun queued(m: MediaItem): Queued? {
        val id = runCatching { Id.ofHex(m.mediaId) }.getOrNull() ?: return null
        val x = m.mediaMetadata.extras
        return Queued(
            album = m.mediaMetadata.albumTitle?.toString() ?: "",
            creator = m.mediaMetadata.artist?.toString() ?: "",
            durationMs = x?.getLong(DURATION) ?: 0,
            file = x?.getString(FILE) ?: "",
            id = id,
            title = m.mediaMetadata.title?.toString() ?: "",
        )
    }

    /** Where a media item was played from. */
    fun from(m: MediaItem?): String = m?.mediaMetadata?.extras?.getString(FROM) ?: ""

    /** `m`, saying where it is playing: what a car, a lock screen, draw under the title. */
    fun on(m: MediaItem, device: String): MediaItem =
        m.buildUpon().setMediaMetadata(m.mediaMetadata.buildUpon().setSubtitle("Playing on $device").build()).build()
}

/**
 * The ExoPlayer as the reconciler sees it ([Sound]): what it last said of itself, kept on the main
 * thread as one value and read from the session's, and what it is told, posted back to the main
 * thread — and written into that value at once, so the next reconcile sees what it has just asked.
 */
@OptIn(UnstableApi::class)
class ExoSound(private val exo: ExoPlayer, private val main: Handler) : Sound {
    private data class Said(
        val queue: List<Queued> = emptyList(),
        val index: Int = -1,
        val playing: Boolean = false,
        val buffering: Boolean = false,
        val positionMs: Long = 0,
        /** When the position was read, by the elapsed-realtime clock. */
        val readAt: Long = 0,
        /** Sounding as it was read: the position runs on from it. */
        val moving: Boolean = false,
        val ended: Boolean = false,
        val from: String = "",
    )

    @Volatile private var said = Said()
    private val stalled = AtomicBoolean(false)

    // Commands posted and not yet run: what the player says meanwhile is from before them, and
    // reading it would undo what was just asked. The command's own read follows it.
    private val inFlight = java.util.concurrent.atomic.AtomicInteger()

    /** Told of every event, so it reconciles; null while there is no session. */
    @Volatile var listen: Listen? = null

    /** The phone's media stream, as the session's 0 to 100: this player's volume. */
    @Volatile var stream: StreamLevel? = null

    private val tick = Runnable { poke(false) }

    init {
        exo.addListener(
            object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) = poke(events.contains(Player.EVENT_TIMELINE_CHANGED))

                // Paused by anything but a press — focus taken by a call, the headphones out — is
                // the sound stopping by itself.
                override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                    if (!playWhenReady && reason != Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST) stalled.set(true)
                }

                override fun onPlayerError(error: PlaybackException) = stalled.set(true)
            })
    }

    // What the player says now, kept; then a reconcile. On the main thread.
    private fun poke(timeline: Boolean) {
        if (inFlight.get() > 0) return
        read(timeline || said.queue.size != exo.mediaItemCount)
        listen?.poke()
    }

    // Told, on the main thread, in order; then read.
    private fun command(f: () -> Unit) {
        inFlight.incrementAndGet()
        main.post {
            try {
                f()
            } finally {
                inFlight.decrementAndGet()
            }
            poke(true)
        }
    }

    private fun read(timelineMoved: Boolean) {
        val q =
            if (!timelineMoved) said.queue
            else (0 until exo.mediaItemCount).mapNotNull { Items.queued(exo.getMediaItemAt(it)) }
        said =
            Said(
                queue = q,
                index = if (exo.mediaItemCount == 0) -1 else exo.currentMediaItemIndex,
                playing = exo.playWhenReady && exo.playerError == null,
                buffering = exo.playbackState == Player.STATE_BUFFERING,
                positionMs = exo.currentPosition.coerceAtLeast(0),
                readAt = SystemClock.elapsedRealtime(),
                moving = exo.isPlaying,
                ended = exo.playbackState == Player.STATE_ENDED,
                from = Items.from(exo.currentMediaItem),
            )
    }

    override val queue: List<Queued>
        get() = said.queue

    override val index: Int
        get() = said.index

    override val playing: Boolean
        get() = said.playing

    override val buffering: Boolean
        get() = said.buffering

    override val positionMs: Long
        get() = said.let { if (it.moving) it.positionMs + (SystemClock.elapsedRealtime() - it.readAt) else it.positionMs }

    override val ended: Boolean
        get() = said.ended

    override val from: String
        get() = said.from

    override fun takeStalled(): Boolean = stalled.getAndSet(false)

    private fun now(): Long = SystemClock.elapsedRealtime()

    override fun load(queue: List<Queued>, at: Int, positionMs: Long, playing: Boolean, from: String) {
        val l = listen
        val server = l?.server ?: return
        val items = queue.map { Items.of(it, server, l.art(it.album), from) }
        said = said.copy(queue = queue, index = at, playing = playing, positionMs = positionMs, readAt = now(), moving = false, ended = false, from = from, buffering = true)
        command {
            exo.setMediaItems(items, at, positionMs)
            exo.prepare()
            exo.playWhenReady = playing
        }
    }

    override fun jump(at: Int, positionMs: Long) {
        said = said.copy(index = at, positionMs = positionMs, readAt = now(), moving = false, ended = false)
        command { exo.seekTo(at, positionMs) }
    }

    override fun resume() {
        said = said.copy(playing = true)
        command {
            if (exo.playbackState == Player.STATE_IDLE) exo.prepare()
            exo.play()
        }
    }

    override fun pause() {
        said = said.copy(playing = false, moving = false, positionMs = positionMs, readAt = now())
        command { exo.pause() }
    }

    override fun seek(ms: Long) {
        said = said.copy(positionMs = ms, readAt = now(), ended = false)
        command { exo.seekTo(ms) }
    }

    override fun wakeIn(ms: Long) {
        main.removeCallbacks(tick)
        main.postDelayed(tick, ms)
    }

    // AudioManager's calls are the system's and safe from any thread: read and written here, on the
    // session's, so the reconciler reads back what it has just set.
    override fun volume(): Pair<Int, Boolean>? = stream?.read()

    override fun setVolume(level: Int, muted: Boolean) {
        stream?.apply(level, muted)
    }
}

/**
 * The phone's media stream (`STREAM_MUSIC`), as `AudioManager` has it: written without a flag, so no
 * panel rises for a volume another device set. Do Not Disturb, or a device policy, can refuse a
 * write; refused, it stays where it was, which the next read says.
 */
class MusicStream(context: Context) : Stream {
    private val audio = context.getSystemService(AudioManager::class.java)
    private val resolver = context.contentResolver

    override val min: Int
        get() = if (android.os.Build.VERSION.SDK_INT >= 28) audio.getStreamMinVolume(AudioManager.STREAM_MUSIC) else 0

    override val max: Int
        get() = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)

    override val index: Int
        get() = audio.getStreamVolume(AudioManager.STREAM_MUSIC)

    override val muted: Boolean
        get() = audio.isStreamMute(AudioManager.STREAM_MUSIC)

    override fun set(index: Int) {
        try {
            audio.setStreamVolume(AudioManager.STREAM_MUSIC, index, 0)
        } catch (e: SecurityException) {}
    }

    override fun mute(muted: Boolean) {
        try {
            audio.adjustStreamVolume(AudioManager.STREAM_MUSIC, if (muted) AudioManager.ADJUST_MUTE else AudioManager.ADJUST_UNMUTE, 0)
        } catch (e: SecurityException) {}
    }

    /**
     * `onChange`, on the main thread, whenever the system writes a volume setting down — which it does
     * for every stream's level on every device, whatever moved it: the buttons, the system's panel,
     * the settings, another app. A doorbell, not a reading: what rang it is read from the stream. A
     * `ContentObserver` on `Settings.System` rather than `VOLUME_CHANGED_ACTION`, which is hidden —
     * no part of the SDK, its extras undocumented — where a settings observer is public and has
     * always rung; its cost is that the system writes the setting down half a second after the
     * change, so the report is that late. A mute is not written down on every version, so a mute
     * alone may ring nothing; the next reconcile — any change of the session, any event of the
     * player — reads it.
     */
    fun watch(onChange: () -> Unit): () -> Unit {
        val o =
            object : ContentObserver(Handler(Looper.getMainLooper())) {
                override fun onChange(selfChange: Boolean) = onChange()
            }
        resolver.registerContentObserver(Settings.System.CONTENT_URI, true, o)
        return { resolver.unregisterContentObserver(o) }
    }
}

/**
 * The media session's player while this phone is part of the listening session: the session, as a
 * `Player` — its queue the playlist, its track the current item, its clock the position, counted on
 * from the server's time — whichever device is making the sound. Nothing it is asked is done here:
 * play, pause, a seek, the next or previous track, a queue set or added to, are each a press of the
 * session (`Listen.press`), and its state is the session's again once that press has been
 * previewed. With nothing queued it is empty, so there is no notification.
 */
@OptIn(UnstableApi::class)
class RemotePlayer(looper: Looper) : SimpleBasePlayer(looper) {
    /** The session this player draws; set on its looper, and the state asked again. */
    private var heard = Heard()

    /** The listening it presses; null, and every press is answered with nothing. */
    var listen: Listen? = null

    /**
     * Whether the controller asking now is a car's (the service asks the media session). A car's
     * choice, and a car's play, bring the sound to this phone first: in a car the phone is the
     * speaker, and a song chosen there playing on a laptop at home would be wrong.
     */
    var fromCar: () -> Boolean = { false }

    private val handler = Handler(looper)

    /** The system's volume panel, dragged: throttled as the full player's slider is. */
    private val panel =
        Drag(SystemClock::uptimeMillis, { ms, f -> handler.postDelayed(f, ms) }) { of, level -> listen?.press(Press.Loud(of, Change.To(level))) }

    fun show(h: Heard) {
        if (h == heard) return
        heard = h
        invalidateState()
    }

    override fun getState(): State {
        val loud = Forward.loud(heard)
        val b =
            State.Builder()
                .setAvailableCommands(if (loud != null) Forward.remoteCommands else Forward.commands)
                .setDeviceInfo(Forward.deviceInfo(loud))
                .setDeviceVolume(loud?.level ?: 0)
                .setIsDeviceMuted(loud?.muted ?: false)
        val shown = Forward.shown(heard) ?: return b.setPlaybackState(Player.STATE_IDLE).setPlaylist(emptyList()).build()
        val server = listen?.server ?: ""
        val art = listen?.art ?: { "" }
        return b
            .setPlaylist(
                shown.queue.mapIndexed { i, t ->
                    MediaItemData.Builder("$i:${t.id.hex}")
                        .setMediaItem(Items.of(t, server, art(t.album), shown.from).let { m -> if (i == shown.at && shown.on != null) Items.on(m, shown.on) else m })
                        .setDurationUs(t.durationMs * 1000)
                        .setIsSeekable(true)
                        .build()
                })
            .setCurrentMediaItemIndex(shown.at)
            .setContentPositionMs(
                if (shown.counting) PositionSupplier.getExtrapolating(shown.positionMs, 1f) else PositionSupplier.getConstant(shown.positionMs))
            .setPlayWhenReady(shown.playing, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
            .setPlaybackState(Player.STATE_READY)
            .build()
    }

    // Presses, in order, answered once the last has been previewed and the session published again.
    private fun press(vararg ps: Press?): ListenableFuture<*> {
        val l = listen ?: return Futures.immediateVoidFuture()
        val all = ps.filterNotNull()
        if (all.isEmpty()) return Futures.immediateVoidFuture()
        val done = SettableFuture.create<Unit>()
        for (p in all.dropLast(1)) l.press(p)
        l.press(all.last()) {
            val answer = Runnable {
                heard = l.heard.value
                done.set(Unit)
            }
            // On this player's own thread already — a session with no thread of its own — answered
            // at once; otherwise on it.
            if (Looper.myLooper() == handler.looper) answer.run() else handler.post(answer)
        }
        return done
    }

    // Play from a car is the sound brought here first; a pause is a pause, wherever it is asked.
    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> =
        if (!playWhenReady) press(Press.Pause) else if (fromCar()) press(Press.Here, Press.Play) else press(Press.Play)

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> =
        press(Forward.seek(heard, mediaItemIndex, positionMs))

    // A queue chosen in a car is started and the sound brought here; chosen anywhere else, it
    // plays where the session plays.
    override fun handleSetMediaItems(mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<*> {
        val start = Forward.start(mediaItems, startIndex, startPositionMs) ?: return Futures.immediateVoidFuture()
        return if (fromCar()) press(start, Press.Here) else press(start)
    }

    override fun handleAddMediaItems(index: Int, mediaItems: MutableList<MediaItem>): ListenableFuture<*> =
        press(mediaItems.firstOrNull()?.let(Items::queued)?.let { Press.Next(it) })

    // A headset's stop is a pause: stopping the session would forget a queue nobody asked to lose.
    override fun handleStop(): ListenableFuture<*> = press(Press.Pause)

    // The volume of whatever is playing, while that is another device: the buttons, the system's
    // panel. A car's own knob is the car's — in a car the phone is the speaker — so what a car's
    // controller asks of a volume is never sent to one.
    override fun handleIncreaseDeviceVolume(flags: Int): ListenableFuture<*> = press(Forward.nudge(heard, NUDGE, fromCar()))

    override fun handleDecreaseDeviceVolume(flags: Int): ListenableFuture<*> = press(Forward.nudge(heard, -NUDGE, fromCar()))

    override fun handleSetDeviceMuted(muted: Boolean, flags: Int): ListenableFuture<*> =
        press(Forward.loud(heard)?.takeIf { !fromCar() }?.let { Press.Loud(it.of, Change.Mute(muted)) })

    override fun handleSetDeviceVolume(deviceVolume: Int, flags: Int): ListenableFuture<*> {
        val loud = Forward.loud(heard)?.takeIf { !fromCar() } ?: return Futures.immediateVoidFuture()
        panel.to(loud.of, deviceVolume.coerceIn(0, 100))
        return Futures.immediateVoidFuture()
    }

    override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleRelease(): ListenableFuture<*> = Futures.immediateVoidFuture()
}

/** What [RemotePlayer] shows and what each command it is given asks: plain values, so a test reads them. */
@OptIn(UnstableApi::class)
object Forward {
    /** Everything a remote for the session can be asked; never shuffle or repeat, which it has not got. */
    val commands: Player.Commands =
        Player.Commands.Builder()
            .addAll(
                Player.COMMAND_PLAY_PAUSE,
                Player.COMMAND_PREPARE,
                Player.COMMAND_STOP,
                Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
                Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_PREVIOUS,
                Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_NEXT,
                Player.COMMAND_SEEK_TO_MEDIA_ITEM,
                Player.COMMAND_SET_MEDIA_ITEM,
                Player.COMMAND_CHANGE_MEDIA_ITEMS,
                Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                Player.COMMAND_GET_TIMELINE,
                Player.COMMAND_GET_METADATA,
                Player.COMMAND_RELEASE,
            )
            .build()

    /**
     * …and, while the sound is on another device with a volume, that volume too: read, set, moved a
     * step, muted — what a media session needs to hand the buttons and the system's panel to this
     * player rather than to the phone's own stream. Both the flagged commands and the older ones, so
     * a controller of either age finds the one it calls.
     */
    @Suppress("DEPRECATION")
    val remoteCommands: Player.Commands =
        commands
            .buildUpon()
            .addAll(
                Player.COMMAND_GET_DEVICE_VOLUME,
                Player.COMMAND_SET_DEVICE_VOLUME,
                Player.COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS,
                Player.COMMAND_ADJUST_DEVICE_VOLUME,
                Player.COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS,
            )
            .build()

    /**
     * The volume the buttons are, while they are another device's: the output's — its group's, while
     * it leads one — when that is not this phone and it has a volume. Null, and the phone plays
     * locally: the buttons are its own stream's, as they are whenever the sound is here — in a car
     * above all — or nowhere.
     */
    fun loud(h: Heard): Volume? = if (!h.joined) null else h.listening?.volume(h.me)?.takeIf { it.remote }

    /** Remote, 0 to 100, while the buttons are another device's; local otherwise. */
    fun deviceInfo(loud: Volume?): DeviceInfo =
        if (loud == null) DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_LOCAL).build()
        else DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE).setMinVolume(0).setMaxVolume(100).build()

    /** A volume button: the remote's volume moved a step — nothing from a car, whose knob is its own. */
    fun nudge(h: Heard, delta: Int, fromCar: Boolean = false): Press? =
        if (fromCar) null else loud(h)?.let { Press.Loud(it.of, Change.By(delta)) }

    /**
     * What the player shows of the session: its queue, the track, where in it — counted on from
     * the server's clock — and whether it plays. Null, and it is empty: no notification, since
     * nothing is queued anywhere, or this phone is not part of a session at all.
     */
    data class Shown(
        val queue: List<Queued>,
        val at: Int,
        val positionMs: Long,
        val playing: Boolean,
        val from: String,
        /** The device the sound is on, while that is not this phone: "Playing on Laptop". */
        val on: String? = null,
        /** Whether the position moves: playing, and not standing while the sound is handed over. */
        val counting: Boolean = playing,
    )

    fun shown(h: Heard): Shown? {
        val s = h.listening
        val a = s?.active
        if (!h.joined || a == null) return null
        val on = a.output.id?.takeIf { it != h.me }?.let { id -> s.devices[id]?.name ?: (a.output as? Output.Away)?.name ?: id }
        return Shown(a.queue, a.at.toInt().coerceIn(0, a.queue.size - 1), a.position(h.serverNow()), a.clock.playing, a.from, on, a.counting)
    }

    /**
     * A seek, as the session takes it: within the track it is a seek; to the next or the one before,
     * a skip — so `next` still means next if the queue moved meanwhile; anywhere else, a jump.
     */
    fun seek(h: Heard, index: Int, positionMs: Long): Press? {
        val a = h.listening?.active ?: return null
        val at = a.at.toInt()
        val ms = if (positionMs == C.TIME_UNSET) 0 else positionMs
        return when (index) {
            at -> Press.Seek(ms)
            at + 1 -> Press.Skip(1)
            at - 1 -> Press.Skip(-1)
            else -> if (index in a.queue.indices) Press.Jump(index) else null
        }
    }

    /** A queue set from outside — a car, a watch, this app's own controller — is a track pressed. */
    fun start(items: List<MediaItem>, startIndex: Int, startPositionMs: Long): Press? {
        val queue = items.mapNotNull(Items::queued)
        if (queue.isEmpty()) return null
        val at = if (startIndex == C.INDEX_UNSET) 0 else startIndex.coerceIn(0, queue.size - 1)
        return Press.Start(queue, at, if (startPositionMs == C.TIME_UNSET) 0 else startPositionMs, Items.from(items.getOrNull(at)))
    }
}

/** What is playing, as a screen draws it. */
data class Playing(val id: String, val title: String, val artist: String, val album: String, val art: String)

enum class Repeat {
    Off,
    All,
    One,
}

/** Where the sound is, as the player's device button says it, while this phone is part of the session. */
data class Where(
    val name: String,
    val kind: String,
    /** This phone: making the sound, or — with the sound nowhere — where the next press plays. */
    val here: Boolean,
    /** The device the sound belongs to has gone: "(not answering)". */
    val away: Boolean = false,
    /** The sound is on its way to it: "connecting…". */
    val moving: Boolean = false,
)

/** The player as the screens see it — all of it but the position, which is [Playback.position]. */
data class PlayerState(
    val now: Playing? = null,
    val playing: Boolean = false,
    val buffering: Boolean = false,
    val durationMs: Long = 0,
    val shuffle: Boolean = false,
    val repeat: Repeat = Repeat.Off,
    /** What plays after this one, in the order it will play — shuffled, when shuffle is on. */
    val upNext: List<Playing> = emptyList(),
    /** Where the sound is, while this phone is part of the session; null alone, and no device button. */
    val on: Where? = null,
    /**
     * What is playing's volume — the group's, while the output leads one; this phone's own stream,
     * alone — which the full player draws a slider and a mute for. Null, and none is drawn.
     */
    val volume: Volume? = null,
)

/** The screens' end: the session while this phone is part of it, a controller of the service's player otherwise. */
class Playback(
    private val context: Context,
    private val server: String,
    /** Connect to the media session; a test that drives [show] itself does not. */
    connect: Boolean = true,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow(PlayerState())
    val state: StateFlow<PlayerState> = _state.asStateFlow()
    private val _position = MutableStateFlow(0L)

    /** Where in the track the player is, in milliseconds: moved while it plays, and on a seek. */
    val position: StateFlow<Long> = _position.asStateFlow()

    private val _scrubbing = MutableStateFlow<Long?>(null)

    /** Where another device's scrubber is being dragged to, while one is: what this one shows meanwhile. */
    val scrubbing: StateFlow<Long?> = _scrubbing.asStateFlow()

    private var controller: MediaController? = null
    private var ticking: Job? = null

    /** The phone's own media stream: the volume the slider moves while this phone is its own session. */
    private val stream: StreamLevel?
    private val unwatch: () -> Unit

    /** A volume slider dragged — the full player's, a picker row's — sent as throttled transitions. */
    private val drag =
        Drag(SystemClock::uptimeMillis, { ms, f -> scope.launch { delay(ms); f() } }) { of, level -> listen?.press(Press.Loud(of, Change.To(level))) }
    private var listen: Listen? = null
    private var heard = Heard()
    private var following: Job? = null

    private val future: ListenableFuture<MediaController>? =
        if (!connect) null
        else MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync()

    /** What the screens are shown, set directly — a test's, with no player behind it. */
    internal fun show(state: PlayerState) {
        _state.value = state
    }

    /** Where the screens are shown the player is — a test's position ticking. */
    internal fun showPosition(ms: Long) {
        _position.value = ms
    }

    /** Where the screens are shown another device is scrubbing to — a test's. */
    internal fun showScrubbing(ms: Long?) {
        _scrubbing.value = ms
    }

    /** Whether the position ticker is running: only while something plays. */
    internal val ticks: Boolean
        get() = ticking?.isActive == true

    init {
        val music = if (connect) MusicStream(context) else null
        stream = music?.let(::StreamLevel)
        unwatch = music?.watch { if (!joined) showVolume() } ?: {}
        val future = future
        if (future != null) future.addListener(
            {
                val c = runCatching { future.get() }.getOrNull() ?: return@addListener
                controller = c
                c.addListener(
                    object : Player.Listener {
                        override fun onEvents(player: Player, events: Player.Events) = read()
                    })
                read()
            },
            MoreExecutors.directExecutor())
    }

    /** Follow the account's listening session: drawn while this phone is part of it, and pressed. */
    fun follow(l: Listen) {
        listen = l
        following?.cancel()
        following = scope.launch {
            l.heard.collect { h ->
                val was = heard.joined
                heard = h
                _scrubbing.value = if (h.joined) h.scrubbing else null
                if (h.joined) drawSession() else if (was) read()
            }
        }
    }

    private val joined: Boolean
        get() = listen != null && heard.joined

    // The session, as one value; and while it plays anywhere — and only then — the position counted
    // on from its clock twice a second.
    private fun drawSession() {
        val l = listen ?: return
        val h = heard
        val s = h.listening
        val a = s?.active
        if (s == null || a == null) {
            _state.value = PlayerState(on = where(h), volume = s?.volume(h.me))
            stopTicking()
            return
        }
        val at = a.at.toInt()
        _state.value =
            PlayerState(
                now = playing(a.now, l),
                playing = a.clock.playing,
                durationMs = a.now.durationMs,
                upNext = a.queue.drop(at + 1).take(UP_NEXT).map { playing(it, l) },
                on = where(h),
                volume = s.volume(h.me),
            )
        _position.value = a.position(h.serverNow())
        if (!a.counting) stopTicking()
        else if (ticking?.isActive != true)
            ticking =
                scope.launch {
                    while (isActive) {
                        delay(TICK_MS)
                        val active = heard.listening?.active ?: break
                        if (!heard.joined || !active.counting) break
                        _position.value = active.position(heard.serverNow())
                    }
                }
    }

    private fun stopTicking() {
        ticking?.cancel()
        ticking = null
    }

    private fun playing(t: Queued, l: Listen): Playing =
        Playing(t.id.hex, t.title, t.creator, t.album, l.art(t.album).takeIf { it.isNotEmpty() }?.let { Media.url(server, it) } ?: "")

    // The device button: the output, or — with the sound nowhere — this phone, lit.
    private fun where(h: Heard): Where {
        val s = h.listening
        val name = listen?.name ?: "This phone"
        return when (val o = s?.active?.output) {
            is Output.Device ->
                if (o.id == h.me) Where(name, "phone", here = true, moving = o.moving)
                else
                    s.devices[o.id].let { d ->
                        // A group is named for its leader and how many follow it: "Kitchen + 1".
                        val n = s.followers(o.id).size
                        val named = d?.name ?: o.id
                        Where(if (n > 0) "$named + $n" else named, d?.kind ?: "computer", here = false, moving = o.moving)
                    }
            is Output.Away -> Where(o.name, "computer", here = o.id == h.me, away = true)
            Output.Nowhere, null -> Where(name, "phone", here = true)
        }
    }

    // What the player says, as one value; and while it plays — and only then — the position twice a
    // second, since nothing else moves the scrubber. Alone only: while joined the session is drawn.
    private fun read() {
        if (joined) return
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
                durationMs = c.duration.takeIf { it != C.TIME_UNSET }?.coerceAtLeast(0) ?: 0,
                shuffle = c.shuffleModeEnabled,
                repeat =
                    when (c.repeatMode) {
                        Player.REPEAT_MODE_ALL -> Repeat.All
                        Player.REPEAT_MODE_ONE -> Repeat.One
                        else -> Repeat.Off
                    },
                upNext = next,
                volume = local(),
            )
        _position.value = c.currentPosition.coerceAtLeast(0)
        if (!c.isPlaying) stopTicking()
        else if (ticking?.isActive != true) {
            ticking =
                scope.launch {
                    while (isActive) {
                        delay(TICK_MS)
                        if (joined) break
                        val now = controller ?: break
                        _position.value = now.currentPosition.coerceAtLeast(0)
                        if (!now.isPlaying) break
                    }
                }
        }
    }

    private fun playing(m: MediaItem): Playing =
        Playing(
            id = m.mediaId,
            title = m.mediaMetadata.title?.toString() ?: "",
            artist = m.mediaMetadata.artist?.toString() ?: "",
            album = m.mediaMetadata.albumTitle?.toString() ?: "",
            art = m.mediaMetadata.artworkUri?.toString() ?: "",
        )

    // A press of the session while this phone is part of it, or — alone, or refused because the
    // link went since — `alone`, to the player.
    private fun press(p: Press, alone: () -> Unit) {
        val l = listen
        if (l == null || !heard.joined) return alone()
        l.press(p) { why -> if (why == NOT_PART) scope.launch { alone() } }
    }

    /** A library row as the session carries it. */
    private fun queued(t: Track, albumOf: (Track) -> Album?): Queued = Queued(albumOf(t)?.name ?: "", t.creator, t.durationMs, t.file, t.id, t.title)

    /**
     * Play `queue` from `at`: the list the track was started from, with each track's album, and the
     * page it was on (`from`, a route as `Page.place` spells it).
     */
    fun play(queue: List<Track>, at: Int, albumOf: (Track) -> Album?, shuffled: Boolean = false, from: String = "") {
        if (queue.isEmpty()) return
        val start = at.coerceIn(0, queue.size - 1)
        // The session has no shuffle: a shuffled queue is a queue in another order, this track first.
        val list = if (shuffled && joined) listOf(queue[start]) + (queue.take(start) + queue.drop(start + 1)).shuffled() else queue
        press(Press.Start(list.map { queued(it, albumOf) }, if (shuffled && joined) 0 else start, 0, from)) {
            val c = controller ?: return@press
            c.shuffleModeEnabled = shuffled
            c.setMediaItems(queue.map { t -> Items.of(queued(t, albumOf), server, albumOf(t)?.art ?: "", from) }, start, 0)
            c.prepare()
            c.play()
        }
    }

    /** Put `track` right after what is playing; or play it, when nothing is. */
    fun playNext(track: Track, album: Album?) {
        val q = queued(track) { album }
        press(Press.Next(q)) {
            val c = controller ?: return@press
            if (c.mediaItemCount == 0) return@press play(listOf(track), 0, { album })
            c.addMediaItem(c.currentMediaItemIndex + 1, Items.of(q, server, album?.art ?: "", Items.from(c.currentMediaItem)))
        }
    }

    fun toggle() =
        press(Press.Toggle) {
            val c = controller ?: return@press
            if (c.isPlaying) c.pause() else c.play()
        }

    /** Play: the session's, where it is — or, alone, this player's. */
    fun resume() = press(Press.Play) { controller?.play() }

    fun next() = press(Press.Skip(1)) { controller?.seekToNext() }

    fun previous() = press(Press.Skip(-1)) { controller?.seekToPrevious() }

    /** Seek, on release; while joined the scrubber's slot is emptied with it. */
    fun seek(ms: Long) {
        _position.value = ms
        press(Press.Seek(ms)) { controller?.seekTo(ms) }
        if (joined) listen?.press(Press.Scrub(null))
    }

    /** Where this phone's scrubber is being dragged to: said to the session's other devices, throttled; nothing alone. */
    fun scrub(ms: Long) {
        if (joined) listen?.press(Press.Scrub(ms))
    }

    /** Hand the sound to another device of the account. */
    fun transfer(to: String) {
        if (joined) listen?.press(Press.Transfer(to))
    }

    fun shuffle() {
        if (joined) return
        val c = controller ?: return
        c.shuffleModeEnabled = !c.shuffleModeEnabled
    }

    /** Off, all, one, off. */
    fun repeat() {
        if (joined) return
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
        val a = heard.listening?.active
        if (joined && a != null) return press(Press.Jump(a.at.toInt() + 1 + n)) {}
        val c = controller ?: return
        var i = c.currentMediaItemIndex
        for (step in 0..n) {
            if (i == C.INDEX_UNSET) break
            i = c.currentTimeline.getNextWindowIndex(i, Player.REPEAT_MODE_OFF, c.shuffleModeEnabled)
        }
        if (i != C.INDEX_UNSET) c.seekTo(i, 0)
    }

    // This phone's own stream as the slider draws it, alone.
    private fun local(): Volume? = stream?.read()?.let { (level, muted) -> Volume(Level.Here, level, muted, remote = false) }

    private fun showVolume() {
        if (!joined) _state.value = _state.value.copy(volume = local())
    }

    /**
     * What is playing's volume, dragged to `level`: a throttled transition while this phone is part of
     * the session — the group's, the output's, or this phone's own while the sound is nowhere — and,
     * alone, this phone's media stream.
     */
    fun volume(level: Int) {
        val v = _state.value.volume ?: return
        if (v.of != Level.Here) return drag.to(v.of, level)
        stream?.apply(level, false)
        showVolume()
    }

    /** A volume slider let go: whatever the throttle held goes now. */
    fun volumeDone() = drag.done()

    /** What is playing's mute, the other way from how it is. */
    fun mute() {
        val v = _state.value.volume ?: return
        if (v.of != Level.Here) return press(Press.Loud(v.of, Change.Mute(!v.muted))) {}
        stream?.apply(v.level, !v.muted)
        showVolume()
    }

    /** A picker row's slider dragged to `level`: throttled, as the full player's is. */
    fun level(of: Level, level: Int) {
        if (joined) drag.to(of, level)
    }

    /** A press of the session from the picker — a mute, a join, an unjoin; nothing alone. */
    fun ask(p: Press) {
        if (joined) listen?.press(p)
    }

    /** Let go of the controller; the service, and the music, go on. */
    fun close() {
        unwatch()
        stopTicking()
        following?.cancel()
        scope.cancel()
        future?.let { MediaController.releaseFuture(it) }
        controller = null
    }

    companion object {
        /** How much of what is to come the queue sheet shows. */
        const val UP_NEXT = 100

        /** How often the position is read while a track plays. */
        const val TICK_MS = 500L
    }
}
