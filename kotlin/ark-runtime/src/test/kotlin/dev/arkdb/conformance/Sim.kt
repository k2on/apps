// §15 The simulation (Ark.Sim), ported so that the seeded fleet vectors run
// here: a trusting `Server` that is the log's authority, clients each
// holding the log whole, and a seeded scheduler that delivers frames out of
// order, twice, or not at all. Every choice it makes is `lcg`'s, in the
// spec's order, so a seed is a run and the vector's hash is this one's.
package dev.arkdb.conformance

import dev.arkdb.Args
import dev.arkdb.Authenticate
import dev.arkdb.Authority
import dev.arkdb.Client
import dev.arkdb.ClientMsg
import dev.arkdb.Closure
import dev.arkdb.ConnId
import dev.arkdb.Ctx
import dev.arkdb.Fault
import dev.arkdb.FnHash
import dev.arkdb.Hash
import dev.arkdb.Id
import dev.arkdb.MemoryStore
import dev.arkdb.Mode
import dev.arkdb.Replica
import dev.arkdb.Schema
import dev.arkdb.Seq
import dev.arkdb.Server
import dev.arkdb.ServerMsg
import java.util.TreeMap

class Sim(sch: Schema, bodies: Map<FnHash, Closure>, n: Int, seed: Long) {
    val server = Server(Authenticate.trusting, { true }, Authority(sch, bodies))
    val clients = TreeMap<Int, Client>()
    private val conn = TreeMap<Int, ConnId>()
    private var nextConn: ConnId = 1
    private val toServer = TreeMap<Int, MutableList<ClientMsg>>()
    private val toClient = TreeMap<Int, MutableList<ServerMsg>>()
    private var seed: Long = seed

    init {
        for (i in 0 until n) clients[i] = Client(Replica.open(sch, bodies, MemoryStore(sch), 0, emptyList()), Mode.Whole, name(i))
        for (i in 0 until n) heal(i)
    }

    private fun name(i: Int) = "peer-$i"

    // Under dev auth the server names every login "dev", and an entry is
    // held to the login that pushed it, so this is what a client authors under.
    private fun ctxOf(i: Int) = Ctx(name(i), "dev")

    /** A client authors an intent; a refusal by its own view is dropped, as it would be in an app. */
    fun mutate(i: Int, eid: Id, fh: FnHash, autos: Args, args: Args) {
        val c = clients[i] ?: return
        try {
            c.mutate(eid, ctxOf(i), fh, autos, args)
        } catch (f: Fault.Refuse) {
            return
        }
        flushClient(i)
    }

    /** A client goes dark: its connection closes, frames in flight are lost. */
    fun partition(i: Int) {
        val c = conn[i] ?: return
        server.disconnect(c)
        clients[i]?.disconnected()
        conn.remove(i)
        toServer.remove(i)
        toClient.remove(i)
        flushServer()
    }

    /** A client comes back on a fresh connection and says hello. */
    fun heal(i: Int) {
        if (i in conn) return
        conn[i] = nextConn++
        clients[i]?.connected()
        flushClient(i)
    }

    private fun flushClient(i: Int) {
        val c = clients[i] ?: return
        toServer.getOrPut(i) { ArrayList() }.addAll(c.takeOutgoing())
    }

    private fun flushServer() {
        val byClient = conn.entries.associate { (i, c) -> c to i }
        for ((c, m) in server.takeOutgoing()) {
            val i = byClient[c] ?: continue
            toClient.getOrPut(i) { ArrayList() }.add(m)
        }
    }

    private fun roll(): Long {
        seed = lcg(seed)
        return seed ushr 11
    }

    /** One delivery: a random frame in flight, one in eight twice, one in sixteen never. */
    fun step() {
        val r = roll()
        val candidates = toServer.filter { it.value.isNotEmpty() }.keys.map { true to it } +
            toClient.filter { it.value.isNotEmpty() }.keys.map { false to it }
        if (candidates.isEmpty()) return
        val (up, i) = candidates[(r % candidates.size).toInt()]
        val fate = roll() % 16
        val times = if (fate == 0L) 0 else if (fate <= 2L) 2 else 1
        if (up) {
            val m = toServer.getValue(i).removeAt(0)
            repeat(times) { deliverToServer(i, m) }
        } else {
            val m = toClient.getValue(i).removeAt(0)
            repeat(times) { deliverToClient(i, m) }
        }
    }

    private fun deliverToServer(i: Int, m: ClientMsg) {
        val c = conn[i] ?: return
        server.recv(c, m)
        flushServer()
    }

    private fun deliverToClient(i: Int, m: ServerMsg) {
        val c = clients[i] ?: return
        c.recv(m)
        flushClient(i)
    }

    /** Reconnect everyone and deliver everything, perfectly, until nothing is in flight and nothing is pending. */
    fun settle() {
        val peers = clients.keys.toList()
        for (i in peers) partition(i)
        for (i in peers) heal(i)
        var rounds = 10000
        while (!quiet()) {
            if (rounds-- == 0) throw AssertionError("settle: the fleet did not converge in 10000 rounds")
            for (i in toServer.keys.toList()) {
                val ms = toServer.getValue(i).toList()
                toServer[i] = ArrayList()
                for (m in ms) deliverToServer(i, m)
            }
            for (i in toClient.keys.toList()) {
                val ms = toClient.getValue(i).toList()
                toClient[i] = ArrayList()
                for (m in ms) deliverToClient(i, m)
            }
        }
    }

    /** Nothing in flight and nothing pending. */
    fun quiet(): Boolean =
        toServer.values.all { it.isEmpty() } && toClient.values.all { it.isEmpty() } &&
            clients.values.all { it.replica.pending.isEmpty() }

    /** Each client's confirmed claim. */
    fun clientHashes(): List<Triple<Int, Seq, ByteArray>> = clients.map { (i, c) ->
        val (n, h) = c.replica.verifyAt()
        Triple(i, n, h)
    }

    /** The server's claim, at the head. */
    fun serverHash(): Pair<Seq, ByteArray> = server.authority.log.headSeq to Hash.stateHash(server.authority.store)

    companion object {
        /** Knuth's MMIX constants: the whole of the simulation's randomness. */
        fun lcg(s: Long): Long = s * 6364136223846793005L + 1442695040888963407L
    }
}
