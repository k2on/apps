// §12 The protocol (Ark.Protocol): the frames as values, and the client
// machine. The server machine is not here.
package dev.arkdb

import java.util.TreeMap

/** How a client holds a scope: replaying intents, or fed the facts. */
public enum class Mode { Whole, ByFacts }

public data class Subscription(val scope: String, val since: Seq, val mode: Mode)

public sealed class ClientMsg {
    public data class Hello(val subs: List<Subscription>, val token: String?, val spec: Int) : ClientMsg()
    public data class Push(val scope: String, val entries: List<Entry>) : ClientMsg()
    public data class NeedFacts(val scope: String, val seqs: List<Seq>) : ClientMsg()
    public data class NeedClosures(val hashes: List<FnHash>) : ClientMsg()
    public class Verify(public val scope: String, public val seq: Seq, public val hash: ByteArray) : ClientMsg() {
        override fun equals(other: Any?): Boolean =
            other is Verify && scope == other.scope && seq == other.seq && hash.contentEquals(other.hash)
        override fun hashCode(): Int = scope.hashCode() * 31 + seq.hashCode()
    }
    public class Say(public val frame: ByteArray) : ClientMsg() {
        override fun equals(other: Any?): Boolean = other is Say && frame.contentEquals(other.frame)
        override fun hashCode(): Int = frame.contentHashCode()
    }

    public fun toValue(): Value = Protocol.clientValue(this)
}

public sealed class ServerMsg {
    public data class Batch(val scope: String, val items: List<Triple<Seq, Entry, Facts?>>, val hasMore: Boolean) : ServerMsg()
    public data class FactsFor(val scope: String, val items: List<Pair<Seq, Facts>>) : ServerMsg()
    public class SnapshotOf(
        public val scope: String,
        public val seq: Seq,
        public val stateHash: ByteArray,
        public val rows: Map<String, List<Value>>,
    ) : ServerMsg() {
        override fun equals(other: Any?): Boolean = other is SnapshotOf && scope == other.scope && seq == other.seq &&
            stateHash.contentEquals(other.stateHash) && rows == other.rows
        override fun hashCode(): Int = scope.hashCode() * 31 + rows.hashCode()
    }
    public data class Ack(val scope: String, val ids: List<Id>, val seqs: List<Seq>) : ServerMsg()
    public data class Reject(val scope: String, val id: Id, val reason: String) : ServerMsg()
    public data class Denied(val reason: String) : ServerMsg()
    public data class Closures(val items: List<Pair<FnHash, Closure>>) : ServerMsg()
    public class Agree(public val scope: String, public val seq: Seq, public val hash: ByteArray, public val ok: Boolean) : ServerMsg() {
        override fun equals(other: Any?): Boolean = other is Agree && scope == other.scope && seq == other.seq &&
            hash.contentEquals(other.hash) && ok == other.ok
        override fun hashCode(): Int = scope.hashCode() * 31 + seq.hashCode()
    }
    public class Heard(public val frame: ByteArray) : ServerMsg() {
        override fun equals(other: Any?): Boolean = other is Heard && frame.contentEquals(other.frame)
        override fun hashCode(): Int = frame.contentHashCode()
    }

    public fun toValue(): Value = Protocol.serverValue(this)
}

public object Protocol {
    /** Entries per page. */
    public const val BATCH_LIMIT: Int = 256

    private fun node(t: String, vararg fs: Pair<String, Value>): Value =
        Value.VStruct(mapOf("t" to Value.VText(t), *fs))

    private fun int(n: Long): Value = Value.VInt(n)

    public fun entryValue(e: Entry): Value = Value.record(
        "id" to Value.VId(e.id),
        "actor" to Value.VText(e.actor),
        "session" to Value.VText(e.session),
        "fn" to Value.VBytes(e.fn.bytes),
        "args" to Value.VStruct(e.args),
        "autos" to Value.VStruct(e.autos),
    )

    public fun changeValue(ch: Change): Value = when (ch) {
        is Change.Add -> node("add", "table" to Value.VText(ch.table), "row" to ch.row)
        is Change.Remove -> node("remove", "table" to Value.VText(ch.table), "row" to ch.row)
        is Change.Edit -> node("edit", "table" to Value.VText(ch.table), "old" to ch.old, "new" to ch.new)
    }

    public fun factsValue(f: Facts): Value = Value.VList(f.map { changeValue(it) })

    public fun clientValue(m: ClientMsg): Value = when (m) {
        is ClientMsg.Hello -> node(
            "hello",
            "scopes" to Value.VList(
                m.subs.map { s ->
                    node(
                        "sub",
                        "scope" to Value.VText(s.scope),
                        "since" to int(s.since),
                        "mode" to Value.VText(if (s.mode == Mode.Whole) "whole" else "facts"),
                    )
                },
            ),
            "token" to (m.token?.let { Value.VText(it) } ?: Value.VNull),
            "spec" to int(m.spec.toLong()),
        )
        is ClientMsg.Push -> node("push", "scope" to Value.VText(m.scope), "entries" to Value.VList(m.entries.map { entryValue(it) }))
        is ClientMsg.NeedFacts -> node("need_facts", "scope" to Value.VText(m.scope), "seqs" to Value.VList(m.seqs.map { int(it) }))
        is ClientMsg.NeedClosures -> node("need_closures", "hashes" to Value.VList(m.hashes.map { Value.VBytes(it.bytes) }))
        is ClientMsg.Verify -> node("verify", "scope" to Value.VText(m.scope), "seq" to int(m.seq), "hash" to Value.VBytes(m.hash))
        is ClientMsg.Say -> node("say", "say" to Value.VBytes(m.frame))
    }

    public fun serverValue(m: ServerMsg): Value = when (m) {
        is ServerMsg.Batch -> node(
            "batch",
            "scope" to Value.VText(m.scope),
            "items" to Value.VList(
                m.items.map { (n, e, f) ->
                    Value.record("seq" to int(n), "entry" to entryValue(e), "facts" to (f?.let { factsValue(it) } ?: Value.VNull))
                },
            ),
            "has_more" to Value.VBool(m.hasMore),
        )
        is ServerMsg.FactsFor -> node(
            "facts",
            "scope" to Value.VText(m.scope),
            "items" to Value.VList(m.items.map { (n, f) -> Value.record("seq" to int(n), "facts" to factsValue(f)) }),
        )
        is ServerMsg.SnapshotOf -> node(
            "snapshot",
            "scope" to Value.VText(m.scope),
            "seq" to int(m.seq),
            "hash" to Value.VBytes(m.stateHash),
            "rows" to Value.VStruct(m.rows.mapValues { Value.VList(it.value) }),
        )
        is ServerMsg.Ack -> node(
            "ack",
            "scope" to Value.VText(m.scope),
            "ids" to Value.VList(m.ids.map { Value.VId(it) }),
            "seqs" to Value.VList(m.seqs.map { int(it) }),
        )
        is ServerMsg.Reject -> node("reject", "scope" to Value.VText(m.scope), "id" to Value.VId(m.id), "reason" to Value.VText(m.reason))
        is ServerMsg.Denied -> node("denied", "reason" to Value.VText(m.reason))
        is ServerMsg.Closures -> node(
            "closures",
            "items" to Value.VList(m.items.map { (h, c) -> Value.record("hash" to Value.VBytes(h.bytes), "closure" to Hash.closureValue(c)) }),
        )
        is ServerMsg.Agree -> node(
            "agree",
            "scope" to Value.VText(m.scope),
            "seq" to int(m.seq),
            "hash" to Value.VBytes(m.hash),
            "ok" to Value.VBool(m.ok),
        )
        is ServerMsg.Heard -> node("heard", "hear" to Value.VBytes(m.frame))
    }

    // Decoding ----------------------------------------------------------------

    private fun bad(what: String): Nothing = throw Decode.DecodeError(listOf("frame"), what)

    private fun struct(v: Value): Map<String, Value> = (v as? Value.VStruct)?.fields ?: bad("expected a struct")

    private fun need(m: Map<String, Value>, k: String): Value = m[k] ?: bad("missing $k")

    private fun text(v: Value): String = (v as? Value.VText)?.value ?: bad("expected text")

    private fun bytes(v: Value): ByteArray = (v as? Value.VBytes)?.value ?: bad("expected bytes")

    private fun int64(v: Value): Long = (v as? Value.VInt)?.value ?: bad("expected an int")

    private fun bool(v: Value): Boolean = (v as? Value.VBool)?.value ?: bad("expected a bool")

    private fun <A> list(v: Value, f: (Value) -> A): List<A> = (v as? Value.VList)?.items?.map(f) ?: bad("expected a list")

    private fun ident(v: Value): Id = (v as? Value.VId)?.value ?: bad("expected an id")

    private fun row(v: Value): Row = (v as? Value.VStruct) ?: bad("expected a struct")

    public fun entryFromValue(v: Value): Entry {
        val m = struct(v)
        return Entry(
            ident(need(m, "id")),
            text(need(m, "actor")),
            text(need(m, "session")),
            FnHash(bytes(need(m, "fn"))),
            struct(need(m, "args")),
            struct(need(m, "autos")),
        )
    }

    public fun changeFromValue(v: Value): Change {
        val m = struct(v)
        val t = text(need(m, "t"))
        val tbl = text(need(m, "table"))
        return when (t) {
            "add" -> Change.Add(tbl, row(need(m, "row")))
            "remove" -> Change.Remove(tbl, row(need(m, "row")))
            "edit" -> Change.Edit(tbl, row(need(m, "old")), row(need(m, "new")))
            else -> bad("unknown change $t")
        }
    }

    public fun clientFromValue(v: Value): ClientMsg {
        val m = struct(v)
        return when (val t = text(need(m, "t"))) {
            "hello" -> ClientMsg.Hello(
                list(need(m, "scopes")) { x ->
                    val sm = struct(x)
                    val md = when (val ms = text(need(sm, "mode"))) {
                        "whole" -> Mode.Whole
                        "facts" -> Mode.ByFacts
                        else -> bad("unknown mode $ms")
                    }
                    Subscription(text(need(sm, "scope")), int64(need(sm, "since")), md)
                },
                need(m, "token").let { if (it is Value.VNull) null else text(it) },
                Math.toIntExact(int64(need(m, "spec"))),
            )
            "push" -> ClientMsg.Push(text(need(m, "scope")), list(need(m, "entries")) { entryFromValue(it) })
            "need_facts" -> ClientMsg.NeedFacts(text(need(m, "scope")), list(need(m, "seqs")) { int64(it) })
            "need_closures" -> ClientMsg.NeedClosures(list(need(m, "hashes")) { FnHash(bytes(it)) })
            "verify" -> ClientMsg.Verify(text(need(m, "scope")), int64(need(m, "seq")), bytes(need(m, "hash")))
            "say" -> ClientMsg.Say(bytes(need(m, "say")))
            else -> bad("unknown client frame $t")
        }
    }

    public fun serverFromValue(v: Value): ServerMsg {
        val m = struct(v)
        return when (val t = text(need(m, "t"))) {
            "batch" -> ServerMsg.Batch(
                text(need(m, "scope")),
                list(need(m, "items")) { x ->
                    val im = struct(x)
                    Triple(
                        int64(need(im, "seq")),
                        entryFromValue(need(im, "entry")),
                        need(im, "facts").let { f -> if (f is Value.VNull) null else list(f) { changeFromValue(it) } },
                    )
                },
                bool(need(m, "has_more")),
            )
            "facts" -> ServerMsg.FactsFor(
                text(need(m, "scope")),
                list(need(m, "items")) { x ->
                    val im = struct(x)
                    int64(need(im, "seq")) to list(need(im, "facts")) { changeFromValue(it) }
                },
            )
            "snapshot" -> ServerMsg.SnapshotOf(
                text(need(m, "scope")),
                int64(need(m, "seq")),
                bytes(need(m, "hash")),
                struct(need(m, "rows")).mapValues { list(it.value) { v -> v } },
            )
            "ack" -> ServerMsg.Ack(text(need(m, "scope")), list(need(m, "ids")) { ident(it) }, list(need(m, "seqs")) { int64(it) })
            "reject" -> ServerMsg.Reject(text(need(m, "scope")), ident(need(m, "id")), text(need(m, "reason")))
            "denied" -> ServerMsg.Denied(text(need(m, "reason")))
            "closures" -> ServerMsg.Closures(
                list(need(m, "items")) { x ->
                    val im = struct(x)
                    FnHash(bytes(need(im, "hash"))) to Decode.closureFromValue(need(im, "closure"))
                },
            )
            "agree" -> ServerMsg.Agree(text(need(m, "scope")), int64(need(m, "seq")), bytes(need(m, "hash")), bool(need(m, "ok")))
            "heard" -> ServerMsg.Heard(bytes(need(m, "hear")))
            else -> bad("unknown server frame $t")
        }
    }
}

/** A peer's end of one connection: its replicas, and what it has queued. */
public class Client(public val schema: Schema, public val token: String?) {
    /** The scopes held, in scope order, each with the mode it is held in. */
    public val scopes: TreeMap<String, Pair<Replica, Mode>> = TreeMap(CodePointOrder)

    public var linked: Boolean = false
        private set

    /** Counts connections, so a live room that has never heard of this device can be told apart. */
    public var epoch: Int = 0
        private set

    private val out: MutableList<ClientMsg> = ArrayList()
    private val heard: MutableList<ByteArray> = ArrayList()

    public var denied: String? = null
        private set

    public val agreed: MutableList<Triple<String, Seq, Boolean>> = ArrayList()

    /** Hold a scope, with the replica as opened from what was durable. */
    public fun subscribe(mode: Mode, r: Replica) {
        scopes[r.scope] = r to mode
    }

    public fun replica(scope: String): Replica? = scopes[scope]?.first

    // Unlinked, nothing is queued; `connected` says it all again.
    private fun emit(m: ClientMsg) {
        if (linked) out.add(m)
    }

    /** §12.1 A connection opened: hello for every scope at its cursor, then everything pending. */
    public fun connected() {
        linked = true
        epoch += 1
        out.clear()
        heard.clear()
        emit(ClientMsg.Hello(scopes.map { (s, rm) -> Subscription(s, rm.first.cursor, rm.second) }, token, SPEC_VERSION))
        for ((s, rm) in scopes) if (rm.first.pending.isNotEmpty()) emit(ClientMsg.Push(s, rm.first.pending))
    }

    public fun disconnected() {
        linked = false
        out.clear()
        heard.clear()
    }

    /** Author an intent into a scope and push it if linked. Throws `Fault.Refuse`. */
    public fun mutate(scope: String, i: Id, ctx: Ctx, fh: FnHash, autos: Args, args: Args): Entry {
        val (r, _) = scopes[scope] ?: throw Fault.Refuse(Refusal.Refused("not holding scope $scope"))
        val e = r.mutate(i, ctx, fh, autos, args)
        emit(ClientMsg.Push(scope, listOf(e)))
        return e
    }

    /** §12.2 A frame from the server. */
    public fun recv(m: ServerMsg) {
        when (m) {
            is ServerMsg.Heard -> heard.add(m.frame)
            is ServerMsg.Denied -> {
                denied = m.reason
                linked = false
                out.clear()
            }
            is ServerMsg.Batch -> {
                val (r, md) = scopes[m.scope] ?: return
                for ((n, e, f) in m.items) if (f != null) r.receiveWith(n, e, f) else r.receive(n, e)
                val needs = r.needs()
                if (needs.isNotEmpty()) emit(ClientMsg.NeedFacts(m.scope, needs))
                if (m.hasMore) emit(ClientMsg.Hello(listOf(Subscription(m.scope, r.cursor, md)), token, SPEC_VERSION))
            }
            is ServerMsg.FactsFor -> {
                val (r, _) = scopes[m.scope] ?: return
                for ((n, f) in m.items) r.receiveFacts(n, f)
            }
            is ServerMsg.SnapshotOf -> {
                // Below the horizon: the confirmed store is replaced by the
                // snapshot and the cursor moves to it; pending replays on top.
                val (r, md) = scopes[m.scope] ?: return
                val st = MemoryStore.of(schema, m.rows)
                scopes[m.scope] = Replica.open(r.schema, m.scope, r.bodies, st, m.seq, r.pending, r.natives) to md
            }
            is ServerMsg.Ack -> {
                val (r, _) = scopes[m.scope] ?: return
                for ((i, n) in m.ids.zip(m.seqs)) r.ack(i, n)
            }
            is ServerMsg.Reject -> {
                val (r, _) = scopes[m.scope] ?: return
                r.reject(m.id, Refusal.Refused(m.reason))
            }
            is ServerMsg.Closures -> {
                // New closures may unblock entries waiting in an inbox.
                val more = m.items.toMap()
                for ((r, _) in scopes.values) {
                    r.learn(more)
                    r.retry()
                }
            }
            is ServerMsg.Agree -> agreed.add(Triple(m.scope, m.seq, m.ok))
        }
    }

    /** A live frame; dropped while unlinked, never queued. */
    public fun say(frame: ByteArray): Unit = emit(ClientMsg.Say(frame))

    /** Ask the authority whether it agrees with every replica's confirmed state. */
    public fun verifyAll() {
        for ((s, rm) in scopes) {
            val (n, h) = rm.first.verifyAt()
            emit(ClientMsg.Verify(s, n, h))
        }
    }

    public fun takeOutgoing(): List<ClientMsg> {
        val o = out.toList()
        out.clear()
        return o
    }

    public fun takeHeard(): List<ByteArray> {
        val h = heard.toList()
        heard.clear()
        return h
    }
}
