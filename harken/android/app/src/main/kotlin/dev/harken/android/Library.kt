// The library, as the screens see it: harken's queries over the session the
// onboarding opened (`kotlin/ark-android`'s `ArkSession`) — the Kotlin
// `Peer` over `harken.ark`, signed in, linked, pumped on a thread of its
// own. Everything that touches the peer runs on that thread: every
// mutation, every read, and the splice after each pump.
//
// **The lists are spliced, not re-read.** The library, the playlists, the
// albums, the artists and the songs' details are maintained views
// (`ClientView`), hydrated once — Home's three first — and handed every `Changes` the peer
// reports; the patches each answers with are applied to the list the
// screens hold (`ClientView.splice`), and only a `Reset` reads one again.
// A page — an album, an artist, a playlist — is a view too, held while a
// screen shows it and dropped when the last one leaves. Search is a single
// read per needle, of the module's `search` over its text index.
//
// Every change this phone makes is kept with its entry's id, so the
// diagnostics can say where each one stands; a rejection from the server
// is said once, in a sentence, as a notice.
package dev.harken.android

import dev.arkdb.Changes
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.android.ArkSession
import dev.arkdb.android.SessionStatus
import dev.arkdb.client.ClientView
import dev.arkdb.client.LaunchView
import dev.arkdb.client.Peer
import dev.arkdb.client.PeerError
import dev.arkdb.client.Standing
import dev.arkdb.client.Status
import dev.arkdb.client.Update
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

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

    /** Every song, in the order they were added. */
    object Songs : Page() {
        override val route: String = "songs"
    }

    companion object {
        /** The page a route names, or null. */
        fun parse(route: String): Page? {
            val kind = route.substringBefore('/')
            val arg = route.substringAfter('/', "")
            fun name(): String = String(Hex.decode(arg), Charsets.UTF_8)
            return try {
                when (kind) {
                    "album" -> ForAlbum(name())
                    "artist" -> ForArtist(name())
                    "playlist" -> ForPlaylist(Id.ofHex(arg))
                    "songs" -> Songs
                    else -> null
                }
            } catch (e: Exception) {
                null
            }
        }
    }
}

/** One change this phone made, and what to call it beside its standing. */
data class Authored(val id: Id, val what: String)

/** Every number the diagnostics show. */
data class Diagnostics(
    val status: Status?,
    val changes: List<Pair<String, String>>,
    val verdict: String?,
)

class Library(private val session: ArkSession) {
    /** Who is signed in. */
    val user: String = session.login.user.id

    /** What to call them. */
    val name: String = session.login.user.name.ifBlank { session.login.user.id }

    val email: String = session.login.user.email

    /** The server, as the onboarding found it. */
    val server: String = session.server

    val status: StateFlow<SessionStatus> = session.status

    private val _loaded = MutableStateFlow(false)
    /** Whether Home's views — the songs, the playlists, the albums — have been read once. */
    val loaded: StateFlow<Boolean> = _loaded.asStateFlow()

    private val _provisional = MutableStateFlow(false)
    /**
     * Whether what Home shows came from the launch views' files and the live views have not yet
     * answered (Zero's `unknown`). harken draws it the same; a screen could tell.
     */
    val provisional: StateFlow<Boolean> = _provisional.asStateFlow()

    private val _complete = MutableStateFlow(false)
    /** Whether the rest — the artists, the songs' details — have been read too, after. */
    val complete: StateFlow<Boolean> = _complete.asStateFlow()

    private val tracksFlow = MutableStateFlow<List<Track>>(emptyList())
    private val playlistsFlow = MutableStateFlow<List<Playlist>>(emptyList())
    private val albumsFlow = MutableStateFlow<List<Album>>(emptyList())
    private val artistsFlow = MutableStateFlow<List<Artist>>(emptyList())
    private val detailsFlow = MutableStateFlow<Map<Id, TrackDetail>>(emptyMap())
    private val noticeFlow = MutableStateFlow<String?>(null)

    /** Every song, in the order it was added. */
    val tracks: StateFlow<List<Track>> = tracksFlow.asStateFlow()
    val playlists: StateFlow<List<Playlist>> = playlistsFlow.asStateFlow()
    val albums: StateFlow<List<Album>> = albumsFlow.asStateFlow()
    val artists: StateFlow<List<Artist>> = artistsFlow.asStateFlow()

    /** What only a song has, by its media id. */
    val details: StateFlow<Map<Id, TrackDetail>> = detailsFlow.asStateFlow()

    /** The last refusal or rejection, in a sentence; cleared by [dismiss]. */
    val notice: StateFlow<String?> = noticeFlow.asStateFlow()

    // On the peer's thread only — except, until the live view is there, the
    // launch view's provisional rows, under the held's own lock.
    private inner class Held<T>(val name: String, val args: Map<String, Value>, val of: (Value) -> T, val set: (List<T>) -> Unit = {}) {
        fun publish(rows: List<T>) {
            homePublishes.incrementAndGet()
            set(rows)
        }

        var view: ClientView? = null
        val rows = ArrayList<T>()
        var launch: LaunchView? = null
        var shown: List<Value>? = null
        var live = false
    }

    /** How many times one of Home's lists was published: a test counts them across a launch. */
    internal val homePublishes = java.util.concurrent.atomic.AtomicInteger()

    private val library = Held("library", Phone.libraryArgs(null), Track::of) { tracksFlow.value = it }
    private val playlistsHeld = Held("playlists", emptyMap(), Playlist::of) { playlistsFlow.value = it }
    private val albumsHeld = Held("albums", emptyMap(), Album::of) { albumsFlow.value = it }
    private val home = listOf<Held<*>>(library, playlistsHeld, albumsHeld)
    private val artistsHeld = Held("artists", emptyMap(), Artist::of)
    private val detailsHeld = Held("track_details", emptyMap(), TrackDetail::of)

    // A page's view, and how many screens hold it. The map is shared with the
    // main thread; each entry's view is touched on the peer's thread only.
    private class Opened(val held: Held<Track>, val flow: MutableStateFlow<List<Track>>) {
        var refs = 0
    }

    private val pages = ConcurrentHashMap<Page, Opened>()

    private val changes = ArrayList<Authored>()
    private var verdict: String? = null
    private var verdictsSeen = 0
    private var unhook: () -> Unit = {}

    // **What the first screen draws is read first.** Home is the songs, the
    // playlists and the albums: those are hydrated and published, and only
    // then — on a later turn of the peer's thread, so nothing that published
    // waits on it — the artists and every song's details, which Library's
    // Artists tab and a row's album ask for and which cost as much again.
    // A pump between the two splices what it can; a view not yet hydrated
    // has nothing to splice, and hydrates from the store as it then is.
    //
    // **And before that, from disk.** Home's three are launch views
    // (`dev.arkdb.client.LaunchView`): what they held when the app last went
    // to the background is read on a thread of its own while the peer opens,
    // and published as provisional — Home, populated, before the replica has
    // been read at all. When the peer is open the live views answer against
    // what was shown: equal, and nothing is published; different, and the
    // difference is spliced in. What a tap does in that window: playing a
    // provisional row starts at once (it needs only the row's file); a
    // change (a playlist made, a song added to one) is posted to the peer's
    // thread like any other, runs after the open, against the real replica,
    // and its own checks refuse a row that is gone — no queue of our own.
    init {
        for (h in home) launch(h)
        session.post { p ->
            p.takeChanges()
            var moved = false
            for (h in home) moved = goLive(p, h) || moved
            _provisional.value = false
            homeShown()
            if (moved) session.launchChanged()
            unhook = session.onPump { tick(it) }
            session.post { q ->
                hydrate(q, artistsHeld)
                hydrate(q, detailsHeld)
                artistsFlow.value = artistsHeld.rows.toList()
                detailsFlow.value = detailsHeld.rows.associateBy { it.mediaId }
                _complete.value = true
            }
        }
    }

    private fun <T> launch(h: Held<T>) {
        h.launch =
            session.launchView(h.name, h.name, h.args) { rows ->
                val mapped =
                    try {
                        rows.map(h.of)
                    } catch (e: Exception) {
                        return@launchView
                    }
                synchronized(h) {
                    if (h.live) return@launchView
                    h.shown = rows
                    h.rows.clear()
                    h.rows.addAll(mapped)
                    h.publish(mapped)
                }
                if (home.all { synchronized(it) { it.shown != null || it.live } }) {
                    _provisional.value = home.none { synchronized(it) { it.live } }
                    homeShown()
                }
            }
    }

    // Home has what it draws: the splash may go.
    private fun homeShown() {
        _loaded.value = true
        session.ready()
    }

    // The live view, against whatever was shown provisionally; true when its
    // rows moved (and were published), so the files are written again.
    private fun <T> goLive(p: Peer, h: Held<T>): Boolean {
        val shown = synchronized(h) {
            h.live = true
            h.shown
        }
        val lv = h.launch
        if (lv == null) {
            hydrate(p, h)
            h.publish(h.rows.toList())
            return false
        }
        val u =
            try {
                lv.live(p, shown)
            } catch (e: PeerError) {
                h.view = null
                h.rows.clear()
                h.publish(emptyList())
                return shown != null
            }
        h.view = lv.view
        return when (u) {
            is Update.Unchanged -> false
            is Update.Patched -> {
                ClientView.splice(h.rows, u.patches, h.of)
                h.publish(h.rows.toList())
                true
            }
            is Update.Reset -> {
                h.rows.clear()
                lv.view?.rows?.mapTo(h.rows, h.of)
                h.publish(h.rows.toList())
                true
            }
        }
    }

    private fun <T> hydrate(p: Peer, h: Held<T>) {
        h.view =
            try {
                p.view(h.name, h.args)
            } catch (e: PeerError) {
                null
            }
        h.rows.clear()
        h.view?.rows?.mapTo(h.rows, h.of)
    }

    // A Home view, handed what moved: spliced and published when it moved.
    private fun <T> moveHome(p: Peer, h: Held<T>, ch: Changes): Boolean {
        if (!update(p, h, ch)) return false
        h.publish(h.rows.toList())
        return true
    }

    // Hand what moved to a view, and splice its list; true when it moved.
    private fun <T> update(p: Peer, h: Held<T>, ch: Changes): Boolean {
        val v = h.view ?: return false
        return when (val u = v.update(p, ch)) {
            is Update.Unchanged -> false
            is Update.Patched -> {
                ClientView.splice(h.rows, u.patches, h.of)
                true
            }
            is Update.Reset -> {
                h.rows.clear()
                v.rows.mapTo(h.rows, h.of)
                true
            }
        }
    }

    private fun splice(p: Peer) {
        val ch = p.takeChanges()
        if (ch is Changes.Applied && ch.changes.isEmpty()) return
        var moved = false
        for (h in home) if (moveHome(p, h, ch)) moved = true
        if (moved) session.launchChanged()
        if (update(p, artistsHeld, ch)) artistsFlow.value = artistsHeld.rows.toList()
        if (update(p, detailsHeld, ch)) detailsFlow.value = detailsHeld.rows.associateBy { it.mediaId }
        for (o in pages.values) if (update(p, o.held, ch)) o.flow.value = o.held.rows.toList()
    }

    private fun tick(p: Peer) {
        splice(p)
        val rejected = p.takeRejections()
        if (rejected.isNotEmpty()) noticeFlow.value = "Not saved: ${rejected.last().reason}"
        val agreed = p.agreed
        if (agreed.size > verdictsSeen) {
            val (n, ok) = agreed.last()
            verdictsSeen = agreed.size
            verdict = "at $n the server ${if (ok) "agrees" else "DIVERGES"}"
        }
    }

    // Pages -------------------------------------------------------------------

    private fun pageView(page: Page): Held<Track>? =
        when (page) {
            is Page.ForAlbum -> Held("album", Phone.namedArgs(page.name), Track::of)
            is Page.ForArtist -> Held("artist", Phone.namedArgs(page.name), Track::of)
            is Page.ForPlaylist -> Held("playlist", Phone.playlistArgs(page.id), Track::of)
            is Page.Songs -> null
        }

    /** A page's tracks, kept up to date while it is held; [release] it when the screen goes. */
    fun hold(page: Page): StateFlow<List<Track>> {
        if (page is Page.Songs) return tracks
        val o = pages.computeIfAbsent(page) { Opened(pageView(page)!!, MutableStateFlow(emptyList())) }
        synchronized(o) { o.refs++ }
        session.post { p ->
            if (o.held.view == null) {
                hydrate(p, o.held)
                o.flow.value = o.held.rows.toList()
            }
        }
        return o.flow.asStateFlow()
    }

    fun release(page: Page) {
        val o = pages[page] ?: return
        val gone = synchronized(o) { --o.refs <= 0 }
        if (gone) pages.remove(page, o)
    }

    /** A page's tracks once, for playing it from somewhere that does not show it (Home's last played). */
    suspend fun read(page: Page): List<Track> =
        if (page is Page.Songs) tracks.value
        else
            session.run { p ->
                val h = pageView(page)!!
                try {
                    p.query(h.name, h.args).asList().map(Track::of)
                } catch (e: PeerError) {
                    emptyList()
                }
            }

    /** The library's songs matching `needle`, by the module's own text index. */
    suspend fun search(needle: String): List<Track> = session.run { p -> Phone.search(p, needle) }

    /** Which of the person's playlists `track` is on. */
    suspend fun playlistsOf(track: Track): Set<Id> = session.run { p -> Phone.playlistsOf(p, track.id).map { it.id }.toSet() }

    /** What `create_playlist`'s own checks say of a name as it is typed; null when it would pass. */
    suspend fun nameProblem(name: String): String? = session.run { p -> Phone.nameProblem(p, name) }

    /** The album a song is on, by the details. */
    fun albumOf(track: Track): Album? {
        val name = details.value[track.id]?.album?.takeIf { it.isNotEmpty() } ?: return null
        return albums.value.firstOrNull { it.name == name } ?: Album(name, track.creator, "", 0)
    }

    // Writing -----------------------------------------------------------------

    private fun write(f: (Peer) -> Unit) =
        session.post { p ->
            try {
                f(p)
            } catch (e: Exception) {
                noticeFlow.value = e.message ?: e.javaClass.simpleName
            }
            splice(p)
        }

    private fun record(w: Wrote, what: String, refused: String) {
        when (w) {
            is Wrote.Refused -> noticeFlow.value = "$refused: ${w.reason}"
            is Wrote.Entry -> synchronized(changes) { changes.add(0, Authored(w.id, what)) }
        }
    }

    /** Make a playlist, and put `then` on it once it is there. */
    fun createPlaylist(name: String, then: Track? = null) = write { p ->
        val before = playlistsHeld.rows.map { it.id }.toSet()
        record(Phone.createPlaylist(p, name), "make the playlist ${name.trim()}", "Couldn't make it")
        splice(p)
        val made = playlistsHeld.rows.firstOrNull { it.id !in before }
        if (then != null && made != null)
            record(Phone.addToPlaylist(p, made.id, then.id), "add ${then.title} to ${made.name}", "Couldn't add it")
    }

    fun addToPlaylist(track: Track, playlist: Playlist) = write { p ->
        record(Phone.addToPlaylist(p, playlist.id, track.id), "add ${track.title} to ${playlist.name}", "Couldn't add it")
    }

    fun removeFromPlaylist(track: Track, playlist: Id) = write { p ->
        val name = playlistsHeld.rows.firstOrNull { it.id == playlist }?.name ?: "a playlist"
        record(Phone.removeFromPlaylist(p, playlist, track.id), "take ${track.title} off $name", "Couldn't take it off")
    }

    fun dismiss() {
        noticeFlow.value = null
    }

    // Diagnostics -------------------------------------------------------------

    /** Every number the old settings screen had, and every change with where it stands. */
    suspend fun diagnostics(): Diagnostics =
        session.run { p ->
            val mine = synchronized(changes) { changes.toList() }
            Diagnostics(
                status = p.status(),
                changes = mine.map { it.what to standing(p.standing(it.id)) },
                verdict = verdict,
            )
        }

    private fun standing(st: Standing): String =
        when (st) {
            is Standing.Pending -> "not synced yet"
            is Standing.Confirmed -> "saved"
            is Standing.Unknown -> "unknown"
            is Standing.Rejected -> "not saved: ${st.why}"
        }

    /** Ask the server whether it agrees with this phone; the answer arrives on a later pump. */
    fun verify() = write { p -> p.verify() }

    fun signOut() = session.signOut()

    /** Stop listening to the session; closing it is the onboarding's. */
    fun close() {
        session.post { unhook() }
    }
}
