// How the app gets to a budget: the server's address (money.koon.us unless
// somebody types another), its `/healthz` asked whether it runs money's
// domain, the server's own sign-in page in a Custom Tab, the code it sends
// back to `money://auth` exchanged for a login — or no server at all, the
// demo's household in memory. The step is state (`step`), the effects are
// carried out here, and what was decided is remembered on the phone so it is
// asked once: the server, the login, and whether this phone runs the demo.
//
// The machine is plain Kotlin over a [Shelf] and three functions it is
// handed (find a server, exchange a code, open a book), so the JVM tests
// walk it without a network or a phone. `Start` is the ViewModel that gives
// it the real ones and outlives a rotation; the activity only opens the
// browser it asks for and hands it the redirect.
package dev.money.android

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.ffi.Login
import dev.arkdb.ffi.exchange
import dev.arkdb.ffi.loginUrl
import dev.arkdb.ffi.socketUrl
import dev.arkui.SignInPhase
import dev.arkui.server.Found
import dev.arkui.server.Probe
import dev.money.ffi.moneyDemo
import dev.money.ffi.moneyDomain
import dev.money.ffi.moneyOpen
import dev.money.ffi.moneyToday
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** The household's server, unless somebody says another. */
const val DEFAULT_SERVER: String = "money.koon.us"

/** Where the sign-in sends its code back to (the manifest's filter). */
const val SCHEME: String = "money"

/** Where the app is on its way to a budget. */
sealed class Step {
    /** The server's address, as typed; the last check's sentence; whether one is under way. */
    data class Welcome(val typed: String = DEFAULT_SERVER, val error: String? = null, val checking: Boolean = false) : Step()

    data class SignIn(val server: String, val phase: SignInPhase = SignInPhase.Idle, val error: String? = null) : Step()

    /** A budget: the server's, or the demo's. */
    data class Running(val book: Book) : Step()
}

/** A key/value store of strings; `null` removes. */
interface Shelf {
    fun get(key: String): String?

    fun put(key: String, value: String?)
}

class MemoryShelf : Shelf {
    val map: MutableMap<String, String> = LinkedHashMap()

    override fun get(key: String): String? = map[key]

    override fun put(key: String, value: String?) {
        if (value == null) map.remove(key) else map[key] = value
    }
}

/** What the start remembers, so that it is asked once. */
class Remembered(private val shelf: Shelf) {
    var server: String?
        get() = shelf.get("money.server")
        set(v) = shelf.put("money.server", v)

    /** The login, or null — including when what was kept no longer reads as one. */
    var login: Login?
        get() = shelf.get("money.login")?.let { runCatching { Logins.parse(it) }.getOrNull() }
        set(v) = shelf.put("money.login", v?.let(Logins::json))

    var demo: Boolean
        get() = shelf.get("money.demo") == "1"
        set(v) = shelf.put("money.demo", if (v) "1" else null)
}

/** A login as `ark_auth::Login`'s JSON. */
object Logins {
    fun json(l: Login): String =
        JSONObject()
            .put("token", l.token)
            .put("session", l.session)
            .put(
                "user",
                JSONObject()
                    .put("id", l.user)
                    .put("name", l.name)
                    .put("email", l.email)
                    .put("picture", l.picture)
                    .put("roles", JSONArray(l.roles)))
            .put("expires_ms", l.expiresMs)
            .toString()

    fun parse(s: String): Login {
        val o = JSONObject(s)
        val u = o.getJSONObject("user")
        val roles = u.optJSONArray("roles")
        return Login(
            token = o.getString("token"),
            session = o.getString("session"),
            user = u.getString("id"),
            name = u.optString("name"),
            email = u.optString("email"),
            roles = List(roles?.length() ?: 0) { roles!!.getString(it) },
            expiresMs = o.optLong("expires_ms"),
            picture = u.optString("picture"),
        )
    }
}

/** What the machine is handed: the network, the browser and the kernel, as functions. */
class Effects(
    /** The server an address means, asked of its `/healthz` (off the main thread). */
    val find: (typed: String) -> Found,
    /** A code from the sign-in, exchanged for a login (off the main thread). */
    val exchange: (server: String, code: String) -> Login,
    /** The server's book, signed in as `login`. */
    val openServer: (server: String, login: Login) -> Book,
    /** The demo's household. */
    val openDemo: () -> Book,
    /** Open `url` in a browser tab. */
    val browse: (String) -> Unit,
)

/** The start's machine: one step at a time, its effects done through [Effects] in `scope`. */
class Starter(
    private val shelf: Shelf,
    private val fx: Effects,
    private val scope: CoroutineScope,
    private val io: kotlinx.coroutines.CoroutineDispatcher = Dispatchers.IO,
) {
    private val remembered = Remembered(shelf)
    private val _step = MutableStateFlow<Step>(Step.Welcome())
    val step: StateFlow<Step> = _step.asStateFlow()

    /** Where a launch starts: the budget it had, signing in again, or the welcome. */
    fun begin() {
        val server = remembered.server
        val login = remembered.login
        _step.value =
            when {
                remembered.demo -> Step.Running(fx.openDemo())
                server != null && login != null -> Step.Running(fx.openServer(server, login))
                server != null -> Step.SignIn(server)
                else -> Step.Welcome()
            }
    }

    fun type(typed: String) {
        val s = _step.value as? Step.Welcome ?: return
        _step.value = s.copy(typed = typed, error = null)
    }

    /** Ask the typed address whether it is a money server; sign in there if it is. */
    fun check() {
        val s = _step.value as? Step.Welcome ?: return
        if (s.checking) return
        _step.value = s.copy(checking = true, error = null)
        scope.launch {
            val found = withContext(io) { fx.find(s.typed) }
            val now = _step.value as? Step.Welcome ?: return@launch
            _step.value =
                when (found) {
                    is Found.Server -> {
                        remembered.server = found.url
                        Step.SignIn(found.url)
                    }
                    is Found.Failed -> now.copy(checking = false, error = found.why)
                }
        }
    }

    /** No server: the demo's household, until somebody leaves it. */
    fun demo() {
        remembered.demo = true
        _step.value = Step.Running(fx.openDemo())
    }

    fun openSignIn() {
        val s = _step.value as? Step.SignIn ?: return
        _step.value = s.copy(phase = SignInPhase.Browser, error = null)
        fx.browse(loginUrlOf(s.server))
    }

    /** Back from the browser with no code: the person closed it. */
    fun returned() {
        val s = _step.value as? Step.SignIn ?: return
        if (s.phase == SignInPhase.Browser) _step.value = s.copy(phase = SignInPhase.Idle)
    }

    /** The code the server sent back to `money://auth`. */
    fun code(code: String) {
        val s = _step.value as? Step.SignIn ?: return
        if (s.phase == SignInPhase.Exchanging) return
        _step.value = s.copy(phase = SignInPhase.Exchanging, error = null)
        scope.launch {
            val login =
                try {
                    withContext(io) { fx.exchange(s.server, code) }
                } catch (e: Exception) {
                    _step.value = s.copy(phase = SignInPhase.Idle, error = "Signing in didn't finish: ${e.message ?: "no answer"}. Try again.")
                    return@launch
                }
            remembered.login = login
            _step.value =
                try {
                    Step.Running(fx.openServer(s.server, login))
                } catch (e: Exception) {
                    s.copy(phase = SignInPhase.Idle, error = "This phone's copy of the budget couldn't be opened. Sign in to try again.")
                }
        }
    }

    fun changeServer() {
        val s = _step.value as? Step.SignIn ?: return
        _step.value = Step.Welcome(typed = Probe.display(s.server))
    }

    /** Sign out, or leave the demo: the book closed, the login forgotten, the server kept. */
    fun signOut(why: String? = null) {
        val s = _step.value
        if (s is Step.Running) {
            s.book.logout()
            s.book.close()
        }
        val server = remembered.server
        remembered.login = null
        _step.value =
            if (s is Step.Running && s.book.demo) {
                remembered.demo = false
                Step.Welcome()
            } else if (server != null) Step.SignIn(server, error = why)
            else Step.Welcome()
    }

    /** The process is going: the book's peer closed. */
    fun close() {
        (_step.value as? Step.Running)?.book?.close()
    }

    companion object {
        fun loginUrlOf(server: String): String = loginUrl(server, "$SCHEME://auth")
    }
}

/** The app's private `SharedPreferences`, as a [Shelf]. */
class PrefsShelf(context: Context) : Shelf {
    private val prefs = context.getSharedPreferences("money", Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)

    override fun put(key: String, value: String?) {
        prefs.edit().apply { if (value == null) remove(key) else putString(key, value) }.apply()
    }
}

/** The start for the activity: the machine with the real network, browser and kernel. */
class Start(app: Application) : AndroidViewModel(app) {
    private val _browse = MutableStateFlow<String?>(null)

    /** A URL the activity is to open in a Custom Tab, once. */
    val browse: StateFlow<String?> = _browse.asStateFlow()

    val starter: Starter =
        Starter(
            PrefsShelf(app),
            Effects(
                find = { typed -> Probe.find(typed, moneyDomain(), "money", Probe::http) },
                exchange = { server, code -> exchange(server, code) },
                openServer = { server, login -> openServer(app, server, login) },
                openDemo = { Book.open(moneyDemo(null), moneyToday(), demo = true, server = "") },
                browse = { _browse.value = it },
            ),
            viewModelScope,
        )

    init {
        starter.begin()
    }

    fun browsed() {
        _browse.value = null
    }

    override fun onCleared() {
        starter.close()
    }

    private companion object {
        /** The server's replica on this phone: one directory per server, kept across sign-ins. */
        fun openServer(app: Application, server: String, login: Login): Book {
            val dir = File(app.filesDir, "money/server-" + server.replace(Regex("[^A-Za-z0-9]"), "_") + "/replica").apply { mkdirs() }
            val peer = moneyOpen(dir.path, login)
            peer.connect(socketUrl(server))
            return Book.open(peer, moneyToday(), demo = false, server = server)
        }
    }
}
