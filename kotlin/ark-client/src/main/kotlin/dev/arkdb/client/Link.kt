// The link: a transport driving the runtime's `Client` machine.
//
// Everything that touches the `Client` happens inside `pump()`, on the
// caller's thread and timer (50 ms is the intended cadence); what a
// connection reports on its own threads is queued until then. Reconnecting
// is a schedule read off the clock on each pump, 500 ms doubling to 30 s,
// reset by a connection that opened.
package dev.arkdb.client

import dev.arkdb.Canon
import dev.arkdb.Client
import dev.arkdb.ClientMsg
import dev.arkdb.Protocol
import dev.arkdb.ServerMsg
import java.util.concurrent.ConcurrentLinkedQueue

public class Link(
    public val client: Client,
    public val transport: Transport,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    public companion object {
        public const val MIN_BACKOFF_MS: Long = 500
        public const val MAX_BACKOFF_MS: Long = 30_000
    }

    private sealed class Event(val gen: Int) {
        class Opened(gen: Int) : Event(gen)
        class Frame(gen: Int, val bytes: ByteArray) : Event(gen)
        class Closed(gen: Int, val reason: String) : Event(gen)
    }

    private val events = ConcurrentLinkedQueue<Event>()

    // The connection generation: events from a connection that has been
    // given up on are ignored, however late they arrive.
    private var gen: Int = 0
    private var conn: Connection? = null
    private var open: Boolean = false

    /** Whether the link is trying to be connected at all. */
    public var enabled: Boolean = true
        private set

    private var backoff: Long = MIN_BACKOFF_MS
    private var nextAttemptAt: Long = 0

    /** Why the last connection ended, for a status line. */
    public var lastClose: String? = null
        private set

    /** Frames that arrived and did not decode as a server frame, counted rather than thrown. */
    public var undecodable: Int = 0
        private set

    /** Frames sent and received, for a debug screen. */
    public var sent: Int = 0
        private set
    public var received: Int = 0
        private set

    /** The socket is open and the machine has been told so. */
    public val linked: Boolean get() = open && client.linked

    /** Milliseconds until the next attempt, or null when connected or not trying. */
    public fun retryIn(): Long? = if (!enabled || open || conn != null) null else maxOf(0L, nextAttemptAt - clock())

    /** Start (or resume) connecting; the first attempt happens on the next pump. */
    public fun connect() {
        enabled = true
        if (conn == null) nextAttemptAt = clock()
    }

    /** Stop, and stay stopped until `connect()`. */
    public fun disconnect() {
        enabled = false
        drop()
        backoff = MIN_BACKOFF_MS
    }

    private fun drop() {
        val c = conn
        gen += 1
        conn = null
        if (open) client.disconnected()
        open = false
        c?.close()
    }

    /** One turn: deliver what arrived, connect if it is time, deliver again, send what the machine queued. */
    public fun pump() {
        deliver()
        // A peer turned away keeps its database and stops reconnecting.
        if (client.denied != null && enabled) {
            enabled = false
            drop()
        }
        if (enabled && conn == null && clock() >= nextAttemptAt) {
            attempt()
            // A transport that answers at once (the in-memory one) has
            // already queued its open or its failure; take it this turn.
            deliver()
        }
        val c = conn
        if (open && c != null) {
            for (m in client.takeOutgoing()) {
                sent += 1
                if (!c.send(Canon.encode(m.toValue()))) {
                    // The socket is gone under us; its close event will follow.
                    break
                }
            }
            // What a synchronous transport answered with is taken now too.
            deliver()
        }
    }

    private fun deliver() {
        while (true) {
            val ev = events.poll() ?: break
            if (ev.gen != gen) continue
            when (ev) {
                is Event.Opened -> {
                    open = true
                    backoff = MIN_BACKOFF_MS
                    client.connected()
                }
                is Event.Frame -> {
                    received += 1
                    val msg: ServerMsg? = try {
                        Protocol.serverFromValue(Canon.decode(ev.bytes))
                    } catch (e: Exception) {
                        undecodable += 1
                        null
                    }
                    if (msg != null) client.recv(msg)
                }
                is Event.Closed -> {
                    lastClose = ev.reason
                    val c = conn
                    conn = null
                    gen += 1
                    if (open) client.disconnected()
                    open = false
                    c?.close()
                    nextAttemptAt = clock() + backoff
                    backoff = minOf(backoff * 2, MAX_BACKOFF_MS)
                }
            }
        }
    }

    private fun attempt() {
        val g = gen
        conn = transport.open(
            object : ConnectionListener {
                override fun onOpen() {
                    events.add(Event.Opened(g))
                }

                override fun onFrame(frame: ByteArray) {
                    events.add(Event.Frame(g, frame))
                }

                override fun onClose(reason: String) {
                    events.add(Event.Closed(g, reason))
                }
            },
        )
    }

    /** What the machine would send now, encoded, without sending it; for tests of the frame path. */
    public fun encode(m: ClientMsg): ByteArray = Canon.encode(m.toValue())
}
