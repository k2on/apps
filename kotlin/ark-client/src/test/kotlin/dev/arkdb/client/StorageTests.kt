// The storage held to two things, as a plain `main` (no test framework on
// the classpath, as the runtime's conformance runner):
//
// 1. The bytes. A fixture a Rust `ark_client::Peer` wrote — the demo module,
//    seeded autos, alone, reopened, joined to an in-process server, synced,
//    authoring online and offline, reopened again — is replayed into a
//    Kotlin `Peer` step by step, and after every step its storage must be
//    the Rust one's, key for key and byte for byte. Then the Rust peer's
//    last storage is opened by a Kotlin peer, which must find the state the
//    Rust peer reported.
// 2. The semantics. Every trace — spec/vectors/trace/ and, when named, the
//    corpus — replayed with the client's durable copy kept through a
//    `Storage` in these records (`StoredCopy`) rather than in memory: a
//    reopen opens what the records say, so a record that loses anything a
//    trace's reopen needs fails that trace at the step after it.
package dev.arkdb.client

import dev.arkdb.Alone
import dev.arkdb.Canon
import dev.arkdb.Client
import dev.arkdb.Ctx
import dev.arkdb.Decode
import dev.arkdb.Durable
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.Value
import dev.arkdb.conformance.ClientChecks
import dev.arkdb.conformance.Failed
import dev.arkdb.conformance.Json
import dev.arkdb.reopenFrom
import java.io.File
import kotlin.system.exitProcess

/**
 * A client's durable copy kept through a `Storage` in the records `Storage.kt` writes, by the
 * writer a `Peer` writes them with: after every step the replica is persisted, and a reopen opens
 * what `Records.load` reads back — the store, the cursor, the log, the union and the pending
 * intents all from the records.
 */
class StoredCopy : Durable {
    var storage = MemoryStorage()
    private var writer = ReplicaWriter.fresh(storage)
    private var fork = Fork()

    override fun clear() {
        wrote = emptyMap()
        storage = MemoryStorage()
        writer = ReplicaWriter.fresh(storage)
        fork = Fork()
    }

    // The intents as last written: an intent rewritten in place — a sign-in
    // making nobody's intents somebody's, a join re-stamping the local
    // history — keeps its id, so the writer is told to write them whole, as
    // `Peer.signIn` and `Peer.join` tell it.
    private var wrote: Map<dev.arkdb.Id, dev.arkdb.LogEntry> = emptyMap()

    override fun keep(c: Client, alone: Alone?) {
        if (alone != null) fork = Fork(alone.forkLog, alone.fork)
        val pending = c.replica.pending
        if (pending.any { e -> wrote[e.id]?.let { it != e } == true }) writer.forgetPending()
        writer.persist(c.replica, fork, Ctx.nobody)
        wrote = pending.associateBy { it.id }
    }

    override fun reopen(old: Client): Client {
        val st = Records.load(storage, old.schema) ?: throw Failed("a reopen with nothing stored")
        val f = st.file
        return reopenFrom(old, f.confirmed, f.cursor, st.logId, f.partial, f.pending)
    }
}

object StorageTests {
    private val problems = ArrayList<String>()

    private fun storageOf(v: Value): Map<String, ByteArray> =
        v.asStruct().fields.mapValues { it.value.asBytes() }

    private fun differ(want: Map<String, ByteArray>, got: MemoryStorage): String? {
        val keys = (want.keys + got.keys()).toSortedSet()
        val bad =
            keys.filter { k ->
                val w = want[k]
                val g = got.load(k)
                w == null || g == null || !w.contentEquals(g)
            }
        if (bad.isEmpty()) return null
        return bad.joinToString("; ") { k ->
            val w = want[k]?.let(Hex::encode)?.take(120) ?: "absent"
            val g = got.load(k)?.let(Hex::encode)?.take(120) ?: "absent"
            "$k: rust $w, kotlin $g"
        }
    }

    /** The Rust-written fixture, replayed into a Kotlin peer, storage compared after every step. */
    fun fixture(path: File) {
        val v = Json.parse(path.readText(Charsets.UTF_8))
        val domain = Domain(Decode.module(Canon.decode(v.field("module").asBytes())))
        val storage = MemoryStorage()
        var peer: Peer? = null
        val steps = v.field("steps").asList()
        for ((k, s) in steps.withIndex()) {
            val what = s.field("do").asText()
            fun args(x: Value): Map<String, Value> = x.asStruct().fields
            when (what) {
                "open" -> {
                    peer?.persist()
                    val token =
                        s.field("token").let { if (it is Value.VNull) null else it.asText() }
                    val opts =
                        Options(
                            s.field("user").asText(),
                            s.field("session").asText(),
                            token,
                            s.field("alone").asBool(),
                            Autos.Seeded(s.field("seed").asInt()))
                    peer = Peer.open(domain, storage, opts)
                    val state = s.asStruct().fields["state"]
                    if (state != null) {
                        val p = peer
                        val got =
                            Value.record(
                                "cursor" to Value.VInt(p.cursor),
                                "hash" to Value.VBytes(Hash.stateHash(p.replica.confirmed)),
                                "view_hash" to Value.VBytes(Hash.stateHash(p.replica.view)),
                                "pending" to
                                    Value.VList(p.replica.pending.map { Value.VId(it.id) }))
                        if (got != state)
                            problems.add(
                                "fixture step $k: the Rust peer's storage opens as ${got.show()}, it said ${state.show()}")
                    }
                }
                "mutate" -> {
                    val refused =
                        try {
                            peer!!.mutate(s.field("name").asText(), args(s.field("args")))
                            false
                        } catch (_: Refused) {
                            true
                        }
                    if (refused != s.field("refused").asBool())
                        problems.add(
                            "fixture step $k: refused $refused, the Rust peer ${s.field("refused").asBool()}")
                }
                "persist" -> peer!!.persist()
                "join" ->
                    peer!!.join(
                        s.field("user").asText(),
                        s.field("session").asText(),
                        s.field("token").asText())
                "connected" -> {
                    peer!!.connected()
                    peer.takeOutgoingFrames()
                }
                "disconnected" -> peer!!.disconnected()
                "recv" -> {
                    for (f in s.field("frames").asList()) peer!!.recvFrame(f.asBytes())
                    peer!!.persist()
                    peer.takeOutgoingFrames()
                }
                else -> problems.add("fixture step $k: no such step $what")
            }
            val d = differ(storageOf(s.field("storage")), storage)
            if (d != null) {
                problems.add("fixture step $k ($what): $d")
                return
            }
        }
        println(
            "storage fixture ${path.name}: ${steps.size} steps, every record the Rust peer's, byte for byte")
    }

    /** Every trace replayed with the durable copy kept in these records. */
    fun traces(vectors: File, corpus: String?) {
        val dir = File(vectors, "trace")
        val files =
            (dir.listFiles() ?: emptyArray())
                .filter { it.isFile && it.name.endsWith(".json") }
                .sortedBy { it.name }
        var ran = 0
        for (f in files) {
            try {
                ClientChecks.trace(Json.parse(f.readText(Charsets.UTF_8)), ::StoredCopy)
                ran += 1
            } catch (e: Failed) {
                problems.add("trace/${f.name} through storage: ${e.message}")
            }
        }
        println("trace/: $ran of ${files.size} replayed with the durable copy in storage records")
        for (f in
            (File(dir, "falsify").listFiles() ?: emptyArray()).filter {
                it.name.endsWith(".json")
            }) {
            try {
                ClientChecks.trace(Json.parse(f.readText(Charsets.UTF_8)), ::StoredCopy)
                problems.add("trace/falsify/${f.name} replayed through storage and must not")
            } catch (e: Failed) {
                println("trace/falsify/${f.name}: fails, as it must (${e.message?.take(120)})")
            }
        }
        if (corpus != null) {
            val (n, failed, ms) = ClientChecks.corpus(File(corpus), ::StoredCopy)
            println(
                "trace corpus $corpus through storage: ${n - failed.size} of $n replayed in $ms ms")
            for (f in failed.take(20)) problems.add("corpus through storage: $f")
            if (failed.size > 20) problems.add("… and ${failed.size - 20} more")
            if (n == 0) problems.add("the corpus at $corpus holds no trace")
        }
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val vectors = File(args.getOrElse(0) { "../spec/vectors" })
        val fixture = File(args.getOrElse(1) { "ark-client/fixtures/rust-peer.json" })
        val corpus =
            args.getOrNull(2)?.takeIf { it.isNotEmpty() } ?: System.getenv("ARK_TRACE_CORPUS")
        dev.arkdb.conformance.Conformance.root = vectors
        fixture(fixture)
        traces(vectors, corpus)
        if (problems.isNotEmpty()) {
            println("storage: ${problems.size} problem(s)")
            for (p in problems) println("  $p")
            exitProcess(1)
        }
        println("storage: every record as the reference writes it, and every trace through it")
    }
}
