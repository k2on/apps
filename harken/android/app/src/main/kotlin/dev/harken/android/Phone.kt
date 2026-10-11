// The domain as the screens see it, and nothing Android: rows of harken's
// queries read into the types the screens draw, every write one of
// `harken.ark`'s mutations by name — the module interpreted by the Kotlin
// client, as every peer interprets it — returning the entry's id so that
// where it stands can be asked later. Kept free of Android so that it can be
// driven off a phone (harken/android/README.md).
package dev.harken.android

import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.client.Peer
import dev.arkdb.client.PeerError
import dev.arkdb.client.Standing

/**
 * Something playable as a list draws it: a row of the `library` or `playlist` query (the domain's
 * `LibraryEntry` record). `playlistPos` is where it sits on the playlist the list was read
 * against, when it is on it.
 */
data class Track(
    val id: Id,
    val title: String,
    val creator: String,
    val kind: String,
    val durationMs: Long,
    val file: String,
    val playlistPos: Long?,
) {
    companion object {
        fun of(v: Value): Track =
            Track(
                id = v.field("id").asId(),
                title = v.field("title").asText(),
                creator = v.field("creator").asText(),
                kind = v.field("kind").asText(),
                durationMs = v.field("duration_ms").asInt(),
                file = v.field("file").asText(),
                playlistPos = v.field("playlist_pos").let { if (it.isNull()) null else it.asInt() },
            )
    }
}

/** A row of `playlists`. */
data class Playlist(val id: Id, val name: String, val userId: String, val createdMs: Long) {
    companion object {
        fun of(v: Value): Playlist =
            Playlist(
                id = v.field("id").asId(),
                name = v.field("name").asText(),
                userId = v.field("user_id").asText(),
                createdMs = v.field("created_ms").asInt(),
            )
    }
}

/** What a write came to: the entry, whose standing `Peer.standing` answers, or the local refusal. */
sealed class Wrote {
    data class Entry(val id: Id) : Wrote()

    data class Refused(val reason: String) : Wrote()
}

object Phone {
    /** What a phone calls the playlist it makes for somebody who has none. */
    const val DEFAULT_PLAYLIST = "Favorites"

    /** The playlist a list is read against when there is none: `library` marks nothing. */
    val NO_PLAYLIST: Id = Id.nil

    /** The `library` view's arguments, read against a playlist. */
    fun libraryArgs(against: Id?): Map<String, Value> =
        mapOf("playlist_id" to Value.id(against ?: NO_PLAYLIST))

    /** The `playlist` view's arguments. */
    fun playlistArgs(id: Id): Map<String, Value> = mapOf("playlist_id" to Value.id(id))

    /** Which of the caller's playlists a track is on; nothing when it is refused. */
    fun playlistsOf(p: Peer, media: Id): List<Playlist> =
        try {
            p.query("playlists_of", mapOf("media_id" to Value.id(media))).asList().map(Playlist::of)
        } catch (e: PeerError) {
            emptyList()
        }

    /**
     * What the form validator says of a name as it is typed: the message `create_playlist`'s own
     * checks would refuse it with, or null. A name the person already has passes: the log numbers
     * it "Name (1)".
     */
    fun nameProblem(p: Peer, name: String): String? =
        p.check("create_playlist", mapOf("name" to Value.text(name)))
            .messages
            .firstOrNull { it.first == "name" }
            ?.second

    private fun wrote(f: () -> Id): Wrote =
        try {
            Wrote.Entry(f())
        } catch (e: PeerError) {
            Wrote.Refused(e.message ?: "refused")
        }

    fun createPlaylist(p: Peer, name: String): Wrote =
        wrote { p.mutate("create_playlist", mapOf("name" to Value.text(name))) }

    fun addToPlaylist(p: Peer, playlist: Id, media: Id): Wrote =
        wrote {
            p.mutate(
                "add_to_playlist",
                mapOf("playlist_id" to Value.id(playlist), "media_id" to Value.id(media)))
        }

    fun removeFromPlaylist(p: Peer, playlist: Id, media: Id): Wrote =
        wrote {
            p.mutate(
                "remove_from_playlist",
                mapOf("playlist_id" to Value.id(playlist), "media_id" to Value.id(media)))
        }

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
