// The one model: a `Session` and what the screens read from it. Every call
// into the session happens on the main thread — the pump from a coroutine
// on `viewModelScope`, every mutation from a click — which is the session's
// single-thread contract; the link marshals the socket's callbacks onto it.
//
// The app works with nobody signed in: the session is opened with no user,
// everything is authored as nobody, kept and written down, and nothing is
// said to any server. Signing in makes all of it the signer's and syncs it;
// signing out keeps what that person left pending for when they sign in
// again (the server accepts an older login of the same person).
package dev.harken.android

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.Id
import dev.arkdb.client.ItemState
import dev.arkdb.client.Session
import dev.arkdb.client.Status
import harken.gen.module as harkenDomain
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File

/** What Settings holds: where the server is, whether to work alone, and who is signed in (null: nobody). */
data class Prefs(val serverUrl: String, val workAlone: Boolean, val user: String?) {
    companion object {
        private const val PREFS = "harken"

        fun load(ctx: Context): Prefs {
            val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            return Prefs(
                serverUrl = p.getString("serverUrl", "ws://10.0.2.2:8787/sync") ?: "",
                workAlone = p.getBoolean("workAlone", false),
                user = p.getString("user", null),
            )
        }

        fun save(ctx: Context, s: Prefs) {
            ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString("serverUrl", s.serverUrl)
                .putBoolean("workAlone", s.workAlone)
                .putString("user", s.user)
                .apply()
        }
    }
}

/** One change this phone made, and what to call it beside its standing. */
data class Authored(val id: Id, val what: String)

class Model(app: Application) : AndroidViewModel(app) {
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

    /** Where each of them stands, re-read on every pump. */
    private val _standings = MutableStateFlow<Map<Id, ItemState>>(emptyMap())
    val standings: StateFlow<Map<Id, ItemState>> = _standings.asStateFlow()

    /** The last refusal or rejection, for a snackbar; cleared by `dismissNotice`. */
    private val _notice = MutableStateFlow<String?>(null)
    val notice: StateFlow<String?> = _notice.asStateFlow()

    // Which entry made a playlist, and which last touched a (playlist, item)
    // pair: so a row can say it is not synced yet, or why it was not saved.
    private val madeBy = HashMap<Id, Id>()
    private val touchedBy = HashMap<Pair<Id, Id>, Id>()

    private var session: Session? = null
    /** Since when the link has been up without a break: the default playlist waits for the log to have arrived. */
    private var linkedSince: Long? = null
    private var defaultAsked = false

    /**
     * harken's domain as the printed Kotlin authors it
     * (`domain/gen/kotlin`): emitted once for the module the replica hashes
     * and verifies against, and run natively for everything this phone
     * authors and replays. What it does not carry (`add_song`) arrives
     * from the server.
     */
    private val domain by lazy { harkenDomain() }

    init {
        open()
        viewModelScope.launch {
            while (isActive) {
                val s = session
                if (s != null) {
                    try {
                        s.pump()
                    } catch (t: Throwable) {
                        _notice.value = "pump: ${t.message ?: t.javaClass.simpleName}"
                    }
                    tick(s)
                }
                delay(50)
            }
        }
    }

    /**
     * Where the replica lives: one directory per server (or alone), not per
     * person — the log is the server's and every person reads it — so that
     * work done signed out is still here when somebody signs in.
     */
    private fun directory(st: Prefs): File {
        val app = getApplication<Application>()
        val place = if (st.workAlone) "alone" else "server-" + st.serverUrl.replace(Regex("[^A-Za-z0-9]"), "_")
        return File(app.filesDir, "ark/$place")
    }

    private fun open() {
        val st = _settings.value
        val s = Session.open(directory(st), domain, st.user, if (st.workAlone) null else st.serverUrl)
        s.onChange { refresh() }
        session = s
        linkedSince = null
        defaultAsked = false
        _status.value = s.status
        refresh()
    }

    /** Re-read the lists through the procedures, natively; called after every change the session reports. */
    private fun refresh() {
        val s = session ?: return
        val pls = Phone.playlists(s)
        _playlists.value = pls
        // A playlist the rebase dropped is no longer selected.
        val sel = _selected.value?.takeIf { id -> pls.any { it.id == id } } ?: pls.firstOrNull()?.id
        _selected.value = sel
        _library.value = Phone.library(s, sel)
        _items.value = if (sel == null) emptyList() else Phone.playlist(s, sel)
    }

    /** Every pump: the status, where each change stands, a new rejection's reason, and the default playlist. */
    private fun tick(s: Session) {
        val st = s.status
        _status.value = st
        linkedSince = if (st.linked) linkedSince ?: System.currentTimeMillis() else null
        val now = _changes.value.associate { it.id to s.statusOf(it.id) }
        if (now != _standings.value) _standings.value = now
        val rejected = s.takeRejections()
        if (rejected.isNotEmpty()) _notice.value = "not saved: ${rejected.last().second}"
        makeDefaultIfNone(s, st)
    }

    /**
     * The default playlist, for somebody who has none at all, once the view
     * is known to hold every playlist of theirs: alone at once, signed in
     * after the link has been up two seconds (the log arrives first).
     * Signed out it is made on first use instead (`add`).
     */
    private fun makeDefaultIfNone(s: Session, st: Status) {
        if (defaultAsked) return
        val caughtUp = linkedSince?.let { System.currentTimeMillis() - it >= 2000 } ?: false
        if (!(st.serverless || (s.signedIn && caughtUp))) return
        defaultAsked = true
        Phone.ensureDefault(s, known = true)?.let { recordCreate(s, it, Phone.DEFAULT_PLAYLIST) }
    }

    fun select(playlistId: Id) {
        _selected.value = playlistId
        refresh()
    }

    fun playlist(id: Id): Playlist? = _playlists.value.firstOrNull { it.id == id }

    /** Which of the person's playlists a track is on, for the menu's ticks. */
    fun playlistsOf(track: Track): Set<Id> = session?.let { s -> Phone.playlistsOf(s, track.id).map { it.id }.toSet() } ?: emptySet()

    // Where things stand -------------------------------------------------------

    // Each takes the standings a screen collected, so that it recomposes as they move.

    fun captionForPlaylist(id: Id, standings: Map<Id, ItemState>): String? =
        madeBy[id]?.let { standings[it] }?.let(Phone::caption)

    fun captionFor(track: Track, playlist: Id, standings: Map<Id, ItemState>): String? =
        touchedBy[playlist to track.id]?.let { standings[it] }?.let(Phone::caption)

    fun captionFor(change: Authored, standings: Map<Id, ItemState>): String = standings[change.id]?.let(Phone::caption) ?: "saved"

    // Writing ------------------------------------------------------------------

    /** The `create_playlist` input's checks on a name being typed; null when it would pass. */
    fun playlistNameProblem(name: String): String? = session?.let { Phone.nameProblem(it, name) }

    fun createPlaylist(name: String) {
        val s = session ?: return
        recordCreate(s, Phone.createPlaylist(s, name), name.trim())
    }

    /** Put a track on a playlist — the given one, the selected one, or for somebody with none, the default, made now. */
    fun addToPlaylist(track: Track, playlistId: Id? = _selected.value) {
        val s = session ?: return
        val target = playlistId ?: run {
            val (t, why) = Phone.playlistForAdding(s)
            if (t == null) {
                _notice.value = "refused: $why"
                return
            }
            t.made?.let {
                record(Wrote.Entry(it), "make the playlist ${Phone.DEFAULT_PLAYLIST}", null)
                madeBy[t.playlist] = it
            }
            t.playlist
        }
        val name = playlist(target)?.name ?: Phone.DEFAULT_PLAYLIST
        record(Phone.addToPlaylist(s, target, track.id), "add ${track.title} to $name", target to track.id)
    }

    fun removeFromPlaylist(playlistId: Id, track: Track) {
        val s = session ?: return
        val name = playlist(playlistId)?.name ?: "a playlist"
        record(Phone.removeFromPlaylist(s, playlistId, track.id), "take ${track.title} off $name", playlistId to track.id)
    }

    /** On the playlist it is not on, or off the one it is on. */
    fun toggle(track: Track, playlistId: Id) {
        if (playlistId in playlistsOf(track)) removeFromPlaylist(playlistId, track) else addToPlaylist(track, playlistId)
    }

    private fun recordCreate(s: Session, w: Wrote, name: String) {
        val before = _playlists.value.map { it.id }.toSet()
        record(w, "make the playlist $name", null)
        if (w is Wrote.Entry) for (p in Phone.playlists(s)) if (p.id !in before) madeBy[p.id] = w.id
    }

    private fun record(w: Wrote, what: String, key: Pair<Id, Id>?) {
        when (w) {
            is Wrote.Refused -> _notice.value = "refused: ${w.reason}"
            is Wrote.Entry -> {
                _changes.value = (listOf(Authored(w.id, what)) + _changes.value).take(100)
                if (key != null) touchedBy[key] = w.id
                session?.let { s -> _standings.value = _standings.value + (w.id to s.statusOf(w.id)) }
            }
        }
        refresh()
    }

    fun verify() {
        val s = session ?: return
        val now = s.verify()
        if (now != null) _notice.value = "at ${now.first}: ${if (now.second) "agrees" else "DIVERGED"}"
    }

    fun dismissNotice() {
        _notice.value = null
    }

    // Signing in ---------------------------------------------------------------

    /**
     * Somebody signs in (dev auth: the name is the login's token, and the
     * server calls every login "dev"). Everything done so far as nobody
     * becomes theirs and is pushed.
     */
    fun signIn(name: String) {
        val who = name.trim()
        val s = session ?: return
        if (who.isEmpty()) return
        s.signIn(who)
        val next = _settings.value.copy(user = who)
        Prefs.save(getApplication(), next)
        _settings.value = next
        defaultAsked = false
        refresh()
    }

    /** Sign out: reopen with nobody signed in, over the same replica. What that person left pending stays theirs. */
    fun signOut() = applySettings(_settings.value.copy(user = null))

    /** Save and reopen: the old session is closed and written down first. */
    fun applySettings(s: Prefs) {
        Prefs.save(getApplication(), s)
        _settings.value = s
        session?.close()
        session = null
        _selected.value = null
        open()
    }

    override fun onCleared() {
        session?.close()
        session = null
    }
}
