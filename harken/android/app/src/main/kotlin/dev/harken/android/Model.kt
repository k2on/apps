// The one model: a Kotlin `Peer` over `harken.ark` and what the screens read
// from it. The peer is single-threaded, so it lives on a thread of its own
// (`ark`): the pump every fifty milliseconds, every mutation, every read a
// screen asks for synchronously. The screens see lists that are *spliced*,
// never re-read: `library`, `playlists` and the open `playlist` are
// maintained views (`ClientView`), each handed every `Changes` the peer
// reports, and the patches they answer with are applied to the list the
// screen was given; only a `Reset` — a rebase that replaced the store, a
// sign-in, a selection that moved — reads one again.
//
// The app works with nobody signed in: the peer is opened signed out, what
// is authored is nobody's, kept and written down, and nothing is dialled.
// Signing in (dev auth: a name) makes all of it the signer's and syncs it;
// signing out keeps what that person left pending for when they sign in
// again. Alone, the peer is its own authority over a local history, and
// joins a server when it is told one (Settings).
package dev.harken.android

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.Changes
import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.client.Auth
import dev.arkdb.client.AuthError
import dev.arkdb.client.ClientView
import dev.arkdb.client.DirStorage
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import dev.arkdb.client.PeerError
import dev.arkdb.client.Standing
import dev.arkdb.client.Status
import dev.arkdb.client.Update
import java.io.File
import java.util.concurrent.Executors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext

/**
 * What Settings holds: the server's address (`http://host:port`; the socket is derived from it),
 * whether to work alone, and the login, if somebody signed in — kept whole, so a restart reuses
 * it.
 */
data class Prefs(val server: String, val workAlone: Boolean, val login: Login?) {
    val user: String?
        get() = login?.user?.id

    companion object {
        private const val PREFS = "harken"

        fun load(ctx: Context): Prefs {
            val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            return Prefs(
                server = p.getString("server", "http://10.0.2.2:8787") ?: "",
                workAlone = p.getBoolean("workAlone", false),
                login = p.getString("login", null)?.let { runCatching { Login.parse(it) }.getOrNull() },
            )
        }

        fun save(ctx: Context, s: Prefs) {
            ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit()
                .putString("server", s.server)
                .putBoolean("workAlone", s.workAlone)
                .putString("login", s.login?.toJson())
                .apply()
        }
    }
}

/** One change this phone made, and what to call it beside its standing. */
data class Authored(val id: Id, val what: String)

class Model(app: Application) : AndroidViewModel(app) {
    /** The peer's thread: everything that touches it runs here. */
    private val ark = Executors.newSingleThreadExecutor { r -> Thread(r, "ark-peer") }.asCoroutineDispatcher()

    private val _settings = MutableStateFlow(Prefs.load(app))
    val settings: StateFlow<Prefs> = _settings.asStateFlow()

    private val _library = MutableStateFlow<List<Track>>(emptyList())
    val library: StateFlow<List<Track>> = _library.asStateFlow()

    private val _playlists = MutableStateFlow<List<Playlist>>(emptyList())
    val playlists: StateFlow<List<Playlist>> = _playlists.asStateFlow()

    private val _selected = MutableStateFlow<Id?>(null)
    /** The playlist the Library's per-row button adds to, and the Playlist screen shows. */
    val selected: StateFlow<Id?> = _selected.asStateFlow()

    private val _items = MutableStateFlow<List<Track>>(emptyList())
    val items: StateFlow<List<Track>> = _items.asStateFlow()

    private val _status = MutableStateFlow<Status?>(null)
    val status: StateFlow<Status?> = _status.asStateFlow()

    /** Every change this phone made while it has been running, newest first. */
    private val _changes = MutableStateFlow<List<Authored>>(emptyList())
    val changes: StateFlow<List<Authored>> = _changes.asStateFlow()

    /** Where each of them stands, read again on every pump. */
    private val _standings = MutableStateFlow<Map<Id, Standing>>(emptyMap())
    val standings: StateFlow<Map<Id, Standing>> = _standings.asStateFlow()

    /** The last refusal, rejection or failure, for a snackbar; cleared by `dismissNotice`. */
    private val _notice = MutableStateFlow<String?>(null)
    val notice: StateFlow<String?> = _notice.asStateFlow()

    // Which entry made a playlist, and which last touched a (playlist, item)
    // pair: so a row can say it is not synced yet, or why it was not saved.
    private val madeBy = HashMap<Id, Id>()
    private val touchedBy = HashMap<Pair<Id, Id>, Id>()

    // On the `ark` thread only.
    private var peer: Peer? = null
    private var libraryView: ClientView? = null
    private var playlistsView: ClientView? = null
    private var itemsView: ClientView? = null
    private val libraryRows = ArrayList<Track>()
    private val playlistRows = ArrayList<Playlist>()
    private val itemRows = ArrayList<Track>()
    private var verdictsSeen = 0

    /** harken's module, as the domain crate writes it: in the APK's assets, verified on load. */
    private val domain: Domain by lazy {
        Domain.fromBytes(getApplication<Application>().assets.open("harken.ark").use { it.readBytes() })
    }

    init {
        viewModelScope.launch(ark) {
            open()
            while (isActive) {
                tick()
                delay(50)
            }
        }
    }

    /**
     * Where the replica lives: one directory per server (or alone), not per person — the log is the
     * server's and every person reads it — so that work done signed out is still here when somebody
     * signs in.
     */
    private fun directory(st: Prefs): File {
        val app = getApplication<Application>()
        val place = if (st.workAlone) "alone" else "server-" + st.server.replace(Regex("[^A-Za-z0-9]"), "_")
        return File(app.filesDir, "ark/$place/replica")
    }

    // Open the peer the settings describe, and hydrate the three views.
    private fun open() {
        val st = _settings.value
        val login = st.login
        val opts =
            when {
                st.workAlone -> Options.alone(login?.user?.id ?: "me")
                login != null -> Options.server(login.user.id, login.session, login.token)
                else -> Options.signedOut()
            }
        val p =
            try {
                Peer.open(domain, DirStorage(directory(st)), opts)
            } catch (e: Exception) {
                _notice.value = "could not open the replica: ${e.message}"
                return
            }
        login?.let { p.setRoles(it.user.roles) }
        if (!st.workAlone) p.connect(Auth.socketUrl(st.server))
        peer = p
        p.takeChanges()
        playlistsView = hold(p, "playlists", emptyMap())
        playlistRows.clear()
        playlistsView?.rows?.mapTo(playlistRows, Playlist::of)
        _playlists.value = playlistRows.toList()
        reselect(p, _selected.value)
        _status.value = p.status()
    }

    // A view, or none when its middleware refuses (a playlist that is not
    // this person's): a screen draws an empty list, not an error.
    private fun hold(p: Peer, name: String, args: Map<String, Value>): ClientView? =
        try {
            p.view(name, args)
        } catch (e: PeerError) {
            null
        }

    // The selection moved, or has to: the first playlist when the one held
    // went away. The library is read against it and the playlist view is of
    // it, so both are opened again — a hydrate, once per selection.
    private fun reselect(p: Peer, want: Id?) {
        val sel = want?.takeIf { id -> playlistRows.any { it.id == id } } ?: playlistRows.firstOrNull()?.id
        _selected.value = sel
        libraryView = hold(p, "library", Phone.libraryArgs(sel))
        libraryRows.clear()
        libraryView?.rows?.mapTo(libraryRows, Track::of)
        _library.value = libraryRows.toList()
        itemsView = sel?.let { hold(p, "playlist", Phone.playlistArgs(it)) }
        itemRows.clear()
        itemsView?.rows?.mapTo(itemRows, Track::of)
        _items.value = itemRows.toList()
    }

    // What moved, handed to every view; each list spliced by the patches it
    // answers with, or read again on a reset.
    private fun splice(p: Peer) {
        val ch = p.takeChanges()
        if (ch is Changes.Applied && ch.changes.isEmpty()) return
        val before = _selected.value
        if (apply(p, playlistsView, ch, playlistRows, Playlist::of)) _playlists.value = playlistRows.toList()
        if (before != null && playlistRows.none { it.id == before }) {
            reselect(p, null)
            return
        }
        if (before == null && playlistRows.isNotEmpty()) {
            reselect(p, playlistRows.first().id)
            return
        }
        if (apply(p, libraryView, ch, libraryRows, Track::of)) _library.value = libraryRows.toList()
        if (apply(p, itemsView, ch, itemRows, Track::of)) _items.value = itemRows.toList()
    }

    private fun <T> apply(p: Peer, v: ClientView?, ch: Changes, rows: MutableList<T>, of: (Value) -> T): Boolean {
        val view = v ?: return false
        return when (val u = view.update(p, ch)) {
            is Update.Unchanged -> false
            is Update.Patched -> {
                ClientView.splice(rows, u.patches, of)
                true
            }
            is Update.Reset -> {
                rows.clear()
                view.rows.mapTo(rows, of)
                true
            }
        }
    }

    /** Every pump: what moved, the status, where each change stands, a new verdict's reason. */
    private fun tick() {
        val p = peer ?: return
        val pumped = p.pump()
        pumped.denied?.let { _notice.value = "turned away: $it" }
        splice(p)
        _status.value = p.status()
        val now = _changes.value.associate { it.id to p.standing(it.id) }
        if (now != _standings.value) _standings.value = now
        val rejected = p.takeRejections()
        if (rejected.isNotEmpty()) _notice.value = "not saved: ${rejected.last().reason}"
        val agreed = p.agreed
        if (agreed.size > verdictsSeen) {
            val (n, ok) = agreed.last()
            verdictsSeen = agreed.size
            _notice.value = "at $n: ${if (ok) "agrees" else "DIVERGED"}"
        }
    }

    private fun onPeer(f: (Peer) -> Unit) {
        viewModelScope.launch(ark) {
            val p = peer ?: return@launch
            try {
                f(p)
            } catch (e: Exception) {
                _notice.value = e.message ?: e.javaClass.simpleName
            }
            splice(p)
        }
    }

    // A read a screen needs at once, on the peer's thread.
    private fun <T> now(default: T, f: (Peer) -> T): T =
        runBlocking(ark) { peer?.let { runCatching { f(it) }.getOrNull() } ?: default }

    fun select(playlistId: Id) {
        _selected.value = playlistId
        viewModelScope.launch(ark) { peer?.let { reselect(it, playlistId) } }
    }

    fun playlist(id: Id): Playlist? = _playlists.value.firstOrNull { it.id == id }

    /** Which of the person's playlists a track is on, for the menu's ticks. */
    fun playlistsOf(track: Track): Set<Id> =
        now(emptySet()) { p -> Phone.playlistsOf(p, track.id).map { it.id }.toSet() }

    // Where things stand -------------------------------------------------------

    fun captionForPlaylist(id: Id, standings: Map<Id, Standing>): String? =
        madeBy[id]?.let { standings[it] }?.let(Phone::caption)

    fun captionFor(track: Track, playlist: Id, standings: Map<Id, Standing>): String? =
        touchedBy[playlist to track.id]?.let { standings[it] }?.let(Phone::caption)

    fun captionFor(change: Authored, standings: Map<Id, Standing>): String =
        standings[change.id]?.let(Phone::caption) ?: "saved"

    // Writing ------------------------------------------------------------------

    /** The `create_playlist` input's checks on a name being typed; null when it would pass. */
    fun playlistNameProblem(name: String): String? = now(null) { p -> Phone.nameProblem(p, name) }

    fun createPlaylist(name: String) = onPeer { p -> created(p, Phone.createPlaylist(p, name), name.trim()) }

    // A playlist made: which entry made it, found by the row it added.
    private fun created(p: Peer, w: Wrote, name: String) {
        val before = playlistRows.map { it.id }.toSet()
        record(w, "make the playlist $name", null)
        splice(p)
        if (w is Wrote.Entry) for (pl in playlistRows) if (pl.id !in before) madeBy[pl.id] = w.id
    }

    /** Put a track on a playlist — the given one, the selected one, or for somebody with none, "Favorites", made now. */
    fun addToPlaylist(track: Track, playlistId: Id? = _selected.value) = onPeer { p ->
        val target =
            playlistId
                ?: run {
                    created(p, Phone.createPlaylist(p, Phone.DEFAULT_PLAYLIST), Phone.DEFAULT_PLAYLIST)
                    playlistRows.lastOrNull()?.id
                }
        if (target == null) {
            _notice.value = "no playlist to add to"
            return@onPeer
        }
        val name = playlistRows.firstOrNull { it.id == target }?.name ?: Phone.DEFAULT_PLAYLIST
        record(Phone.addToPlaylist(p, target, track.id), "add ${track.title} to $name", target to track.id)
    }

    fun removeFromPlaylist(playlistId: Id, track: Track) = onPeer { p ->
        val name = playlistRows.firstOrNull { it.id == playlistId }?.name ?: "a playlist"
        record(
            Phone.removeFromPlaylist(p, playlistId, track.id),
            "take ${track.title} off $name",
            playlistId to track.id)
    }

    /** On the playlist it is not on, or off the one it is on. */
    fun toggle(track: Track, playlistId: Id) {
        if (playlistId in playlistsOf(track)) removeFromPlaylist(playlistId, track) else addToPlaylist(track, playlistId)
    }

    private fun record(w: Wrote, what: String, key: Pair<Id, Id>?) {
        when (w) {
            is Wrote.Refused -> _notice.value = "refused: ${w.reason}"
            is Wrote.Entry -> {
                _changes.value = (listOf(Authored(w.id, what)) + _changes.value).take(100)
                if (key != null) touchedBy[key] = w.id
                peer?.let { p -> _standings.value = _standings.value + (w.id to p.standing(w.id)) }
            }
        }
    }

    /** Ask the authority whether it agrees: alone, at once; linked, on a later pump. */
    fun verify() = onPeer { p -> p.verify() }

    fun dismissNotice() {
        _notice.value = null
    }

    // Signing in ---------------------------------------------------------------

    /**
     * Somebody signs in with a name: the server's dev auth, which answers `/auth/login` with a code
     * and no page. A server with a real provider answers with a page for a browser, which this app
     * does not open: it says so instead. Everything done so far as nobody becomes theirs and is
     * pushed.
     */
    fun signIn(name: String) {
        val who = name.trim()
        if (who.isEmpty()) return
        val server = _settings.value.server
        viewModelScope.launch(Dispatchers.IO) {
            val login =
                try {
                    Auth.login(server, who) {
                        throw AuthError(
                            "$server signs people in through a browser; this app signs in by name only (dev auth)")
                    }
                } catch (e: Exception) {
                    _notice.value = "could not sign in: ${e.message}"
                    return@launch
                }
            withContext(ark) {
                val p = peer ?: return@withContext
                try {
                    p.signIn(login.user.id, login.session, login.token)
                    p.setRoles(login.user.roles)
                } catch (e: PeerError) {
                    _notice.value = "could not sign in: ${e.message}"
                    return@withContext
                }
                val next = _settings.value.copy(login = login)
                Prefs.save(getApplication(), next)
                _settings.value = next
                splice(p)
            }
        }
    }

    /** Sign out: the token forgotten and the link stopped; what that person left pending stays theirs. */
    fun signOut() {
        val next = _settings.value.copy(login = null)
        Prefs.save(getApplication(), next)
        _settings.value = next
        onPeer { p -> p.signOut() }
    }

    /** Save and reopen: the old peer is closed and written down first. */
    fun applySettings(s: Prefs) {
        Prefs.save(getApplication(), s)
        _settings.value = s
        viewModelScope.launch(ark) {
            peer?.close()
            peer = null
            open()
        }
    }

    override fun onCleared() {
        runBlocking(ark) {
            peer?.close()
            peer = null
        }
        ark.close()
    }
}
