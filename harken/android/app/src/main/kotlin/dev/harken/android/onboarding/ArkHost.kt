// The onboarding, running: the [Onboarding] machine's step as state, and
// its effects carried out — the probe and the exchange off the main
// thread, the remembered store written, the browser opened, the session
// held and closed. A `ViewModel`, so a configuration change keeps the
// step and the session; the activity is only how a browser, a permission
// dialog and a redirect reach it.
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
import dev.arkdb.ffi.exchange
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
    private var sessions: Sessions? = null
    private var hold: Hold? = null
    private val remembered = Remembered(PrefsShelf(app))

    private val _step = MutableStateFlow<Step>(Step.Server())
    public val step: StateFlow<Step> = _step.asStateFlow()

    private val _session = MutableStateFlow<ArkSession?>(null)
    /** The session, while the app is running. */
    public val session: StateFlow<ArkSession?> = _session.asStateFlow()

    /** A URL the activity is to open in a Custom Tab, once. */
    private val _browse = MutableStateFlow<String?>(null)
    public val browse: StateFlow<String?> = _browse.asStateFlow()

    /** A permission the activity is to ask the platform for, once. */
    private val _request = MutableStateFlow<Permission?>(null)
    public val request: StateFlow<Permission?> = _request.asStateFlow()

    /** Once, with the app's permissions, scheme and device: where the launch starts. */
    public fun begin(identity: AppIdentity, permissions: List<Permission>) {
        if (machine != null) return
        appName = identity.name
        sessions = ArkSessions.of(getApplication(), SessionSpec(identity.device))
        val m = Onboarding(permissions, Build.VERSION.SDK_INT, identity.scheme)
        machine = m
        apply(m.start(remembered.server, remembered.login, remembered.asked))
    }

    private val _ready = MutableStateFlow(false)
    /**
     * Whether the first frame worth showing is there: a step of the onboarding, which draws at
     * once, or — over a session — the app having said its first screen has what it draws
     * ([ArkSession.ready]). What the activity's splash waits for (`holdSplash`).
     */
    public val ready: StateFlow<Boolean> = _ready.asStateFlow()

    /** An event from a screen, a redirect or a dialog. */
    public fun send(event: Event) {
        val m = machine ?: return
        apply(m.next(_step.value, event, remembered.asked))
    }

    /** The activity opened the browser, or asked the platform: forget the request. */
    public fun browsed() {
        _browse.value = null
    }

    public fun requested() {
        _request.value = null
    }

    private fun apply(move: Move) {
        _step.value = move.step
        if (move.step !is Step.Running) _ready.value = true
        for (e in move.effects) run(e)
    }

    private fun run(e: Effect) {
        when (e) {
            is Effect.FindServer ->
                viewModelScope.launch {
                    val found =
                        withContext(Dispatchers.IO) {
                            Probe.find(e.typed, harkenDomain(), appName, Probe::http)
                        }
                    send(Event.Checked(found))
                }
            is Effect.RememberServer -> remembered.server = e.url
            is Effect.Browse -> _browse.value = e.url
            is Effect.Exchange ->
                viewModelScope.launch {
                    val answer =
                        withContext(Dispatchers.IO) {
                            try {
                                Event.SignedIn(exchange(e.server, e.code))
                            } catch (x: Exception) {
                                Event.SignInFailed(
                                    "Signing in didn't finish: ${x.message ?: "no answer"}. Try again.")
                            }
                        }
                    send(answer)
                }
            is Effect.RememberLogin -> remembered.login = e.login
            is Effect.ForgetLogin -> remembered.login = null
            is Effect.Request -> _request.value = e.permission
            is Effect.MarkAsked -> remembered.markAsked(e.key)
            is Effect.OpenSession -> {
                // At once: the session the process has open for this login — a media
                // service's, say — or one opened now, whose peer opens on its own thread
                // while the app's first screen is composed.
                val server = e.server
                val events =
                    object : SessionEvents {
                        override fun denied(reason: String) {
                            viewModelScope.launch { send(Event.Denied(reason)) }
                        }

                        override fun signOut() {
                            viewModelScope.launch { send(Event.SignOut) }
                        }

                        override fun failed(e: Exception) {
                            viewModelScope.launch { failed(server) }
                        }
                    }
                hold?.release()
                val h = sessions!!.hold(e.server, e.login, "app", events)
                hold = h
                val s = h.session
                _session.value = s
                viewModelScope.launch {
                    s.isReady.first { it }
                    _ready.value = true
                }
                // Signed in just now: opening, until the replica has what the server had —
                // or the server cannot be reached, and there is nothing more to wait for.
                viewModelScope.launch {
                    s.status.first { it.synced || it.link == "waiting" }
                    if (_session.value === s) send(Event.Synced)
                }
            }
            is Effect.CloseSession -> closeHeld()
        }
    }

    // A replica that cannot be opened is not a reason to lose the login:
    // signing in again opens it again.
    private fun failed(server: String) {
        closeHeld()
        _step.value =
            Step.SignIn(
                server,
                error =
                    "This phone's copy of the library couldn't be opened. Sign in to try again.")
        _ready.value = true
    }

    // Closed for everyone: signing out, a denial, a replica that cannot be opened.
    private fun closeHeld() {
        _session.value?.let { sessions?.close(it) }
        hold = null
        _session.value = null
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
