// ark-media-player's media session service on Android (modules/ark-media-player/README.md): a
// Media3 `MediaLibraryService` around the process's one player ([ArkMedia]), declared in the
// app's manifest by module.toml (`foregroundServiceType="mediaPlayback"`, and the library's
// actions, so a car and a watch find it). Media3 puts it in the foreground with its notification
// while the sound plays, and the notification, the lock screen and a headset's buttons reach
// [ShownPlayer], which says each to the app.
//
// It is a media library: the tree a car (Android Auto) or a watch browses is the app's, asked for
// one parent at a time (`Browse`) and answered (`setChildren`), and a choice in it is a
// `PlayItem`. Android Auto needs more of the app than this (README.md).
//
// Open, so an app with more to say to the media session (a car's search, a remote's volume) may
// subclass it, declare its subclass instead, and keep the rest.
package dev.arkui.modules

import android.app.PendingIntent
import android.content.Intent
import androidx.annotation.OptIn
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaLibraryService.LibraryParams
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionError
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import android.net.Uri
import android.os.Handler
import android.os.Looper
import uniffi.ark_media_player.BrowseItem
import uniffi.ark_media_player.MediaEvent
import uniffi.ark_media_player.RemoteCommand

@OptIn(UnstableApi::class)
open class ArkMediaService : MediaLibraryService() {
    /** The media session, while the service lives. */
    protected var session: MediaLibrarySession? = null
        private set

    override fun onCreate() {
        super.onCreate()
        ArkMedia.start(this)
        val player = ArkMedia.shown ?: return
        val b = MediaLibrarySession.Builder(this, player, callback())
        // A tap on the notification opens the app.
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
            b.setSessionActivity(PendingIntent.getActivity(this, 0, launch.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        }
        session = b.build()
    }

    /** What answers the media library: the app's tree, by default. */
    protected open fun callback(): MediaLibrarySession.Callback = Library()

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? = session

    // Swiped away from the recents while nothing plays: nothing to keep.
    override fun onTaskRemoved(rootIntent: Intent?) {
        val p = ArkMedia.exo
        if (p == null || !p.playWhenReady) stopSelf()
    }

    override fun onDestroy() {
        session?.release()
        session = null
        super.onDestroy()
    }

    /** The app's browse tree, as a car or a watch reads it, and what it chooses there. */
    open inner class Library : MediaLibrarySession.Callback {
        override fun onGetLibraryRoot(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> =
            Futures.immediateFuture(LibraryResult.ofItem(container(ROOT_ID, ""), params))

        override fun onGetChildren(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
            val out = SettableFuture.create<LibraryResult<ImmutableList<MediaItem>>>()
            children(parent(parentId)) { all ->
                val from = page.toLong() * pageSize
                val list = if (page < 0 || pageSize <= 0 || from >= all.size) emptyList() else all.subList(from.toInt(), minOf(all.size.toLong(), from + pageSize).toInt())
                out.set(LibraryResult.ofItemList(list.map(::item), params))
            }
            return out
        }

        override fun onGetItem(session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> {
            val found = ArkMedia.tree.values.asSequence().flatten().firstOrNull { it.id == mediaId }
            return Futures.immediateFuture(found?.let { LibraryResult.ofItem(item(it), null) } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE))
        }

        // A choice: handed to the player as it is, which says it to the app (`PlayItem`).
        override fun onSetMediaItems(mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<MediaSession.MediaItemsWithStartPosition> =
            Futures.immediateFuture(MediaSession.MediaItemsWithStartPosition(mediaItems, startIndex, startPositionMs))

        override fun onAddMediaItems(mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>): ListenableFuture<MutableList<MediaItem>> =
            Futures.immediateFuture(mediaItems)

        // Play pressed with nothing loaded — a car starting, a headset's button: the app's to say what.
        override fun onPlaybackResumption(mediaSession: MediaSession, controller: MediaSession.ControllerInfo): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
            ArkMedia.tell(MediaEvent.Remote(RemoteCommand.Play))
            return Futures.immediateFailedFuture(UnsupportedOperationException("the app resumes"))
        }
    }

    /** `parent`'s children: the app's, at once if it gave them, else when it does (asked now). */
    private fun children(parent: String, then: (List<BrowseItem>) -> Unit) {
        ArkMedia.tree[parent]?.let { return then(it) }
        val first = ArkMedia.waiting[parent] == null
        ArkMedia.waiting.getOrPut(parent) { mutableListOf() }.add(then)
        if (first) ArkMedia.tell(MediaEvent.Browse(parent))
        // An app that never answers is an empty list, not a car that waits for ever.
        Handler(Looper.getMainLooper()).postDelayed({ ArkMedia.waiting.remove(parent)?.forEach { it(emptyList()) } }, ANSWER_MS)
    }

    companion object {
        /** The root's id as Media3 carries it; the app's root is "" (`ROOT`). */
        const val ROOT_ID = "/"
        private const val ANSWER_MS = 5_000L

        private fun parent(id: String): String = if (id == ROOT_ID) "" else id

        private fun container(id: String, title: String): MediaItem =
            MediaItem.Builder().setMediaId(id)
                .setMediaMetadata(MediaMetadata.Builder().setTitle(title).setIsBrowsable(true).setIsPlayable(false).setMediaType(MediaMetadata.MEDIA_TYPE_FOLDER_MIXED).build())
                .build()

        /** A row of the app's tree as Media3 carries it. */
        fun item(b: BrowseItem): MediaItem =
            MediaItem.Builder().setMediaId(b.id)
                .setMediaMetadata(
                    MediaMetadata.Builder()
                        .setTitle(b.title)
                        .setSubtitle(b.subtitle)
                        .setArtworkUri(b.artwork?.let(Uri::parse))
                        .setIsBrowsable(b.browsable)
                        .setIsPlayable(b.playable)
                        .setMediaType(if (b.browsable) MediaMetadata.MEDIA_TYPE_FOLDER_MIXED else MediaMetadata.MEDIA_TYPE_MUSIC)
                        .build())
                .build()
    }
}
