// The domain as the screens see it, and nothing Android: rows of harken's
// views read into the types the screens draw — the library's and the
// playlists' typed, UniFFI's own records with no JSON (`LibraryEntry`,
// `Playlist`) — and every write one of the domain's functions by name — the generated, typed calls over the bridge
// (`gen/Harken.kt`, `ark-dev gen kotlin`), run by the Rust kernel the phone
// embeds — returning the entry's id so that where it stands can be asked
// later. Kept free of Android so that it can be driven off a phone.
package dev.harken.android

import dev.arkdb.ffi.Id
import dev.arkdb.ffi.Refusal
import dev.arkdb.ffi.Standing
import dev.arkdb.ffi.View
import dev.harken.android.gen.AddToPlaylist
import dev.harken.android.gen.AlbumInput
import dev.harken.android.gen.AlbumsEntry
import dev.harken.android.gen.Artist as ArtistInput
import dev.harken.android.gen.ArtistsEntry
import dev.harken.android.gen.CreatePlaylist
import dev.harken.android.gen.Harken
import dev.harken.android.gen.Library as LibraryInput
import uniffi.harken_domain.LibraryEntry
import dev.harken.android.gen.Play
import uniffi.harken_domain.Playlist
import dev.harken.android.gen.PlaylistInput
import dev.harken.android.gen.PlaylistsOf
import dev.harken.android.gen.RemoveFromPlaylist
import dev.harken.android.gen.Search
import dev.harken.android.gen.TrackDetailsEntry

/**
 * Something playable as a list draws it: a row of the `library` or `playlist` view (the domain's
 * `LibraryEntry`). `playlistPos` is where it sits on the playlist the list was read against, when it
 * is on it.
 */
data class Track(
    val id: Id,
    val title: String,
    val creator: String,
    val kind: String,
    val durationMs: Long,
    val file: String,
    val playlistPos: Long?,
    val addedMs: Long = 0,
) {
    companion object {
        fun of(e: LibraryEntry): Track = Track(e.id, e.title, e.creator, e.kind, e.durationMs, e.file, e.playlistPos, e.addedMs)
    }
}

/** A row of `albums`: every album some song is on, with whoever made its first track. */
data class Album(val name: String, val creator: String, val art: String, val tracks: Long) {
    companion object {
        fun of(e: AlbumsEntry): Album = Album(e.name, e.creator, e.art, e.tracks)

        /**
         * The album `track` is on, by its details: the album's own row, found by its name — or,
         * while the albums have not caught up with a song that names a new one, an album made of
         * the name.
         */
        fun ofTrack(track: Track, details: Map<Id, TrackDetail>, byName: Map<String, Album>): Album? {
            val name = details[track.id]?.album?.takeIf { it.isNotEmpty() } ?: return null
            return byName[name] ?: Album(name, track.creator, "", 0)
        }
    }
}

/** A row of `artists`: everyone who made something in the library. */
data class Artist(val name: String, val art: String, val tracks: Long) {
    companion object {
        fun of(e: ArtistsEntry): Artist = Artist(e.name, e.art, e.tracks)
    }
}

/** A row of `track_details`: what only a song has — its album, its part of a work, its number on the release. */
data class TrackDetail(
    val mediaId: Id,
    val album: String,
    val part: String,
    val track: Long,
    val performer: String,
    val licence: String,
) {
    companion object {
        fun of(e: TrackDetailsEntry): TrackDetail = TrackDetail(e.mediaId, e.album, e.part, e.track, e.performer, e.licence)
    }
}

/**
 * A row of `last_played` and `recently_played`: a track as it was heard — `record_play`'s, so the
 * track need not be in the library — when, and the page it was played from (`#album/…`).
 */
data class Played(
    val track: Id?,
    val title: String,
    val creator: String,
    val album: String,
    val durationMs: Long,
    val file: String,
    val playedMs: Long,
    val from: String,
) {
    companion object {
        fun of(p: Play): Played =
            Played(Id(p.track).takeIf { p.track.length == 32 }, p.title, p.creator, p.album, p.durationMs, p.file, p.playedMs, p.from)
    }
}

/** What a write came to: the entry, whose standing the peer answers, or the local refusal. */
sealed class Wrote {
    data class Entry(val id: String) : Wrote()

    data class Refused(val reason: String) : Wrote()
}

/** A refusal's sentence. */
val Refusal.why: String
    get() = (this as? Refusal.Refused)?.why ?: message ?: "refused"

object Phone {
    /** What a phone calls the playlist it makes for somebody who has none. */
    const val DEFAULT_PLAYLIST = "Favorites"

    /** The playlist a list is read against when there is none: `library` marks nothing. */
    val NO_PLAYLIST: Id = Id.nil

    /** The library, read against a playlist. */
    fun library(h: Harken, against: Id?): View<LibraryEntry> = h.library(LibraryInput(against ?: NO_PLAYLIST))

    /** A page's songs: an album's or an artist's by name, read against no playlist, or a playlist's. */
    fun album(h: Harken, name: String): View<LibraryEntry> = h.album(AlbumInput(NO_PLAYLIST, name))

    fun artist(h: Harken, name: String): View<LibraryEntry> = h.artist(ArtistInput(NO_PLAYLIST, name))

    fun playlist(h: Harken, id: Id): View<LibraryEntry> = h.playlist(PlaylistInput(id))

    /** A view's rows once, and the view let go. */
    fun <E, T> once(v: () -> View<E>, of: (E) -> T): List<T> =
        try {
            v().use { it.rows().map(of) }
        } catch (e: Refusal) {
            emptyList()
        }

    /** The library's rows whose title or creator holds `needle` (the domain's `search`). */
    fun search(h: Harken, needle: String): List<Track> = once({ h.search(Search(NO_PLAYLIST, needle)) }, Track::of)

    /** Which of the caller's playlists a track is on; nothing when it is refused. */
    fun playlistsOf(h: Harken, media: Id): List<Playlist> = once({ h.playlistsOf(PlaylistsOf(media)) }) { it }

    /**
     * What the form validator says of a name as it is typed: the sentence `create_playlist`'s own
     * checks would refuse it with, or null. A name the person already has passes: the log numbers
     * it "Name (1)".
     */
    fun nameProblem(h: Harken, name: String): String? = h.peer.check("playlists.create_playlist", CreatePlaylist(name).json())

    private fun wrote(f: () -> String): Wrote =
        try {
            Wrote.Entry(f())
        } catch (e: Refusal) {
            Wrote.Refused(e.why)
        }

    fun createPlaylist(h: Harken, name: String): Wrote = wrote { h.createPlaylist(CreatePlaylist(name)) }

    fun addToPlaylist(h: Harken, playlist: Id, media: Id): Wrote = wrote { h.addToPlaylist(AddToPlaylist(playlist, media)) }

    fun removeFromPlaylist(h: Harken, playlist: Id, media: Id): Wrote = wrote { h.removeFromPlaylist(RemoveFromPlaylist(playlist, media)) }

    /**
     * What a screen writes beside an item: nothing once it is in the log, the reason word for word
     * when it never will be.
     */
    fun caption(st: Standing): String? =
        when (st) {
            is Standing.Pending -> "not synced yet"
            is Standing.Confirmed,
            is Standing.Unknown -> null
            is Standing.Rejected -> "not saved: ${st.why}"
        }
}

/** Bytes as lowercase hex, and back: what a route or a cover's address carries a name as. */
object Hex {
    fun encode(b: ByteArray): String = b.joinToString("") { "%02x".format(it.toInt() and 0xff) }

    fun decode(s: String): ByteArray {
        require(s.length % 2 == 0) { "hex of odd length" }
        return ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
    }
}
