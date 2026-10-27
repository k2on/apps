// The domain as the screens see it, and nothing Android: rows of harken's
// views read into the types the screens draw — the library's and the
// playlists' typed, UniFFI's own records with no JSON (`LibraryEntry`,
// `Playlist`) — and every write one of the domain's functions by name — the generated, typed calls over the bridge
// (`gen/Harken.kt`, `arkdb gen kotlin`), run by the Rust kernel the phone
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
import dev.harken.android.gen.ComposersEntry
import dev.harken.android.gen.CreatePlaylist
import dev.harken.android.gen.ChaptersEntry
import dev.harken.android.gen.DownloadsEntry
import dev.harken.android.gen.EpisodeDetailsEntry
import dev.harken.android.gen.EpisodeInput
import dev.harken.android.gen.MarkPlayed
import dev.harken.android.gen.PeopleEntry
import dev.harken.android.gen.ProgressEntry
import dev.harken.android.gen.FetchOlder
import dev.harken.android.gen.Harken
import dev.harken.android.gen.Library as LibraryInput
import uniffi.harken_domain.LibraryEntry
import dev.harken.android.gen.Play
import uniffi.harken_domain.Playlist
import dev.harken.android.gen.PlaylistInput
import dev.harken.android.gen.PlaylistsOf
import dev.harken.android.gen.RecordingInput
import dev.harken.android.gen.RemoveFromPlaylist
import dev.harken.android.gen.Search
import dev.harken.android.gen.ShowInput
import dev.harken.android.gen.ShowsEntry
import dev.harken.android.gen.Subscribe
import dev.harken.android.gen.SubscriptionsEntry
import dev.harken.android.gen.Unsubscribe
import dev.harken.android.gen.TrackDetailsEntry
import dev.harken.android.gen.TranscriptEntry

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

/**
 * A row of `albums`: every album some song is on, with whoever made its first track, who played the
 * most of it (`performer`, empty where nobody is credited), whose label put it out, and the year it
 * came out (0 for nobody said).
 */
data class Album(
    val name: String,
    val creator: String,
    val art: String,
    val tracks: Long,
    val performer: String = "",
    val label: String = "",
    val released: Long = 0,
) {
    /** Who played it, as a catalogue lists a record: whose it is when nobody is credited. */
    val by: String
        get() = performer.ifEmpty { creator }

    companion object {
        fun of(e: AlbumsEntry): Album = Album(e.name, e.creator, e.art, e.tracks, e.performer, e.label, e.released)

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

/**
 * A row of `artists`: everyone who made something in the library, their portrait, their banner (a
 * wide picture, empty for none) and their years (0 for unknown).
 */
data class Artist(val name: String, val art: String, val tracks: Long, val banner: String = "", val born: Long = 0, val died: Long = 0) {
    companion object {
        fun of(e: ArtistsEntry): Artist = Artist(e.name, e.art, e.tracks, e.banner, e.born, e.died)
    }
}

/** A row of `composers`: everyone some work is by, with how many works and tracks of theirs there are. */
data class Composer(
    val name: String,
    val art: String,
    val banner: String,
    val born: Long,
    val died: Long,
    val works: Long,
    val tracks: Long,
) {
    companion object {
        fun of(e: ComposersEntry): Composer = Composer(e.name, e.art, e.banner, e.born, e.died, e.works, e.tracks)
    }
}

/**
 * A row of `shows`: a podcast or a series of sermons — its key (`podcast/full-time-nix`), its kind
 * (`podcast`, `sermon`), who makes it, what it is about, its picture, and how many of its episodes
 * the server has.
 */
data class Show(
    val id: String,
    val kind: String,
    val title: String,
    val author: String,
    val description: String,
    val art: String,
    val episodes: Long,
    val newestMs: Long = 0,
) {
    companion object {
        fun of(e: ShowsEntry): Show = Show(e.id, e.kind, e.title, e.author, e.description, e.art, e.episodes, e.newestMs)
    }
}

/**
 * A feed the library follows (`subscriptions`): its show once the server has read it, how many of
 * its episodes are here and how many the feed has, how many it keeps (0 for every one), and why
 * its last read failed.
 */
data class Subscription(
    val url: String,
    val kind: String,
    val title: String,
    val showId: String,
    val episodes: Long,
    val items: Long,
    val keep: Long,
    val error: String,
) {
    /** Whether the feed has older episodes than the library keeps of it. */
    val hasOlder: Boolean
        get() = keep > 0 && items > keep

    companion object {
        fun of(e: SubscriptionsEntry): Subscription = Subscription(e.url, e.kind, e.title, e.showId, e.episodes, e.items, e.keep, e.error)
    }
}

/** A file the server has not got yet (`downloads`): queued, downloading and how far, or failed and why. */
data class Download(val mediaId: Id, val state: String, val bytes: Long, val total: Long, val error: String) {
    /** What a row says in place of its length: the desktop's `Download::short`. */
    val short: String
        get() =
            when (state) {
                "downloading" -> if (total > 0) "${(bytes * 100 / total).coerceIn(0, 99)}%" else "\u2193"
                "failed" -> "failed"
                else -> "queued"
            }

    /** What it is, said whole. */
    val said: String
        get() =
            when (state) {
                "downloading" -> if (total > 0) "Downloading to the server: $short" else "Downloading to the server"
                "failed" -> if (error.isNotEmpty()) "Could not download: $error" else "Could not download"
                else -> "Not on the server yet"
            }

    companion object {
        fun of(e: DownloadsEntry): Download = Download(e.mediaId, e.state, e.bytes, e.total, e.error)
    }
}

/** How far somebody has listened to an episode (`my_progress`, `show_progress`). */
data class Listened(val mediaId: Id, val userId: String, val name: String, val positionMs: Long, val durationMs: Long, val finished: Boolean) {
    /** How far, short: played, or a share of it — the desktop's `Listened::short`. */
    val short: String
        get() =
            when {
                finished -> "played"
                durationMs > 0 -> "${(positionMs * 100 / durationMs).coerceIn(0, 99)}%"
                else -> "started"
            }

    /** Where a play of it starts: a few seconds before where they stopped; from the start once played. */
    val resumeMs: Long
        get() = if (finished) 0 else (positionMs - 3_000).coerceAtLeast(0)

    companion object {
        fun of(e: ProgressEntry): Listened = Listened(e.mediaId, e.userId, e.name, e.positionMs, e.durationMs, e.finished)

        /** Everybody's place in one episode, the signed-in person first as "you": `you 40% · Ada played`. */
        fun said(all: List<Listened>, me: String): String =
            all.sortedBy { it.userId != me }.joinToString(" · ") { l ->
                val who = if (l.userId == me) "you" else l.name.ifEmpty { l.userId }
                "$who ${l.short}"
            }
    }
}

/**
 * What an episode's row says beyond its title (`episode_details`): what it is about, in plain text,
 * and when it came out (0 for nobody said).
 */
data class EpisodeDetail(val mediaId: Id, val description: String, val publishedMs: Long) {
    companion object {
        fun of(e: EpisodeDetailsEntry): EpisodeDetail = EpisodeDetail(e.mediaId, e.description, e.publishedMs)
    }
}

/** What an episode's row says, as a podcast app's do: when it came out, and how much of it is left. */
object Episodes {
    private val MONTHS = arrayOf("JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC")

    /**
     * When it came out, in small capitals: "TODAY", "YESTERDAY", "3 DAYS AGO" in its first week, then
     * its day — "OCT 21" — with its year when that is not this one. Nothing, for nobody said.
     */
    fun dated(publishedMs: Long, nowMs: Long, zone: java.time.ZoneId = java.time.ZoneId.systemDefault()): String {
        if (publishedMs <= 0) return ""
        val day = java.time.Instant.ofEpochMilli(publishedMs).atZone(zone).toLocalDate()
        val today = java.time.Instant.ofEpochMilli(nowMs).atZone(zone).toLocalDate()
        val ago = java.time.temporal.ChronoUnit.DAYS.between(day, today)
        return when {
            ago == 0L -> "TODAY"
            ago == 1L -> "YESTERDAY"
            ago in 2L..6L -> "$ago DAYS AGO"
            day.year == today.year -> "${MONTHS[day.monthValue - 1]} ${day.dayOfMonth}"
            else -> "${MONTHS[day.monthValue - 1]} ${day.dayOfMonth}, ${day.year}"
        }
    }

    /**
     * What stands beside its play: how its download to the server goes while its file is not
     * there yet, "Played", the time left once begun, or its length.
     */
    fun left(track: Track, mine: Listened?, download: Download?): String {
        val whole = mine?.durationMs?.takeIf { it > 0 } ?: track.durationMs
        return when {
            download != null -> download.short
            mine?.finished == true -> "Played"
            mine != null && mine.positionMs > 0 && whole > 0 -> "${Media.spell((whole - mine.positionMs).coerceAtLeast(0))} left"
            track.durationMs > 0 -> Media.spell(track.durationMs)
            else -> ""
        }
    }

    /** How far through it the listener is, 0 to 1; null for not begun, played, or no length known. */
    fun progress(track: Track, mine: Listened?): Float? {
        if (mine == null || mine.finished || mine.positionMs <= 0) return null
        val whole = mine.durationMs.takeIf { it > 0 } ?: track.durationMs
        if (whole <= 0) return null
        return (mine.positionMs.toFloat() / whole).coerceIn(0f, 1f)
    }
}

/** One chapter of an episode (`chapters`). */
data class Chapter(val startMs: Long, val title: String) {
    companion object {
        fun of(e: ChaptersEntry): Chapter = Chapter(e.startMs, e.title)

        /** The chapter `ms` is in: the last that starts at or before it. */
        fun at(chapters: List<Chapter>, ms: Long): Chapter? = chapters.lastOrNull { it.startMs <= ms }
    }
}

/**
 * Somebody on an episode (`people`), as its feed names them: what they are on it as the feed says
 * it — "Host", "Guest" — their picture, a media path drawn as a cover is, and their own page on the
 * web, empty for none.
 */
data class OnEpisode(val name: String, val role: String, val art: String, val link: String) {
    /** Whether they make the show rather than visit it. */
    val hosts: Boolean
        get() = role.equals("host", ignoreCase = true)

    companion object {
        fun of(e: PeopleEntry): OnEpisode = OnEpisode(e.name, e.role, e.art, e.link)

        /** Who an episode's row names: everybody on it but its hosts, whom the show's page names once. */
        fun guests(all: List<OnEpisode>): List<OnEpisode> = all.filterNot { it.hosts }

        /** Their names as a sentence lists them: "Ada", "Ada and Grace", "Ada, Grace and Edsger". */
        fun named(all: List<OnEpisode>): String =
            if (all.size < 2) all.firstOrNull()?.name.orEmpty() else all.dropLast(1).joinToString(", ") { it.name } + " and " + all.last().name
    }
}

/** One line of an episode's transcript (`transcript`): when it is said, by whom (often nobody says), and what. */
data class Cue(val startMs: Long, val endMs: Long, val speaker: String, val text: String) {
    companion object {
        fun of(e: TranscriptEntry): Cue = Cue(e.startMs, e.endMs, e.speaker, e.text)

        /** The line being said `ms` in, by its place: the last that starts at or before it; -1 before the first. */
        fun at(lines: List<Cue>, ms: Long): Int = lines.indexOfLast { it.startMs <= ms }

        /** The name over each line: its speaker's where the speaker changes, else nothing. */
        fun speakers(lines: List<Cue>): List<String> =
            lines.mapIndexed { i, l -> if (l.speaker.isNotEmpty() && (i == 0 || lines[i - 1].speaker != l.speaker)) l.speaker else "" }
    }
}

object Shows {
    /** The show an episode is of: the one of its kind its creator names; null for a song. */
    fun of(track: Track, shows: List<Show>): Show? =
        if (track.kind == "song") null else shows.firstOrNull { it.kind == track.kind && it.title == track.creator }
}

/** A row of `track_details`: what only a song has — its album, its part of a work, its number on the release. */
data class TrackDetail(
    val mediaId: Id,
    val album: String,
    val part: String,
    val track: Long,
    val performer: String,
    val licence: String,
    /** The work it is a movement of, and whose, as an album's headings name them; empty for a song. */
    val composer: String = "",
    val work: String = "",
) {
    companion object {
        fun of(e: TrackDetailsEntry): TrackDetail = TrackDetail(e.mediaId, e.album, e.part, e.track, e.performer, e.licence, e.composer, e.work)
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

    /** One performance's tracks, in the order the work goes (`recording`). */
    fun recording(h: Harken, id: String): View<LibraryEntry> = h.recording(RecordingInput(NO_PLAYLIST, id))

    /** One show's episodes, newest first. */
    fun show(h: Harken, id: String): View<LibraryEntry> = h.show(ShowInput(NO_PLAYLIST, id))

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

    /** Follow a feed, as a podcast or as sermons (`library.subscribe`): the library's role's. */
    fun subscribe(h: Harken, kind: String, url: String): Wrote = wrote { h.subscribe(Subscribe(kind, url)) }

    fun unsubscribe(h: Harken, url: String): Wrote = wrote { h.unsubscribe(Unsubscribe(url)) }

    /** An episode said played, or not, by the signed-in person. */
    fun markPlayed(h: Harken, media: Id, finished: Boolean): Wrote = wrote { h.markPlayed(MarkPlayed(media, finished, "")) }

    fun showProgress(h: Harken, show: String): View<ProgressEntry> = h.showProgress(ShowInput(NO_PLAYLIST, show))

    /** One show's episodes' descriptions and dates, in the order its page lists them. */
    fun episodeDetails(h: Harken, show: String): View<EpisodeDetailsEntry> = h.episodeDetails(ShowInput(NO_PLAYLIST, show))

    fun chapters(h: Harken, media: Id): View<ChaptersEntry> = h.chapters(EpisodeInput(media))

    /** Who is on an episode, in its feed's order. */
    fun people(h: Harken, media: Id): View<PeopleEntry> = h.people(EpisodeInput(media))

    /** An episode's transcript, line by line in the order it is said. */
    fun transcript(h: Harken, media: Id): View<TranscriptEntry> = h.transcript(EpisodeInput(media))

    /** Every episode somebody is on, newest first: their page's episodes. */
    fun appearances(h: Harken, name: String): View<LibraryEntry> = h.appearances(ArtistInput(NO_PLAYLIST, name))

    /** `more` of a followed feed's older episodes, or 0 for every one. */
    fun fetchOlder(h: Harken, url: String, more: Long): Wrote = wrote { h.fetchOlder(FetchOlder(url, more)) }

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
