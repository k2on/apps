// The domain as the screens see it, and nothing Android: every read is one
// of the printed domain's queries (`../../domain/gen/kotlin`, run natively
// by the session), and every write one of its mutations by name, returning
// the entry's id so that where it stands can be asked later. Kept free of
// Android so that it can be driven off a phone (harken/android/README.md).
package dev.harken.android

import dev.arkdb.Fault
import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.client.ItemState
import dev.arkdb.client.Outcome
import dev.arkdb.client.Session

/**
 * Something playable as a list draws it: a row of the `library` or
 * `playlist` query (the domain's `LibraryEntry` record). `playlistPos` is
 * where it sits on the playlist the list was read against, when it is on it.
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
        fun of(v: Value): Track = Track(
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

/** A row of `playlist`. */
data class Playlist(val id: Id, val name: String, val userId: String, val createdMs: Long) {
    companion object {
        fun of(v: Value): Playlist = Playlist(
            id = v.field("id").asId(),
            name = v.field("name").asText(),
            userId = v.field("user_id").asText(),
            createdMs = v.field("created_ms").asInt(),
        )
    }
}

/** What a write came to: the entry, whose standing `Session.statusOf` answers, or the local refusal. */
sealed class Wrote {
    data class Entry(val id: Id) : Wrote()
    data class Refused(val reason: String) : Wrote()
}

/** Where a track goes when nobody said: the playlist, and the entry that made it when it was made just now. */
data class Target(val playlist: Id, val made: Id?)

object Phone {
    /** What a phone calls the playlist it makes for somebody who has none. */
    const val DEFAULT_PLAYLIST = "Favorites"

    /** The playlist a list is read against when there is none: `library` does not look it up, and no playlist has it. */
    val NO_PLAYLIST: Id = Id(ByteArray(16))

    // A query's answer, or nothing when it is refused (a playlist that is not
    // this person's): a screen draws an empty list, not an error.
    private fun ask(s: Session, name: String, args: Map<String, Value> = emptyMap()): List<Value> = try {
        s.query(name, args).asList()
    } catch (f: Fault.Refuse) {
        emptyList()
    }

    /** The whole library in the order it was added, read against a playlist. */
    fun library(s: Session, against: Id?): List<Track> =
        ask(s, "library", mapOf("playlist_id" to Value.id(against ?: NO_PLAYLIST))).map(Track::of)

    /** The caller's playlists, in the order they were made. */
    fun playlists(s: Session): List<Playlist> = ask(s, "playlists").map(Playlist::of)

    /** One playlist's contents, in playlist order, as library rows. */
    fun playlist(s: Session, id: Id): List<Track> = ask(s, "playlist", mapOf("playlist_id" to Value.id(id))).map(Track::of)

    /** Which of the caller's playlists a track is on. */
    fun playlistsOf(s: Session, media: Id): List<Playlist> =
        ask(s, "playlists_of", mapOf("media_id" to Value.id(media))).map(Playlist::of)

    /**
     * What the form validator says of a name as it is typed: the message
     * `create_playlist`'s own checks would refuse it with, or null. A name
     * the person already has passes: the log numbers it "Name (1)".
     */
    fun nameProblem(s: Session, name: String): String? =
        s.check("create_playlist", mapOf("name" to Value.text(name))).messageFor("name")

    private fun wrote(o: Outcome): Wrote = when (o) {
        is Outcome.Applied -> Wrote.Entry(o.entry.id)
        is Outcome.Refused -> Wrote.Refused(o.reason)
    }

    fun createPlaylist(s: Session, name: String): Wrote =
        wrote(s.mutate("create_playlist", mapOf("name" to Value.text(name))))

    fun addToPlaylist(s: Session, playlist: Id, media: Id): Wrote =
        wrote(s.mutate("add_to_playlist", mapOf("playlist_id" to Value.id(playlist), "media_id" to Value.id(media))))

    fun removeFromPlaylist(s: Session, playlist: Id, media: Id): Wrote =
        wrote(s.mutate("remove_from_playlist", mapOf("playlist_id" to Value.id(playlist), "media_id" to Value.id(media))))

    /**
     * Make the default playlist, and only for somebody who has no playlist
     * at all: a name that person already has would be renamed "Favorites
     * (1)" by the log rather than refused, so the one question worth asking
     * is whether they have any. `known` is the caller's word that the view
     * holds every playlist of theirs (alone; or signed in and caught up).
     * Null when nothing was made.
     */
    fun ensureDefault(s: Session, known: Boolean): Wrote? {
        if (!known || playlists(s).isNotEmpty()) return null
        return createPlaylist(s, DEFAULT_PLAYLIST)
    }

    /**
     * The playlist a track goes on when the person has none yet: the
     * default, made now — for somebody signed out, the only way one is made
     * for them, so that signing in later with playlists of their own does
     * not hand them a "Favorites (1)" they never asked for.
     */
    fun playlistForAdding(s: Session): Pair<Target?, String?> {
        playlists(s).firstOrNull()?.let { return Target(it.id, null) to null }
        return when (val w = createPlaylist(s, DEFAULT_PLAYLIST)) {
            is Wrote.Refused -> null to w.reason
            is Wrote.Entry -> playlists(s).firstOrNull()?.let { Target(it.id, w.id) to null } ?: (null to "no playlist to add to")
        }
    }

    /** What a screen writes beside an item: nothing once it is in the log, the reason word for word when it never will be. */
    fun caption(st: ItemState): String? = when (st) {
        is ItemState.Pending -> "not synced yet"
        is ItemState.Confirmed, is ItemState.Unknown -> null
        is ItemState.Rejected -> "not saved: ${st.reason}"
    }
}
