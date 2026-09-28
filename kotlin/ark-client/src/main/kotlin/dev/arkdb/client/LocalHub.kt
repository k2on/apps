// An in-process server, and the transport that reaches it without a
// socket: the runtime's sans-io `Server` (the spec's §12 server — one
// authority, identity asked once at `Hello`, every push held to it, and
// fan-out after every message) with connections numbered and delivered to
// in-process. It is what a test needs; it is not the production server,
// which is Rust.
package dev.arkdb.client

import dev.arkdb.Authenticate
import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.ClientMsg
import dev.arkdb.Closure
import dev.arkdb.ConnId
import dev.arkdb.FnHash
import dev.arkdb.Hash
import dev.arkdb.Identity
import dev.arkdb.Module
import dev.arkdb.Protocol
import dev.arkdb.Server
import dev.arkdb.ServerMsg

public class LocalHub(
    public val module: Module,
    /**
     * What a token proves. By default dev auth, where a token names who is
     * asking: `"alice"` is alice under the session `"dev"` (the spec's
     * `trusting`), `"alice:phone"` is alice under the session `"phone"`,
     * and no token is nobody.
     */
    authenticate: Authenticate = dev,
    /** May this identity receive the log? */
    access: (Identity) -> Boolean = { true },
) {
    private val bodies: Map<FnHash, Closure> = Hash.closures(module)

    /** The authority for the log. */
    public val authority: Authority = Authority(module.schema, bodies)

    /** The server machine every connection talks to. */
    public val server: Server = Server(authenticate, access, authority)

    public companion object {
        public val dev: Authenticate = Authenticate { tok ->
            when {
                tok == null -> null
                ':' in tok -> Identity(tok.substringBefore(':'), tok.substringAfter(':'))
                else -> Identity(tok, "dev")
            }
        }
    }

    /** Install the sessions a user owns (`Server.withOwns`): an entry authored under an older login of the same user is then accepted. */
    public fun withOwns(f: (user: String, session: String) -> Boolean): LocalHub {
        server.withOwns(f)
        return this
    }

    private class Conn(val deliver: (ServerMsg) -> Unit) {
        var closed = false
    }

    private val conns = java.util.TreeMap<ConnId, Conn>()
    private var next: ConnId = 1

    /** Frames the hub has handled, by tag, for a test to read. */
    public val handled: MutableList<String> = ArrayList()

    /** The number of live connections. */
    public val connections: Int get() = conns.values.count { !it.closed }

    /** Attach, and get back both directions plus a way to close. */
    public fun connect(deliver: (ServerMsg) -> Unit): Pair<(ClientMsg) -> Unit, () -> Unit> {
        val id = next++
        val c = Conn(deliver)
        conns[id] = c
        val send: (ClientMsg) -> Unit = { m ->
            if (!c.closed) {
                handled.add(m::class.simpleName ?: "?")
                server.recv(id, m)
                flush()
            }
        }
        val close = {
            if (!c.closed) {
                c.closed = true
                conns.remove(id)
                server.disconnect(id)
            }
        }
        return send to close
    }

    // What the server queued, to each connection still open, in order.
    private fun flush() {
        for ((to, m) in server.takeOutgoing()) conns[to]?.let { if (!it.closed) it.deliver(m) }
    }
}

/**
 * The in-memory transport: frames are encoded and decoded exactly as they
 * would be on a socket — `Canon.encode(msg.toValue())` one way and
 * `Protocol.clientFromValue(Canon.decode(bytes))` the other — so the wire
 * path is the one exercised, and only the socket is missing. `online`
 * false refuses connections and drops the open one, which is how a test
 * takes a peer offline.
 */
public class InMemoryTransport(public val hub: LocalHub) : Transport {
    public var online: Boolean = true
        set(v) {
            field = v
            if (!v) for (c in live.toList()) c.drop("dropped")
        }

    private val live = ArrayList<Conn>()

    /** Every frame that crossed, as bytes, for a test to look at. */
    public val frames: MutableList<Pair<String, ByteArray>> = ArrayList()

    private inner class Conn(val listener: ConnectionListener) : Connection {
        var closed = false
        lateinit var toHub: (ClientMsg) -> Unit
        lateinit var shut: () -> Unit

        fun drop(reason: String) {
            if (closed) return
            closed = true
            shut()
            live.remove(this)
            listener.onClose(reason)
        }

        override fun send(frame: ByteArray): Boolean {
            if (closed) return false
            frames.add("client" to frame)
            toHub(Protocol.clientFromValue(Canon.decode(frame)))
            return true
        }

        override fun close() = drop("closed")
    }

    override fun open(listener: ConnectionListener): Connection {
        if (!online) {
            listener.onClose("offline")
            return object : Connection {
                override fun send(frame: ByteArray): Boolean = false
                override fun close() = Unit
            }
        }
        val c = Conn(listener)
        val (send, shut) = hub.connect { m ->
            val bytes = Canon.encode(m.toValue())
            frames.add("server" to bytes)
            if (!c.closed) listener.onFrame(bytes)
        }
        c.toHub = send
        c.shut = shut
        live.add(c)
        listener.onOpen()
        return c
    }
}
