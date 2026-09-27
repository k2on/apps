// The one model: a `Session` and what the screens read from it. Every call
// into the session happens on the main thread — the pump from a coroutine
// on `viewModelScope`, every mutation from a click — which is the session's
// single-thread contract; the link marshals the socket's callbacks onto it.
package dev.harken.android

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.Id
import dev.arkdb.Value
import dev.arkdb.client.Outcome
import dev.arkdb.client.Session
import dev.arkdb.client.Status
import harken.gen.HarkenGen
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File

/** A row of `track`, typed for a screen. */
data class Track(
    val id: Id,
    val title: String,
    val artist: String,
    val album: String?,
    val durationMs: Long,
    val file: String,
) {
    companion object {
        fun of(v: Value): Track = Track(
            id = v.field("id").asId(),
            title = v.field("title").asText(),
            artist = v.field("artist").asText(),
            album = v.field("album").let { if (it.isNull()) null else it.asText() },
            durationMs = v.field("duration_ms").asInt(),
            file = v.field("file").asText(),
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

/**
 * A row of `playlist_item` joined to its track by the screen, because the
 * two are in different scopes and a query reads one store (harken/README.md).
 * A track that has not arrived is `null`, and drawn as unavailable.
 */
data class PlaylistRow(val playlistId: Id, val trackId: Id, val pos: Long, val track: Track?)

/** What Settings holds: where the server is, who this is, and whether to work alone. */
data class Prefs(val serverUrl: String, val user: String, val workAlone: Boolean) {
    companion object {
        private const val PREFS = "harken"

        fun load(ctx: Context): Prefs {
            val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            return Prefs(
                serverUrl = p.getString("serverUrl", "ws://10.0.2.2:8787/sync") ?: "",
                user = p.getString("user", "alice") ?: "alice",
                workAlone = p.getBoolean("workAlone", false),
            )
        }

        fun save(ctx: Context, s: Prefs) {
            ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString("serverUrl", s.serverUrl)
                .putString("user", s.user)
                .putBoolean("workAlone", s.workAlone)
                .apply()
        }
    }
}

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

    private val _items = MutableStateFlow<List<PlaylistRow>>(emptyList())
    val items: StateFlow<List<PlaylistRow>> = _items.asStateFlow()

    private val _status = MutableStateFlow<Status?>(null)
    val status: StateFlow<Status?> = _status.asStateFlow()

    /** The last refusal or rejection, for a snackbar; cleared by `dismissNotice`. */
    private val _notice = MutableStateFlow<String?>(null)
    val notice: StateFlow<String?> = _notice.asStateFlow()

    private var session: Session? = null

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
                    val rejected = s.takeRejections()
                    if (rejected.isNotEmpty()) _notice.value = rejected.last().second
                    _status.value = s.status
                }
                delay(50)
            }
        }
    }

    private fun open() {
        val app = getApplication<Application>()
        val st = _settings.value
        val module = Session.moduleOfHex(HarkenGen.MODULE_BYTES)
        val safeUser = st.user.replace(Regex("[^A-Za-z0-9_.-]"), "_")
        val dir = File(app.filesDir, "ark/$safeUser/${if (st.workAlone) "alone" else "server"}")
        val s = Session.open(dir, module, st.user, if (st.workAlone) null else st.serverUrl)
        s.onChange { refresh() }
        session = s
        _status.value = s.status
        refresh()
    }

    /** Re-read the three lists from the merged view; called after every change the session reports. */
    private fun refresh() {
        val s = session ?: return
        val lib = s.read { db -> HarkenGen.query("library", db, mapOf()).asList().map { Track.of(it) } }
        val pls = s.read { db -> HarkenGen.query("playlists", db, mapOf()).asList().map { Playlist.of(it) } }
        _library.value = lib
        _playlists.value = pls
        // A playlist the rebase dropped (a duplicate that lost) is no longer selected.
        val sel = _selected.value?.takeIf { id -> pls.any { it.id == id } } ?: pls.firstOrNull()?.id
        _selected.value = sel
        _items.value = if (sel == null) {
            emptyList()
        } else {
            val byId = lib.associateBy { it.id }
            s.read { db -> HarkenGen.query("playlist_items", db, mapOf("playlist_id" to Value.id(sel))).asList() }
                .map { row ->
                    val tid = row.field("track_id").asId()
                    PlaylistRow(row.field("playlist_id").asId(), tid, row.field("pos").asInt(), byId[tid])
                }
        }
    }

    fun select(playlistId: Id) {
        _selected.value = playlistId
        refresh()
    }

    fun playlist(id: Id): Playlist? = _playlists.value.firstOrNull { it.id == id }

    private fun outcome(o: Outcome) {
        if (o is Outcome.Refused) _notice.value = o.reason
    }

    /** Every mutation runs the generated code, through `mutateWith`; the entry is the same one the interpreter would record. */
    fun createPlaylist(name: String) {
        val s = session ?: return
        outcome(s.mutateWith("create_playlist", HarkenGen.createPlaylistArgs(name)) { db -> HarkenGen.createPlaylist(db, ctx, autos, args) })
    }

    fun addToPlaylist(trackId: Id, playlistId: Id? = _selected.value) {
        val s = session ?: return
        val pid = playlistId ?: run {
            _notice.value = "make a playlist first"
            return
        }
        outcome(s.mutateWith("add_to_playlist", HarkenGen.addToPlaylistArgs(pid, trackId)) { db -> HarkenGen.addToPlaylist(db, ctx, autos, args) })
    }

    fun removeFromPlaylist(playlistId: Id, trackId: Id) {
        val s = session ?: return
        outcome(s.mutateWith("remove_from_playlist", HarkenGen.removeFromPlaylistArgs(playlistId, trackId)) { db -> HarkenGen.removeFromPlaylist(db, ctx, autos, args) })
    }

    fun verify() {
        val s = session ?: return
        val now = s.verify()
        if (now.isNotEmpty()) _notice.value = now.joinToString { (sc, n, ok) -> "$sc@$n ${if (ok) "agrees" else "DIVERGED"}" }
    }

    fun dismissNotice() {
        _notice.value = null
    }

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
