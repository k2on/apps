// The onboarding, running: the sign-in machine (`ark_auth::onboarding`)
// held, its step as state and its effects carried out — the probe and the
// exchange off the main thread, the remembered store written, the browser
// opened, the session held and closed — and the permissions an app asks
// for, once, over the session opening beneath them. A `ViewModel`, so a
// configuration change keeps the step and the session; the activity is
// only how a browser, a permission dialog and a redirect reach it.
//
// **It holds the session; the process owns it** (Sessions.kt). Opening is
// a [Hold] on the process's session for the login, which a media service
// may already have open — then the app attaches to it as it is, its
// screens' state already read — and the activity going is letting go of
// that hold, not closing the session: whatever else holds it (a player
// playing, a car bound to it) keeps it, its socket and its place in any
// live state. Signing out and a denial close it for everyone.
package dev.harken.android.onboarding

import android.app.Application
import android.content.Context
import android.os.Build
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.auth.AppInfo
import dev.arkdb.auth.Effect
import dev.arkdb.auth.Event
import dev.arkdb.auth.Found
import dev.arkdb.auth.Login
import dev.arkdb.auth.Onboarding
import dev.arkdb.auth.Said
import dev.arkdb.auth.Step
import dev.arkdb.ffi.exchange
import dev.arkdb.ffi.redeem
import dev.arkui.server.Probe
import dev.harken.ffi.harkenDomain
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The app's private `SharedPreferences`, as a [Shelf]. */
public class PrefsShelf(context: Context, name: String = "ark") : Shelf {
    private val prefs = context.getSharedPreferences(name, Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)

    override fun put(key: String, value: String?) {
        prefs.edit().apply { if (value == null) remove(key) else putString(key, value) }.apply()
    }
}

public class ArkHost(app: Application) : AndroidViewModel(app) {
    private var machine: Onboarding? = null
    private var appName: String = ""
    private var permissions: List<Permission> = emptyList()
    private var sessions: Sessions? = null
    private var hold: Hold? = null
    private val kept = Kept(PrefsShelf(app))

    private val _step = MutableStateFlow<Step>(Step.Server("", false, null))
    public val step: StateFlow<Step> = _step.asStateFlow()

    private val _said = MutableStateFlow(Said(null, null, null))
    /** What the page says now: its question, what it waits on, its error. */
    public val said: StateFlow<Said> = _said.asStateFlow()

    private val _session = MutableStateFlow<ArkSession?>(null)
    /** The session, while the app is running. */
    public val session: StateFlow<ArkSession?> = _session.asStateFlow()

    /** A URL the activity is to open in a Custom Tab, once. */
    private val _browse = MutableStateFlow<String?>(null)
    public val browse: StateFlow<String?> = _browse.asStateFlow()

    private val _ask = MutableStateFlow<Permission?>(null)
    /** The permission to ask before the app, while one is due: asked once, whatever the answer. */
    public val ask: StateFlow<Permission?> = _ask.asStateFlow()

    /** A permission the activity is to ask the platform for, once. */
    private val _request = MutableStateFlow<Permission?>(null)
    public val request: StateFlow<Permission?> = _request.asStateFlow()

    /** Once, with the app's permissions, scheme and device: where the launch starts. */
    public fun begin(identity: AppIdentity, permissions: List<Permission>) {
        if (machine != null) return
        appName = identity.name
        this.permissions = permissions
        sessions = ArkSessions.of(getApplication(), SessionSpec(identity.device))
        machine = Onboarding(AppInfo(identity.name, identity.scheme, false, "your library"))
        send(Event.Launched(kept.remembered, null, false))
    }

    private val _ready = MutableStateFlow(false)
    /**
     * Whether the first frame worth showing is there: a step of the onboarding, which draws at
     * once, or — over a session — the app having said its first screen has what it draws
     * ([ArkSession.ready]). What the activity's splash waits for (`holdSplash`).
     */
    public val ready: StateFlow<Boolean> = _ready.asStateFlow()

    /** An event from a screen, a redirect or the app's phase. */
    public fun send(event: Event) {
        val m = machine ?: return
        if (event is Event.Foreground) _session.value?.dialNow()
        val effects = m.next(event)
        val step = m.step()
        _step.value = step
        _said.value = m.said()
        if (step !is Step.Running) _ready.value = true
        for (e in effects) run(e)
    }

    /** The activity opened the browser, or asked the platform: forget the request. */
    public fun browsed() {
        _browse.value = null
    }

    public fun requested() {
        _request.value = null
    }

    /** "Allow" on a permission: the platform's dialog. */
    public fun allow() {
        _request.value = _ask.value
    }

    /** The permission answered — by the platform's dialog, or "Not now": the next, or the app. */
    public fun answered() {
        _ask.value?.let { kept.markAsked(it.key) }
        _ask.value = pending(permissions, Build.VERSION.SDK_INT, kept.asked).firstOrNull()
    }

    private fun run(e: Effect) {
        when (e) {
            is Effect.FindServer ->
                viewModelScope.launch {
                    val found =
                        withContext(Dispatchers.IO) {
                            Probe.find(e.typed, harkenDomain(), appName, Probe::http)
                        }
                    send(Event.Checked(found.machine()))
                }
            is Effect.Remember -> kept.remembered = e.remembered
            is Effect.Browse -> _browse.value = e.url
            is Effect.Exchange -> trade { exchange(e.server, e.code) }
            is Effect.Redeem -> trade { redeem(e.server, e.code) }
            is Effect.OpenSession -> open(e.server, e.login)
            is Effect.OpenDemo -> {}
            is Effect.CloseSession -> closeHeld()
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

    // At once: the session the process has open for this login — a media service's, say — or
    // one opened now, whose peer opens on its own thread while the app's first screen is
    // composed. Any permission not yet asked is asked over it.
    private fun open(server: String, login: Login) {
        val events =
            object : SessionEvents {
                override fun denied(reason: String) {
                    viewModelScope.launch { send(Event.Denied(reason)) }
                }

                override fun signOut() {
                    viewModelScope.launch { send(Event.SignOut) }
                }

                override fun failed(e: Exception) {
                    viewModelScope.launch { send(Event.OpenFailed(e.message ?: e.javaClass.simpleName)) }
                }
            }
        hold?.release()
        val h = sessions!!.hold(server, login, "app", events)
        hold = h
        val s = h.session
        _session.value = s
        _ask.value = pending(permissions, Build.VERSION.SDK_INT, kept.asked).firstOrNull()
        viewModelScope.launch {
            s.isReady.first { it }
            _ready.value = true
        }
        // Signed in just now: opening, until the replica has what the server had — or the
        // server cannot be reached, and there is nothing more to wait for.
        viewModelScope.launch {
            s.status.first { it.synced || it.link == "waiting" }
            if (_session.value === s) send(Event.Synced)
        }
    }

    // Closed for everyone: signing out, a denial, a replica that cannot be opened.
    private fun closeHeld() {
        _session.value?.let { sessions?.close(it) }
        hold = null
        _session.value = null
        _ask.value = null
    }

    /**
     * The activity is gone for good: this hold let go. The session closes unless something else
     * holds it — a player playing, a car bound to the player — which keeps it, its socket and its
     * place in the app's live state, and the next launch attaches to it as it is.
     */
    override fun onCleared() {
        hold?.release()
        hold = null
    }
}

/** What ArkUI's probe found, as the machine takes it. */
internal fun dev.arkui.server.Found.machine(): Found =
    when (this) {
        is dev.arkui.server.Found.Server -> Found.Server(url, newer)
        is dev.arkui.server.Found.Failed -> Found.Failed(why)
    }
