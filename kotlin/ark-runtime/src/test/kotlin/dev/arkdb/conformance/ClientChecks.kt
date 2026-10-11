// The checks of the client's kinds — `protocol`, `rebase` (its scripted
// half) and `trace` — as spec/RUNNERS.md says them and
// rust/ark/tests/vectors.rs and rust/ark/tests/traces.rs read them. Each
// throws `Failed` with its first unmet claim, so a `falsify/` vector must
// fail the same check a vector passes.
package dev.arkdb.conformance

import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.Change
import dev.arkdb.Changes
import dev.arkdb.ClientMsg
import dev.arkdb.Closure
import dev.arkdb.Ctx
import dev.arkdb.Durable
import dev.arkdb.Expr
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.KeptStore
import dev.arkdb.LogEntry
import dev.arkdb.MemoryStore
import dev.arkdb.Module
import dev.arkdb.Op
import dev.arkdb.Received
import dev.arkdb.Replica
import dev.arkdb.Schema
import dev.arkdb.Sequenced
import dev.arkdb.ServerMsg
import dev.arkdb.Snapshot
import dev.arkdb.Stmt
import dev.arkdb.TraceError
import dev.arkdb.Value
import dev.arkdb.changeFromValue
import dev.arkdb.entryFromValue
import dev.arkdb.localCommit
import dev.arkdb.replay
import java.io.File

object ClientChecks {
    // protocol ------------------------------------------------------------

    /**
     * `check_protocol`: the bytes decode to the frame and the frame encodes to the bytes; the frame
     * reads as a frame of its side and writes back as itself; and a server frame read as a client
     * reads the socket — under the demo's schema and under none — is the frame as read.
     */
    fun protocol(path: String, v: Value) {
        val frame = v.field("frame")
        val bytes = Hex.decode(v.field("bytes").asText())
        ensureEq(Canon.decode(bytes), frame, "the bytes decode to the frame")
        ensureBytes(Canon.encode(frame), bytes, "the frame encodes to the bytes")
        if (path.substringAfterLast('/').startsWith("client-")) {
            val f = ClientMsg.fromValue(frame)
            ensureEq(f.toValue(), frame, "decode then encode")
            ensureEq(ClientMsg.fromValue(f.toValue()).toValue(), f.toValue(), "encode then decode")
        } else {
            val f = ServerMsg.fromValue(frame)
            ensureEq(f.toValue(), frame, "decode then encode")
            ensureEq(ServerMsg.fromValue(f.toValue()).toValue(), f.toValue(), "encode then decode")
            val demo =
                Checks.moduleOf(Conformance.vector("rebase/three-peers.json").field("module"))
                    .schema
            for (sch in listOf(demo, Schema(emptyList()))) {
                val want: Value =
                    if (f is ServerMsg.SnapshotOf)
                        Snapshot.ofValues(sch, f.seq, f.rows, f.logId, f.module, f.held).toValue()
                    else f.toValue()
                val got: Value =
                    when (val r = ServerMsg.decodeFor(bytes, sch)) {
                        is Received.Snap ->
                            if (f is ServerMsg.SnapshotOf) r.snapshot.toValue()
                            else throw Failed("read as a client reads it: a snapshot")
                        is Received.Msg ->
                            if (f !is ServerMsg.SnapshotOf) r.msg.toValue()
                            else throw Failed("read as a client reads it: not a snapshot")
                    }
                ensureEq(got, want, "read as a client reads it")
            }
        }
    }

    // rebase/three-peers ----------------------------------------------------

    /** The steps of `rebase/three-peers.json` a client does not take, named (spec/RUNNERS.md). */
    val serverOnly =
        listOf(
            "the authority adopting eve's log by replaying it, and refusing it once its facts are " +
                "touched (Authority::adopt is a server's)",
            "compaction: the horizon moved to 2, a page at 0 answered with the snapshot and at 2 " +
                "with the tail, the state at the head from facts (a server's)")

    private fun Replica.claim(): List<Any> = verifyAt().let { (n, h) -> listOf(n, Hex.encode(h)) }

    private fun id(k: Int): Id = Id(ByteArray(16).also { it[15] = k.toByte() })

    private fun hashOf(bodies: Map<String, Closure>, name: String): ByteArray =
        Hex.decode(bodies.entries.first { it.value.function.name == name }.key)

    private fun posOf(r: Replica, k: Int): Value? =
        r.view.get("item", listOf(Value.VId(id(1)), Value.VText("t$k")))?.fields?.get("pos")

    private fun push(a: Authority, e: LogEntry): Pair<Long, List<Change>> =
        when (val s = a.sequenceEntry(e)) {
            is Sequenced.Appended -> s.seq to s.facts
            else -> throw Failed("push: $s")
        }

    /**
     * `rebase_three_peers`, a client's half: sequencing, a whole replica out of order, one by facts
     * alone, and the scenario — the rebase, its transitions, a duplicate delivery, a divergence
     * healed by facts, and a peer alone confirming its own intents by a local commit.
     */
    fun threePeers(v: Value) {
        val m = Checks.moduleOf(v.field("module"))
        val sch = m.schema
        val bodies = Hash.closures(m)
        val entries =
            v.field("entries").asList().map { e ->
                val f = e.asStruct().fields.filterKeys { it != "seq" }
                e.field("seq").asInt() to entryFromValue(Value.VStruct(f))
            }
        val facts = v.field("facts").asList().map { f -> f.asList().map(::changeFromValue) }
        val finalHash = v.field("final_hash").asText()

        // 1. The transcript through an authority.
        val auth = Authority(sch, bodies)
        for ((k, ne) in entries.withIndex()) {
            val (n, e) = ne
            when (val s = auth.sequenceEntry(e)) {
                is Sequenced.Appended -> {
                    ensureEq(s.seq, n, "the sequence of entry $n")
                    ensureEq(
                        Checks.changesValue(s.facts), Checks.changesValue(facts[k]), "facts at $n")
                }
                else -> throw Failed("entry $n: $s")
            }
        }
        ensureEq(Hex.encode(Hash.stateHash(auth.store)), finalHash, "the authority's hash")
        ensureEq(auth.store.storeValue(), v.field("final_store"), "the authority's store")

        // 2. A whole replica, given every entry in reverse order.
        val fresh = { Replica.open(sch, bodies, MemoryStore.empty(sch), 0, emptyList()) }
        val whole = fresh()
        for ((n, e) in entries.asReversed()) whole.receive(n, e)
        whole.settle()
        ensureEq(whole.cursor, entries.size.toLong(), "the whole replica's cursor")
        ensureEq(Hex.encode(whole.verifyAt().second), finalHash, "the whole replica's hash")
        ensure(whole.diverged.isEmpty()) { "the whole replica diverged" }

        // 3. A replica with no closures, by facts alone.
        val factsOnly = Replica.open(sch, emptyMap(), MemoryStore.empty(sch), 0, emptyList())
        for ((n, e) in entries) factsOnly.receive(n, e)
        ensureEq(factsOnly.needs(), (1..entries.size.toLong()).toList(), "what it needs")
        for ((k, ne) in entries.withIndex()) factsOnly.receiveFacts(ne.first, facts[k])
        factsOnly.settle()
        ensureEq(Hex.encode(factsOnly.verifyAt().second), finalHash, "by facts alone")
        ensure(factsOnly.diverged.isEmpty()) { "by facts, diverged" }

        // 4. The scenario.
        val hCreate = hashOf(bodies, "create_playlist")
        val hAdd = hashOf(bodies, "add_to_playlist")
        fun ctx(who: String) = Ctx(who, "$who-session")
        val now = emptyMap<String, Value>()
        fun addArgs(k: Int) =
            mapOf("playlist_id" to Value.VId(id(1)), "track_id" to Value.VText("t$k"))
        fun mutate(
            r: Replica,
            i: Int,
            who: String,
            fh: ByteArray,
            autos: Map<String, Value>,
            args: Map<String, Value>
        ): LogEntry {
            val (e, why) = r.mutate(id(i), ctx(who), fh, autos, args)
            return e ?: throw Failed("mutate $i refused: $why")
        }
        val a = Authority(sch, bodies)
        val alice = fresh()
        val bob = fresh()
        val e1 =
            mutate(
                alice,
                101,
                "alice",
                hCreate,
                mapOf("id" to Value.VId(id(1))),
                mapOf("name" to Value.VText(" Favorites ")))
        val (s1, _) = push(a, e1)
        alice.ack(e1.id, s1)
        alice.settle()
        bob.receive(s1, e1)
        bob.settle()
        ensureEq(s1, 1L, "the playlist was sequenced first")
        ensureEq(
            alice.view.get("playlist", listOf(Value.VId(id(1))))?.fields?.get("name"),
            Value.VText("Favorites"),
            "alice's name was trimmed")
        ensureEq(alice.claim(), bob.claim(), "alice and bob agree after step 1")
        val e2 = mutate(bob, 102, "bob", hAdd, now, addArgs(1))
        val (s2, _) = push(a, e2)
        bob.ack(e2.id, s2)
        bob.settle()
        val e3 = mutate(bob, 103, "bob", hAdd, now, addArgs(2))
        val (s3, _) = push(a, e3)
        bob.ack(e3.id, s3)
        bob.settle()
        alice.takeChanges()
        val e9 = mutate(alice, 109, "alice", hAdd, now, addArgs(9))
        ensureEq(
            posOf(alice, 9),
            v.field("alice_alone_pos_of_9"),
            "alone, alice's track is first on her view")
        val local = alice.takeChanges()
        ensure(local is Changes.Applied && local.changes.size == 1) {
            "a local mutation reports its changes, not a rebuild: $local"
        }
        ensureEq(posOf(bob, 1), Value.VInt(1), "bob's first")
        ensureEq(posOf(bob, 2), Value.VInt(2), "bob's second")
        alice.receive(s2, e2)
        alice.settle()
        alice.receive(s3, e3)
        alice.settle()
        val rebased = alice.takeChanges()
        ensure(rebased is Changes.Applied && rebased.changes.size == 6) {
            "the rebase is reported as its transitions: $rebased"
        }
        ensureEq(
            posOf(alice, 9),
            v.field("alice_after_rebase_pos_of_9"),
            "after the rebase alice's track is third")
        ensureEq(alice.claim(), bob.claim(), "alice's confirmed state is bob's")
        val (s9, f9) = push(a, e9)
        alice.receiveFacts(s9, f9)
        alice.ack(e9.id, s9)
        alice.settle()
        bob.receive(s9, e9)
        bob.settle()
        ensure(alice.pending.isEmpty()) { "nothing is pending on alice once acked" }
        ensure(alice.takeChanges() is Changes.Applied) {
            "with nothing pending the ack costs no rebuild"
        }
        ensure(alice.view == alice.confirmed) { "alice's view is her confirmed store" }
        ensure(f9.any { it is Change.Add && it.row["pos"] == Value.VInt(3) }) {
            "the authority's facts say pos 3 too"
        }
        ensureEq(alice.claim(), bob.claim(), "alice and bob")
        ensureEq(Hex.encode(bob.verifyAt().second), finalHash, "three replicas, one hash")
        // a duplicate delivery changes nothing
        val before =
            listOf(
                bob.cursor,
                Hex.encode(bob.verifyAt().second),
                bob.view.storeValue(),
                bob.pending.size,
                bob.inbox.size)
        bob.receive(s2, e2)
        bob.settle()
        ensureEq(
            listOf(
                bob.cursor,
                Hex.encode(bob.verifyAt().second),
                bob.view.storeValue(),
                bob.pending.size,
                bob.inbox.size),
            before,
            "a duplicate delivery is a no-op")
        // dave's build of add_to_playlist steps by two; facts catch it
        val key = Hex.encode(hAdd)
        val right = bodies[key]!!
        val wrongBody =
            right.function.body.map { s ->
                val ins = s as? Stmt.Insert
                val row = ins?.row as? Expr.StructE
                val pos = row?.fields?.get("pos") as? Expr.OpE
                if (ins != null && row != null && pos != null && pos.op == Op.Add) {
                    val two = Expr.OpE(Op.Add, listOf(pos.args[0], Expr.Lit(Value.VInt(2))))
                    Stmt.Insert(ins.table, Expr.StructE(row.fields + ("pos" to two)), ins.on)
                } else s
            }
        ensure(wrongBody != right.function.body) { "dave's build is the right one" }
        val wrong =
            bodies.toMutableMap().also {
                it[key] = Closure(right.function.copy(body = wrongBody), right.helpers)
            }
        val dave = Replica.open(sch, wrong, MemoryStore.empty(sch), 0, emptyList())
        for ((k, ne) in entries.withIndex()) dave.receiveWith(ne.first, ne.second, facts[k])
        dave.settle()
        ensureEq(dave.diverged.toList(), listOf(2L, 3L, 4L), "a divergent runtime is detected")
        ensureEq(Hex.encode(dave.verifyAt().second), finalHash, "and healed by the facts")
        // eve has no server: she is her own authority
        val eve = fresh()
        val eveAuth = Authority(sch, bodies)
        mutate(
            eve,
            201,
            "eve",
            hCreate,
            mapOf("id" to Value.VId(id(2))),
            mapOf("name" to Value.VText("Road")))
        mutate(
            eve,
            202,
            "eve",
            hAdd,
            now,
            mapOf("playlist_id" to Value.VId(id(2)), "track_id" to Value.VText("t5")))
        localCommit(eveAuth, eve)
        ensure(eve.cursor == 2L && eve.pending.isEmpty() && eve.view == eve.confirmed) {
            "alone, eve confirms her own intents"
        }
        ensureBytes(
            eve.verifyAt().second,
            Hash.stateHash(eveAuth.store),
            "and her state is her authority's")
    }

    // trace -----------------------------------------------------------------

    /** A trace file read back (`trace::read`): its module and its steps. */
    fun readTrace(v: Value): Pair<Module, List<Pair<Value, Value>>> {
        val m = Checks.moduleOf(v.field("module"))
        val steps = v.field("steps").asList().map { it.field("in") to it.field("out") }
        return m to steps
    }

    /** `trace::replay` over a trace vector, a difference a `Failed` naming the step and fields. */
    fun trace(v: Value, durable: () -> Durable = ::KeptStore) {
        val (m, steps) = readTrace(v)
        try {
            replay(m, steps, durable())
        } catch (e: TraceError) {
            throw Failed(e.message ?: "a trace that does not replay")
        }
    }

    /**
     * Every trace of a corpus directory (`packages.trace-corpus`) replayed: the names that ran and
     * the first failure of each that did not, and how long it took.
     */
    fun corpus(dir: File, durable: () -> Durable = ::KeptStore): Triple<Int, List<String>, Long> {
        val files =
            (dir.listFiles() ?: throw Failed("$dir is not a directory"))
                .filter { it.isFile && it.name.endsWith(".json") }
                .sortedBy { it.name }
        val failed = ArrayList<String>()
        val t0 = System.nanoTime()
        for (f in files) {
            try {
                trace(Json.parse(f.readText(Charsets.UTF_8)), durable)
            } catch (e: Failed) {
                failed.add("${f.name}: ${e.message}")
            } catch (e: Throwable) {
                failed.add(
                    "${f.name}: BUG ${e.javaClass.simpleName}: ${e.message}\n" +
                        e.stackTrace.take(12).joinToString("\n") { "      at $it" })
            }
        }
        return Triple(files.size, failed, (System.nanoTime() - t0) / 1_000_000)
    }
}
