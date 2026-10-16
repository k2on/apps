// `docs/plan-phones.md` S2 A trace, as `rust/ark/src/trace.rs` defines it:
// one client's life in a session, written down as its inputs and its
// outputs, so that this client is tested as a function of what it was given
// — with nothing left to a scheduler, and no server in it: what the server
// said is already the frames. `spec/RUNNERS.md` § trace is the contract;
// `Player` is it as code, as `trace::Player` is in the reference.
//
// The client is `Client` over its `Replica`, with what it makes durable
// (`Durable`) and, alone, its own `Authority`. A step is one input and what
// the client did after it: the frames it queued as canonical bytes, a local
// refusal, what each kept view reports, and its state, written only where
// it moved — every field after an open.
package dev.arkdb

/** A view a trace keeps up to date (`trace::ViewSpec`). */
public data class ViewSpec(val query: String, val ctx: Ctx, val args: Map<String, Value>)

/** One input to a client (`trace::Input`). */
public sealed class Input {
    /** `device`: §14 the device its `hello` declares, for a client of a module with live state. */
    public data class Open(
        val mode: Mode,
        val closures: Boolean,
        val token: String?,
        val device: DeviceDecl? = null,
    ) : Input()

    public object Reopen : Input()

    public object Connected : Input()

    public object Disconnected : Input()

    public class Mutate(
        public val ctx: Ctx,
        public val function: String,
        public val fnHash: ByteArray,
        public val id: Id,
        public val autos: Map<String, Value>,
        public val args: Map<String, Value>,
    ) : Input()

    public data class SignIn(val ctx: Ctx, val token: String?) : Input()

    public data class Token(val token: String?) : Input()

    public class Receive(public val frame: ByteArray) : Input()

    public object Settle : Input()

    public object Verify : Input()

    public object Alone : Input()

    public data class Join(val ctx: Ctx) : Input()

    public data class Hydrate(val views: List<ViewSpec>) : Input()

    /**
     * §14 `Client.call`: a transition of a live instance, by this author, with the time it guessed
     * for the server's and the id and counter it drew — frozen, as a mutation's autos are.
     */
    public class Call(
        public val ctx: Ctx,
        public val instance: Instance,
        public val transition: String,
        public val input: Map<String, Value>,
        public val id: Id,
        public val now: Long,
        public val counter: Long,
    ) : Input()

    /** Its `t`, as a trace writes it. */
    public val t: String
        get() =
            when (this) {
                is Open -> "open"
                Reopen -> "reopen"
                Connected -> "connected"
                Disconnected -> "disconnected"
                is Mutate -> "mutate"
                is SignIn -> "sign_in"
                is Token -> "token"
                is Receive -> "receive"
                Settle -> "settle"
                Verify -> "verify"
                Alone -> "alone"
                is Join -> "join"
                is Hydrate -> "hydrate"
                is Call -> "call"
            }

    public companion object {
        /** `Input::from_value`: an input read back; a `TraceError` names what is not one. */
        public fun fromValue(v: Value): Input {
            val m = strct(v, "an input")
            fun get(k: String): Value = m[k] ?: throw TraceError("an input without $k: ${v.show()}")
            fun token(k: String): String? =
                when (val x = get(k)) {
                    is Value.VNull -> null
                    is Value.VText -> x.value
                    else -> throw TraceError("$k is neither text nor null")
                }
            fun args(k: String): Map<String, Value> = strct(get(k), k)
            val t =
                (get("t") as? Value.VText)?.value
                    ?: throw TraceError("an input whose t is not text")
            return when (t) {
                "open" ->
                    Open(
                        when ((get("mode") as? Value.VText)?.value) {
                            "whole" -> Mode.Whole
                            "facts" -> Mode.ByFacts
                            else -> throw TraceError("mode is neither whole nor facts")
                        },
                        (get("closures") as? Value.VBool)?.value
                            ?: throw TraceError("closures is not a bool"),
                        token("token"),
                        m["device"]?.let {
                            try {
                                DeviceDecl.fromValue(it)
                            } catch (e: FactError) {
                                throw TraceError(e.message ?: "a device")
                            }
                        })
                "reopen" -> Reopen
                "connected" -> Connected
                "disconnected" -> Disconnected
                "mutate" ->
                    Mutate(
                        ctxOf(get("ctx")),
                        (get("function") as? Value.VText)?.value
                            ?: throw TraceError("function is not text"),
                        (get("fn") as? Value.VBytes)?.value ?: throw TraceError("fn is not bytes"),
                        (get("id") as? Value.VId)?.value ?: throw TraceError("id is not an id"),
                        args("autos"),
                        args("args"))
                "sign_in" -> SignIn(ctxOf(get("ctx")), token("token"))
                "token" -> Token(token("token"))
                "receive" ->
                    Receive(
                        (get("frame") as? Value.VBytes)?.value
                            ?: throw TraceError("frame is not bytes"))
                "settle" -> Settle
                "verify" -> Verify
                "alone" -> Alone
                "join" -> Join(ctxOf(get("ctx")))
                "call" ->
                    Call(
                        ctxOf(get("ctx")),
                        Instance(
                            (get("live") as? Value.VText)?.value
                                ?: throw TraceError("live is not text"),
                            (get("key") as? Value.VText)?.value
                                ?: throw TraceError("key is not text")),
                        (get("transition") as? Value.VText)?.value
                            ?: throw TraceError("transition is not text"),
                        args("input"),
                        (get("id") as? Value.VId)?.value ?: throw TraceError("id is not an id"),
                        (get("now") as? Value.VInt)?.value ?: throw TraceError("now is not an int"),
                        (get("counter") as? Value.VInt)?.value
                            ?: throw TraceError("counter is not an int"))
                "hydrate" ->
                    Hydrate(
                        ((get("views") as? Value.VList) ?: throw TraceError("views is not a list"))
                            .items
                            .map { s ->
                                val f = strct(s, "a view")
                                ViewSpec(
                                    (f["query"] as? Value.VText)?.value
                                        ?: throw TraceError("a view's query: ${f["query"]}"),
                                    ctxOf(f["ctx"] ?: throw TraceError("a view without ctx")),
                                    strct(
                                        f["args"] ?: throw TraceError("a view without args"),
                                        "args"))
                            })
                else -> throw TraceError("unknown input $t")
            }
        }

        private fun strct(v: Value, what: String): Map<String, Value> =
            (v as? Value.VStruct)?.fields ?: throw TraceError("$what is not a struct: ${v.show()}")

        private fun ctxOf(v: Value): Ctx {
            val m = strct(v, "a ctx")
            fun text(k: String): String =
                (m[k] as? Value.VText)?.value ?: throw TraceError("ctx $k: ${m[k]}")
            val roles =
                when (val r = m["roles"]) {
                    null -> emptyList()
                    is Value.VList ->
                        r.items.map {
                            (it as? Value.VText)?.value ?: throw TraceError("a role: $it")
                        }
                    else -> throw TraceError("ctx roles: $r")
                }
            return Ctx(text("user"), text("session")).withRoles(roles)
        }
    }
}

/** A trace that is not one, or a step a client cannot take (`trace.rs`'s `Err(String)`). */
public class TraceError(what: String) : Exception(what)

/**
 * A client that is its own authority (`trace::Alone`): where it forked — the log and cursor it last
 * shared with a server — and the authority over its local history since.
 */
public class Alone(public val authority: Authority, public val fork: Seq, public val forkLog: Id?)

/**
 * Server → alone (`trace::alone`, `docs/plan-alone.md` §1): the fork is where the replica stands;
 * the authority starts from the confirmed store with nothing in its log, over the closures the
 * client holds, and is no server's — its private blocks are not run (D3); the replica holds no
 * named log from here, and nothing of the server's it had not yet applied — the inbox is left
 * behind, or it would be applied as the local history's own next sequence; and what is pending is
 * sequenced, locally.
 */
public fun goAlone(c: Client): Alone {
    val r = c.replica
    r.inbox.clear()
    r.through = r.cursor
    val a = Authority(r.schema, r.bodies)
    a.log = Log(r.cursor, MemoryStore.empty(r.schema), ByteArray(0), null)
    a.store = r.confirmed.copy()
    val out = Alone(a, r.cursor, r.logId)
    r.logId = null
    commit(out, c)
    return out
}

/** Alone, everything pending sequenced by the local authority (`trace::commit`). */
public fun commit(a: Alone, c: Client) {
    localCommit(a.authority, c.replica)
}

/**
 * Alone → server (`trace::join`): the local history above the fork taken back out of the confirmed
 * store and re-queued as `who`'s — the same ids, functions, autos and arguments
 * (`Replica.forkBack`). The caller connects.
 */
public fun join(c: Client, a: Alone, who: Ctx) {
    val facts = ArrayList<FactList>()
    val requeue = ArrayList<LogEntry>()
    for ((n, ef) in a.authority.log.entries) {
        if (n > a.fork) {
            requeue.add(ef.first.by(who.user, who.session))
            facts.add(ef.second)
        }
    }
    c.replica.forkBack(facts, a.fork, a.forkLog, requeue)
}

/**
 * What a client makes durable, as a runner keeps it (§11.1): told after every step, and what a
 * `reopen` opens from. `KeptStore` is the trace's own (`trace::keep`, `trace::reopen`); a driver's
 * storage is another (`ark-client`'s `Stored`), held to the same traces.
 */
public interface Durable {
    /** A fresh client: nothing durable yet. */
    public fun clear()

    /** After every step: what the confirmed store moved by, with the cursor, log and union. */
    public fun keep(c: Client, alone: Alone?)

    /** `reopen`: the client stopped and opened again from what it made durable. */
    public fun reopen(old: Client): Client
}

/**
 * `sim::Kept` and `trace::keep`: the confirmed store moved only by `Replica.takeConfirmed`, the
 * cursor, the log and the union beside it; the pending intents durable as the replica's own list.
 */
public class KeptStore : Durable {
    private var store: MemoryStore? = null
    private var cursor: Seq = 0
    private var logId: Id? = null
    private var partial: Map<String, List<String>>? = null

    override fun clear() {
        store = null
    }

    override fun keep(c: Client, alone: Alone?) {
        val r = c.replica
        if (store == null) store = r.confirmed.copy()
        when (val m = r.takeConfirmed()) {
            is Moved.Replaced -> store = r.confirmed.copy()
            is Moved.Run -> for ((_, f) in m.run) store!!.applyChanges(f)
        }
        cursor = r.cursor
        logId = r.logId
        partial = r.partial
    }

    override fun reopen(old: Client): Client {
        val st = store ?: throw TraceError("a reopen with nothing durable")
        return reopenFrom(old, st.copy(), cursor, logId, partial, old.replica.pending)
    }
}

/**
 * `trace::reopen`: a client stopped and opened again from what it made durable — the replica over
 * the store at its cursor, under the device's schema when it held a union (D2), with its log, its
 * union, a page horizon at the cursor, the pending intents, and the closures it held; the client
 * over it with the same mode and token, and the module's schema. Unlinked.
 */
public fun reopenFrom(
    old: Client,
    store: MemoryStore,
    cursor: Seq,
    logId: Id?,
    partial: Map<String, List<String>>?,
    pending: List<LogEntry>,
): Client {
    val r = Replica.open(store.schema, old.replica.bodies, store, cursor, pending)
    r.logId = logId
    r.partial =
        partial?.let {
            java.util.TreeMap<String, List<String>>(CodePointOrder).apply { putAll(it) }
        }
    r.through = cursor
    val c = Client.open(r, old.mode, old.token).withSchema(old.schema)
    // §14 Live state is about now and nothing of it is durable: a client started again holds none
    // of it, and is told it whole when it next connects.
    val w = old.live ?: return c
    return c.withLive(Watch(w.runner, w.device))
}

/** `client_bodies`: the module's closures as a client holds them — private blocks stripped (D3). */
public fun clientBodies(bodies: Map<String, Closure>): Map<String, Closure> =
    bodies.mapValues { Hash.stripped(it.value) }

private class Watched(val spec: ViewSpec, var view: MaintainedView?)

/**
 * What a client's step produced, read off it (`trace::Observer`): the views the trace keeps and the
 * state the last step wrote, which is what lets a step write only what moved.
 */
public class Observer(bodies: Map<String, Closure>) {
    private val queries: Map<String, Closure> =
        bodies.values.filter { it.function.kind == FnKind.Query }.associateBy { it.function.name }
    private val views = ArrayList<Watched>()
    private val events = ArrayList<Value>()
    private val last = HashMap<String, Value>()

    /** The client was opened: the next step writes every state field. */
    public fun reset() {
        last.clear()
    }

    private fun refused(why: String): Value = Value.record("refused" to Value.VText(why))

    private fun faultText(e: EvalFault): String =
        when (e) {
            is EvalFault.Verdict -> refusalText(e.refusal)
            is EvalFault.Bug -> bugText(e.error)
        }

    // A view read over `st`: what it holds, and what it reports.
    private fun openView(spec: ViewSpec, st: MemoryStore): Pair<MaintainedView?, Value> {
        val sch = st.schema
        val c = queries[spec.query] ?: return null to refused("no query ${spec.query}")
        val plan = c.function.plan ?: return null to refused("${spec.query} has no plan")
        return try {
            val (args, provided) = Eval.middleware(sch, c, spec.ctx, spec.args, st)
            val v = Views.hydrate(sch, plan, ViewEnv(c.helpers, spec.ctx, args, provided), st)
            v to Value.record("rows" to Value.VList(v.rows()))
        } catch (e: EvalFault) {
            null to refused(faultText(e))
        }
    }

    /** These views in place of any held, hydrated over `st`. */
    public fun hydrate(specs: List<ViewSpec>, st: MemoryStore) {
        views.clear()
        events.clear()
        for (spec in specs) {
            val (view, ev) = openView(spec, st)
            views.add(Watched(spec, view))
            events.add(ev)
        }
    }

    /** `Observer::step`: the step's outputs. */
    public fun step(
        c: Client,
        frames: List<ClientMsg>,
        changes: Changes,
        refusal: Refusal?
    ): Value {
        val out = java.util.TreeMap<String, Value>(CodePointOrder)
        if (frames.isNotEmpty())
            out["frames"] = Value.VList(frames.map { Value.VBytes(it.encode()) })
        if (refusal != null) out["refused"] = Value.VText(refusalText(refusal))
        val st = c.replica.view
        while (events.size < views.size) events.add(Value.VNull)
        while (events.size > views.size) events.removeAt(events.size - 1)
        when (changes) {
            is Changes.Rebuilt ->
                for (k in views.indices) {
                    val (view, ev) = openView(views[k].spec, st)
                    views[k].view = view
                    events[k] = ev
                }
            is Changes.Applied ->
                if (changes.changes.isNotEmpty()) {
                    val sch = st.schema
                    for (k in views.indices) {
                        val v = views[k].view ?: continue
                        events[k] =
                            try {
                                Value.record(
                                    "patches" to
                                        Value.VList(
                                            Views.pushAll(sch, st, changes.changes, v)
                                                .map(Views::patchValue)))
                            } catch (e: EvalFault) {
                                views[k].view = null
                                refused(faultText(e))
                            }
                    }
                }
        }
        if (events.any { it != Value.VNull }) out["views"] = Value.VList(ArrayList(events))
        events.clear()
        for ((k, v) in state(c)) {
            if (last[k] != v) {
                out[k] = v
                last[k] = v
            }
        }
        return Value.VStruct(out)
    }
}

/** `trace::state`: the state a step is held to, field by field. */
public fun state(c: Client): List<Pair<String, Value>> {
    val r = c.replica
    return listOf(
        "cursor" to Value.VInt(r.cursor),
        "log" to (r.logId?.let { Value.VId(it) } ?: Value.VNull),
        "pending" to Value.VList(r.pending.map { Value.VId(it.id) }),
        "rejections" to
            Value.VList(
                r.rejections.map { (id, why) ->
                    Value.record("id" to Value.VId(id), "reason" to Value.VText(refusalText(why)))
                }),
        "diverged" to Value.VList(r.diverged.map { Value.VInt(it) }),
        "agreed" to
            Value.VList(
                c.agreed.map { (n, ok) ->
                    Value.record(
                        "seq" to Value.VInt(n),
                        "ok" to (ok?.let { Value.VBool(it) } ?: Value.VNull))
                }),
        "denied" to (c.denied?.let { Value.VText(it) } ?: Value.VNull),
        "union" to
            (r.partial?.let { h ->
                Value.VStruct(h.mapValues { (_, cs) -> Value.VList(cs.map { Value.VText(it) }) })
            } ?: Value.VNull),
        "hash" to Value.VBytes(Hash.stateHash(r.confirmed)),
        "view_hash" to Value.VBytes(Hash.stateHash(r.view)),
    ) + liveState(c)
}

// §14 A client holding live state is held to it too: each instance's sequence, whether a snapshot
// has come on this connection, the hash of what is confirmed, its view whole and the ids still
// pending; and the server's verdicts on its calls. Absent for every client that holds none.
private fun liveState(c: Client): List<Pair<String, Value>> {
    val w = c.live ?: return emptyList()
    return listOf(
        "live" to
            Value.VList(
                w.instances.map { (i, x) ->
                    Value.record(
                        "live" to Value.VText(i.state),
                        "key" to Value.VText(i.key),
                        "seq" to Value.VInt(x.seq),
                        "synced" to Value.VBool(x.synced),
                        "hash" to Value.VBytes(w.runner.hash(i.state, x.confirmed)),
                        "view" to x.view,
                        "pending" to Value.VList(x.pending.map { Value.VId(it.id) }))
                }),
        "live_refused" to
            Value.VList(
                w.refused.map { (id, why) ->
                    Value.record("id" to Value.VId(id), "reason" to Value.VText(why))
                }),
    )
}

/**
 * A client driven by nothing but a trace's inputs (`trace::Player`): what a runner in any language
 * is. `durable` is what it makes durable — the trace's own copy by default.
 */
public class Player(m: Module, private val durable: Durable = KeptStore()) {
    private val moduleSchema: Schema = m.schema
    private val live: Runner = Runner.ofModule(m)
    private val bodies: Map<String, Closure> = Hash.closures(m)
    private val observer = Observer(bodies)
    private var client: Client? = null
    private var alone: Alone? = null

    /** The client as it stands, once opened. */
    public val current: Client?
        get() = client

    /** One input, and what the client did after it. */
    public fun step(input: Input): Value {
        var refusal: Refusal? = null
        if (input is Input.Open) {
            val bs = if (input.closures) clientBodies(bodies) else emptyMap()
            val r = Replica.open(moduleSchema, bs, MemoryStore.empty(moduleSchema), 0, emptyList())
            val c = Client.open(r, input.mode, input.token)
            // §14 A client of a module with live state that declares a device holds it, as that
            // device. One that declares none holds none and reports none (spec/RUNNERS.md,
            // `live`), however much live state its module has.
            if (!live.isEmpty && input.device != null) c.withLive(Watch(live, input.device))
            client = c
            durable.clear()
            alone = null
            observer.reset()
        }
        var c = client ?: throw TraceError("an input before the client was opened")
        when (input) {
            is Input.Open -> {}
            Input.Reopen -> {
                c = durable.reopen(c)
                client = c
                observer.reset()
            }
            Input.Connected -> c.connected()
            Input.Disconnected -> c.disconnected()
            is Input.Mutate -> {
                refusal =
                    c.mutate(input.id, input.ctx, input.fnHash, input.autos, input.args).second
                alone?.let { commit(it, c) }
            }
            is Input.SignIn -> c.signIn(input.ctx, input.token)
            is Input.Token -> c.token = input.token
            is Input.Receive ->
                when (val got = ServerMsg.decodeFor(input.frame, c.schema)) {
                    is Received.Snap -> c.recvSnapshot(got.snapshot)
                    is Received.Msg -> c.recv(got.msg)
                }
            Input.Settle -> c.settle()
            Input.Verify -> c.verifyAll()
            Input.Alone -> alone = goAlone(c)
            is Input.Join -> {
                val a = alone ?: throw TraceError("a join while not alone")
                alone = null
                join(c, a, input.ctx)
            }
            is Input.Hydrate -> observer.hydrate(input.views, c.replica.view)
            is Input.Call ->
                try {
                    c.call(
                        input.ctx,
                        input.instance,
                        input.transition,
                        input.input,
                        input.id,
                        input.now,
                        input.counter)
                } catch (e: LiveRefusal) {
                    refusal = e.refusal
                }
        }
        val frames = c.takeOutgoing()
        val changes = c.replica.takeChanges()
        durable.keep(c, alone)
        return observer.step(c, frames, changes, refusal)
    }
}

/**
 * `trace::replay`: every step replayed into a fresh client and its outputs held to the recorded
 * ones, value for value — every frame byte for byte. The first step that differs is named, with
 * each field that does, as a `TraceError`.
 */
public fun replay(m: Module, steps: List<Pair<Value, Value>>, durable: Durable = KeptStore()) {
    val p = Player(m, durable)
    for ((k, step) in steps.withIndex()) {
        val (i, want) = step
        val t = ((i as? Value.VStruct)?.fields?.get("t") as? Value.VText)?.value ?: "?"
        val input =
            try {
                Input.fromValue(i)
            } catch (e: TraceError) {
                throw TraceError("step $k: ${e.message}")
            }
        val got =
            try {
                p.step(input)
            } catch (e: TraceError) {
                throw TraceError("step $k ($t): ${e.message}")
            } catch (e: FactError) {
                throw TraceError("step $k ($t): ${e.message}")
            } catch (e: Canon.DecodeError) {
                throw TraceError("step $k ($t): decoding a frame: ${e.why}")
            }
        if (got != want) throw TraceError("step $k ($t): ${differs(want, got)}")
    }
}

/** `trace::differs`: the fields of two outputs that differ, each with both sides. */
public fun differs(want: Value, got: Value): String {
    val w = (want as? Value.VStruct)?.fields ?: emptyMap()
    val g = (got as? Value.VStruct)?.fields ?: emptyMap()
    val keys =
        java.util.TreeSet<String>(CodePointOrder).apply {
            addAll(w.keys)
            addAll(g.keys)
        }
    fun show(v: Value?): String = v?.show()?.take(400) ?: "absent"
    return keys
        .filter { w[it] != g[it] }
        .joinToString("; ") { "$it: recorded ${show(w[it])}, replayed ${show(g[it])}" }
}
