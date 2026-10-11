// §12 The protocol, as `rust/ark/src/protocol.rs` defines it: the frames two
// peers exchange, as values (so that `Canon` is their wire form), and the
// client's state machine around them — a `Client` holding a replica of the
// log and what it has queued. Sans-io: frames go in as values or bytes and
// come out as values, and the driver owns the socket.
//
// A client writes every client frame and reads every server frame, so both
// directions are here, encoded and decoded exactly as the reference does,
// each optional field written only where the reference writes it — a frame
// a runtime writes is the bytes Rust writes for the same frame. What a
// server does with them (`Server`) is not a client's, and is not here.
package dev.arkdb

import java.util.TreeMap

/** How a client holds the log (`protocol::Mode`): `Whole` replays; `ByFacts` is fed facts. */
public enum class Mode(public val wire: String) {
    Whole("whole"),
    ByFacts("facts"),
}

/** A `hello`'s subscription (`protocol::Subscription`). */
public data class Subscription(
    /** The cursor: the last sequence applied. */
    val since: Seq,
    val mode: Mode,
    /** Which log `since` is of, as the peer last heard it named; `log` on the wire, or absent. */
    val logId: Id?,
    /** D2 The peer holds a union: `partial: true` on the wire, absent otherwise. */
    val partial: Boolean,
)

/** What a partial page covers (`protocol::Covers`): the sequences after `after` through `upto`. */
public data class Covers(val after: Seq, val upto: Seq)

/** A frame a client writes (`protocol::ClientMsg`). */
public sealed class ClientMsg {
    public data class Hello(val sub: Subscription, val token: String?, val spec: Long) :
        ClientMsg()

    public data class Push(val entries: List<LogEntry>) : ClientMsg()

    public data class NeedFacts(val seqs: List<Seq>) : ClientMsg()

    public class NeedClosures(public val hashes: List<ByteArray>) : ClientMsg()

    /** Whether the authority holds the state `hash` at `seq`, of the log `logId`. */
    public class Verify(
        public val seq: Seq,
        public val hash: ByteArray,
        public val logId: Id?,
        public val partial: Boolean
    ) : ClientMsg()

    public class Say(public val frame: ByteArray) : ClientMsg()

    /** The frame as a value; its wire form is `Canon.encode` of this. */
    public fun toValue(): Value =
        when (this) {
            is Hello -> {
                val f =
                    mutableListOf<Pair<String, Value>>(
                        "since" to Value.VInt(sub.since),
                        "mode" to Value.VText(sub.mode.wire),
                        "token" to (token?.let { Value.VText(it) } ?: Value.VNull),
                        "spec" to Value.VInt(spec))
                named(f, sub.logId)
                flagged(f, "partial", sub.partial)
                node("hello", f)
            }
            is Push -> node("push", listOf("entries" to Value.VList(entries.map(::entryValue))))
            is NeedFacts ->
                node("need_facts", listOf("seqs" to Value.VList(seqs.map { Value.VInt(it) })))
            is NeedClosures ->
                node(
                    "need_closures",
                    listOf("hashes" to Value.VList(hashes.map { Value.VBytes(it) })))
            is Verify -> {
                val f = mutableListOf("seq" to Value.VInt(seq), "hash" to Value.VBytes(hash))
                named(f, logId)
                flagged(f, "partial", partial)
                node("verify", f)
            }
            is Say -> node("say", listOf("say" to Value.VBytes(frame)))
        }

    /** Its canonical bytes. */
    public fun encode(): ByteArray = Canon.encode(toValue())

    public companion object {
        /** `ClientMsg::from_value`. */
        public fun fromValue(v: Value): ClientMsg {
            val m = strctOf(v)
            return when (val t = text(need(m, "t"))) {
                "hello" ->
                    Hello(
                        sub(m),
                        when (val tok = need(m, "token")) {
                            is Value.VNull -> null
                            else -> text(tok)
                        },
                        int64(need(m, "spec")))
                "push" -> Push(list(need(m, "entries"), ::entryFromValue))
                "need_facts" -> NeedFacts(list(need(m, "seqs"), ::int64))
                "need_closures" -> NeedClosures(list(need(m, "hashes"), ::bytes))
                "verify" ->
                    Verify(
                        int64(need(m, "seq")), bytes(need(m, "hash")), logOf(m), flag(m, "partial"))
                "say" -> Say(bytes(need(m, "say")))
                else -> bad("unknown client frame $t")
            }
        }

        private fun sub(m: Map<String, Value>): Subscription {
            val mode =
                when (val w = text(need(m, "mode"))) {
                    "whole" -> Mode.Whole
                    "facts" -> Mode.ByFacts
                    else -> bad("unknown mode $w")
                }
            return Subscription(int64(need(m, "since")), mode, logOf(m), flag(m, "partial"))
        }
    }
}

/** A frame a client reads (`protocol::ServerMsg`). */
public sealed class ServerMsg {
    /**
     * A page: entries with their facts where the peer is fed facts; the log they are of, the
     * server's module hash, and — for a peer that is not whole — the sequences it covers.
     */
    public class Batch(
        public val items: List<Triple<Seq, LogEntry, FactList?>>,
        public val hasMore: Boolean,
        public val logId: Id?,
        public val module: ByteArray?,
        public val covers: Covers?,
    ) : ServerMsg()

    public class FactsFor(public val items: List<Pair<Seq, FactList>>) : ServerMsg()

    /** The state at `seq` as rows by table; `held`, a partial snapshot's tables held in part. */
    public class SnapshotOf(
        public val seq: Seq,
        public val hash: ByteArray,
        public val rows: Map<String, List<Value>>,
        public val logId: Id?,
        public val module: ByteArray?,
        public val held: Map<String, List<String>>?,
    ) : ServerMsg()

    /** Acknowledgements; `facts`, the facts of entries stamped with other roles (D1). */
    public class Ack(
        public val ids: List<Id>,
        public val seqs: List<Seq>,
        public val logId: Id?,
        public val facts: List<Pair<Seq, FactList>>,
    ) : ServerMsg()

    public class Reject(public val id: Id, public val reason: String) : ServerMsg()

    /** An intent the server cannot run yet (`docs/plan-db.md` D1): kept pending. */
    public class Held(public val id: Id, public val reason: String) : ServerMsg()

    public class Denied(public val reason: String) : ServerMsg()

    public class Closures(public val items: List<Pair<ByteArray, Closure>>) : ServerMsg()

    /** The answer to a `verify`; `unknown`, the authority could not say. */
    public class Agree(
        public val seq: Seq,
        public val hash: ByteArray,
        public val ok: Boolean,
        public val unknown: Boolean
    ) : ServerMsg()

    public class Heard(public val frame: ByteArray) : ServerMsg()

    /** The frame as a value. */
    public fun toValue(): Value =
        when (this) {
            is Batch -> {
                val f =
                    mutableListOf<Pair<String, Value>>(
                        "items" to
                            Value.VList(
                                items.map { (n, e, fs) ->
                                    Value.record(
                                        "seq" to Value.VInt(n),
                                        "entry" to entryValue(e),
                                        "facts" to (fs?.let(::factsValue) ?: Value.VNull))
                                }),
                        "has_more" to Value.VBool(hasMore))
                named(f, logId)
                ofModule(f, module)
                if (covers != null) {
                    f.add("after" to Value.VInt(covers.after))
                    f.add("upto" to Value.VInt(covers.upto))
                }
                node("batch", f)
            }
            is FactsFor -> node("facts", listOf("items" to seqFactsValue(items)))
            is SnapshotOf -> {
                val f =
                    mutableListOf<Pair<String, Value>>(
                        "seq" to Value.VInt(seq),
                        "hash" to Value.VBytes(hash),
                        "rows" to Value.VStruct(rows.mapValues { Value.VList(it.value) }))
                named(f, logId)
                ofModule(f, module)
                if (held != null) {
                    f.add("partial" to Value.VBool(true))
                    f.add("held" to heldValue(held))
                }
                node("snapshot", f)
            }
            is Ack -> {
                val f =
                    mutableListOf<Pair<String, Value>>(
                        "ids" to Value.VList(ids.map { Value.VId(it) }),
                        "seqs" to Value.VList(seqs.map { Value.VInt(it) }))
                named(f, logId)
                if (facts.isNotEmpty()) f.add("facts" to seqFactsValue(facts))
                node("ack", f)
            }
            is Reject ->
                node("reject", listOf("id" to Value.VId(id), "reason" to Value.VText(reason)))
            is Held -> node("held", listOf("id" to Value.VId(id), "reason" to Value.VText(reason)))
            is Denied -> node("denied", listOf("reason" to Value.VText(reason)))
            is Closures ->
                node(
                    "closures",
                    listOf(
                        "items" to
                            Value.VList(
                                items.map { (h, c) ->
                                    Value.record(
                                        "hash" to Value.VBytes(h), "closure" to Encode.closure(c))
                                })))
            is Agree -> {
                val f =
                    mutableListOf<Pair<String, Value>>(
                        "seq" to Value.VInt(seq),
                        "hash" to Value.VBytes(hash),
                        "ok" to Value.VBool(ok))
                if (unknown) f.add("unknown" to Value.VBool(true))
                node("agree", f)
            }
            is Heard -> node("heard", listOf("hear" to Value.VBytes(frame)))
        }

    public companion object {
        /** `ServerMsg::from_value`. */
        public fun fromValue(v: Value): ServerMsg {
            val m = strctOf(v)
            return when (val t = text(need(m, "t"))) {
                "batch" ->
                    Batch(
                        list(need(m, "items")) { x ->
                            val im = strctOf(x)
                            val facts =
                                when (val f = need(im, "facts")) {
                                    is Value.VNull -> null
                                    else -> list(f, ::changeFromValue)
                                }
                            Triple(int64(need(im, "seq")), entryFromValue(need(im, "entry")), facts)
                        },
                        boolean(need(m, "has_more")),
                        logOf(m),
                        moduleOf(m),
                        coversOf(m))
                "facts" -> FactsFor(seqFactsOf(need(m, "items")))
                "snapshot" ->
                    SnapshotOf(
                        int64(need(m, "seq")),
                        bytes(need(m, "hash")),
                        strctOf(need(m, "rows")).mapValues { (_, vs) -> list(vs) { it } },
                        logOf(m),
                        moduleOf(m),
                        heldOf(m))
                "ack" ->
                    Ack(
                        list(need(m, "ids"), ::ident),
                        list(need(m, "seqs"), ::int64),
                        logOf(m),
                        when (val f = m["facts"]) {
                            null -> emptyList()
                            else ->
                                seqFactsOf(f).also {
                                    if (it.isEmpty())
                                        bad("ack: empty facts are written as no field")
                                }
                        })
                "reject" -> Reject(ident(need(m, "id")), text(need(m, "reason")))
                "held" -> Held(ident(need(m, "id")), text(need(m, "reason")))
                "denied" -> Denied(text(need(m, "reason")))
                "closures" ->
                    Closures(
                        list(need(m, "items")) { x ->
                            val im = strctOf(x)
                            bytes(need(im, "hash")) to Decode.closure(need(im, "closure"))
                        })
                "agree" ->
                    Agree(
                        int64(need(m, "seq")),
                        bytes(need(m, "hash")),
                        boolean(need(m, "ok")),
                        when (m["unknown"]) {
                            null -> false
                            Value.VBool(true) -> true
                            else -> bad("an agree's unknown is true or absent")
                        })
                "heard" -> Heard(bytes(need(m, "hear")))
                else -> bad("unknown server frame $t")
            }
        }

        /**
         * `ServerMsg::decode_for`: a frame from its bytes, for a client of `schema` — the one way a
         * client reads the socket: a snapshot with its rows built (`Snapshot.ofValues`), any other
         * frame as `fromValue` reads it.
         */
        public fun decodeFor(bytes: ByteArray, schema: Schema): Received =
            when (val msg = fromValue(Canon.decode(bytes))) {
                is SnapshotOf ->
                    Received.Snap(
                        Snapshot.ofValues(
                            schema, msg.seq, msg.rows, msg.logId, msg.module, msg.held))
                else -> Received.Msg(msg)
            }
    }
}

/**
 * D7.4 What a `snapshot` frame brings a client (`protocol::Snapshot`): the state at `seq` as its
 * rows — each a struct, what is not one dropped, a table left with none not named — the log, the
 * server's module, and the tables a partial one holds in part.
 */
public class Snapshot(
    public val seq: Seq,
    public val rows: Map<String, List<Row>>,
    public val logId: Id?,
    public val module: ByteArray?,
    public val held: Map<String, List<String>>?,
) {
    /** The same snapshot, as a value — for comparing two of them. */
    public fun toValue(): Value {
        val f =
            mutableListOf<Pair<String, Value>>(
                "seq" to Value.VInt(seq),
                "rows" to Value.VStruct(rows.mapValues { Value.VList(it.value) }))
        named(f, logId)
        ofModule(f, module)
        if (held != null) f.add("held" to heldValue(held))
        return Value.record(f)
    }

    public companion object {
        /** `Snapshot::of_values`. */
        public fun ofValues(
            @Suppress("UNUSED_PARAMETER") schema: Schema,
            seq: Seq,
            rows: Map<String, List<Value>>,
            logId: Id?,
            module: ByteArray?,
            held: Map<String, List<String>>?,
        ): Snapshot {
            val out = TreeMap<String, List<Row>>(CodePointOrder)
            for ((t, vs) in rows) {
                val rs = vs.filterIsInstance<Value.VStruct>()
                if (rs.isNotEmpty()) out[t] = rs
            }
            return Snapshot(seq, out, logId, module, held)
        }
    }
}

/** A server frame as a client of a known schema reads it (`protocol::Received`). */
public sealed class Received {
    public class Msg(public val msg: ServerMsg) : Received()

    public class Snap(public val snapshot: Snapshot) : Received()
}

// The wire's spelling ---------------------------------------------------------

private fun node(t: String, fields: List<Pair<String, Value>>): Value =
    Value.VStruct(
        LinkedHashMap<String, Value>().apply {
            put("t", Value.VText(t))
            for ((k, v) in fields) put(k, v)
        })

// `named`: a log's identity on the wire — `log`, an id, absent where none is known.
private fun named(f: MutableList<Pair<String, Value>>, id: Id?) {
    if (id != null) f.add("log" to Value.VId(id))
}

// `of_module`: the server's module hash — `module`, bytes, absent where not said.
private fun ofModule(f: MutableList<Pair<String, Value>>, module: ByteArray?) {
    if (module != null) f.add("module" to Value.VBytes(module))
}

// `flagged`: a flag that is `true` when present; absent is the one encoding of `false`.
private fun flagged(f: MutableList<Pair<String, Value>>, name: String, on: Boolean) {
    if (on) f.add(name to Value.VBool(true))
}

private fun heldValue(h: Map<String, List<String>>): Value =
    Value.VStruct(h.mapValues { (_, cs) -> Value.VList(cs.map { Value.VText(it) }) })

/**
 * `entry_value`: an entry as a value — its wire form in a `push` or a `batch`, and its form on
 * disk. `roles` is present only when the entry holds one (D1).
 */
public fun entryValue(e: LogEntry): Value {
    val m = LinkedHashMap<String, Value>()
    m["id"] = Value.VId(e.id)
    m["actor"] = Value.VText(e.actor)
    m["session"] = Value.VText(e.session)
    m["fn"] = Value.VBytes(e.fnHash)
    m["args"] = Value.VStruct(e.args)
    m["autos"] = Value.VStruct(e.autos)
    if (e.roles.isNotEmpty()) m["roles"] = Value.VList(e.roles.map { Value.VText(it) })
    return Value.VStruct(m)
}

/** `entry_from_value`. */
public fun entryFromValue(v: Value): LogEntry {
    val m = strctOf(v)
    return LogEntry(
        ident(need(m, "id")),
        text(need(m, "actor")),
        text(need(m, "session")),
        rolesOf(m),
        bytes(need(m, "fn")),
        strctOf(need(m, "args")),
        strctOf(need(m, "autos")))
}

// `roles_of`: none where absent (an empty list refused), otherwise texts in
// strictly ascending order.
private fun rolesOf(m: Map<String, Value>): List<String> {
    val v = m["roles"] ?: return emptyList()
    val rs = list(v, ::text)
    if (rs.isEmpty()) bad("roles: an empty list is written as no field")
    for (i in 1 until rs.size) if (CodePointOrder.compare(rs[i - 1], rs[i]) >= 0)
        bad("roles: not in ascending order, or repeated")
    return rs
}

/** `change_value`. */
public fun changeValue(c: Change): Value = Facts.changeValue(c)

/** `change_from_value`, refusing what is not a change with a frame's error. */
public fun changeFromValue(v: Value): Change {
    val m = strctOf(v)
    val t = text(need(m, "t"))
    val tbl = text(need(m, "table"))
    fun row(k: String): Row = need(m, k) as? Value.VStruct ?: bad("expected a struct")
    return when (t) {
        "add" -> Change.Add(tbl, row("row"))
        "remove" -> Change.Remove(tbl, row("row"))
        "edit" -> Change.Edit(tbl, row("old"), row("new"))
        else -> bad("unknown change $t")
    }
}

/** A list of facts as a value: a list of changes. */
public fun factsValue(f: FactList): Value = Value.VList(f.map(::changeValue))

private fun seqFactsValue(items: List<Pair<Seq, FactList>>): Value =
    Value.VList(
        items.map { (n, f) -> Value.record("seq" to Value.VInt(n), "facts" to factsValue(f)) })

private fun seqFactsOf(v: Value): List<Pair<Seq, FactList>> =
    list(v) { x ->
        val m = strctOf(x)
        int64(need(m, "seq")) to list(need(m, "facts"), ::changeFromValue)
    }

private fun flag(m: Map<String, Value>, name: String): Boolean =
    when (m[name]) {
        null -> false
        Value.VBool(true) -> true
        else -> bad("$name is true or absent")
    }

private fun coversOf(m: Map<String, Value>): Covers? {
    val a = m["after"]
    val u = m["upto"]
    return when {
        a == null && u == null -> null
        a != null && u != null -> Covers(int64(a), int64(u))
        else -> bad("a page's after and upto go together")
    }
}

private fun heldOf(m: Map<String, Value>): Map<String, List<String>>? {
    val partial = flag(m, "partial")
    val h = m["held"]
    return when {
        !partial && h == null -> null
        partial && h != null -> {
            val out = TreeMap<String, List<String>>(CodePointOrder)
            for ((t, cs) in strctOf(h)) out[t] = list(cs, ::text)
            out
        }
        else -> bad("a snapshot's partial and held go together")
    }
}

private fun logOf(m: Map<String, Value>): Id? = m["log"]?.let(::ident)

private fun moduleOf(m: Map<String, Value>): ByteArray? = m["module"]?.let(::bytes)

// Decoding primitives, each refusing with the frame's error (`protocol::DecodeError`).

private fun bad(what: String): Nothing = throw FactError("frame: $what")

private fun strctOf(v: Value): Map<String, Value> =
    (v as? Value.VStruct)?.fields ?: bad("expected a struct")

private fun need(m: Map<String, Value>, k: String): Value = m[k] ?: bad("missing $k")

private fun text(v: Value): String = (v as? Value.VText)?.value ?: bad("expected text")

private fun bytes(v: Value): ByteArray = (v as? Value.VBytes)?.value ?: bad("expected bytes")

private fun int64(v: Value): Long = (v as? Value.VInt)?.value ?: bad("expected an int")

private fun boolean(v: Value): Boolean = (v as? Value.VBool)?.value ?: bad("expected a bool")

private fun ident(v: Value): Id = (v as? Value.VId)?.value ?: bad("expected an id")

private fun <T> list(v: Value, f: (Value) -> T): List<T> =
    (v as? Value.VList)?.items?.map(f) ?: bad("expected a list")

/**
 * What an intent naming a function no closure is held for is answered with
 * (`protocol::unknown_function`): a `held` from a server that holds, and before that a `reject`
 * with this reason, which a `Client` reads as the hold it was.
 */
public fun unknownFunction(fh: ByteArray): String = "unknown function ${Hex.encode(fh)}"

/**
 * §12.5 `refusal_text`: the reason a `reject` carries — a mutator's own refusal word for word, a
 * constraint named in a sentence.
 */
public fun refusalText(r: Refusal): String =
    when (r) {
        is Refusal.Refused -> r.why
        is Refusal.NoSuchTable -> "no table ${r.table}"
        is Refusal.MalformedRow -> "${r.table}: ${r.why}"
        is Refusal.NotNull -> "${r.table}.${r.column} may not be empty"
        is Refusal.UniqueViolation ->
            "${r.table}: another row has the same ${r.columns.joinToString(", ")}"
        is Refusal.MissingParent -> "${r.table}.${r.column} names no ${r.parent}"
        is Refusal.StillReferenced -> "${r.table}: still referenced by ${r.child}"
        is Refusal.Forbidden -> "${r.table}: not this login's to write"
    }

// ---------------------------------------------------------------------------
// The client

/**
 * A peer's end of one connection (`protocol::Client`): its replica of the log, and what it has
 * queued. The fields are the Rust's, each documented there.
 */
public class Client(
    public var replica: Replica,
    public var mode: Mode,
    /** The token every `hello` carries. */
    public var token: String?,
) {
    /** The module's schema: what every later snapshot's device schema is made from (D2). */
    public var schema: Schema = replica.schema
    public var linked: Boolean = false

    /** Counts connections. */
    public var epoch: Long = 0

    /** Oldest first. */
    public val out: MutableList<ClientMsg> = ArrayList()
    public val heard: MutableList<ByteArray> = ArrayList()
    public var denied: String? = null

    /** Every answer to a `verify`, oldest first: true, false, or null where it could not say. */
    public val agreed: MutableList<Pair<Seq, Boolean?>> = ArrayList()

    /** A page arrived since the last `settle`. */
    public var paged: Boolean = false

    /** A page said there is more: the `hello` asking for it is said at the settle. */
    public var more: Boolean = false

    /** This peer's own module hash, where the driver says it. */
    public var module: ByteArray? = null

    /** The server's module hash, as its last page or snapshot said it (D1). */
    public var serverModule: ByteArray? = null

    /** Pending intents the server answered `held` (D1). */
    public val heldIds: MutableSet<Id> = HashSet()

    /** D2 This connection has been sent a page or a snapshot. */
    public var served: Boolean = false

    /** This connection has brought a snapshot. */
    public var started: Boolean = false

    /** `with_schema`: the module's schema, for a client opened over a union. */
    public fun withSchema(module: Schema): Client = apply { schema = module }

    /** §12 `behind`: both module hashes are known and they differ. */
    public fun behind(): Boolean {
        val ours = module
        val theirs = serverModule
        return ours != null && theirs != null && !ours.contentEquals(theirs)
    }

    /** Pending intents the server has answered `held`, and nothing since. */
    public fun held(): Int = replica.pending.count { it.id in heldIds }

    private fun heardModule(m: ByteArray?) {
        if (m != null) serverModule = m
        replica.behind = behind()
    }

    private fun holdIntent(id: Id) {
        if (replica.pending.any { it.id == id }) heldIds.add(id)
    }

    // Unlinked: nothing is queued; `connected` says it all again.
    private fun emit(m: ClientMsg) {
        if (linked) out.add(m)
    }

    /** `sign_in`: the token every later `hello` carries, and nobody's intents made `who`'s. */
    public fun signIn(who: Ctx, token: String?) {
        replica.signIn(who)
        this.token = token
    }

    private fun hello(): ClientMsg =
        ClientMsg.Hello(
            Subscription(replica.cursor, mode, replica.logId, replica.partial != null),
            token,
            SPEC_VERSION)

    /** §12.1 `connected`: hello at the cursor, then everything pending pushed. */
    public fun connected() {
        linked = true
        epoch += 1
        out.clear()
        more = false
        served = false
        started = false
        heard.clear()
        emit(hello())
        if (replica.pending.isNotEmpty()) emit(ClientMsg.Push(ArrayList(replica.pending)))
    }

    public fun disconnected() {
        linked = false
        out.clear()
        more = false
        heard.clear()
    }

    /**
     * `mutate`: author an intent and push it if linked. A refusal here is the optimistic verdict,
     * on the state this peer has.
     */
    public fun mutate(
        id: Id,
        ctx: Ctx,
        fh: ByteArray,
        autos: Map<String, Value>,
        args: Map<String, Value>
    ): Pair<LogEntry?, Refusal?> {
        val (e, why) = replica.mutate(id, ctx, fh, autos, args)
        if (e != null) emit(ClientMsg.Push(listOf(e)))
        return e to why
    }

    /**
     * §12.2 `recv`: a frame from the server, placed in the replica's inbox and applied by `settle`
     * (`docs/plan-perf.md` R8).
     */
    public fun recv(msg: ServerMsg) {
        when (msg) {
            is ServerMsg.Heard -> heard.add(msg.frame)
            is ServerMsg.Denied -> {
                denied = msg.reason
                linked = false
                out.clear()
                more = false
            }
            is ServerMsg.Batch -> {
                heardModule(msg.module)
                served = true
                val r = replica
                val covers = msg.covers
                if ((covers != null) != (r.partial != null)) {
                    more = true
                    return
                }
                if (covers != null && !started) {
                    more = true
                    return
                }
                if (covers != null) {
                    if (covers.upto <= r.through) return
                    if (covers.after > r.through) {
                        more = true
                        return
                    }
                    r.through = covers.upto
                }
                if (r.logId == null) r.logId = msg.logId
                for ((n, e, f) in msg.items) if (f != null) r.receiveWith(n, e, f)
                else r.receive(n, e)
                paged = true
                more = more || msg.hasMore
            }
            is ServerMsg.FactsFor -> for ((n, f) in msg.items) replica.receiveFacts(n, f)
            is ServerMsg.SnapshotOf ->
                recvSnapshot(
                    Snapshot.ofValues(schema, msg.seq, msg.rows, msg.logId, msg.module, msg.held))
            is ServerMsg.Ack -> {
                if (replica.logId == null) replica.logId = msg.logId
                for ((n, f) in msg.facts) replica.receiveFacts(n, f)
                for ((i, n) in msg.ids.zip(msg.seqs)) {
                    heldIds.remove(i)
                    replica.ack(i, n)
                }
            }
            is ServerMsg.Reject -> {
                val unknown =
                    replica.pending.any {
                        it.id == msg.id && msg.reason == unknownFunction(it.fnHash)
                    }
                if (unknown) holdIntent(msg.id)
                else {
                    heldIds.remove(msg.id)
                    replica.reject(msg.id, Refusal.Refused(msg.reason))
                }
            }
            is ServerMsg.Held -> holdIntent(msg.id)
            is ServerMsg.Closures -> for ((h, c) in msg.items) replica.bodies[hashKey(h)] = c
            is ServerMsg.Agree -> agreed.add(msg.seq to if (msg.unknown) null else msg.ok)
        }
    }

    /**
     * §12.2 `recv_snapshot`: below the horizon, past the head, or of another log — the confirmed
     * store replaced by the snapshot, pending intents kept and replayed on top, verdicts not yet
     * taken kept ahead of any the replay makes; what earlier frames of the pump placed applied
     * first.
     */
    public fun recvSnapshot(s: Snapshot) {
        heardModule(s.module)
        served = true
        started = true
        replica.settle()
        val behind = replica.behind
        val held = s.held
        val sch = if (held != null) deviceSchema(schema, held) else schema
        val st = MemoryStore.empty(sch)
        for ((t, rs) in s.rows) {
            val tbl = sch.lookupTable(t)
            for (row0 in rs) {
                val row =
                    if (tbl != null && behind) Rules.projectRow(tbl, row0).first ?: row0 else row0
                st.applyChange(Change.Add(t, row))
            }
        }
        val r = replica
        val opened = Replica.open(sch, r.bodies, st, s.seq, r.pending)
        opened.logId = s.logId
        opened.behind = r.behind
        opened.partial =
            held?.let { TreeMap<String, List<String>>(CodePointOrder).apply { putAll(it) } }
        opened.through = s.seq
        val told = ArrayList(r.rejections)
        told.addAll(opened.rejections)
        opened.rejections.clear()
        opened.rejections.addAll(told)
        replica = opened
    }

    /**
     * §12.2 `settle`: the end of a pump — the inbox applied once, the facts a page left it waiting
     * on asked for, and the next page if it said there is one.
     */
    public fun settle() {
        replica.settle()
        if (paged) {
            paged = false
            val needs = replica.needs()
            if (needs.isNotEmpty()) emit(ClientMsg.NeedFacts(needs))
        }
        if (more) {
            more = false
            emit(hello())
        }
    }

    /** A live frame; dropped while unlinked, never queued. */
    public fun say(frame: ByteArray) {
        emit(ClientMsg.Say(frame))
    }

    /** `verify_all`: ask whether the authority agrees with the confirmed state. */
    public fun verifyAll() {
        val partial = replica.partial != null
        if (behind() || (partial && !served)) return
        val (seq, hash) = replica.verifyAt()
        emit(ClientMsg.Verify(seq, hash, replica.logId, partial))
    }

    public fun takeOutgoing(): List<ClientMsg> {
        val o = ArrayList(out)
        out.clear()
        return o
    }

    public fun takeHeard(): List<ByteArray> {
        val h = ArrayList(heard)
        heard.clear()
        return h
    }

    public companion object {
        /** `Client::open`: over the replica as opened from what was durable. */
        public fun open(replica: Replica, mode: Mode, token: String?): Client =
            Client(replica, mode, token)
    }
}
