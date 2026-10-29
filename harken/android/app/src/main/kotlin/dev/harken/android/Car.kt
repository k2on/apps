// harken in a car: what Android Auto browses, searches and plays, read from
// the replica — the library's own views and the domain's own queries
// (Library.kt), never a second layer of reads — so a car with no signal
// browses what the phone has, as the phone's screens do.
//
// **The tree.** A root of four tabs, which is all Android Auto draws: Home,
// Playlists, Albums, Artists — and Songs as a fifth only for a car that says
// it draws more (`EXTRAS_KEY_ROOT_CHILDREN_LIMIT`); otherwise every song is
// a row at the end of Home. Home is what the phone's Home opens on, as
// rows: the listening session's queue from where it is — or, with none, the
// last thing played — and the plays before it, newest first. Albums and
// artists are grids, a playlist, an album and an artist open to their
// tracks, and every track is playable. Pages of a list are slices of it
// (`page`, `pageSize`), so a long one costs what is drawn.
//
// **An id says where a track was found**, so that choosing it plays the
// list it was in from there, as a tap on the phone does: `t|<list>|<n>|<id>`
// — the list (a page's route, `songs`, the session's `queue`, a `recent` or
// the `last` play, a `search:` and its words), where in it, and the track,
// which is checked against the list as it now is; a list that has moved
// since finds the track by its id. A page's route (`album/…`, `artist/…`,
// `playlist/…`) is the page.
//
// **Voice.** "Play X on Harken" arrives as a search query, with the
// assistant's guess at what X is when it has one (`EXTRA_MEDIA_FOCUS`). An
// album plays in its order and a playlist in its; an artist is shuffled;
// anything else is the songs the domain's `search` finds, the best match
// first. Nothing at all ("play Harken") is carrying on: the session's queue
// where it is, or the last thing played, from the page it was played from.
package dev.harken.android

import uniffi.harken_domain.Playlist
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import androidx.annotation.OptIn
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaConstants

import dev.arkdb.ffi.Id
import dev.harken.android.gen.Track as Queued
import kotlin.random.Random

/** What a choice in the car plays: a queue, where in it, from what point, and the page it came from. */
data class Pick(val queue: List<Queued>, val at: Int, val positionMs: Long = 0, val from: String = "")

@OptIn(UnstableApi::class)
class Car(
    private val kept: Kept,
    /** The session's server, which a cover's path is joined to. */
    private val server: String,
    /** How an artist is shuffled: a test's is seeded. */
    private val random: Random = Random.Default,
) {
    private val library = kept.library

    /** The size of the pictures, as the car asked at its root. */
    var artPx: Int = CarArt.SIZE

    companion object {
        const val ROOT = "root"
        const val HOME = "home"
        const val PLAYLISTS = "playlists"
        const val ALBUMS = "albums"
        const val ARTISTS = "artists"
        const val SONGS = "songs"

        /** What Android Auto draws at the root: four tabs. */
        const val TABS = 4

        /** How much of the session's queue, and of the plays, Home lists. */
        const val HOME_ROWS = 25

        /** The extras a browsable item carries to say how its children are drawn. */
        fun style(browsable: Int, playable: Int = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM): Bundle =
            Bundle().apply {
                putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, browsable)
                putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_PLAYABLE, playable)
            }

        /** What the root says of the tree: grids by default, lists of tracks. */
        fun rootExtras(): Bundle = style(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM)

        /** The id of the track at `n` of `list`. */
        fun trackId(list: String, n: Int, track: Id?): String = "t|$list|$n|${track?.hex ?: ""}"

        private fun searchList(needle: String): String = "search:" + Hex.encode(needle.toByteArray(Charsets.UTF_8))

        private fun folder(id: String, title: String, type: Int, extras: Bundle): MediaItem =
            MediaItem.Builder()
                .setMediaId(id)
                .setMediaMetadata(
                    MediaMetadata.Builder()
                        .setTitle(title)
                        .setIsBrowsable(true)
                        .setIsPlayable(false)
                        .setMediaType(type)
                        .setExtras(extras)
                        .build())
                .build()

        /** The root: what Android Auto asks for first, and whose children are the tabs. */
        fun rootItem(): MediaItem = folder(ROOT, "Harken", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED, rootExtras())
    }

    // Items ------------------------------------------------------------------

    /** The tabs, as many as the car draws. */
    fun tabs(limit: Int): List<MediaItem> {
        val list = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM
        val grid = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM
        val tabs =
            mutableListOf(
                folder(HOME, "Home", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED, style(list)),
                folder(PLAYLISTS, "Playlists", MediaMetadata.MEDIA_TYPE_FOLDER_PLAYLISTS, style(list)),
                folder(ALBUMS, "Albums", MediaMetadata.MEDIA_TYPE_FOLDER_ALBUMS, style(grid)),
                folder(ARTISTS, "Artists", MediaMetadata.MEDIA_TYPE_FOLDER_ARTISTS, style(grid)),
            )
        if (limit > TABS) tabs.add(songsFolder())
        return tabs.take(maxOf(limit, 1))
    }

    private fun songsFolder(): MediaItem = folder(SONGS, "Songs", MediaMetadata.MEDIA_TYPE_FOLDER_MIXED, style(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_LIST_ITEM))

    private fun art(path: String, seed: String): Uri = CarArt.uri(path.takeIf { it.isNotEmpty() }?.let { Media.url(server, it) }, seed, artPx)

    private fun grouped(b: MediaMetadata.Builder, group: String?): MediaMetadata.Builder =
        if (group == null) b else b.setExtras(Bundle().apply { putString(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_GROUP_TITLE, group) })

    private fun pageItem(page: Page, title: String, subtitle: String?, picture: Uri, type: Int, group: String? = null): MediaItem =
        MediaItem.Builder()
            .setMediaId(page.route)
            .setMediaMetadata(
                grouped(
                        MediaMetadata.Builder()
                            .setTitle(title)
                            .setSubtitle(subtitle)
                            .setArtist(subtitle)
                            .setArtworkUri(picture)
                            .setIsBrowsable(true)
                            .setIsPlayable(false)
                            .setMediaType(type),
                        group)
                    .build())
            .build()

    fun album(a: Album, group: String? = null): MediaItem = pageItem(Page.ForAlbum(a.id), a.name, a.creator, art(a.art, a.name), MediaMetadata.MEDIA_TYPE_ALBUM, group)

    fun artist(a: Artist, group: String? = null): MediaItem = pageItem(Page.ForArtist(a.name), a.name, null, art(a.art, a.name), MediaMetadata.MEDIA_TYPE_ARTIST, group)

    fun playlist(p: Playlist): MediaItem = pageItem(Page.ForPlaylist(p.id), p.name, null, art("", p.name), MediaMetadata.MEDIA_TYPE_PLAYLIST)

    /** A playable row: `t` in `list` at `n`. */
    fun track(list: String, n: Int, t: Queued, group: String? = null): MediaItem =
        MediaItem.Builder()
            .setMediaId(trackId(list, n, t.id))
            .setMediaMetadata(
                grouped(
                        MediaMetadata.Builder()
                            .setTitle(t.title)
                            .setArtist(t.creator)
                            .setAlbumTitle(t.album)
                            .setArtworkUri(art(library.artOf(t.album, t.creator), t.album.ifEmpty { t.title }))
                            .setDurationMs(t.durationMs.takeIf { it > 0 })
                            .setIsBrowsable(false)
                            .setIsPlayable(true)
                            .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC),
                        group)
                    .build())
            .build()

    // Reading ----------------------------------------------------------------

    /** A library row as the session carries it: with its album's name. */
    fun queued(t: Track): Queued {
        val byId = library.albums.value.associateBy { it.id }
        return Queued(Album.ofTrack(t, library.details.value, byId)?.name ?: "", t.creator, t.durationMs, t.file, t.id, t.title)
    }

    private fun queued(list: List<Track>): List<Queued> {
        val byId = library.albums.value.associateBy { it.id }
        val details = library.details.value
        return list.map { t -> Queued(Album.ofTrack(t, details, byId)?.name ?: "", t.creator, t.durationMs, t.file, t.id, t.title) }
    }

    private fun played(p: Played): Queued = Queued(p.album, p.creator, p.durationMs, p.file, p.track ?: Id.nil, p.title)

    /** The plays Home lists after what is playing: newest first, a track once, not `except`. */
    private suspend fun recent(except: Set<Id>): List<Played> {
        val seen = HashSet(except)
        return library.recentlyPlayed().filter { p -> p.track != null && seen.add(p.track) }.take(HOME_ROWS)
    }

    private fun page(list: List<MediaItem>, page: Int, size: Int): List<MediaItem> {
        if (page < 0 || size <= 0) return emptyList()
        val from = page.toLong() * size
        if (from >= list.size) return emptyList()
        return list.subList(from.toInt(), minOf(list.size.toLong(), from + size).toInt())
    }

    /** The children of `parent`, a page of them; null for an id that is not a list here. */
    suspend fun children(parent: String, page: Int, size: Int, limit: Int): List<MediaItem>? {
        val all: List<MediaItem> =
            when (parent) {
                ROOT -> tabs(limit)
                HOME -> home(limit)
                PLAYLISTS -> library.playlists.value.map(::playlist)
                ALBUMS -> library.albums.value.map { album(it) }
                ARTISTS -> {
                    library.whole()
                    library.artists.value.map { artist(it) }
                }
                SONGS -> {
                    library.whole()
                    queued(library.tracks.value).mapIndexed { i, t -> track(SONGS, i, t) }
                }
                else -> {
                    val p = Page.parse(parent) ?: return null
                    library.whole()
                    queued(library.read(p)).mapIndexed { i, t -> track(p.route, i, t) }
                }
            }
        return page(all, page, size)
    }

    // Home: the session's queue from where it is, or the last play; the plays before; and, when
    // Songs is not a tab, every song.
    private suspend fun home(limit: Int): List<MediaItem> {
        val out = ArrayList<MediaItem>()
        val h = kept.listen.heard.value
        val a = if (h.joined) h.listening?.active else null
        val shown = HashSet<Id>()
        if (a != null) {
            val at = a.at.toInt()
            for (i in at until minOf(a.queue.size, at + HOME_ROWS)) {
                val t = a.queue[i]
                shown.add(t.id)
                out.add(track("queue", i, t, if (i == at) "Playing now" else "Up next"))
            }
        } else {
            library.lastPlayed.value?.let { p ->
                p.track?.let(shown::add)
                out.add(track("last", 0, played(p), "Last played"))
            }
        }
        recent(shown).forEachIndexed { i, p -> out.add(track("recent", i, played(p), "Recently played")) }
        if (limit <= TABS) out.add(songsFolder())
        return out
    }

    /** The item an id names, for a car that asks for one by itself. */
    suspend fun item(id: String): MediaItem? {
        if (id == ROOT) return rootItem()
        tabs(TABS + 1).firstOrNull { it.mediaId == id }?.let {
            return it
        }
        if (id.startsWith("t|")) {
            val p = resolve(id) ?: return null
            return track(id.split('|')[1], p.at, p.queue[p.at])
        }
        return when (val p = Page.parse(id)) {
            is Page.ForAlbum -> library.albums.value.firstOrNull { it.id == p.id }?.let { album(it) }
            is Page.ForArtist -> library.artists.value.firstOrNull { it.name == p.name }?.let { artist(it) }
            is Page.ForPlaylist -> library.playlists.value.firstOrNull { it.id == p.id }?.let(::playlist)
            else -> null
        }
    }

    // Choosing ---------------------------------------------------------------

    /** What choosing the item `id` plays: the list it was in, from it; a page, from its start. */
    suspend fun resolve(id: String): Pick? {
        if (!id.startsWith("t|")) {
            val p = Page.parse(id) ?: return null
            library.whole()
            return Pick(queued(library.read(p)), 0, 0, p.place(library.playlists.value)).takeIf { it.queue.isNotEmpty() }
        }
        val parts = id.split('|')
        if (parts.size != 4) return null
        val list = parts[1]
        val n = parts[2].toIntOrNull() ?: return null
        val hex = parts[3]
        fun at(queue: List<Queued>): Int =
            if (queue.getOrNull(n)?.id?.hex == hex) n else queue.indexOfFirst { it.id.hex == hex }
        library.whole()
        return when {
            list == "queue" -> {
                val a = kept.listen.heard.value.listening?.active ?: return null
                at(a.queue).takeIf { it >= 0 }?.let { Pick(a.queue, it, 0, a.from) }
            }
            list == "last" -> library.lastPlayed.value?.let { again(it) }
            list == "recent" -> recent(emptySet()).firstOrNull { it.track?.hex == hex }?.let { again(it) }
            list.startsWith("search:") -> {
                val needle = String(Hex.decode(list.removePrefix("search:")), Charsets.UTF_8)
                val q = queued(library.search(needle))
                at(q).takeIf { it >= 0 }?.let { Pick(q, it) }
            }
            list == SONGS -> {
                val q = queued(library.tracks.value)
                at(q).takeIf { it >= 0 }?.let { Pick(q, it, 0, Page.Songs.place(library.playlists.value)) }
            }
            else -> {
                val p = Page.parse(list) ?: return null
                val q = queued(library.read(p))
                at(q).takeIf { it >= 0 }?.let { Pick(q, it, 0, p.place(library.playlists.value)) }
            }
        }
    }

    // A play heard again: from the page it was played from, where the track is on it — as Home's
    // last played does — or the track alone, when the page has not got it any more.
    private suspend fun again(p: Played): Pick {
        val page = Page.ofPlace(p.from, library.playlists.value)
        if (page != null) {
            val q = queued(library.read(page))
            val at = q.indexOfFirst { it.id == p.track }
            if (at >= 0) return Pick(q, at, 0, p.from)
        }
        return Pick(listOf(played(p)), 0, 0, p.from)
    }

    /** Carrying on: the session where it is, or the last thing played; else every song, shuffled. */
    suspend fun resume(): Pick? {
        val h = kept.listen.heard.value
        val a = if (h.joined) h.listening?.active else null
        if (a != null) return Pick(a.queue, a.at.toInt().coerceIn(0, a.queue.size - 1), a.position(h.serverNow()), a.from)
        library.whole()
        library.lastPlayed.value?.let {
            return again(it)
        }
        val all = queued(library.tracks.value)
        return if (all.isEmpty()) null else Pick(all.shuffled(random), 0, 0, Page.Songs.place(library.playlists.value))
    }

    /** What the search for `query` finds, as the phone's search groups it: albums, artists, songs. */
    suspend fun search(query: String): List<MediaItem> {
        val needle = query.trim()
        if (needle.isEmpty()) return emptyList()
        library.whole()
        val r = Results.of(needle, library.search(needle), library.albums.value, library.artists.value)
        val list = searchList(needle)
        return r.albums.map { album(it, "Albums") } + r.artists.map { artist(it, "Artists") } + queued(r.songs).mapIndexed { i, t -> track(list, i, t, "Songs") }
    }

    /**
     * "Play X": the album, playlist or artist X names — exactly, or as the assistant guessed it —
     * else the songs X finds, the best match first; else the albums, artists or playlists whose
     * names hold it. Nothing asked is carrying on ([resume]). Null when nothing matches.
     */
    suspend fun voice(query: String?, extras: Bundle?): Pick? {
        val q = query?.trim().orEmpty()
        val focus = extras?.getString(MediaStore.EXTRA_MEDIA_FOCUS)
        if (q.isEmpty() && focus == null) return resume()
        library.whole()
        fun named(key: String): String = extras?.getString(key)?.trim().orEmpty().ifEmpty { q }
        when (focus) {
            MediaStore.Audio.Albums.ENTRY_CONTENT_TYPE -> albumNamed(named(MediaStore.EXTRA_MEDIA_ALBUM), exact = false)?.let { return it }
            MediaStore.Audio.Artists.ENTRY_CONTENT_TYPE -> artistNamed(named(MediaStore.EXTRA_MEDIA_ARTIST), exact = false)?.let { return it }
            MediaStore.Audio.Playlists.ENTRY_CONTENT_TYPE -> playlistNamed(named("android.intent.extra.playlist"), exact = false)?.let { return it }
            MediaStore.Audio.Media.ENTRY_CONTENT_TYPE -> song(named(MediaStore.EXTRA_MEDIA_TITLE))?.let { return it }
        }
        if (q.isEmpty()) return resume()
        return albumNamed(q, exact = true)
            ?: playlistNamed(q, exact = true)
            ?: artistNamed(q, exact = true)
            ?: song(q)
            ?: albumNamed(q, exact = false)
            ?: artistNamed(q, exact = false)
            ?: playlistNamed(q, exact = false)
    }

    private fun matches(name: String, q: String, exact: Boolean): Boolean = if (exact) name.equals(q, ignoreCase = true) else name.contains(q, ignoreCase = true)

    private suspend fun albumNamed(q: String, exact: Boolean): Pick? {
        val a = library.albums.value.firstOrNull { matches(it.name, q, exact) } ?: return null
        val page = Page.ForAlbum(a.id)
        return Pick(queued(library.read(page)), 0, 0, page.place(library.playlists.value)).takeIf { it.queue.isNotEmpty() }
    }

    private suspend fun playlistNamed(q: String, exact: Boolean): Pick? {
        val p = library.playlists.value.firstOrNull { matches(it.name, q, exact) } ?: return null
        val page = Page.ForPlaylist(p.id)
        return Pick(queued(library.read(page)), 0, 0, page.place(library.playlists.value)).takeIf { it.queue.isNotEmpty() }
    }

    // An artist, shuffled: every track of theirs, in an order of its own.
    private suspend fun artistNamed(q: String, exact: Boolean): Pick? {
        val a = library.artists.value.firstOrNull { matches(it.name, q, exact) } ?: return null
        val page = Page.ForArtist(a.name)
        val q2 = queued(library.read(page)).shuffled(random)
        return Pick(q2, 0, 0, page.place(library.playlists.value)).takeIf { it.queue.isNotEmpty() }
    }

    // The songs `q` finds, from the one whose title is `q` if there is one, else the first.
    private suspend fun song(q: String): Pick? {
        val found = queued(library.search(q))
        if (found.isEmpty()) return null
        val at = found.indexOfFirst { it.title.equals(q, ignoreCase = true) }.coerceAtLeast(0)
        return Pick(found, at)
    }
}
