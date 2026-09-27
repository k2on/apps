// An in-process authority host, and the transport that reaches it without a
// socket. The runtime carries the client machine only; this is the server
// half a test needs — hello, push, need_facts, verify, and fan-out — written
// against `Authority` and `Log`, and small enough to read as the protocol's
// §12 server rules. It is not the production server, which is Rust.
package dev.arkdb.client

import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.ClientMsg
import dev.arkdb.Closure
import dev.arkdb.FnHash
import dev.arkdb.Hash
import dev.arkdb.Module
import dev.arkdb.Page
import dev.arkdb.Protocol
import dev.arkdb.Sequenced
import dev.arkdb.ServerMsg

public class LocalHub(public val module: Module) {
    private val bodies: Map<FnHash, Closure> = Hash.closures(module)

    /** One authority per scope of the module's schema. */
    public val authorities: Map<String, Authority> =
        module.schema.scopes.associate { it.name to Authority(it.name, module.schema, bodies) }

    private inner class Conn(val deliver: (ServerMsg) -> Unit) {
        /** The sequence each subscribed scope has been sent up to. */
        val sentUpTo = HashMap<String, Long>()
        var token: String? = null
        var closed = false
    }

    private val conns = ArrayList<Conn>()

    /** Frames the hub has handled, by tag, for a test to read. */
    public val handled: MutableList<String> = ArrayList()

    /** The number of live connections. */
    public val connections: Int get() = conns.count { !it.closed }

    /** Attach, and get back both directions plus a way to close. */
    public fun connect(deliver: (ServerMsg) -> Unit): Pair<(ClientMsg) -> Unit, () -> Unit> {
        val c = Conn(deliver)
        conns.add(c)
        val send: (ClientMsg) -> Unit = { m -> if (!c.closed) handle(c, m) }
        return send to { c.closed = true }
    }

    private fun handle(c: Conn, m: ClientMsg) {
        handled.add(m::class.simpleName ?: "?")
        when (m) {
            is ClientMsg.Hello -> {
                // Dev auth: a token is a name, and no token is nobody.
                if (m.token == null) {
                    c.deliver(ServerMsg.Denied("not signed in"))
                    c.closed = true
                    return
                }
                c.token = m.token
                for (s in m.subs) {
                    val a = authorities[s.scope] ?: continue
                    c.sentUpTo[s.scope] = s.since
                    page(c, a)
                }
            }
            is ClientMsg.Push -> {
                val a = authorities[m.scope] ?: return
                val acked = ArrayList<Pair<dev.arkdb.Id, Long>>()
                for (e in m.entries) {
                    // The login that pushes an entry is the one it was authored under.
                    if (c.token != null && e.actor != c.token) {
                        c.deliver(ServerMsg.Reject(m.scope, e.id, "not yours"))
                        continue
                    }
                    when (val s = a.sequenceEntry(e)) {
                        is Sequenced.Appended -> acked.add(e.id to s.seq)
                        is Sequenced.Duplicate -> acked.add(e.id to s.seq)
                        is Sequenced.Rejected -> c.deliver(ServerMsg.Reject(m.scope, e.id, s.refusal.text))
                    }
                }
                if (acked.isNotEmpty()) c.deliver(ServerMsg.Ack(m.scope, acked.map { it.first }, acked.map { it.second }))
                // Fan out: everyone subscribed, the author included, gets what
                // is above their cursor. The author's own entry comes back as
                // a duplicate delivery its replica ignores.
                for (o in conns) if (!o.closed && m.scope in o.sentUpTo) page(o, a)
            }
            is ClientMsg.NeedFacts -> {
                val a = authorities[m.scope] ?: return
                val items = m.seqs.mapNotNull { n -> a.log.entries[n]?.let { n to it.second } }
                c.deliver(ServerMsg.FactsFor(m.scope, items))
            }
            is ClientMsg.NeedClosures -> {
                c.deliver(ServerMsg.Closures(m.hashes.mapNotNull { h -> bodies[h]?.let { h to it } }))
            }
            is ClientMsg.Verify -> {
                val a = authorities[m.scope] ?: return
                val st = a.log.stateAt(m.seq)
                val ok = st != null && Hash.stateHash(st).contentEquals(m.hash)
                c.deliver(ServerMsg.Agree(m.scope, m.seq, m.hash, ok))
            }
            is ClientMsg.Say -> {
                for (o in conns) if (!o.closed && o !== c && o.token == c.token) o.deliver(ServerMsg.Heard(m.frame))
            }
        }
    }

    // Everything above the connection's cursor for a scope, a page at a time
    // (the client says hello again for the rest), or the snapshot.
    private fun page(c: Conn, a: Authority) {
        val cursor = c.sentUpTo[a.scope] ?: return
        when (val p = a.page(cursor, Protocol.BATCH_LIMIT)) {
            is Page.Entries -> {
                if (p.items.isEmpty()) return
                c.deliver(ServerMsg.Batch(a.scope, p.items, p.hasMore))
                c.sentUpTo[a.scope] = p.items.last().first
            }
            is Page.BelowHorizon -> {
                val sn = p.snapshot
                val rows = sn.store.tableNames.associateWith { t -> sn.store.scan(t).map { it as dev.arkdb.Value } }
                c.deliver(ServerMsg.SnapshotOf(a.scope, sn.seq, sn.hash, rows))
                c.sentUpTo[a.scope] = sn.seq
                page(c, a)
            }
        }
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
