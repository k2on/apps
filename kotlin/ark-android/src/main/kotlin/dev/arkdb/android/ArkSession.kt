// What the onboarding hands an app: the Kotlin `Peer`, opened over a
// replica at the server it found and signed in as the login it got, linked
// to the server's `/sync`, on a thread of its own.
//
// A peer is single-threaded, so everything that touches it runs on that
// thread: the pump, and whatever the app hands [post] or [run]. **The thread
// sleeps until something happens** (`docs/plan-reactive.md`): it drains —
// one pump — when the socket wakes it (opened, a frame or a burst of them,
// closed), after a [post], and at the one deadline the peer asks for, the
// redial after a close, which is one one-shot timer replaced when it moves
// and cancelled when the link opens. Connected and idle, it has no timer and
// runs nothing; the keepalive is OkHttp's pings, which wake nobody. After a
// drain that moved something every [onChange] listener is called there,
// with the peer and what moved — which is where an app takes its changes and
// splices its views; a drain that moved nothing tells nobody. What a screen
// may ask without the thread is [status]: linked, how much is not synced,
// and whether the replica holds what the server had, published when it
// changes by value. A `Denied` from the server is the onboarding's, not the
// app's: the session stops and says so to its host, which goes back to
// *Sign in*.
//
// **The session exists at once; its peer opens on its thread.** [open]
// returns before anything is read: the module is decoded (and verified,
// unless these bytes passed before), the replica loaded and what was
// pending replayed as the thread's first task, and everything the app
// posts runs after it, in order. So the app's first screen is composed
// while the peer opens rather than after, and says it has what it draws
// with [ready] — which is what the activity's splash waits for. A replica
// that cannot be opened ends the session and says so ([open]'s `onFailed`).
//
// The replica is one directory per server, not per person: the log is the
// server's and every person reads it, and a person signing in again after
// a denial finds their pending intents where they left them.
package dev.arkdb.android

import dev.arkdb.DeviceDecl
import dev.arkdb.Value
import dev.arkdb.client.Auth
import dev.arkdb.client.DirStorage
import dev.arkdb.client.Domain
import dev.arkdb.client.LaunchBinding
import dev.arkdb.client.LaunchView
import dev.arkdb.client.LaunchViews
import dev.arkdb.client.Link
import dev.arkdb.client.Login
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import dev.arkdb.client.Pumped
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** What a screen may know of the session without asking the peer. */
public data class SessionStatus(
    /** The socket is open and `Hello` was said. */
    val linked: Boolean = false,
    /**
     * The link in a word (`connecting`, `waiting`, `open`, …), for a screen that says why it is not
     * linked.
     */
    val link: String = "connecting",
    /** Changes made here the server has not answered yet. */
    val pending: Int = 0,
    /**
     * The replica holds what the server had: it confirmed something before and is not partway
     * through a page, or the server has served this connection to the end. False on a first sign-in
     * until the server's snapshot has arrived.
     */
    val synced: Boolean = false,
)

public class ArkSession
private constructor(
    /** The server's base URL. */
    public val server: String,
    /** Who is signed in. */
    public val login: Login,
    private val onDenied: (String) -> Unit,
    private val onSignOut: () -> Unit,
    // Every post run at once on the caller's thread, and no pump: a test's (`inline`).
    private val inline: Boolean = false,
    // The account's launch views, when the session was opened with its module's bytes.
    private val launches: LaunchViews? = null,
) {
    // Reads and writes launch views: off the UI thread, and off the peer's, which is busy
    // opening while the first are read.
    private val launcher =
        Executors.newSingleThreadScheduledExecutor { r ->
            Thread(r, "ark-launch").apply { isDaemon = true }
        }
    private val launchViews = CopyOnWriteArrayList<LaunchView>()
    private var saving: ScheduledFuture<*>? = null

    private val thread =
        Executors.newSingleThreadScheduledExecutor { r ->
            Thread(r, "ark-peer").apply { isDaemon = true }
        }
    private val listeners = CopyOnWriteArrayList<(Peer, Pumped) -> Unit>()
    private val _status = MutableStateFlow(SessionStatus())
    private val _ready = MutableStateFlow(false)
    @Volatile private var closed = false

    // A drain is asked for and not yet begun: a burst of wakes and posts is one.
    private val draining = AtomicBoolean(false)
    // The one timer, and the monotonic millisecond it is for; on the peer's thread only.
    private var due: ScheduledFuture<*>? = null
    @Volatile private var dueAt: Long? = null

    /** Drains run (pumps), for the tests and the measurements that count them. */
    internal val drains = AtomicInteger()

    /** Drains that moved something, and so told the listeners. */
    internal val changes = AtomicInteger()

    /** One-shot timers armed for a deadline. */
    internal val armed = AtomicInteger()

    /** Whether a timer is armed now: a link waiting to redial, and nothing else. */
    internal val timed: Boolean
        get() = dueAt != null

    // Set on the peer's thread, by its first task; read there only.
    private var peer: Peer? = null

    /**
     * Linked, how much is not synced, and whether it holds what the server had; published when one
     * of them changes, and only then.
     */
    public val status: StateFlow<SessionStatus> = _status.asStateFlow()

    /**
     * The app has what its first screen draws ([ready] was called): what the activity's splash
     * waits for (`holdSplash`).
     */
    public val isReady: StateFlow<Boolean> = _ready.asStateFlow()

    /** Say the first screen has what it draws: the splash may go. Once is enough; more is no-op. */
    public fun ready() {
        _ready.value = true
    }

    /**
     * A launch view of the query `name` with `args`, kept under `key`
     * (`dev.arkdb.client.LaunchView`): its file read now, on a thread of its own and not behind the
     * peer's open, and handed to `onProvisional` if it is there, whole and bound to this module and
     * this login — then, once the peer is open, the app's [LaunchView.live] over it says what the
     * live view makes of what was shown. Null when this session keeps none.
     */
    public fun launchView(
        key: String,
        name: String,
        args: Map<String, Value>,
        onProvisional: (List<Value>) -> Unit,
    ): LaunchView? {
        val ls = launches ?: return null
        val v = ls.view(key, name, args)
        launchViews.add(v)
        try {
            launcher.execute { v.provisional()?.let(onProvisional) }
        } catch (e: RejectedExecutionException) {
            // Closed: nothing to read.
        }
        return v
    }

    /** A launch view's rows moved: write them all a beat after the last such move. */
    public fun launchChanged() {
        if (launches == null || closed) return
        synchronized(launchViews) {
            saving?.cancel(false)
            saving =
                try {
                    launcher.schedule({ saveLaunchViews() }, SAVE_AFTER_MS, TimeUnit.MILLISECONDS)
                } catch (e: RejectedExecutionException) {
                    null
                }
        }
    }

    /**
     * Write every launch view now: their rows and where the replica stands taken on the peer's
     * thread, written on the launch views' — what the app does when it goes to the background, the
     * write that matters most. `done` is called after, on that thread.
     */
    public fun saveLaunchViews(done: () -> Unit = {}) {
        if (launches == null || launchViews.isEmpty()) return done()
        post { p ->
            val taken = launchViews.mapNotNull { v -> v.capture(p)?.let { v to it } }
            try {
                launcher.execute {
                    for ((v, c) in taken) try {
                        v.write(c)
                    } catch (e: Exception) {
                        // A write that failed leaves the last one, or none: the next launch
                        // takes the ordinary path for that view.
                    }
                    done()
                }
            } catch (e: RejectedExecutionException) {
                done()
            }
        }
    }

    // What an app keeps with the session (`kept`), by key, in the order it was made.
    private val kept = LinkedHashMap<Any, Any>()

    /**
     * What the app keeps as long as the session: made by `make` the first time `key` is asked for,
     * and the same one after, whoever asks — a screen, a service — and closed with the session,
     * newest first, when it is [AutoCloseable]. A session already closed makes one and keeps
     * nothing.
     */
    public fun <T : Any> kept(key: Any, make: () -> T): T {
        synchronized(kept) {
            @Suppress("UNCHECKED_CAST")
            (kept[key] as T?)?.let {
                return it
            }
            val made = make()
            if (!isClosed) kept[key] = made
            return made
        }
    }

    /** Close and forget what is kept under `key`: the next [kept] makes it again. */
    public fun drop(key: Any) {
        val gone = synchronized(kept) { kept.remove(key) }
        (gone as? AutoCloseable)?.close()
    }

    // Everything kept, closed newest first, before the peer's thread is told to stop: what they
    // post on their way out still runs.
    private fun closeKept() {
        val all = synchronized(kept) { kept.values.toList().also { kept.clear() } }
        for (k in all.asReversed()) {
            try {
                (k as? AutoCloseable)?.close()
            } catch (e: Exception) {
                // Its own failure; the session closes regardless.
            }
        }
    }

    /** Whether [close] was called. */
    public val isClosed: Boolean
        get() = closed || closing

    @Volatile private var closing = false

    /** Forget this account's launch views: on signing out, or a denial. */
    public fun forgetLaunchViews() {
        launches?.clear()
    }

    // The peer is open: the socket's wakes ask for a drain, and the first
    // drain dials.
    private fun start(p: Peer) {
        peer = p
        p.setWaker { drainSoon() }
        drain()
    }

    // Ask the peer's thread for one drain, unless one is already asked for
    // and not begun: what the socket's wake and every post come to.
    private fun drainSoon() {
        if (closed || inline || draining.getAndSet(true)) return
        try {
            thread.execute { drain() }
        } catch (e: RejectedExecutionException) {
            // Closed: nothing to drain.
        }
    }

    // One pump, on the peer's thread, and whatever it moved told: the
    // status if it changed, the listeners if anything did, the deadline
    // re-armed if it moved.
    private fun drain() {
        draining.set(false)
        val peer = this.peer
        if (closed || peer == null) return
        drains.incrementAndGet()
        val pumped =
            try {
                peer.pump()
            } catch (e: Exception) {
                null
            }
        if (pumped != null && !pumped.quiet()) {
            changes.incrementAndGet()
            val s = peer.status()
            val c = peer.client
            val synced = (c.served && !c.more) || (s.cursor > 0 && !c.more)
            val now = SessionStatus(linked = s.linked, link = s.link, pending = s.pending, synced)
            if (now != _status.value) _status.value = now
            for (l in listeners) {
                try {
                    l(peer, pumped)
                } catch (e: Exception) {
                    // A listener's failure is the listener's: the session goes on.
                }
            }
        }
        arm(peer.deadline())
        val denied = pumped?.denied
        if (denied != null) onDenied(denied)
    }

    // The one timer: armed for `at` when it moves, cancelled when there is
    // none — which is the link dialling or open.
    private fun arm(at: Long?) {
        if (at == dueAt) return
        due?.cancel(false)
        due = null
        dueAt = at
        if (at == null || closed) return
        val wait = maxOf(at - Link.monotonicMs(), 0)
        due =
            try {
                armed.incrementAndGet()
                thread.schedule({ drainAt(at) }, wait, TimeUnit.MILLISECONDS)
            } catch (e: RejectedExecutionException) {
                null
            }
    }

    // The timer for `at` fired: a drain, unless something already moved it.
    private fun drainAt(at: Long) {
        if (dueAt != at) return
        due = null
        dueAt = null
        drain()
    }

    /**
     * Called on the peer's thread after every drain that moved something — a frame, the link, a
     * verdict, the store, the status — with what moved; never for one that moved nothing. Returns
     * what removes it.
     */
    public fun onChange(listener: (Peer, Pumped) -> Unit): () -> Unit {
        listeners.add(listener)
        return { listeners.remove(listener) }
    }

    /**
     * Run `f` on the peer's thread, once the peer is open, and return; then one drain, so whatever
     * `f` asked of the peer is told — a mutation's own push has already gone, by `mutate`.
     */
    public fun post(f: (Peer) -> Unit) {
        if (closed) return
        if (inline) return peer?.let(f) ?: Unit
        try {
            thread.execute {
                peer?.takeIf { !closed }?.let(f)
                drainSoon()
            }
        } catch (e: RejectedExecutionException) {
            // Closed: nothing to run.
        }
    }

    /**
     * Run `f` on the peer's thread and wait for what it returns: a read, which drains nothing. A
     * session that closed — or whose replica could not be opened — never answers: the wait is
     * cancelled.
     */
    public suspend fun <T> run(f: (Peer) -> T): T {
        if (inline && !closed)
            peer?.let {
                return f(it)
            }
        val d = CompletableDeferred<T>()
        try {
            thread.execute {
                val p = peer
                if (closed || p == null) d.cancel(CancellationException("the session is closed"))
                else
                    try {
                        d.complete(f(p))
                    } catch (e: Throwable) {
                        d.completeExceptionally(e)
                    }
            }
        } catch (e: java.util.concurrent.RejectedExecutionException) {
            d.cancel(CancellationException("the session is closed"))
        }
        return d.await()
    }

    /**
     * Sign out: the onboarding forgets the login, closes this session — its replica and whatever is
     * pending kept — and goes back to *Sign in* with the server kept.
     */
    public fun signOut() {
        onSignOut()
    }

    /** Stop the thread and close the peer, written down; its storage stays for the next session. */
    public fun close() {
        if (closed || closing) return
        closing = true
        closeKept()
        launcher.shutdown()
        if (inline) {
            closed = true
            thread.shutdown()
            return
        }
        try {
            thread.execute {
                due?.cancel(false)
                closed = true
                try {
                    peer?.close()
                } catch (e: Exception) {
                    // Written down as far as it could be; the next open replays the rest.
                }
            }
        } catch (e: java.util.concurrent.RejectedExecutionException) {
            closed = true
        }
        thread.shutdown()
    }

    public companion object {
        /** How long after the last change a launch view is written. */
        public const val SAVE_AFTER_MS: Long = 1000

        /** The replica's directory for `server` under `root` (the app's files): one per server. */
        public fun directory(root: File, server: String): File =
            File(root, "ark/server-" + server.replace(Regex("[^A-Za-z0-9]"), "_") + "/replica")

        /**
         * A session over a peer the caller opened, and links itself if at all — a test, or a
         * transport of the caller's own — woken like any other.
         */
        public fun over(peer: Peer, server: String, login: Login): ArkSession =
            ArkSession(server, login, {}, {}).also { s -> s.thread.execute { s.start(peer) } }

        /**
         * A session over a peer with no thread of its own: every post and run happens at once on
         * the caller's thread, and nothing pumps — a test that drives a screen on one clock and
         * cannot have the peer answer on another. The peer is the caller's to close, and to pump.
         */
        public fun inline(peer: Peer, server: String, login: Login): ArkSession =
            ArkSession(server, login, {}, {}, inline = true).also { it.peer = peer }

        /**
         * The session over `server`'s replica under `root`, as `login`, at once; on its thread,
         * first, the module from `domain` and the peer opened over the replica, linked and woken.
         * If that fails — a module that does not verify, a replica that does not read — the session
         * closes, nothing posted to it runs, and `onFailed` says why.
         */
        public fun open(
            domain: () -> Domain,
            root: File,
            server: String,
            login: Login,
            onDenied: (String) -> Unit,
            onSignOut: () -> Unit,
            onFailed: (Exception) -> Unit = {},
            /** The module's bytes, which a launch view is bound to; none, and none are kept. */
            module: ByteArray? = null,
            /**
             * What this install declares itself as at `hello` (`docs/plan-live.md`); none, and the
             * peer is no device and holds no live state.
             */
            device: DeviceDecl? = null,
        ): ArkSession {
            val launches =
                module?.let {
                    LaunchViews(
                        LaunchViews.directory(directory(root, server), login.user.id),
                        LaunchBinding.of(it, login.user.id, login.user.roles))
                }
            val s = ArkSession(server, login, onDenied, onSignOut, launches = launches)
            s.thread.execute {
                try {
                    val peer =
                        Peer.open(
                            domain(),
                            DirStorage(directory(root, server)),
                            Options.server(login.user.id, login.session, login.token).let { o ->
                                if (device != null) o.withDevice(device) else o
                            })
                    peer.setRoles(login.user.roles)
                    peer.connect(Auth.socketUrl(server))
                    s.start(peer)
                } catch (e: Exception) {
                    s.closed = true
                    s.thread.shutdown()
                    onFailed(e)
                }
            }
            return s
        }
    }
}
