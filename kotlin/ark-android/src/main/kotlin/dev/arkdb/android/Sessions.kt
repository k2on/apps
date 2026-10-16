// The session, owned by the process rather than by a screen.
//
// An activity is not the only thing that needs the session. A media
// service plays on after the app is swiped away, and a car, a watch or a
// headset binds to that service with no activity at all; each needs the
// same peer — the same replica, the same socket, the same device in
// whatever live state the app keeps — and two peers over one replica would
// be two devices and a race over its files. So the session is the
// process's: one per signed-in server and login, opened by whichever end
// asks first and shared by every end that asks after, and closed when the
// last one lets go ([Hold.release]) — or, for everyone at once, when the
// person signs out or the server turns the login away ([Sessions.close]).
//
// The onboarding's [ArkHost] is one holder, for as long as the activity's
// `ViewModel` lives. A service is another, for as long as it lives — which
// Android ties to its being bound or playing. Neither knows of the other:
// each holds, and lets go.
//
// What opening needs that is the app's own — its module and the device it
// declares — is a [SessionSpec], which both ends can name without a screen.
// [ArkSessions] is the process's [Sessions] over the app's private storage,
// and what the onboarding remembered is there for an end that opens one
// with no onboarding in front of it ([ArkSessions.remembered]).
package dev.arkdb.android

import android.content.Context
import dev.arkdb.DeviceDecl
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * What opening an app's session needs that is the app's own and not a screen's: its module — the
 * `.ark` the peer interprets, read when first asked — and the device it declares at `hello`
 * (`AppIdentity.device`).
 */
public class SessionSpec(
    public val module: () -> ByteArray,
    public val device: DeviceDecl? = null,
)

/** What a holder is told of the session it holds, on whichever thread it happened. */
public interface SessionEvents {
    /** The server turned the login away; the session is closed for everyone after. */
    public fun denied(reason: String) {}

    /** Somebody asked to sign out ([ArkSession.signOut]). */
    public fun signOut() {}

    /** The replica could not be opened; the session is gone. */
    public fun failed(e: Exception) {}

    /** The session was closed for everyone holding it ([Sessions.close]); not said on a release. */
    public fun closed() {}

    public companion object {
        /** Told nothing. */
        public val None: SessionEvents = object : SessionEvents {}
    }
}

/** One reason a session is open: who holds it, and how it is let go. */
public class Hold
internal constructor(
    public val session: ArkSession,
    /** Who: what the diagnostics and the tests call it. */
    public val who: String,
    internal val events: SessionEvents,
    private val owner: Sessions,
) {
    /** Let go: the session closes when nothing else holds it. Once; again is nothing. */
    public fun release() {
        owner.release(this)
    }
}

/**
 * The open sessions of a process, one per server and login, each open while something holds it.
 * `open` makes one, told where to say what happens to it.
 */
public class Sessions(
    private val open: (server: String, login: Login, events: SessionEvents) -> ArkSession,
    /** The server turned a login away: what the onboarding would forget, forgotten here too. */
    private val forget: (Login) -> Unit = {},
) {
    private class Open(val key: String, val login: Login) {
        var session: ArkSession? = null
        val holds = ArrayList<Hold>()
    }

    private val lock = Any()
    private val opened = LinkedHashMap<String, Open>()
    private val _current = MutableStateFlow<ArkSession?>(null)

    /** The session most recently opened and still open; null with none. */
    public val current: StateFlow<ArkSession?> = _current.asStateFlow()

    private fun key(server: String, login: Login): String =
        server + "\u0000" + login.user.id + "\u0000" + login.session

    /**
     * Hold the session for `server` and `login`: the one already open, or one opened now. `who` is
     * a name for the holder, and `events` what it is told.
     */
    public fun hold(
        server: String,
        login: Login,
        who: String,
        events: SessionEvents = SessionEvents.None
    ): Hold =
        synchronized(lock) {
            val k = key(server, login)
            val o = opened[k] ?: Open(k, login).also { opened[k] = it }
            val s = o.session ?: open(server, login, told(o)).also { o.session = it }
            val h = Hold(s, who, events, this)
            o.holds.add(h)
            _current.value = s
            h
        }

    // What the session says, said to every holder — and, for a denial, the login forgotten and
    // the session closed for everyone, as the onboarding would.
    private fun told(o: Open): SessionEvents =
        object : SessionEvents {
            override fun denied(reason: String) {
                forget(o.login)
                for (h in holders(o)) h.events.denied(reason)
                o.session?.let { close(it) }
            }

            override fun signOut() {
                for (h in holders(o)) h.events.signOut()
            }

            override fun failed(e: Exception) {
                val hs =
                    synchronized(lock) {
                        val hs = o.holds.toList()
                        o.holds.clear()
                        drop(o)
                        hs
                    }
                for (h in hs) h.events.failed(e)
            }
        }

    private fun holders(o: Open): List<Hold> = synchronized(lock) { o.holds.toList() }

    // Forget an entry, under the lock; the current session is the newest left.
    private fun drop(o: Open) {
        if (opened[o.key] === o) opened.remove(o.key)
        if (_current.value === o.session) _current.value = opened.values.lastOrNull()?.session
    }

    internal fun release(h: Hold) {
        val closing =
            synchronized(lock) {
                val o = opened.values.firstOrNull { it.holds.contains(h) } ?: return
                o.holds.remove(h)
                if (o.holds.isNotEmpty()) return
                drop(o)
                o.session
            }
        closing?.close()
    }

    /** Close `session` for everyone holding it — signing out, a denial — and tell them. */
    public fun close(session: ArkSession) {
        val told: List<Hold>
        synchronized(lock) {
            val o = opened.values.firstOrNull { it.session === session } ?: return
            told = o.holds.toList()
            o.holds.clear()
            drop(o)
        }
        session.close()
        for (h in told) h.events.closed()
    }

    /** Who holds `session` now, by name: the tests', and the diagnostics'. */
    public fun holders(session: ArkSession): List<String> =
        synchronized(lock) {
            opened.values.firstOrNull { it.session === session }?.holds?.map { it.who }
                ?: emptyList()
        }
}

/**
 * The process's sessions, over the app's private storage: the replica under its files, the login
 * the onboarding remembered, and the module read and verified once for the process.
 */
public object ArkSessions {
    private val lock = Any()
    @Volatile private var made: Sessions? = null
    private var spec: SessionSpec? = null
    private var app: Context? = null
    private var moduleRead: ByteArray? = null
    private var domain: Domain? = null

    /** The process's sessions, made on the first call with the app's [SessionSpec]. */
    public fun of(context: Context, spec: SessionSpec): Sessions {
        made?.let {
            return it
        }
        synchronized(lock) {
            made?.let {
                return it
            }
            val ctx = context.applicationContext ?: context
            this.app = ctx
            this.spec = spec
            val s =
                Sessions(
                    open = { server, login, events -> open(ctx, spec, server, login, events) },
                    forget = { login -> forget(ctx, login) },
                )
            made = s
            return s
        }
    }

    /**
     * The server and login the onboarding remembered, if it has both: what an end with no screen
     * opens.
     */
    public fun remembered(context: Context): Pair<String, Login>? {
        val r = Remembered(PrefsShelf(context.applicationContext ?: context))
        val server = r.server ?: return null
        val login = r.login ?: return null
        return server to login
    }

    /** The module's bytes, read once for the process. */
    internal fun moduleBytes(): ByteArray =
        synchronized(lock) {
            moduleRead
                ?: (spec ?: error("ArkSessions.of was not called")).module().also {
                    moduleRead = it
                }
        }

    /** The module, decoded — and verified, unless these bytes passed before ([ModuleMemory]). */
    public fun domain(): Domain =
        synchronized(lock) {
            domain
                ?: ModuleMemory(PrefsShelf(app ?: error("ArkSessions.of was not called")))
                    .domain(moduleBytes())
                    .also { domain = it }
        }

    private fun open(
        ctx: Context,
        spec: SessionSpec,
        server: String,
        login: Login,
        events: SessionEvents
    ): ArkSession =
        ArkSession.open(
            ::domain,
            ctx.filesDir,
            server,
            login,
            onDenied = events::denied,
            onSignOut = events::signOut,
            onFailed = events::failed,
            // What the app's launch views are bound to: read here, 50 KB, so that the first
            // file is read before anything is decoded.
            module = moduleBytes(),
            device = spec.device,
        )

    // A denial with no onboarding in front of it forgets the login as the onboarding would, so the
    // next end to ask finds nothing to open rather than a login the server has refused.
    private fun forget(ctx: Context, login: Login) {
        val r = Remembered(PrefsShelf(ctx))
        if (r.login?.session == login.session) r.login = null
    }
}
