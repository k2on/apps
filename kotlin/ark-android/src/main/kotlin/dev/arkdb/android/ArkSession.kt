// What the onboarding hands an app: the Kotlin `Peer`, opened over a
// replica at the server it found and signed in as the login it got, linked
// to the server's `/sync`, and pumped every fifty milliseconds on a thread
// of its own.
//
// A peer is single-threaded, so everything that touches it runs on that
// thread: the pump, and whatever the app hands [post] or [run]. After each
// pump every [onPump] listener is called there, with the peer — which is
// where an app takes its changes and splices its views. What a screen may
// ask without the thread is [status]: linked, and how much is not synced.
// A `Denied` from the server is the onboarding's, not the app's: the
// session stops and says so to its host, which goes back to *Sign in*.
//
// The replica is one directory per server, not per person: the log is the
// server's and every person reads it, and a person signing in again after
// a denial finds their pending intents where they left them.
package dev.arkdb.android

import dev.arkdb.client.Auth
import dev.arkdb.client.DirStorage
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
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
)

public class ArkSession
private constructor(
    /** The server's base URL. */
    public val server: String,
    /** Who is signed in. */
    public val login: Login,
    private val peer: Peer,
    private val onDenied: (String) -> Unit,
    private val onSignOut: () -> Unit,
) {
    private val thread =
        Executors.newSingleThreadScheduledExecutor { r ->
            Thread(r, "ark-peer").apply { isDaemon = true }
        }
    private val listeners = CopyOnWriteArrayList<(Peer) -> Unit>()
    private val _status = MutableStateFlow(SessionStatus())
    private var ticking: ScheduledFuture<*>? = null
    @Volatile private var closed = false

    /** Linked, and how much is not synced; moved by every pump. */
    public val status: StateFlow<SessionStatus> = _status.asStateFlow()

    private fun start() {
        ticking = thread.scheduleWithFixedDelay({ tick() }, 0, PUMP_MS, TimeUnit.MILLISECONDS)
    }

    private fun tick() {
        if (closed) return
        val pumped =
            try {
                peer.pump()
            } catch (e: Exception) {
                null
            }
        val s = peer.status()
        val now = SessionStatus(linked = s.linked, link = s.link, pending = s.pending)
        if (now != _status.value) _status.value = now
        for (l in listeners) {
            try {
                l(peer)
            } catch (e: Exception) {
                // A listener's failure is the listener's: the pump goes on.
            }
        }
        val denied = pumped?.denied
        if (denied != null) onDenied(denied)
    }

    /** Called on the peer's thread after every pump; returns what removes it. */
    public fun onPump(listener: (Peer) -> Unit): () -> Unit {
        listeners.add(listener)
        return { listeners.remove(listener) }
    }

    /** Run `f` on the peer's thread, and return. */
    public fun post(f: (Peer) -> Unit) {
        if (closed) return
        thread.execute { if (!closed) f(peer) }
    }

    /** Run `f` on the peer's thread and wait for what it returns. */
    public suspend fun <T> run(f: (Peer) -> T): T {
        val d = CompletableDeferred<T>()
        thread.execute {
            try {
                d.complete(f(peer))
            } catch (e: Throwable) {
                d.completeExceptionally(e)
            }
        }
        return d.await()
    }

    /** Stop the pump and close the peer, written down; its storage stays for the next session. */
    /**
     * Sign out: the onboarding forgets the login, closes this session — its replica and whatever is
     * pending kept — and goes back to *Sign in* with the server kept.
     */
    public fun signOut() {
        onSignOut()
    }

    public fun close() {
        if (closed) return
        thread.execute {
            ticking?.cancel(false)
            closed = true
            try {
                peer.close()
            } catch (e: Exception) {
                // Written down as far as it could be; the next open replays the rest.
            }
        }
        thread.shutdown()
    }

    public companion object {
        /** How often the peer is pumped: what every client here uses. */
        public const val PUMP_MS: Long = 50

        /** The replica's directory for `server` under `root` (the app's files): one per server. */
        public fun directory(root: File, server: String): File =
            File(root, "ark/server-" + server.replace(Regex("[^A-Za-z0-9]"), "_") + "/replica")

        /**
         * A session over a peer the caller opened, and links itself if at all — a test, or a
         * transport of the caller's own — pumped like any other.
         */
        public fun over(peer: Peer, server: String, login: Login): ArkSession =
            ArkSession(server, login, peer, {}, {}).also { it.start() }

        /**
         * Open the peer over `server`'s replica under `root`, as `login`, link it, and start
         * pumping. Blocking: the replica is read from storage.
         */
        public fun open(
            domain: Domain,
            root: File,
            server: String,
            login: Login,
            onDenied: (String) -> Unit,
            onSignOut: () -> Unit,
        ): ArkSession {
            val peer =
                Peer.open(
                    domain,
                    DirStorage(directory(root, server)),
                    Options.server(login.user.id, login.session, login.token))
            peer.setRoles(login.user.roles)
            peer.connect(Auth.socketUrl(server))
            return ArkSession(server, login, peer, onDenied, onSignOut).also { it.start() }
        }
    }
}
