// How the app gets to a budget: the sign-in machine every Ark app shares
// (`ark_auth::onboarding`, through the bridge — its `Onboarding`, `Step`,
// `Event` and `Effect`, whose rules are written and tested once, in Rust),
// its effects carried out here: the server's `/healthz` asked whether it
// runs money's domain, the server's own sign-in page in a Custom Tab, the
// code it sends back to `money://auth` exchanged for a login, a join link's
// code redeemed — or no server at all, the demo's household in memory. What
// is remembered is kept on the phone so it is asked once: the server, the
// login (in the secure store), and whether this phone runs the demo.
//
// `Start` is the ViewModel that holds the machine — under the first run's
// pages every Ark app shares (`FirstRunApp`: ark_auth::first_run over
// ark_app::welcome, money's words money_app::welcome's) — and the book it
// opened, and outlives a rotation; the activity only opens the browser it
// asks for and hands it the redirect.
package dev.money.android

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.auth.AppInfo
import dev.arkdb.auth.Effect
import dev.arkdb.auth.Event
import dev.arkdb.auth.Found
import dev.arkdb.auth.Login
import dev.arkdb.auth.Remembered
import dev.arkdb.auth.Said
import dev.arkdb.auth.Step
import dev.arkdb.ffi.deviceLink
import dev.arkdb.ffi.LoginKeep
import dev.arkdb.ffi.OldShelf
import dev.arkdb.ffi.exchange
import dev.arkdb.ffi.redeem
import dev.arkdb.ffi.socketUrl
import dev.arkui.DeviceLinkShown
import dev.arkui.modules.PhoneSecureStore
import dev.arkui.server.Probe
import uniffi.ark_secure_store.SecureStore
import uniffi.money_bridge.FirstRunApp
import uniffi.money_bridge.WelcomeMsg
import uniffi.money_bridge.WelcomePage
import uniffi.money_bridge.moneyDemo
import uniffi.money_bridge.moneyDomain
import uniffi.money_bridge.moneyOpen
import uniffi.money_bridge.moneyToday
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** money, to the sign-in machine. */
val MONEY: AppInfo = AppInfo(name = "Money", scheme = "money", demo = true, what = "your budget")

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

/**
 * What the start remembers: the server and whether this phone runs the demo on the shelf, under the
 * keys it always had, and the login in the secure store (ark-secure-store: the Android keystore),
 * through the bridge's [LoginKeep] (`ark_auth::keep`), which moves a login kept on the shelf under
 * `money.login` there once and deletes it. With no [store], the login lasts this [Kept].
 */
class Kept(private val shelf: Shelf, store: SecureStore?) {
    private val login = LoginKeep(store, OldKeys(shelf), "money.login")

    var remembered: Remembered
        get() =
            Remembered(
                server = shelf.get("money.server"),
                login = login.recall(),
                demo = shelf.get("money.demo") == "1")
        set(r) {
            shelf.put("money.server", r.server)
            login.keep(r.login)
            shelf.put("money.demo", if (r.demo) "1" else null)
        }
}

/** A [Shelf] as where a login used to be kept: read once, then removed. */
class OldKeys(private val shelf: Shelf) : OldShelf {
    override fun get(key: String): String? = shelf.get(key)

    override fun remove(key: String) = shelf.put(key, null)
}

/** The app's private `SharedPreferences`, as a [Shelf]. */
class PrefsShelf(context: Context) : Shelf {
    private val prefs = context.getSharedPreferences("money", Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)

    override fun put(key: String, value: String?) {
        prefs.edit().apply { if (value == null) remove(key) else putString(key, value) }.apply()
    }
}

/** The start for the activity: the machine, its effects carried out, and the book it opened. */
class Start(app: Application) : AndroidViewModel(app) {
    // No code is scanned here yet: ark-camera has no QR decoder on Android.
    private val machine = FirstRunApp(MONEY, scan = false, demo = true)
    private val kept = Kept(PrefsShelf(app), PhoneSecureStore(app))

    private val _step = MutableStateFlow(machine.step())
    val step: StateFlow<Step> = _step.asStateFlow()

    private val _said = MutableStateFlow(machine.said())
    /** What the page says now: its question, what it waits on, its error. */
    val said: StateFlow<Said> = _said.asStateFlow()

    private val _page = MutableStateFlow(machine.page())
    /** The first run's page (generated/WelcomeScreen.kt draws it); none of them once the budget shows. */
    val page: StateFlow<WelcomePage> = _page.asStateFlow()

    /** What the generated first run reads and sends to. */
    val holder = FirstRunHolder(page) { press(it) }

    private val _book = MutableStateFlow<Book?>(null)
    /** The budget open now: the server's, or the demo's. */
    val book: StateFlow<Book?> = _book.asStateFlow()

    private val _browse = MutableStateFlow<String?>(null)
    /** A URL the activity is to open in a Custom Tab, once. */
    val browse: StateFlow<String?> = _browse.asStateFlow()

    init {
        // A screenshot's launch goes past money's pages, as a returning person's would; their
        // route shows them (`money://welcome`).
        if (dev.arkui.Shots.on) machine.skipIntro()
        send(Event.Launched(kept.remembered, null, false))
    }

    /** money's pages from the first, over wherever the app stands: their route. */
    fun showIntro() {
        machine.showIntro()
        moved()
    }

    /** Past money's pages: a link that goes straight where it says. */
    fun skipIntro() {
        machine.skipIntro()
        moved()
    }

    private val _links = MutableStateFlow(0)
    /** Grows as a link for the app itself is kept: the app takes it ([takeLink]) once it shows. */
    val links: StateFlow<Int> = _links.asStateFlow()

    /** A link for the app itself (a page, a launch), kept until the app shows: money's pages put away. */
    fun openLink(url: String) {
        machine.openLink(url)
        moved()
        _links.value += 1
    }

    /** The link kept, once the app itself shows; once. */
    fun takeLink(): String? = machine.takeLink()

    /** An event from a screen, the browser, a link or the app's phase. */
    fun send(event: Event) {
        if (event is Event.Foreground) _book.value?.dialNow()
        val effects = machine.event(event)
        moved()
        for (e in effects) run(e)
    }

    /** A press on the first run's page. */
    fun press(msg: WelcomeMsg) {
        val asked = machine.send(msg)
        moved()
        for (e in asked.effects) run(e)
    }

    private fun moved() {
        _step.value = machine.step()
        _said.value = machine.said()
        _page.value = machine.page()
    }

    fun browsed() {
        _browse.value = null
    }

    private fun run(e: Effect) {
        when (e) {
            is Effect.FindServer ->
                viewModelScope.launch {
                    val found =
                        withContext(Dispatchers.IO) { Probe.find(e.typed, moneyDomain(), "money", Probe::http) }
                    send(Event.Checked(found.machine()))
                }
            is Effect.Remember -> kept.remembered = e.remembered
            is Effect.Browse -> _browse.value = e.url
            is Effect.Exchange -> trade { exchange(e.server, e.code) }
            is Effect.Redeem -> trade { redeem(e.server, e.code) }
            is Effect.OpenSession ->
                try {
                    _book.value?.close()
                    _book.value = openServer(getApplication(), e.server, e.login)
                } catch (x: Exception) {
                    send(Event.OpenFailed(x.message ?: x.javaClass.simpleName))
                }
            is Effect.OpenDemo -> {
                _book.value?.close()
                _book.value = Book.open(moneyDemo(null), moneyToday(), demo = true, server = "")
            }
            is Effect.CloseSession -> {
                _book.value?.close()
                _book.value = null
            }
        }
    }

    // A code traded off the main thread: the login, or the sentence why not.
    private fun trade(f: () -> Login) {
        viewModelScope.launch {
            val answer =
                withContext(Dispatchers.IO) {
                    try {
                        Event.SignedIn(f())
                    } catch (x: Exception) {
                        Event.SignInFailed(x.message ?: "no answer")
                    }
                }
            send(answer)
        }
    }

    /** "Add a Device": the join link another phone signs in with, made as this login; or why not. */
    suspend fun deviceLink(): DeviceLinkShown {
        val r = machine.remembered()
        val server = r.server
        val token = r.login?.token
        if (server == null || token.isNullOrEmpty()) return DeviceLinkShown(null, 0, emptyList(), "Sign in first.")
        return withContext(Dispatchers.IO) {
            try {
                val d = deviceLink(MONEY.scheme, server, token)
                DeviceLinkShown(d.link, d.qr.size.toInt(), d.qr.dark, null)
            } catch (x: Exception) {
                DeviceLinkShown(null, 0, emptyList(), "Couldn't make a code: ${x.message ?: "no answer"}.")
            }
        }
    }

    /** Sign out, or leave the demo: the session revoked first, then the machine told. */
    fun signOut() {
        _book.value?.logout()
        send(Event.SignOut)
    }

    override fun onCleared() {
        _book.value?.close()
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

/** What ArkUI's probe found, as the machine takes it. */
fun dev.arkui.server.Found.machine(): Found =
    when (this) {
        is dev.arkui.server.Found.Server -> Found.Server(url, newer)
        is dev.arkui.server.Found.Failed -> Found.Failed(why)
    }

/** The first run as its generated page reads it: the page, and where a press goes. */
class FirstRunHolder(val page: StateFlow<WelcomePage>, private val press: (WelcomeMsg) -> Unit) {
    fun send(msg: WelcomeMsg) = press(msg)
}
