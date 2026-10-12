// The onboarding, running: the [Onboarding] machine's step as state, and
// its effects carried out — the probe and the exchange off the main
// thread, the remembered store written, the browser opened, the session
// opened and closed. A `ViewModel`, so a configuration change keeps the
// step and the session; the activity is only how a browser, a permission
// dialog and a redirect reach it.
package dev.arkdb.android

import android.app.Application
import android.content.Context
import android.os.Build
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.arkdb.client.Auth
import dev.arkdb.client.Domain
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
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
    private var domain: Domain? = null
    private var module: () -> ByteArray = { ByteArray(0) }
    private var appName: String = ""
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

    /** Once, with the app's permissions, scheme and module: where the launch starts. */
    public fun begin(identity: AppIdentity, permissions: List<Permission>) {
        if (machine != null) return
        appName = identity.name
        module = identity.module
        val m = Onboarding(permissions, Build.VERSION.SDK_INT, identity.scheme)
        machine = m
        apply(m.start(remembered.server, remembered.login, remembered.asked))
    }

    // The app's module, read and verified once, off the main thread.
    private suspend fun domain(): Domain =
        domain ?: withContext(Dispatchers.IO) { Domain.fromBytes(module()) }.also { domain = it }

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
        for (e in move.effects) run(e)
    }

    private fun run(e: Effect) {
        when (e) {
            is Effect.FindServer ->
                viewModelScope.launch {
                    val hash = domain().hash
                    val found =
                        withContext(Dispatchers.IO) {
                            Probe.find(e.typed, hash, appName, Probe::http)
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
                                Event.SignedIn(Auth.exchange(e.server, e.code))
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
                viewModelScope.launch {
                    val d = domain()
                    val s =
                        withContext(Dispatchers.IO) {
                            try {
                                ArkSession.open(
                                    d,
                                    getApplication<Application>().filesDir,
                                    e.server,
                                    e.login,
                                    onDenied = { why ->
                                        viewModelScope.launch { send(Event.Denied(why)) }
                                    },
                                    onSignOut = { viewModelScope.launch { send(Event.SignOut) } },
                                )
                            } catch (x: Exception) {
                                null
                            }
                        }
                    if (s == null) {
                        // A replica that cannot be opened is not a reason to lose the login:
                        // signing in again opens it again.
                        _step.value =
                            Step.SignIn(
                                e.server,
                                error =
                                    "This phone's copy of the library couldn't be opened. Sign in to try again.")
                    } else _session.value = s
                }
            }
            is Effect.CloseSession -> {
                _session.value?.close()
                _session.value = null
            }
        }
    }

    override fun onCleared() {
        _session.value?.close()
    }
}
