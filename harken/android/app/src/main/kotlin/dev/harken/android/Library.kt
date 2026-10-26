// The library, as the screens see it: harken's views over the session the
// onboarding opened (`onboarding/ArkSession.kt`) — the Rust kernel through
// the bridge, signed in, linked, driving itself. Everything the app does to
// it runs on the session's thread: every write, every read, and the splice
// after each change the session reports (`onChange`) — never on a clock, so
// a library at rest runs nothing at all.
//
// **The lists are spliced, not re-read.** The library, the playlists, the
// albums, the artists and the songs' details are maintained views — the
// kernel's (§1.10), typed by the generated stubs (`gen.View`) — read once,
// Home's three first; after every change each is asked what it did, and its
// patches are spliced into the list the screens hold; only a `Reset` reads
// one again. A page — an album, an artist, a playlist, a performance — is a
// view too, and so are a catalogue's lists (a person's works, a work's
// performances, who is on one), each held while a screen shows it and
// dropped when the last one leaves. Search is a
// read per needle of the domain's `search`.
//
// Every change this phone makes is kept with its entry's id, so the
// diagnostics can say where each one stands; a rejection from the server
// is said once, in a sentence, as a notice.
package dev.harken.android

import dev.arkdb.ffi.Change
import dev.arkdb.ffi.Id
import dev.arkdb.ffi.Refusal
import dev.arkdb.ffi.Standing
import dev.arkdb.ffi.Status
import dev.arkdb.ffi.Update
import dev.arkdb.ffi.View
import dev.harken.android.gen.Credits
import dev.harken.android.gen.CreditsEntry
import dev.harken.android.gen.Harken
import dev.harken.android.gen.Recordings
import dev.harken.android.gen.RecordingsEntry
import dev.harken.android.gen.WorkInput
import dev.harken.android.gen.WorkSummary
import dev.harken.android.gen.Works
import uniffi.harken_domain.Playlist
import dev.harken.android.onboarding.ArkSession
import dev.harken.android.onboarding.SessionStatus
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull

/** Somewhere a screen goes, and a list it plays from. */
sealed class Page {
    /** What the navigation calls it: names as the hex of their UTF-8, so no character is special. */
    abstract val route: String

    data class ForAlbum(val name: String) : Page() {
        override val route: String
            get() = "album/" + Hex.encode(name.toByteArray(Charsets.UTF_8))
    }

    data class ForArtist(val name: String) : Page() {
        override val route: String
            get() = "artist/" + Hex.encode(name.toByteArray(Charsets.UTF_8))
    }

    data class ForPlaylist(val id: Id) : Page() {
        override val route: String
            get() = "playlist/" + id.hex
    }

    /** One work and its performances, by the key the domain derives (`johann-sebastian-bach/bwv-988`). */
    data class ForWork(val id: String) : Page() {
        override val route: String
            get() = "work/" + Hex.encode(id.toByteArray(Charsets.UTF_8))
    }

    /** One performance, in the order the work goes, by its key (`…/bwv-988@kimiko-ishizaka`). */
    data class ForRecording(val id: String) : Page() {
        override val route: String
            get() = "recording/" + Hex.encode(id.toByteArray(Charsets.UTF_8))
    }

    /** One podcast or series of sermons and its episodes, newest first, by its key (`podcast/full-time-nix`). */
    data class ForShow(val id: String) : Page() {
        override val route: String
            get() = "show/" + Hex.encode(id.toByteArray(Charsets.UTF_8))
    }

    /** Every song, in the order they were added. */
    object Songs : Page() {
        override val route: String = "songs"
    }

    /**
     * Where a track was played from, as the listening session and the plays carry it — the
     * desktop's address-bar spelling (`rust/arkui`'s route: `#album/Water%20Music`), so a page
     * named on one device is the same page on another. A playlist by its name, as there; one this
     * phone cannot name is nowhere in particular.
     */
    fun place(playlists: List<Playlist>): String =
        when (this) {
            is ForAlbum -> "#album/" + Places.encode(name)
            is ForArtist -> "#artist/" + Places.encode(name)
            is ForPlaylist -> playlists.firstOrNull { it.id == id }?.let { "#playlist/" + Places.encode(it.name) } ?: ""
            is ForWork -> "#work/" + Places.encode(id)
            is ForRecording -> "#recording/" + Places.encode(id)
            is ForShow -> "#show/" + Places.encode(id)
            is Songs -> "#library"
        }

    companion object {
        /** The page a place names (`#album/…`, [place]); null for nowhere, or a page this phone has not got. */
        fun ofPlace(place: String, playlists: List<Playlist>): Page? {
            val body = place.removePrefix("#")
            val kind = body.substringBefore('/')
            val name = Places.decode(body.substringAfter('/', ""))
            return when (kind) {
                "album" -> name.takeIf { it.isNotEmpty() }?.let { ForAlbum(it) }
                "artist" -> name.takeIf { it.isNotEmpty() }?.let { ForArtist(it) }
                "playlist" -> playlists.firstOrNull { it.name == name }?.let { ForPlaylist(it.id) }
                // A composer's page is their artist page: one page per person.
                "composer" -> name.takeIf { it.isNotEmpty() }?.let { ForArtist(it) }
                "work" -> name.takeIf { it.isNotEmpty() }?.let { ForWork(it) }
                "recording" -> name.takeIf { it.isNotEmpty() }?.let { ForRecording(it) }
                "show" -> name.takeIf { it.isNotEmpty() }?.let { ForShow(it) }
                "library" -> Songs
                else -> null
            }
        }

        /** The page a route names, or null. */
        fun parse(route: String): Page? {
            val kind = route.substringBefore('/')
            val arg = route.substringAfter('/', "")
            fun name(): String = String(Hex.decode(arg), Charsets.UTF_8)
            return try {
                when (kind) {
                    "album" -> ForAlbum(name())
                    "artist" -> ForArtist(name())
                    "playlist" -> ForPlaylist(Id(arg).also { require(arg.length == 32) })
                    "work" -> ForWork(name())
                    "recording" -> ForRecording(name())
                    "show" -> ForShow(name())
                    "songs" -> Songs
                    else -> null
                }
            } catch (e: Exception) {
                null
            }
        }
    }
}

/** The keys a catalogue's lists are held by ([Library.works] and the rest), for [Library.release]. */
fun worksKey(composer: String): String = "works:$composer"

fun workKey(id: String): String = "work:$id"

fun recordingsKey(work: String): String = "recordings:$work"

fun creditsKey(recording: String): String = "credits:$recording"

fun progressKey(show: String): String = "progress:$show"

/** A place's name, percent-encoded as `rust/arkui`'s `url::encode` writes it: what is not unreserved. */
object Places {
    fun encode(s: String): String {
        val out = StringBuilder()
        for (b in s.toByteArray(Charsets.UTF_8)) {
            val c = b.toInt() and 0xff
            val ch = c.toChar()
            if (ch in 'A'..'Z' || ch in 'a'..'z' || ch in '0'..'9' || ch in "-_.~") out.append(ch) else out.append('%').append("%02X".format(c))
        }
        return out.toString()
    }

    fun decode(s: String): String {
        val bytes = java.io.ByteArrayOutputStream()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            if (c == '%' && i + 2 < s.length) {
                val n = s.substring(i + 1, i + 3).toIntOrNull(16)
                if (n != null) {
                    bytes.write(n)
                    i += 3
                    continue
                }
            }
            bytes.write(c.toString().toByteArray(Charsets.UTF_8))
            i++
        }
        return bytes.toString("UTF-8")
    }
}

/** One change this phone made, and what to call it beside its standing. */
data class Authored(val id: String, val what: String)

/** Every number the diagnostics show. */
data class Diagnostics(
    val status: Status?,
    val changes: List<Pair<String, String>>,
)

/** `changes` made to `rows`, each row read by `of`. */
fun <E, T> MutableList<T>.splice(changes: List<Change<E>>, of: (E) -> T) {
    for (c in changes)
        when (c) {
            is Change.Insert -> add(c.at, of(c.row))
            is Change.Remove -> removeAt(c.at)
            is Change.Update -> set(c.at, of(c.row))
        }
}

class Library(private val session: ArkSession) {
    /** Who is signed in. */
    val user: String = session.login.user

    /** What to call them. */
    val name: String = session.login.name.ifBlank { session.login.user }

    val email: String = session.login.email

    /** Their provider's picture of them, or empty. */
    val picture: String = session.login.picture

    /** The server, as the onboarding found it. */
    val server: String = session.server

    val status: StateFlow<SessionStatus> = session.status

    private val _loaded = MutableStateFlow(false)
    /** Whether Home's views — the songs, the playlists, the albums — have been read once. */
    val loaded: StateFlow<Boolean> = _loaded.asStateFlow()

    private val _complete = MutableStateFlow(false)
    /** Whether the rest — the artists, the songs' details — have been read too, after. */
    val complete: StateFlow<Boolean> = _complete.asStateFlow()

    private val tracksFlow = MutableStateFlow<List<Track>>(emptyList())
    private val playlistsFlow = MutableStateFlow<List<Playlist>>(emptyList())
    private val albumsFlow = MutableStateFlow<List<Album>>(emptyList())
    private val artistsFlow = MutableStateFlow<List<Artist>>(emptyList())
    private val composersFlow = MutableStateFlow<List<Composer>>(emptyList())
    private val showsFlow = MutableStateFlow<List<Show>>(emptyList())
    private val subscriptionsFlow = MutableStateFlow<List<Subscription>>(emptyList())
    private val downloadsFlow = MutableStateFlow<Map<Id, Download>>(emptyMap())
    private val mineFlow = MutableStateFlow<Map<Id, Listened>>(emptyMap())
    private val detailsFlow = MutableStateFlow<Map<Id, TrackDetail>>(emptyMap())
    private val noticeFlow = MutableStateFlow<String?>(null)

    /** Every song, in the order it was added. */
    val tracks: StateFlow<List<Track>> = tracksFlow.asStateFlow()
    val playlists: StateFlow<List<Playlist>> = playlistsFlow.asStateFlow()
    val albums: StateFlow<List<Album>> = albumsFlow.asStateFlow()
    val artists: StateFlow<List<Artist>> = artistsFlow.asStateFlow()

    /** Everyone some work is by, read with the artists; none in a library of pop. */
    val composers: StateFlow<List<Composer>> = composersFlow.asStateFlow()

    /** Every podcast and series of sermons, by title; read with the artists. */
    val shows: StateFlow<List<Show>> = showsFlow.asStateFlow()

    /** The feeds the library follows, by when each was added; read with the artists. */
    val subscriptions: StateFlow<List<Subscription>> = subscriptionsFlow.asStateFlow()

    /** The files the server has not got yet, by their media id; read with the artists. */
    val downloads: StateFlow<Map<Id, Download>> = downloadsFlow.asStateFlow()

    /** How far the signed-in person has got in each episode they have begun, by its media id. */
    val mine: StateFlow<Map<Id, Listened>> = mineFlow.asStateFlow()

    /**
     * An album's cover by its name, "" for none: the albums by name, made again only when the list
     * is another one. What the listening session's tracks — which name an album, not a row — are
     * drawn with, and asked from any thread.
     */
    val artOf: (String) -> String =
        object : (String) -> String {
            private var of: List<Album>? = null
            private var byName: Map<String, String> = emptyMap()

            @Synchronized
            override fun invoke(name: String): String {
                val now = albumsFlow.value
                if (now !== of) {
                    of = now
                    byName = now.associate { it.name to it.art }
                }
                return byName[name] ?: ""
            }
        }

    /** What only a song has, by its media id. */
    val details: StateFlow<Map<Id, TrackDetail>> = detailsFlow.asStateFlow()

    /** The last refusal or rejection, in a sentence; cleared by [dismiss]. */
    val notice: StateFlow<String?> = noticeFlow.asStateFlow()

    private val movedFlow = MutableStateFlow(0L)
    /** How many changes the session has reported: what the diagnostics read again on, rather than on a clock. */
    val moved: StateFlow<Long> = movedFlow.asStateFlow()

    // A view of the domain's and the list it is spliced into; on the session's thread only.
    private class Held<E, T>(val open: (Harken) -> View<E>, val of: (E) -> T, val set: (List<T>) -> Unit = {}) {
        var view: View<E>? = null
        val rows = ArrayList<T>()

        fun publish() = set(rows.toList())
    }

    private val library = Held({ Phone.library(it, null) }, Track::of) { tracksFlow.value = it }
    private val playlistsHeld = Held({ it.playlists() }, { p: Playlist -> p }) { playlistsFlow.value = it }
    private val albumsHeld = Held({ it.albums() }, Album::of) { albumsFlow.value = it }
    private val artistsHeld = Held({ it.artists() }, Artist::of) { artistsFlow.value = it }
    private val composersHeld = Held({ it.composers() }, Composer::of) { composersFlow.value = it }
    private val showsHeld = Held({ it.shows() }, Show::of) { showsFlow.value = it }
    private val subscriptionsHeld = Held({ it.subscriptions() }, Subscription::of) { subscriptionsFlow.value = it }
    private val downloadsHeld = Held({ it.downloads() }, Download::of) { r -> downloadsFlow.value = r.associateBy { it.mediaId } }
    private val mineHeld = Held({ it.myProgress() }, Listened::of) { r -> mineFlow.value = r.associateBy { it.mediaId } }
    private val detailsHeld = Held({ it.trackDetails() }, TrackDetail::of) { r -> detailsFlow.value = r.associateBy { it.mediaId } }
    private val playedHeld = Held({ it.lastPlayed() }, Played::of) { lastFlow.value = it.firstOrNull() }
    private val home = listOf<Held<*, *>>(library, playlistsHeld, albumsHeld)
    private val rest = listOf<Held<*, *>>(artistsHeld, composersHeld, showsHeld, subscriptionsHeld, downloadsHeld, mineHeld, detailsHeld, playedHeld)
    private val lastFlow = MutableStateFlow<Played?>(null)

    /**
     * The newest play in the log, this person's: what Home says was last played while the
     * listening session has nothing of its own to say — the same on every device, and there
     * offline.
     */
    val lastPlayed: StateFlow<Played?> = lastFlow.asStateFlow()

    // A page's view, and how many screens hold it, by what it is of: a page of tracks by its
    // [Page], a catalogue's list by the view and its input ("works:Bach"). The map is shared with
    // the main thread; each entry's view is touched on the session's thread only. Null until the
    // page's view has been read: a page told "none yet" from "none".
    private class Opened<T>(val held: Held<*, T>, val flow: MutableStateFlow<List<T>?>) {
        var refs = 0
    }

    private val pages = ConcurrentHashMap<Any, Opened<*>>()

    private val changes = ArrayList<Authored>()
    private var unhook: () -> Unit = {}

    // The peer, once the session has opened it: on the session's thread only.
    private var harken: Harken? = null

    // **What the first screen draws is read first.** Home is the songs, the playlists and the
    // albums: those are read and published, and only then — on a later turn of the session's
    // thread, so nothing that published waits on it — the artists, every song's details and the
    // last play, which the Library's Artists tab and a row's album ask for.
    init {
        session.post { h ->
            harken = h
            for (held in home) hydrate(h, held)
            homeShown()
            unhook = session.onChange { tick() }
            session.post { q ->
                for (held in rest) hydrate(q, held)
                _complete.value = true
            }
        }
    }

    // Home has what it draws: the splash may go.
    private fun homeShown() {
        _loaded.value = true
        session.ready()
    }

    private fun <E, T> hydrate(h: Harken, held: Held<E, T>) {
        held.view?.close()
        held.view =
            try {
                held.open(h)
            } catch (e: Refusal) {
                null
            }
        held.rows.clear()
        held.view?.rows()?.mapTo(held.rows, held.of)
        held.publish()
    }

    // What a view did since it was last asked, spliced into its list; true when it moved.
    private fun <E, T> update(held: Held<E, T>): Boolean {
        val v = held.view ?: return false
        return when (val u = v.take()) {
            is Update.Unchanged -> false
            is Update.Patched -> {
                held.rows.splice(u.changes, held.of)
                true
            }
            is Update.Reset -> {
                held.rows.clear()
                u.rows.mapTo(held.rows, held.of)
                true
            }
        }
    }

    private fun splice() {
        for (held in home + rest) if (update(held)) held.publish()
        for (o in pages.values) respliced(o)
    }

    private fun <T> respliced(o: Opened<T>) {
        if (update(o.held)) o.flow.value = o.held.rows.toList()
    }

    // What a change the session reported comes to here: the views spliced (a list is published
    // only when its rows moved), a rejection said once.
    private fun tick() {
        val h = harken ?: return
        movedFlow.value++
        splice()
        val rejected = h.peer.takeRejections()
        if (rejected.isNotEmpty()) noticeFlow.value = "Not saved: ${rejected.last().why}"
    }

    // Pages -------------------------------------------------------------------

    // A page's tracks; none for a page that is not a list of them (a work's).
    private fun pageView(page: Page): Held<*, Track>? =
        when (page) {
            is Page.ForAlbum -> Held({ Phone.album(it, page.name) }, Track::of)
            is Page.ForArtist -> Held({ Phone.artist(it, page.name) }, Track::of)
            is Page.ForPlaylist -> Held({ Phone.playlist(it, page.id) }, Track::of)
            is Page.ForRecording -> Held({ Phone.recording(it, page.id) }, Track::of)
            is Page.ForShow -> Held({ Phone.show(it, page.id) }, Track::of)
            is Page.ForWork, is Page.Songs -> null
        }

    /** A page's tracks, kept up to date while it is held; [release] it when the screen goes. */
    fun hold(page: Page): StateFlow<List<Track>?> {
        if (page is Page.Songs) return tracks
        val view = pageView(page) ?: return MutableStateFlow(emptyList<Track>()).asStateFlow()
        return open(page) { view }
    }

    @Suppress("UNCHECKED_CAST")
    private fun <T> open(key: Any, make: () -> Held<*, T>): StateFlow<List<T>?> {
        val o = pages.computeIfAbsent(key) { Opened(make(), MutableStateFlow(null)) } as Opened<T>
        synchronized(o) { o.refs++ }
        session.post { h ->
            if (o.held.view == null) {
                hydrate(h, o.held)
                o.flow.value = o.held.rows.toList()
            }
        }
        return o.flow.asStateFlow()
    }

    /** The works somebody wrote (`works`), in catalogue order — none for somebody who wrote none; [release] [worksKey]. */
    fun works(composer: String): StateFlow<List<WorkSummary>?> = open(worksKey(composer)) { Held({ it.works(Works(composer)) }, { w: WorkSummary -> w }) }

    /** One work's own row (`work`), by its key: none for a work this peer has not got. */
    fun work(id: String): StateFlow<List<WorkSummary>?> = open(workKey(id)) { Held({ it.work(WorkInput(id)) }, { w: WorkSummary -> w }) }

    /** A work's performances (`recordings`), the most complete first. */
    fun recordings(work: String): StateFlow<List<RecordingsEntry>?> = open(recordingsKey(work)) { Held({ it.recordings(Recordings(work)) }, { r: RecordingsEntry -> r }) }

    /** Who is on a performance (`credits`), in billing order. */
    fun credits(recording: String): StateFlow<List<CreditsEntry>?> = open(creditsKey(recording)) { Held({ it.credits(Credits(recording)) }, { c: CreditsEntry -> c }) }

    /** How far everybody has listened to a show's episodes (`show_progress`); [release] [progressKey]. */
    fun showProgress(show: String): StateFlow<List<Listened>?> = open(progressKey(show)) { Held({ Phone.showProgress(it, show) }, Listened::of) }

    /** An episode's chapters once, in order. */
    suspend fun chapters(media: Id): List<Chapter> = session.run { h -> Phone.once({ Phone.chapters(h, media) }, Chapter::of) }

    /** Let go of what [hold], [works], [work], [recordings] or [credits] held, by its key. */
    fun release(key: Any) {
        val o = pages[key] ?: return
        val gone = synchronized(o) { --o.refs <= 0 }
        if (gone && pages.remove(key, o)) session.post { o.held.view?.close() }
    }

    /** A page's tracks once, for playing it from somewhere that does not show it (Home's last played). */
    suspend fun read(page: Page): List<Track> =
        when (page) {
            is Page.Songs -> tracks.value
            is Page.ForAlbum -> session.run { h -> Phone.once({ Phone.album(h, page.name) }, Track::of) }
            is Page.ForArtist -> session.run { h -> Phone.once({ Phone.artist(h, page.name) }, Track::of) }
            is Page.ForPlaylist -> session.run { h -> Phone.once({ Phone.playlist(h, page.id) }, Track::of) }
            is Page.ForRecording -> session.run { h -> Phone.once({ Phone.recording(h, page.id) }, Track::of) }
            is Page.ForShow -> session.run { h -> Phone.once({ Phone.show(h, page.id) }, Track::of) }
            // A work plays as its most complete performance does.
            is Page.ForWork -> recordingsOf(page.id).firstOrNull()?.let { read(Page.ForRecording(it.id)) }.orEmpty()
        }

    /** A work's performances once, the most complete first: what a work's Play reads when it is pressed. */
    suspend fun recordingsOf(work: String): List<RecordingsEntry> = session.run { h -> Phone.once({ h.recordings(Recordings(work)) }) { it } }

    /**
     * This person's plays, newest first — the domain's `recently_played`, the last fifty, a track as
     * often as it was played: what a car's Home lists after what is playing (Car.kt).
     */
    suspend fun recentlyPlayed(): List<Played> = session.run { h -> Phone.once({ h.recentlyPlayed() }, Played::of) }

    /**
     * Every list read once — the artists and the songs' details come a turn after Home's — or ten
     * seconds gone, whichever is first: what a car waits on before it reads them, when it binds to a
     * session that has only just opened.
     */
    suspend fun whole() {
        withTimeoutOrNull(10_000) { complete.first { it } }
    }

    /** The library's songs matching `needle`, by the domain's `search`. */
    suspend fun search(needle: String): List<Track> = session.run { h -> Phone.search(h, needle) }

    /** Which of the person's playlists `track` is on. */
    suspend fun playlistsOf(track: Track): Set<Id> = session.run { h -> Phone.playlistsOf(h, track.id).map { it.id }.toSet() }

    /** What `create_playlist`'s own checks say of a name as it is typed; null when it would pass. */
    suspend fun nameProblem(name: String): String? = session.run { h -> Phone.nameProblem(h, name) }

    // Writing -----------------------------------------------------------------

    private fun write(f: (Harken) -> Unit) =
        session.post { h ->
            try {
                f(h)
            } catch (e: Exception) {
                noticeFlow.value = e.message ?: e.javaClass.simpleName
            }
            splice()
        }

    private fun record(w: Wrote, what: String, refused: String) {
        when (w) {
            is Wrote.Refused -> noticeFlow.value = "$refused: ${w.reason}"
            is Wrote.Entry -> synchronized(changes) { changes.add(0, Authored(w.id, what)) }
        }
    }

    /** Make a playlist, and put `then` on it once it is there. */
    fun createPlaylist(name: String, then: Track? = null) = write { h ->
        val before = playlistsHeld.rows.map { it.id }.toSet()
        record(Phone.createPlaylist(h, name), "make the playlist ${name.trim()}", "Couldn't make it")
        splice()
        val made = playlistsHeld.rows.firstOrNull { it.id !in before }
        if (then != null && made != null)
            record(Phone.addToPlaylist(h, made.id, then.id), "add ${then.title} to ${made.name}", "Couldn't add it")
    }

    fun addToPlaylist(track: Track, playlist: Playlist) = write { h ->
        record(Phone.addToPlaylist(h, playlist.id, track.id), "add ${track.title} to ${playlist.name}", "Couldn't add it")
    }

    fun removeFromPlaylist(track: Track, playlist: Id) = write { h ->
        val name = playlistsHeld.rows.firstOrNull { it.id == playlist }?.name ?: "a playlist"
        record(Phone.removeFromPlaylist(h, playlist, track.id), "take ${track.title} off $name", "Couldn't take it off")
    }

    /** Follow a feed, as a podcast (`podcast`) or as sermons (`sermon`); refused, the notice says why. */
    fun follow(kind: String, url: String) = write { h ->
        record(Phone.subscribe(h, kind, url.trim()), "follow ${url.trim()}", "Couldn't follow it")
    }

    fun unfollow(feed: Subscription) = write { h ->
        record(Phone.unsubscribe(h, feed.url), "unfollow ${feed.title.ifEmpty { feed.url }}", "Couldn't unfollow it")
    }

    /** An episode said played, or not, by the signed-in person. */
    fun markPlayed(track: Track, finished: Boolean) = write { h ->
        record(Phone.markPlayed(h, track.id, finished), "mark ${track.title} ${if (finished) "played" else "unplayed"}", "Couldn't mark it")
    }

    /** `more` of a followed feed's older episodes, or 0 for every one. */
    fun fetchOlder(feed: Subscription, more: Long) = write { h ->
        record(Phone.fetchOlder(h, feed.url, more), "fetch older episodes of ${feed.title.ifEmpty { feed.url }}", "Couldn't fetch them")
    }

    fun dismiss() {
        noticeFlow.value = null
    }

    // Diagnostics -------------------------------------------------------------

    /** Every number the old settings screen had, and every change with where it stands. */
    suspend fun diagnostics(): Diagnostics =
        session.run { h ->
            val mine = synchronized(changes) { changes.toList() }
            Diagnostics(status = h.peer.status(), changes = mine.map { it.what to standing(h.peer.standing(it.id)) })
        }

    private fun standing(st: Standing): String =
        when (st) {
            is Standing.Pending -> "not synced yet"
            is Standing.Confirmed -> "saved"
            is Standing.Unknown -> "unknown"
            is Standing.Rejected -> "not saved: ${st.why}"
        }

    fun signOut() = session.signOut()

    /** Stop listening to the session; closing it is the onboarding's. */
    fun close() {
        session.post {
            unhook()
            for (held in home + rest) held.view?.close()
            for (o in pages.values) o.held.view?.close()
        }
    }
}
