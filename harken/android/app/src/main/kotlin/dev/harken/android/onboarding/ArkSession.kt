// What the onboarding hands the app: harken's peer — the Rust kernel through
// the bridge (`harken/ffi`, `docs/plan-v5.md` §1.16) — opened over a replica
// at the server it found, signed in as the login it got, and dialling the
// server's `/sync`; and the typed calls over it (`gen/Harken.kt`).
//
// **The peer drives itself.** Its link, its redial and its views are the
// kernel's, on a thread of the kernel's own; when anything moves there — a
// frame, a view patched, the link — it tells this session ([Listener]), and
// the session runs one drain on its own thread: the status published when it
// changed, and every [onChange] listener called with the peer, which is where
// the app takes its views' patches. A peer at rest tells nobody, and nothing
// here runs on a clock.
//
// **Everything the app does to the peer runs on the session's thread**, in
// order — [post] and [run] — so the lists the app keeps beside its views are
// touched by one thread, as before. The peer itself may be called from any
// thread.
//
// **The session exists at once; its peer opens on its thread.** [open]
// returns before the replica is read; what the app posts runs after the open,
// in order, and the app says it has what its first screen draws with
// [ready], which is what the activity's splash waits for. A replica that
// cannot be opened ends the session and says so ([open]'s `onFailed`).
//
// The replica is one directory per server, not per person: the log is the
// server's and every person reads it, and a person signing in again after a
// denial finds their pending intents where they left them.
package dev.harken.android.onboarding

import dev.arkdb.ffi.Listener
import dev.arkdb.ffi.Login
import dev.arkdb.ffi.Refusal
import dev.arkdb.ffi.socketUrl
import dev.harken.android.gen.Device
import dev.harken.android.gen.Harken
import dev.harken.ffi.harkenOpen
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** What a screen may know of the session without asking the peer. */
data class SessionStatus(
    /** The socket is open and `Hello` was said. */
    val linked: Boolean = false,
    /** The link in a word (`connecting`, `waiting`, `online`, …), for a screen that says why it is not linked. */
    val link: String = "connecting",
    /** Changes made here the server has not answered yet. */
    val pending: Int = 0,
    /**
     * The replica holds what the server had, as far as this phone can tell: it is linked — the
     * server answers a `Hello` with its pages at once — or it holds a log already.
     */
    val synced: Boolean = false,
)

class ArkSession
private constructor(
    /** The server's base URL. */
    val server: String,
    /** Who is signed in. */
    val login: Login,
    private val onDenied: (String) -> Unit,
    private val onSignOut: () -> Unit,
    // Every post, run and drain at once on the caller's thread: a test's (`inline`).
    private val inline: Boolean = false,
) {
    /** The replica's directory, when the session opened it: what else this device keeps goes beside it. */
    @Volatile var dir: String? = null
        private set

    private val thread = Executors.newSingleThreadExecutor { r -> Thread(r, "harken-session").apply { isDaemon = true } }
    private val listeners = CopyOnWriteArrayList<(Harken) -> Unit>()
    private val _status = MutableStateFlow(SessionStatus())
    private val _ready = MutableStateFlow(false)
    @Volatile private var closed = false
    @Volatile private var closing = false
    // A drain is asked for and not yet begun: a burst of tells is one.
    private val draining = AtomicBoolean(false)
    // Set on the session's thread by the open; read there only.
    private var harken: Harken? = null
    private var deniedSaid = false

    /** Linked, how much is not synced, and whether it holds what the server had; published when one changes. */
    val status: StateFlow<SessionStatus> = _status.asStateFlow()

    /** The app has what its first screen draws ([ready]): what the activity's splash waits for. */
    val isReady: StateFlow<Boolean> = _ready.asStateFlow()

    /** Say the first screen has what it draws: the splash may go. */
    fun ready() {
        _ready.value = true
    }

    // What an app keeps with the session (`kept`), by key, in the order it was made.
    private val kept = LinkedHashMap<Any, Any>()

    /**
     * What the app keeps as long as the session: made by `make` the first time `key` is asked for,
     * the same one after, whoever asks — a screen, a service — and closed with the session, newest
     * first, when it is [AutoCloseable].
     */
    fun <T : Any> kept(key: Any, make: () -> T): T {
        synchronized(kept) {
            @Suppress("UNCHECKED_CAST")
            (kept[key] as T?)?.let { return it }
            val made = make()
            if (!isClosed) kept[key] = made
            return made
        }
    }

    /** Close and forget what is kept under `key`: the next [kept] makes it again. */
    fun drop(key: Any) {
        val gone = synchronized(kept) { kept.remove(key) }
        (gone as? AutoCloseable)?.close()
    }

    private fun closeKept() {
        val all = synchronized(kept) { kept.values.toList().also { kept.clear() } }
        for (k in all.asReversed()) runCatching { (k as? AutoCloseable)?.close() }
    }

    /** Whether [close] was called. */
    val isClosed: Boolean
        get() = closed || closing

    private fun start(h: Harken) {
        harken = h
        h.peer.setListener(
            object : Listener {
                override fun changed() = drainSoon()
            })
        drain()
    }

    private fun drainSoon() {
        if (closed) return
        if (inline) return drain()
        if (draining.getAndSet(true)) return
        try {
            thread.execute { drain() }
        } catch (e: RejectedExecutionException) {
            // Closed: nothing to drain.
        }
    }

    // On the session's thread: the status if it changed, every listener, a denial said once.
    private fun drain() {
        draining.set(false)
        val h = harken
        if (closed || h == null) return
        val s = h.peer.status()
        val link = if (s.link == "waiting to dial") "waiting" else s.link
        val now = SessionStatus(s.linked, link, s.pending.toInt(), s.linked || s.cursor > 0)
        if (now != _status.value) _status.value = now
        for (l in listeners) runCatching { l(h) }
        val denied = s.denied
        if (denied != null && !deniedSaid) {
            deniedSaid = true
            onDenied(denied)
        }
    }

    /** Called on the session's thread after anything moved, with the peer. Returns what removes it. */
    fun onChange(listener: (Harken) -> Unit): () -> Unit {
        listeners.add(listener)
        return { listeners.remove(listener) }
    }

    /** Run `f` on the session's thread once the peer is open; then a drain, so what it moved is told. */
    fun post(f: (Harken) -> Unit) {
        if (closed) return
        if (inline) {
            harken?.let(f)
            return drain()
        }
        try {
            thread.execute {
                harken?.takeIf { !closed }?.let(f)
                drainSoon()
            }
        } catch (e: RejectedExecutionException) {
            // Closed: nothing to run.
        }
    }

    /** Run `f` on the session's thread and wait for its answer: a read. A closed session never answers. */
    suspend fun <T> run(f: (Harken) -> T): T {
        if (inline && !closed) harken?.let { return f(it) }
        val d = CompletableDeferred<T>()
        try {
            thread.execute {
                val h = harken
                if (closed || h == null) d.cancel(CancellationException("the session is closed"))
                else
                    try {
                        d.complete(f(h))
                    } catch (e: Throwable) {
                        d.completeExceptionally(e)
                    }
            }
        } catch (e: RejectedExecutionException) {
            d.cancel(CancellationException("the session is closed"))
        }
        return d.await()
    }

    /** Sign out: the onboarding forgets the login and closes this session, its replica kept. */
    fun signOut() = onSignOut()

    /** Stop telling and close the peer; its disk stays for the next session. */
    fun close() {
        if (closed || closing) return
        closing = true
        closeKept()
        if (inline) {
            closed = true
            thread.shutdown()
            return
        }
        try {
            thread.execute {
                closed = true
                harken?.peer?.let {
                    it.setListener(null)
                    it.close()
                }
            }
        } catch (e: RejectedExecutionException) {
            closed = true
        }
        thread.shutdown()
    }

    companion object {
        /** The replica's directory for `server` under `root` (the app's files): one per server. */
        fun directory(root: File, server: String): File =
            File(root, "harken/server-" + server.replace(Regex("[^A-Za-z0-9]"), "_") + "/replica")

        /**
         * The session over `server`'s replica under `root`, as `login`, at once; on its thread, first,
         * the peer opened, saying `device` in the listening session, and dialling. If that fails the
         * session closes, nothing posted to it runs, and `onFailed` says why.
         */
        fun open(
            root: File,
            server: String,
            login: Login,
            device: Device,
            onDenied: (String) -> Unit,
            onSignOut: () -> Unit,
            onFailed: (Exception) -> Unit = {},
        ): ArkSession {
            val s = ArkSession(server, login, onDenied, onSignOut)
            s.thread.execute {
                try {
                    val dir = directory(root, server).apply { mkdirs() }
                    s.dir = dir.path
                    val peer = harkenOpen(dir.path, login, device.json())
                    peer.connect(socketUrl(server))
                    s.start(Harken(peer))
                } catch (e: Refusal) {
                    s.failed(onFailed, e)
                } catch (e: Exception) {
                    s.failed(onFailed, e)
                }
            }
            return s
        }

        /** A session over a peer the caller opened — a test's, alone or linked by the caller. */
        fun over(harken: Harken, server: String, login: Login): ArkSession =
            ArkSession(server, login, {}, {}).also { s -> s.thread.execute { s.start(harken) } }

        /**
         * A session with no thread of its own: every post, run and drain happens at once on the
         * thread that asks — a test that drives a screen on one clock. The peer is the caller's to
         * close.
         */
        fun inline(harken: Harken, server: String, login: Login): ArkSession =
            ArkSession(server, login, {}, {}, inline = true).also { it.start(harken) }
    }

    private fun failed(onFailed: (Exception) -> Unit, e: Exception) {
        closed = true
        thread.shutdown()
        onFailed(e)
    }
}
