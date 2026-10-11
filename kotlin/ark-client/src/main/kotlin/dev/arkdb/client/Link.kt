// The one place a peer's networking lives, as `rust/ark-client/src/link`
// is the reference's: a socket that moves binary frames, polled, and a `Link`
// around it that dials, notices a drop, and dials again with a backoff (half a
// second, doubling to thirty). The engine is sans-io, so everything here
// happens on `Link.poll`, which the peer's `pump` calls — twenty times a
// second, typically, from a UI's tick or a loop of its own.
//
// The socket is a WebSocket over OkHttp (`WebSocketTransport`), on the JVM
// and on Android alike: one dependency both platforms have, rather than an
// interface with the JDK's `java.net.http` behind it on one and OkHttp on the
// other. OkHttp calls back on threads of its own; `QueuedSocket` keeps what
// they said until the link polls, so the peer is only ever touched from its
// own loop.
//
// **Keepalive** is the server's — it pings — and the socket's: OkHttp pings
// every `Timing.pingEveryMs` and fails the connection when a pong does not
// come back before the next ping, which is the one state no read ever
// reports, a socket the OS thinks is open to a peer that is not there.
package dev.arkdb.client

import java.util.concurrent.ConcurrentLinkedQueue

/**
 * The numbers a link runs on (`link::Timing`); the defaults are a deployment's, a test shortens
 * them.
 */
public data class Timing(
    val firstBackoffMs: Long = 500,
    val maxBackoffMs: Long = 30_000,
    /** How often the socket pings; it gives up when a ping goes unanswered. */
    val pingEveryMs: Long = 20_000,
    /** How long a dial waits for the TCP connection and the WebSocket handshake. */
    val connectTimeoutMs: Long = 5_000,
)

/** What a socket reports (`link::Event`). */
public sealed class Event {
    /** The socket is open: the peer says `Hello`. */
    public object Opened : Event()

    /** A binary frame arrived. */
    public class Frame(public val bytes: ByteArray) : Event()

    /** The socket is gone, or never opened, and why. */
    public data class Closed(val why: String) : Event()
}

/** One connection's worth of socket (`link::Transport`): made by a dial, polled by a link. */
public interface Socket {
    /** Queue a binary frame. Only called after `Opened`. */
    public fun send(frame: ByteArray)

    /** Everything that happened since the last poll, oldest first. Never blocks. */
    public fun poll(): List<Event>

    /** Close it; nothing more is reported. */
    public fun close()
}

/** How a link opens a socket to a URL (`link::Dial`). */
public typealias Dial = (url: String) -> Socket

/** Where a link is (`link::State`). */
public sealed class LinkState {
    /** Not asked to connect, or told to stop (offline, or denied). */
    public object Idle : LinkState()

    /** Dialled, waiting for the socket to open. */
    public object Connecting : LinkState()

    /** Open; the peer is linked. */
    public object Open : LinkState()

    /** Down; the next attempt is at this monotonic millisecond. */
    public data class Waiting(val at: Long) : LinkState()
}

/** What one poll of a link came to, for the peer to hand the engine (`link::Polled`). */
public class Polled {
    /** The socket opened: say `connected()`. */
    public var opened: Boolean = false
    public val frames: MutableList<ByteArray> = ArrayList()
    /** The socket that was open closed: say `disconnected()`. */
    public var closed: String? = null
}

/** A reconnecting connection to one URL (`link::Link`). */
public class Link(public val url: String, private val dial: Dial, private val timing: Timing) {
    private var socket: Socket? = null
    public var state: LinkState = LinkState.Waiting(0)
        private set

    private var backoff: Long = timing.firstBackoffMs
    /** Connections opened so far. */
    public var opens: Long = 0
        private set

    /** Why the last connection ended, or the last attempt failed. */
    public var lastClose: String? = null
        private set

    public val isOpen: Boolean
        get() = state == LinkState.Open

    /** Stop, and stay stopped until [resume]. Whether a socket that was open went. */
    public fun stop(why: String): Boolean {
        val was = state == LinkState.Open
        socket?.close()
        socket = null
        state = LinkState.Idle
        backoff = timing.firstBackoffMs
        lastClose = why
        return was
    }

    /** Dial again on the next poll, after [stop]. */
    public fun resume() {
        if (state == LinkState.Idle) state = LinkState.Waiting(0)
    }

    /**
     * Queue a frame on the open socket; dropped otherwise, which is right: the engine re-offers
     * everything durable on the next `Hello`.
     */
    public fun send(frame: ByteArray) {
        if (state == LinkState.Open) socket?.send(frame)
    }

    /** One turn: dial if due, and collect what the socket did. */
    public fun poll(): Polled = pollAt(monotonicMs())

    /** [poll] at a given time, so the backoff is testable. */
    public fun pollAt(now: Long): Polled {
        val out = Polled()
        val s = state
        if (s is LinkState.Waiting && now >= s.at) {
            socket = dial(url)
            state = LinkState.Connecting
        }
        val t = socket ?: return out
        for (e in t.poll()) {
            when (e) {
                is Event.Opened -> {
                    state = LinkState.Open
                    opens++
                    backoff = timing.firstBackoffMs
                    out.opened = true
                    out.closed = null
                }
                is Event.Frame -> if (state == LinkState.Open) out.frames.add(e.bytes)
                is Event.Closed -> {
                    if (state == LinkState.Open) out.closed = e.why
                    socket?.close()
                    socket = null
                    lastClose = e.why
                    state = LinkState.Waiting(now + backoff)
                    backoff = minOf(backoff * 2, timing.maxBackoffMs)
                    return out
                }
            }
        }
        return out
    }

    public companion object {
        /** A monotonic clock in milliseconds (`autos::monotonic_ms`). */
        public fun monotonicMs(): Long = System.nanoTime() / 1_000_000
    }
}

/**
 * A [Transport]'s callbacks, kept until polled: what a link reads from a connection whose events
 * arrive on another thread. It reports `Opened`, then frames, then exactly one `Closed`.
 */
public class QueuedSocket(transport: Transport) : Socket {
    private val events = ConcurrentLinkedQueue<Event>()
    @Volatile private var done = false
    private val connection: Connection

    init {
        connection =
            transport.open(
                object : ConnectionListener {
                    override fun onOpen() {
                        events.add(Event.Opened)
                    }

                    override fun onFrame(frame: ByteArray) {
                        events.add(Event.Frame(frame))
                    }

                    override fun onClose(reason: String) {
                        events.add(Event.Closed(reason))
                    }
                })
    }

    override fun send(frame: ByteArray) {
        if (!done) connection.send(frame)
    }

    override fun poll(): List<Event> {
        val got = ArrayList<Event>()
        if (done) return got
        while (true) {
            val e = events.poll() ?: break
            got.add(e)
            if (e is Event.Closed) {
                done = true
                break
            }
        }
        return got
    }

    override fun close() {
        if (!done) {
            done = true
            connection.close()
        }
    }
}

/**
 * A socket over two queues, for a test or an in-process server (`link::Queues`): what the link
 * sends goes to [sent], and what is pushed onto [events] is what it reports.
 */
public class Queues : Socket {
    public val sent: MutableList<ByteArray> = java.util.Collections.synchronizedList(ArrayList())
    public val events: ConcurrentLinkedQueue<Event> = ConcurrentLinkedQueue()
    @Volatile public var closed: Boolean = false

    override fun send(frame: ByteArray) {
        sent.add(frame)
    }

    override fun poll(): List<Event> {
        val got = ArrayList<Event>()
        while (true) got.add(events.poll() ?: break)
        return got
    }

    override fun close() {
        closed = true
    }
}

/** The platform's WebSocket, OkHttp's, held to [timing] (`link::platform_dial`). */
public fun platformDial(timing: Timing): Dial {
    val http = WebSocketTransport.client(timing)
    return { url -> QueuedSocket(WebSocketTransport(url, http)) }
}
