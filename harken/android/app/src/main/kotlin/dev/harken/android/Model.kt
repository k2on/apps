// The one model: harken's lists over the session the onboarding opened
// (`kotlin/ark-android`'s `ArkSession`) — the Kotlin `Peer` over
// `harken.ark`, signed in, linked to the server and pumped on a thread of
// its own. Everything that touches the peer runs on that thread: every
// mutation, every read a screen asks for, and the splice after each pump.
// The screens see lists that are *spliced*, never re-read: `library`,
// `playlists` and the open `playlist` are maintained views (`ClientView`),
// each handed every `Changes` the peer reports, and the patches they answer
// with are applied to the list the screen was given; only a `Reset` — a
// rebase that replaced the store, a selection that moved — reads one again.
//
// There is no alone mode and no signed-out mode: harken is used with a
// server, and a session exists only once somebody has signed in to one.
package dev.harken.android

import dev.arkdb.Changes
import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.android.ArkSession
import dev.arkdb.client.ClientView
import dev.arkdb.client.Peer
import dev.arkdb.client.PeerError
import dev.arkdb.client.Standing
import dev.arkdb.client.Status
import dev.arkdb.client.Update
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.runBlocking

/** One change this phone made, and what to call it beside its standing. */
data class Authored(val id: Id, val what: String)

class Model(private val session: ArkSession) {
    /** On the main thread: what the screens launch. */
    private val scope = MainScope()

    /** Who is signed in. */
    val user: String = session.login.user.id

    /** The server, as the onboarding found it. */
    val server: String = session.server

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

    // On the peer's thread only.
    private var peer: Peer? = null
    private var libraryView: ClientView? = null
    private var playlistsView: ClientView? = null
    private var itemsView: ClientView? = null
    private val libraryRows = ArrayList<Track>()
    private val playlistRows = ArrayList<Playlist>()
    private val itemRows = ArrayList<Track>()
    private var verdictsSeen = 0

    private var unhook: () -> Unit = {}

    init {
        session.post { p ->
            open(p)
            unhook = session.onPump { tick(it) }
        }
    }

    // Hydrate the three views, once, on the peer's thread.
    private fun open(p: Peer) {
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
    private fun tick(p: Peer) {
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
        session.post { p ->
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
        runBlocking { runCatching { session.run(f) }.getOrNull() ?: default }

    fun select(playlistId: Id) {
        _selected.value = playlistId
        session.post { reselect(it, playlistId) }
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

    /** Ask the authority whether it agrees: its answer arrives on a later pump. */
    fun verify() = onPeer { p -> p.verify() }

    fun dismissNotice() {
        _notice.value = null
    }

    /** Sign out: the onboarding takes it from here, keeping the server and this phone's copy. */
    fun signOut() = session.signOut()

    /** Stop listening to the session; the session itself is the onboarding's to close. */
    fun close() {
        unhook()
        scope.cancel()
    }
}
