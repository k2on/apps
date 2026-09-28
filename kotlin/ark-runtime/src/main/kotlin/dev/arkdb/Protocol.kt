// §12 The protocol (Ark.Protocol): the frames as values, and the two
// state machines around them — a `Client` holding a replica of the log, and
// a `Server` holding its authority and the connections it has identified.
// Both are sans-io: a transport feeds them frames and drains what they
// queue, and nothing here knows what a socket is.
package dev.arkdb

/** How a client holds the log: replaying intents, or fed the facts. */
public enum class Mode { Whole, ByFacts }

/** Where a connection starts: the last sequence applied, and how it holds the log. */
public data class Subscription(val since: Seq, val mode: Mode)

public sealed class ClientMsg {
    public data class Hello(val sub: Subscription, val token: String?, val spec: Int) : ClientMsg()
    public data class Push(val entries: List<Entry>) : ClientMsg()
    public data class NeedFacts(val seqs: List<Seq>) : ClientMsg()
    public data class NeedClosures(val hashes: List<FnHash>) : ClientMsg()
    public class Verify(public val seq: Seq, public val hash: ByteArray) : ClientMsg() {
        override fun equals(other: Any?): Boolean = other is Verify && seq == other.seq && hash.contentEquals(other.hash)
        override fun hashCode(): Int = seq.hashCode() * 31 + hash.contentHashCode()
    }
    public class Say(public val frame: ByteArray) : ClientMsg() {
        override fun equals(other: Any?): Boolean = other is Say && frame.contentEquals(other.frame)
        override fun hashCode(): Int = frame.contentHashCode()
    }

    public fun toValue(): Value = Protocol.clientValue(this)
}

public sealed class ServerMsg {
    public data class Batch(val items: List<Triple<Seq, Entry, Facts?>>, val hasMore: Boolean) : ServerMsg()
    public data class FactsFor(val items: List<Pair<Seq, Facts>>) : ServerMsg()
    public class SnapshotOf(
        public val seq: Seq,
        public val stateHash: ByteArray,
        public val rows: Map<String, List<Value>>,
    ) : ServerMsg() {
        override fun equals(other: Any?): Boolean = other is SnapshotOf && seq == other.seq &&
            stateHash.contentEquals(other.stateHash) && rows == other.rows
        override fun hashCode(): Int = seq.hashCode() * 31 + rows.hashCode()
    }
    public data class Ack(val ids: List<Id>, val seqs: List<Seq>) : ServerMsg()
    /**
     * A verdict against one entry, with the reason every replica would
     * reach: what a screen shows beside the item that did not happen.
     */
    public data class Reject(val id: Id, val reason: String) : ServerMsg()
    public data class Denied(val reason: String) : ServerMsg()
    public data class Closures(val items: List<Pair<FnHash, Closure>>) : ServerMsg()
    public class Agree(public val seq: Seq, public val hash: ByteArray, public val ok: Boolean) : ServerMsg() {
        override fun equals(other: Any?): Boolean = other is Agree && seq == other.seq &&
            hash.contentEquals(other.hash) && ok == other.ok
        override fun hashCode(): Int = seq.hashCode() * 31 + hash.contentHashCode()
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
            "since" to int(m.sub.since),
            "mode" to Value.VText(if (m.sub.mode == Mode.Whole) "whole" else "facts"),
            "token" to (m.token?.let { Value.VText(it) } ?: Value.VNull),
            "spec" to int(m.spec.toLong()),
        )
        is ClientMsg.Push -> node("push", "entries" to Value.VList(m.entries.map { entryValue(it) }))
        is ClientMsg.NeedFacts -> node("need_facts", "seqs" to Value.VList(m.seqs.map { int(it) }))
        is ClientMsg.NeedClosures -> node("need_closures", "hashes" to Value.VList(m.hashes.map { Value.VBytes(it.bytes) }))
        is ClientMsg.Verify -> node("verify", "seq" to int(m.seq), "hash" to Value.VBytes(m.hash))
        is ClientMsg.Say -> node("say", "say" to Value.VBytes(m.frame))
    }

    public fun serverValue(m: ServerMsg): Value = when (m) {
        is ServerMsg.Batch -> node(
            "batch",
            "items" to Value.VList(
                m.items.map { (n, e, f) ->
                    Value.record("seq" to int(n), "entry" to entryValue(e), "facts" to (f?.let { factsValue(it) } ?: Value.VNull))
                },
            ),
            "has_more" to Value.VBool(m.hasMore),
        )
        is ServerMsg.FactsFor -> node(
            "facts",
            "items" to Value.VList(m.items.map { (n, f) -> Value.record("seq" to int(n), "facts" to factsValue(f)) }),
        )
        is ServerMsg.SnapshotOf -> node(
            "snapshot",
            "seq" to int(m.seq),
            "hash" to Value.VBytes(m.stateHash),
            "rows" to Value.VStruct(m.rows.mapValues { Value.VList(it.value) }),
        )
        is ServerMsg.Ack -> node(
            "ack",
            "ids" to Value.VList(m.ids.map { Value.VId(it) }),
            "seqs" to Value.VList(m.seqs.map { int(it) }),
        )
        is ServerMsg.Reject -> node("reject", "id" to Value.VId(m.id), "reason" to Value.VText(m.reason))
        is ServerMsg.Denied -> node("denied", "reason" to Value.VText(m.reason))
        is ServerMsg.Closures -> node(
            "closures",
            "items" to Value.VList(m.items.map { (h, c) -> Value.record("hash" to Value.VBytes(h.bytes), "closure" to Hash.closureValue(c)) }),
        )
        is ServerMsg.Agree -> node(
            "agree",
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
                Subscription(
                    int64(need(m, "since")),
                    when (val ms = text(need(m, "mode"))) {
                        "whole" -> Mode.Whole
                        "facts" -> Mode.ByFacts
                        else -> bad("unknown mode $ms")
                    },
                ),
                need(m, "token").let { if (it is Value.VNull) null else text(it) },
                Math.toIntExact(int64(need(m, "spec"))),
            )
            "push" -> ClientMsg.Push(list(need(m, "entries")) { entryFromValue(it) })
            "need_facts" -> ClientMsg.NeedFacts(list(need(m, "seqs")) { int64(it) })
            "need_closures" -> ClientMsg.NeedClosures(list(need(m, "hashes")) { FnHash(bytes(it)) })
            "verify" -> ClientMsg.Verify(int64(need(m, "seq")), bytes(need(m, "hash")))
            "say" -> ClientMsg.Say(bytes(need(m, "say")))
            else -> bad("unknown client frame $t")
        }
    }

    public fun serverFromValue(v: Value): ServerMsg {
        val m = struct(v)
        return when (val t = text(need(m, "t"))) {
            "batch" -> ServerMsg.Batch(
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
                list(need(m, "items")) { x ->
                    val im = struct(x)
                    int64(need(im, "seq")) to list(need(im, "facts")) { changeFromValue(it) }
                },
            )
            "snapshot" -> ServerMsg.SnapshotOf(
                int64(need(m, "seq")),
                bytes(need(m, "hash")),
                struct(need(m, "rows")).mapValues { list(it.value) { v -> v } },
            )
            "ack" -> ServerMsg.Ack(list(need(m, "ids")) { ident(it) }, list(need(m, "seqs")) { int64(it) })
            "reject" -> ServerMsg.Reject(ident(need(m, "id")), text(need(m, "reason")))
            "denied" -> ServerMsg.Denied(text(need(m, "reason")))
            "closures" -> ServerMsg.Closures(
                list(need(m, "items")) { x ->
                    val im = struct(x)
                    FnHash(bytes(need(im, "hash"))) to Decode.closureFromValue(need(im, "closure"))
                },
            )
            "agree" -> ServerMsg.Agree(int64(need(m, "seq")), bytes(need(m, "hash")), bool(need(m, "ok")))
            "heard" -> ServerMsg.Heard(bytes(need(m, "hear")))
            else -> bad("unknown server frame $t")
        }
    }
    /**
     * §12.5 The reason a `Reject` carries: a mutator's own refusal is its
     * text, word for word, because that is what an author wrote for a
     * person to read; the store's constraint refusals are named in a
     * sentence.
     */
    public fun refusalText(r: Refusal): String = when (r) {
        is Refusal.Refused -> r.reason
        is Refusal.NoSuchTable -> "no table ${r.table}"
        is Refusal.MalformedRow -> "${r.table}: ${r.what}"
        is Refusal.NotNull -> "${r.table}.${r.column} may not be empty"
        is Refusal.UniqueViolation -> "${r.table}: another row has the same ${r.columns.joinToString(", ")}"
        is Refusal.MissingParent -> "${r.table}.${r.column} names no ${r.parent}"
        is Refusal.StillReferenced -> "${r.table}: still referenced by ${r.child}"
    }
}

/** A peer's end of one connection: its replica of the log, and what it has queued. */
public class Client(replica: Replica, public val mode: Mode, token: String?) {
    public val schema: Schema = replica.schema

    /** The replica, as opened from what was durable; a snapshot replaces it. */
    public var replica: Replica = replica
        private set

    /** What the next `Hello` proves the login with. Takes effect on the next connection. */
    public var token: String? = token

    public var linked: Boolean = false
        private set

    /** Counts connections, so a live room that has never heard of this device can be told apart. */
    public var epoch: Int = 0
        private set

    private val out: MutableList<ClientMsg> = ArrayList()
    private val heard: MutableList<ByteArray> = ArrayList()

    public var denied: String? = null
        private set

    /** The authority's answers to `verifyAll`, oldest first: the sequence asked about, and whether it agreed. */
    public val agreed: MutableList<Pair<Seq, Boolean>> = ArrayList()

    // Unlinked, nothing is queued; `connected` says it all again.
    private fun emit(m: ClientMsg) {
        if (linked) out.add(m)
    }

    /**
     * Somebody signed in (`Ark.Protocol.clientSignIn`): the token every later
     * `Hello` carries, and every intent authored before anyone had signed in
     * made theirs (`Replica.signIn`). Call it before `connected`; the first
     * `Hello` after it pushes all of it.
     */
    public fun signIn(who: Ctx, token: String?) {
        replica.signIn(who)
        this.token = token
    }

    private fun hello(): ClientMsg = ClientMsg.Hello(Subscription(replica.cursor, mode), token, SPEC_VERSION)

    /**
     * §12.1 A connection opened: say hello at the cursor, then push
     * everything pending. What was queued before is dropped, since the
     * hello resends it all.
     */
    public fun connected() {
        linked = true
        epoch += 1
        out.clear()
        heard.clear()
        emit(hello())
        if (replica.pending.isNotEmpty()) emit(ClientMsg.Push(replica.pending))
    }

    public fun disconnected() {
        linked = false
        out.clear()
        heard.clear()
    }

    /**
     * Author an intent and push it if linked. Throws `Fault.Refuse`: the
     * optimistic verdict, on the state this peer has; the authority's may
     * differ, and arrives as a `Reject` with its own reason.
     */
    public fun mutate(i: Id, ctx: Ctx, fh: FnHash, autos: Args, args: Args): Entry {
        val e = replica.mutate(i, ctx, fh, autos, args)
        emit(ClientMsg.Push(listOf(e)))
        return e
    }

    /** §12.2 A frame from the server. */
    public fun recv(m: ServerMsg) {
        val r = replica
        when (m) {
            is ServerMsg.Heard -> heard.add(m.frame)
            is ServerMsg.Denied -> {
                denied = m.reason
                linked = false
                out.clear()
            }
            is ServerMsg.Batch -> {
                for ((n, e, f) in m.items) if (f != null) r.receiveWith(n, e, f) else r.receive(n, e)
                val needs = r.needs()
                if (needs.isNotEmpty()) emit(ClientMsg.NeedFacts(needs))
                if (m.hasMore) emit(hello())
            }
            is ServerMsg.FactsFor -> for ((n, f) in m.items) r.receiveFacts(n, f)
            is ServerMsg.SnapshotOf -> {
                // Below the horizon: the confirmed store is replaced by the
                // snapshot and the cursor moves to it; pending replays on top.
                val st = MemoryStore.of(schema, m.rows)
                val next = Replica.open(r.schema, r.bodies, st, m.seq, r.pending, r.natives)
                next.rejections.addAll(r.rejections)
                next.confirmedOwn.addAll(r.confirmedOwn)
                replica = next
            }
            is ServerMsg.Ack -> for ((i, n) in m.ids.zip(m.seqs)) r.ack(i, n)
            is ServerMsg.Reject -> r.reject(m.id, Refusal.Refused(m.reason))
            is ServerMsg.Closures -> {
                // New closures may unblock entries waiting in the inbox.
                r.learn(m.items.toMap())
                r.retry()
            }
            is ServerMsg.Agree -> agreed.add(m.seq to m.ok)
        }
    }

    /** A live frame; dropped while unlinked, never queued. */
    public fun say(frame: ByteArray): Unit = emit(ClientMsg.Say(frame))

    /** Ask the authority whether it agrees with the replica's confirmed state. */
    public fun verifyAll() {
        val (n, h) = replica.verifyAt()
        emit(ClientMsg.Verify(n, h))
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

// ---------------------------------------------------------------------------
// The server

/** Who a connection is: the user, and the login (one login on one device). Every entry it pushes is held to both. */
public data class Identity(val user: String, val session: String)

/** What a token proves. The server asks this once, at `Hello`, and never looks at a token again. */
public fun interface Authenticate {
    public fun identify(token: String?): Identity?

    public companion object {
        /** Dev auth: anyone is whoever they say, and the token is their name. */
        public val trusting: Authenticate = Authenticate { tok -> Identity(tok ?: "anonymous", "dev") }
    }
}

/** A connection, as the transport numbers it. */
public typealias ConnId = Long

/**
 * The server's end of every connection (`Ark.Protocol.Server`): the
 * authority for the log, the connections it has identified, and what it
 * has queued for each. The rules are the spec's §12.3–12.5:
 *
 * - identity is asked once, at `Hello`; a hello that proves nothing, or
 *   proves nobody (an empty user), is answered `Denied`, and so is one the
 *   access rule turns away;
 * - every pushed entry is held to the connection's identity: its actor is
 *   the user, and its session is the connection's own or one `owns` says
 *   the same user holds — otherwise it is rejected "not yours";
 * - the authority applies before it appends, so a `Reject` is a verdict,
 *   and its reason is `Protocol.refusalText`;
 * - after every message every connection is sent every entry above what it
 *   has been sent, a page at a time, or the snapshot below the horizon.
 *
 * The live room is the account's: a `Say` is heard by the user's other
 * connections, and touches nothing else.
 */
public class Server(
    private val auth: Authenticate,
    /** May this identity receive the log? The read rule. */
    private val access: (Identity) -> Boolean,
    public val authority: Authority,
) {
    /**
     * Does this user own this session? A session outlives its token: an
     * entry authored offline under one login and pushed after the same
     * person signs in again carries the old session, and is still theirs.
     * Only ever asked about the connection's own user. By default, no.
     */
    private var owns: (String, String) -> Boolean = { _, _ -> false }

    public class Conn(public val who: Identity, public val mode: Mode, sent: Seq) {
        /** The sequence the connection has been sent up to (not what it has applied). */
        public var sent: Seq = sent
            internal set
    }

    private val conns = java.util.TreeMap<ConnId, Conn>()
    private val out = ArrayList<Pair<ConnId, ServerMsg>>()

    /** The connections that have said hello, by id. */
    public val connections: Map<ConnId, Conn> get() = conns

    /** Install the sessions a user owns, which the authenticator's session store knows and the engine does not. */
    public fun withOwns(f: (user: String, session: String) -> Boolean): Server {
        owns = f
        return this
    }

    private fun send(c: ConnId, m: ServerMsg) {
        out.add(c to m)
    }

    /** §12.3 A frame from a connection. */
    public fun recv(c: ConnId, msg: ClientMsg) {
        if (msg is ClientMsg.Hello) {
            val who = auth.identify(msg.token)
            when {
                who == null -> send(c, ServerMsg.Denied("not signed in"))
                // Nobody is not an identity: no entry of theirs is ever accepted.
                who.user == Ctx.nobody.user -> send(c, ServerMsg.Denied("not signed in"))
                !access(who) -> send(c, ServerMsg.Denied("not allowed"))
                else -> {
                    // A second Hello on one connection is the log paging, and
                    // says where to continue from.
                    conns[c] = Conn(who, msg.sub.mode, msg.sub.since)
                    fanout()
                }
            }
            return
        }
        val conn = conns[c]
        if (conn == null) {
            send(c, ServerMsg.Denied("hello first"))
            return
        }
        when (msg) {
            is ClientMsg.Hello -> Unit
            is ClientMsg.Push -> {
                val acks = ArrayList<Pair<Id, Seq>>()
                for (e in msg.entries) {
                    if (e.actor != conn.who.user || (e.session != conn.who.session && !owns(e.actor, e.session))) {
                        send(c, ServerMsg.Reject(e.id, "not yours"))
                        continue
                    }
                    when (val s = authority.sequenceEntry(e)) {
                        is Sequenced.Appended -> acks.add(e.id to s.seq)
                        is Sequenced.Duplicate -> acks.add(e.id to s.seq)
                        is Sequenced.Rejected -> send(c, ServerMsg.Reject(e.id, Protocol.refusalText(s.refusal)))
                    }
                }
                if (acks.isNotEmpty()) send(c, ServerMsg.Ack(acks.map { it.first }, acks.map { it.second }))
                fanout()
            }
            is ClientMsg.NeedFacts -> {
                val items = msg.seqs.mapNotNull { n -> authority.log.entries[n]?.let { n to it.second } }
                send(c, ServerMsg.FactsFor(items))
            }
            is ClientMsg.NeedClosures ->
                send(c, ServerMsg.Closures(msg.hashes.mapNotNull { h -> authority.bodies[h]?.let { h to it } }))
            is ClientMsg.Verify -> {
                val st = authority.log.stateAt(msg.seq)
                send(c, ServerMsg.Agree(msg.seq, msg.hash, st != null && Hash.stateHash(st).contentEquals(msg.hash)))
            }
            is ClientMsg.Say -> {
                for ((o, oc) in conns) if (o != c && oc.who.user == conn.who.user) send(o, ServerMsg.Heard(msg.frame))
            }
        }
    }

    /** A connection closed: its cursor is forgotten. */
    public fun disconnect(c: ConnId) {
        conns.remove(c)
    }

    // §12.4 Fan-out: every connection, everything above what it has been
    // sent, a page at a time; a snapshot for one below the horizon.
    private fun fanout() {
        for ((c, conn) in conns) {
            if (conn.sent >= authority.log.headSeq) continue
            when (val p = authority.page(conn.sent, Protocol.BATCH_LIMIT)) {
                is Page.BelowHorizon -> {
                    val sn = p.snapshot
                    val rows = sn.store.tableNames.associateWith { t -> sn.store.scan(t).map { it as Value } }
                    send(c, ServerMsg.SnapshotOf(sn.seq, sn.hash, rows))
                    conn.sent = sn.seq
                }
                is Page.Entries -> {
                    val items = p.items.map { (n, e, f) -> Triple(n, e, if (conn.mode == Mode.ByFacts) f else null) }
                    send(c, ServerMsg.Batch(items, p.hasMore))
                    conn.sent = maxOf(conn.sent, p.items.maxOfOrNull { it.first } ?: conn.sent)
                }
            }
        }
    }

    public fun takeOutgoing(): List<Pair<ConnId, ServerMsg>> {
        val o = out.toList()
        out.clear()
        return o
    }
}
