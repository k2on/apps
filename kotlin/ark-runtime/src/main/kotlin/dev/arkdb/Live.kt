// §14.1 Live state (`rust/ark/src/live/state.rs`, `docs/plan-live.md`): a
// typed state machine per scope, beside the log, on the one socket — the
// frames, how a module's states run, and the client's half.
//
// An **instance** is one state under one key (`listening` of `alice`). The
// server holds every instance and sequences the calls on each with a
// sequence of its own; a client holds what the server last confirmed of the
// instances it subscribes to, and its own calls not yet answered replayed on
// top — the log's rule, **confirmed, then pending replayed on top**, without
// the log.
//
// What crosses the socket, beside the log's frames:
//
// - a `hello` may declare the connection a **device** (`DeviceDecl`);
// - `call`: run this transition on this instance with this input — an
//   ordered call carries an id, an ephemeral one its counter too;
// - `applied`: a call the server took — ordered, with its sequence, who made
//   it, when, and the hash of the state it produced; or ephemeral, with the
//   device's counter and nothing of order;
// - `refused`: to its author alone, a call the server did not take;
// - `live_snapshot`: an instance whole, ephemeral slots included;
// - `resync`: a client's request for one.
//
// What a server does with them is not a client's, and is not here. Sans-io,
// as everything in this runtime is: what arrived is handed in, and what to
// say is handed back.
package dev.arkdb

/**
 * The most bytes an ephemeral slot's value may encode to (`state::SLOT_CAP`): what a pointer or a
 * scrubber needs.
 */
public const val SLOT_CAP: Int = 1024

// ---------------------------------------------------------------------------
// Frames

/** One instance: a live state under one key (`state::Instance`), ordered by state then key. */
public data class Instance(val state: String, val key: String) : Comparable<Instance> {
    override fun compareTo(other: Instance): Int {
        val s = CodePointOrder.compare(state, other.state)
        return if (s != 0) s else CodePointOrder.compare(key, other.key)
    }
}

/**
 * What a connection declares itself as at `hello` (`state::DeviceDecl`): an id that survives its
 * reconnects (a login's session for a client, an entity for a speaker; empty is the connection's
 * session), what kind of thing it is, its name, and whether it can play.
 */
public data class DeviceDecl(
    val id: String,
    val kind: String,
    val name: String,
    val canPlay: Boolean,
) {
    /** On the wire, in a `hello`: `{id, kind, name, can_play}`. */
    public fun toValue(): Value =
        Value.record(
            "id" to Value.VText(id),
            "kind" to Value.VText(kind),
            "name" to Value.VText(name),
            "can_play" to Value.VBool(canPlay))

    /** The device as a hook is handed it (`deviceTy`), as the server verified its user. */
    public fun record(user: String): Value =
        Value.record(
            "can_play" to Value.VBool(canPlay),
            "id" to Value.VText(id),
            "kind" to Value.VText(kind),
            "name" to Value.VText(name),
            "user" to Value.VText(user))

    public companion object {
        public fun fromValue(v: Value): DeviceDecl {
            val m = LiveWire.strct(v, "a device")
            return DeviceDecl(
                LiveWire.text(m, "id"),
                LiveWire.text(m, "kind"),
                LiveWire.text(m, "name"),
                LiveWire.boolean(m, "can_play"))
        }
    }
}

/**
 * `call` (`state::Call`): run a transition on an instance. `id` names the call; `counter`, an
 * ephemeral call's place among its device's, rising, and null for an ordered call.
 */
public class Call(
    public val instance: Instance,
    public val transition: String,
    public val input: Map<String, Value>,
    public val id: Id,
    public val counter: Long?,
) {
    public fun toValue(): Value {
        val f = LiveWire.instanceFields(instance)
        f.add("transition" to Value.VText(transition))
        f.add("input" to Value.VStruct(input))
        f.add("id" to Value.VId(id))
        counter?.let { f.add("counter" to Value.VInt(it)) }
        return LiveWire.tagged("call", f)
    }

    override fun equals(other: Any?): Boolean = other is Call && toValue() == other.toValue()

    override fun hashCode(): Int = toValue().hashCode()

    public companion object {
        public fun fromFields(m: Map<String, Value>): Call =
            Call(
                LiveWire.instanceOf(m),
                LiveWire.text(m, "transition"),
                LiveWire.strct(LiveWire.get(m, "input"), "an input"),
                LiveWire.ident(m, "id"),
                m["counter"]?.let { LiveWire.int(it, "counter") })
    }
}

/** How an `applied` call was taken (`state::Order`). */
public sealed class Order {
    /**
     * Sequenced: the instance's `seq`, the call it answers (null for a hook the server ran itself),
     * its author, the server's time it ran at, and the hash of the state it produced.
     */
    public class Ordered(
        public val seq: Long,
        public val id: Id?,
        public val user: String,
        public val session: String,
        /** In ascending order. */
        public val roles: List<String>,
        public val at: Long,
        public val hash: ByteArray,
    ) : Order()

    /** Relayed: the device's counter, which a replica takes only above the last it took. */
    public class Ephemeral(public val counter: Long) : Order()
}

/** `applied` (`state::Applied`): a call the server took, for every subscriber to run. */
public class LiveApplied(
    public val instance: Instance,
    public val transition: String,
    public val input: Map<String, Value>,
    /** The device that made it — for a hook, the device arriving or leaving. */
    public val device: String,
    public val order: Order,
) {
    /** The sequence of an ordered one. */
    public val seq: Long?
        get() = (order as? Order.Ordered)?.seq

    /** `ctx`: the context every replica runs it under. */
    public fun ctx(): LiveCtx =
        when (val o = order) {
            is Order.Ordered -> LiveCtx(o.user, o.session, o.roles.toSet(), device, o.at)
            is Order.Ephemeral -> LiveCtx(device = device)
        }

    public fun toValue(): Value {
        val f = LiveWire.instanceFields(instance)
        f.add("transition" to Value.VText(transition))
        f.add("input" to Value.VStruct(input))
        f.add("device" to Value.VText(device))
        when (val o = order) {
            is Order.Ordered -> {
                f.add("seq" to Value.VInt(o.seq))
                o.id?.let { f.add("id" to Value.VId(it)) }
                f.add("user" to Value.VText(o.user))
                f.add("session" to Value.VText(o.session))
                if (o.roles.isNotEmpty())
                    f.add("roles" to Value.VList(o.roles.map { Value.VText(it) }))
                f.add("at" to Value.VInt(o.at))
                f.add("hash" to Value.VBytes(o.hash))
            }
            is Order.Ephemeral -> f.add("counter" to Value.VInt(o.counter))
        }
        return LiveWire.tagged("applied", f)
    }

    public companion object {
        /**
         * An `applied` carries a `seq` or a `counter`, never both, and what it carries decides how
         * it is read: one carrying both is refused.
         */
        public fun fromFields(m: Map<String, Value>): LiveApplied {
            val seq = m["seq"]
            val counter = m["counter"]
            val order =
                when {
                    seq != null && counter == null ->
                        Order.Ordered(
                            LiveWire.int(seq, "seq"),
                            m["id"]?.let { LiveWire.ident(m, "id") },
                            LiveWire.text(m, "user"),
                            LiveWire.text(m, "session"),
                            rolesOf(m),
                            LiveWire.int(LiveWire.get(m, "at"), "at"),
                            (LiveWire.get(m, "hash") as? Value.VBytes)?.value
                                ?: LiveWire.bad("hash is bytes"))
                    seq == null && counter != null -> {
                        for (k in listOf("id", "user", "session", "roles", "at", "hash")) if (m
                            .containsKey(k))
                            LiveWire.bad("an ephemeral applied carries no $k")
                        Order.Ephemeral(LiveWire.int(counter, "counter"))
                    }
                    else -> LiveWire.bad("an applied carries a seq or a counter, and not both")
                }
            return LiveApplied(
                LiveWire.instanceOf(m),
                LiveWire.text(m, "transition"),
                LiveWire.strct(LiveWire.get(m, "input"), "an input"),
                LiveWire.text(m, "device"),
                order)
        }

        private fun rolesOf(m: Map<String, Value>): List<String> {
            val r = m["roles"] ?: return emptyList()
            val rs =
                (r as? Value.VList)?.items?.takeIf { it.isNotEmpty() }
                    ?: LiveWire.bad("roles: a non-empty list, or no field")
            val out = rs.map { (it as? Value.VText)?.value ?: LiveWire.bad("a role is text") }
            for (i in 1 until out.size) if (CodePointOrder.compare(out[i - 1], out[i]) >= 0)
                LiveWire.bad("roles: not in ascending order, or repeated")
            return out
        }
    }
}

/** `refused` (`state::Refused`): a call the server did not take, to its author. */
public class LiveRefused(
    public val instance: Instance,
    public val id: Id,
    public val reason: String
) {
    public fun toValue(): Value {
        val f = LiveWire.instanceFields(instance)
        f.add("id" to Value.VId(id))
        f.add("reason" to Value.VText(reason))
        return LiveWire.tagged("refused", f)
    }

    public companion object {
        public fun fromFields(m: Map<String, Value>): LiveRefused =
            LiveRefused(LiveWire.instanceOf(m), LiveWire.ident(m, "id"), LiveWire.text(m, "reason"))
    }
}

/**
 * `live_snapshot` (`state::Snapshot`): an instance whole, ephemeral slots and all, with the last
 * counter the server took from each device — `counters`, present only when there is one.
 */
public class LiveSnapshot(
    public val instance: Instance,
    public val seq: Long,
    public val value: Value,
    public val counters: Map<String, Long>,
) {
    public fun toValue(): Value {
        val f = LiveWire.instanceFields(instance)
        f.add("seq" to Value.VInt(seq))
        f.add("value" to value)
        if (counters.isNotEmpty())
            f.add("counters" to Value.VStruct(counters.mapValues { Value.VInt(it.value) }))
        return LiveWire.tagged("live_snapshot", f)
    }

    public companion object {
        public fun fromFields(m: Map<String, Value>): LiveSnapshot {
            val counters =
                when (val v = m["counters"]) {
                    null -> emptyMap()
                    else -> {
                        val c = LiveWire.strct(v, "counters")
                        if (c.isEmpty()) LiveWire.bad("no counters is written as no field")
                        c.mapValues { LiveWire.int(it.value, "a counter") }
                    }
                }
            return LiveSnapshot(
                LiveWire.instanceOf(m),
                LiveWire.int(LiveWire.get(m, "seq"), "seq"),
                LiveWire.get(m, "value"),
                counters)
        }
    }
}

/** What the server says of live state, to one connection (`state::Said`). */
public sealed class Said {
    public class Applied(public val applied: LiveApplied) : Said()

    public class Refused(public val refused: LiveRefused) : Said()

    public class Snapshot(public val snapshot: LiveSnapshot) : Said()
}

/** The wire forms' small parts: every frame a tagged struct, an instance `live` and `key`. */
internal object LiveWire {
    fun bad(what: String): Nothing = throw FactError("frame: $what")

    fun instanceFields(i: Instance): MutableList<Pair<String, Value>> =
        mutableListOf("live" to Value.VText(i.state), "key" to Value.VText(i.key))

    fun tagged(t: String, fields: MutableList<Pair<String, Value>>): Value {
        fields.add("t" to Value.VText(t))
        return Value.record(fields)
    }

    fun instanceOf(m: Map<String, Value>): Instance = Instance(text(m, "live"), text(m, "key"))

    fun get(m: Map<String, Value>, k: String): Value = m[k] ?: bad("missing $k")

    fun strct(v: Value, what: String): Map<String, Value> =
        (v as? Value.VStruct)?.fields ?: bad("$what is a struct")

    fun text(m: Map<String, Value>, k: String): String =
        (get(m, k) as? Value.VText)?.value ?: bad("$k is text")

    fun boolean(m: Map<String, Value>, k: String): Boolean =
        (get(m, k) as? Value.VBool)?.value ?: bad("$k is a bool")

    fun int(v: Value, k: String): Long = (v as? Value.VInt)?.value ?: bad("$k is an int")

    fun ident(m: Map<String, Value>, k: String): Id =
        (get(m, k) as? Value.VId)?.value ?: bad("$k is an id")
}

/** `resync`'s value: a client asking for an instance whole. */
public fun resyncValue(i: Instance): Value = LiveWire.tagged("resync", LiveWire.instanceFields(i))

// ---------------------------------------------------------------------------
// Running

/**
 * The live states of a module, and how each runs (`state::Runner`): through `Eval.runTransition`,
 * interpreted — what every runtime but Rust does, and what Rust does for a state it holds no native
 * code for.
 */
public class Runner(public val closures: Map<String, LiveClosure>) {
    /** Whether the module has any live state at all. */
    public val isEmpty: Boolean
        get() = closures.isEmpty()

    private fun closure(state: String): LiveClosure =
        closures[state] ?: throw EvalFault.Verdict(Refusal.Refused("no live state $state"))

    /** The kind of a transition: ordered for a state or a name there is not. */
    public fun kind(state: String, transition: String): Kind =
        closures[state]?.kind(transition) ?: Kind.Ordered

    /** The value an instance of `state` starts as. */
    public fun init(state: String): Value = Eval.constant(closure(state).state.init)

    /** The keys of the instances of `state` this author holds. */
    public fun keys(state: String, who: Ctx): List<String> = Eval.scopeKeys(closure(state), who)

    /** The hash of an instance's value, ephemeral fields left out. */
    public fun hash(state: String, v: Value): ByteArray =
        closures[state]?.valueHash(v) ?: Sha256.hash(Canon.encode(v))

    /** The fields of `state` that are ephemeral. */
    public fun ephemeral(state: String): Set<String> = closures[state]?.ephemeral ?: emptySet()

    /**
     * §14 `run`: a transition — a hook by its reserved name — over `value`. Leaving empties the
     * device's slot of every ephemeral field first, which is the engine's and no hook's to forget;
     * and an ephemeral call's slot is held to [SLOT_CAP]. A verdict or a bug is thrown as an
     * `EvalFault`.
     */
    public fun run(
        state: String,
        transition: String,
        value: Value,
        ctx: LiveCtx,
        input: Map<String, Value>
    ): Value {
        val c = closure(state)
        val v =
            if (transition == ON_LEAVE) emptySlots(c.ephemeral, value, deviceOf(input)) else value
        val out = Eval.runTransition(c, transition, v, ctx, input)
        if (c.kind(transition) == Kind.Ephemeral) {
            for (f in c.ephemeral) {
                val slots = (out as? Value.VStruct)?.fields?.get(f) as? Value.VStruct ?: continue
                val slot = slots.fields[ctx.device] ?: continue
                val n = Canon.encode(slot).size
                if (n > SLOT_CAP)
                    throw EvalFault.Verdict(
                        Refusal.Refused("$f: a slot holds at most $SLOT_CAP bytes, not $n"))
            }
        }
        return out
    }

    public companion object {
        /** Every live state of `m`, interpreted. */
        public fun ofModule(m: Module): Runner = Runner(Hash.liveClosures(m))

        // The id of the device a hook is handed.
        private fun deviceOf(input: Map<String, Value>): String =
            ((input["device"] as? Value.VStruct)?.fields?.get("id") as? Value.VText)?.value ?: ""

        // `v` with `device`'s slot of every ephemeral field emptied.
        private fun emptySlots(eph: Set<String>, v: Value, device: String): Value {
            if (v !is Value.VStruct || eph.isEmpty()) return v
            val m = java.util.TreeMap<String, Value>(CodePointOrder)
            m.putAll(v.fields)
            for (f in eph) {
                val slots = m[f] as? Value.VStruct ?: continue
                m[f] = Value.VStruct(slots.fields.filterKeys { it != device })
            }
            return Value.VStruct(m)
        }
    }
}

/** `verdict_text`: a fault as the words a refusal is sent with. */
internal fun liveVerdictText(e: EvalFault): String =
    when (e) {
        is EvalFault.Verdict -> refusalText(e.refusal)
        is EvalFault.Bug -> bugText(e.error)
    }

// ---------------------------------------------------------------------------
// The client's half

/**
 * An ordered call of this device's the server has not yet answered (`state::Pending`), with what
 * its preview ran under.
 */
public class PendingCall(
    public val id: Id,
    public val transition: String,
    public val input: Map<String, Value>,
    public val ctx: LiveCtx,
)

/**
 * One instance as a client holds it (`state::Watched`): what the server last confirmed, at its
 * sequence, and the view — the confirmed state with this device's pending calls replayed on top.
 */
public class WatchedInstance(
    public var confirmed: Value,
    public var seq: Long,
    /** A snapshot has arrived on this connection: before one, nothing of the instance applies. */
    public var synced: Boolean,
    /** A `resync` was asked and its snapshot has not arrived. */
    public var asked: Boolean,
    public val pending: MutableList<PendingCall>,
    public var view: Value,
    /** The last counter taken from each device for its ephemeral calls. */
    public var counters: MutableMap<String, Long>,
)

/** What a client's call came to (`state::Called`): its kind, and the frame to send if linked. */
public class Called(public val kind: Kind, public val call: Call)

/**
 * §14 A client's live state (`state::Watch`): every instance it has heard of or called on, and the
 * device it declares itself as — null, and it is no device, subscribes to nothing, and its frames
 * are the bytes they always were.
 */
public class Watch(public val runner: Runner, public val device: DeviceDecl?) {
    public val instances: java.util.TreeMap<Instance, WatchedInstance> = java.util.TreeMap()

    /** Verdicts the server gave this device's calls, oldest first, until taken. */
    public val refused: MutableList<Pair<Id, String>> = ArrayList()

    private var moved = false

    /** Something moved since the last ask: a view, a verdict. */
    public fun takeMoved(): Boolean = moved.also { moved = false }

    /** `instance_of`: the instance of `state` this author holds — its scope's first key. */
    public fun instanceOf(state: String, who: Ctx): Instance? =
        try {
            runner.keys(state, who).firstOrNull()?.let { Instance(state, it) }
        } catch (e: EvalFault) {
            null
        }

    /** An instance's view: the confirmed state with pending calls on top. */
    public fun view(i: Instance): Value? = instances[i]?.view

    private fun deviceId(who: Ctx): String {
        val d = device
        return if (d != null && d.id.isNotEmpty()) d.id else who.session
    }

    private fun watched(i: Instance): WatchedInstance {
        instances[i]?.let {
            return it
        }
        val v =
            try {
                runner.init(i.state)
            } catch (e: EvalFault) {
                throw LiveRefusal(Refusal.Refused(liveVerdictText(e)))
            }
        val w = WatchedInstance(v, 0, false, false, ArrayList(), v, HashMap())
        instances[i] = w
        return w
    }

    /**
     * §14.2 `call`: this device calls a transition, run at once over its view with `now` its guess
     * at the server's clock. Ordered, it is pending until the server answers; ephemeral, it is
     * applied and done, `counter` its place among this device's. A refusal here is the preview's
     * verdict, thrown as a `LiveRefusal`, and nothing is sent.
     */
    public fun call(
        who: Ctx,
        i: Instance,
        transition: String,
        input: Map<String, Value>,
        id: Id,
        now: Long,
        counter: Long
    ): Called {
        val kind = runner.kind(i.state, transition)
        if (kind == Kind.Ordered &&
            (transition == ON_JOIN || transition == ON_LEAVE || transition == ON_RESTORE))
            throw LiveRefusal(Refusal.Refused("$transition is the server's to run"))
        val ctx = LiveCtx(who.user, who.session, who.roles, deviceId(who), now)
        val w = watched(i)
        val preview =
            try {
                runner.run(i.state, transition, w.view, ctx, input)
            } catch (e: EvalFault.Verdict) {
                throw LiveRefusal(e.refusal)
            } catch (e: EvalFault.Bug) {
                throw LiveRefusal(Refusal.Refused(bugText(e.error)))
            }
        val call =
            when (kind) {
                Kind.Ephemeral -> {
                    try {
                        w.confirmed = runner.run(i.state, transition, w.confirmed, ctx, input)
                    } catch (e: EvalFault) {
                        // The confirmed state no longer takes it; the view still moved.
                    }
                    w.counters[ctx.device] = counter
                    Call(i, transition, input, id, counter)
                }
                Kind.Ordered -> {
                    w.pending.add(PendingCall(id, transition, input, ctx))
                    Call(i, transition, input, id, null)
                }
            }
        w.view = preview
        if (kind == Kind.Ephemeral) replay(i.state, w)
        moved = true
        return Called(kind, call)
    }

    /**
     * `connected`: a connection opened — every instance waits for its snapshot, and every pending
     * call is said again, in the order it was made.
     */
    public fun connected(): List<Call> {
        val out = ArrayList<Call>()
        for ((i, w) in instances) {
            w.synced = false
            w.asked = false
            for (p in w.pending) out.add(Call(i, p.transition, p.input, p.id, null))
        }
        return out
    }

    /**
     * `heard`: what the server said; the instances to ask a snapshot of — a gap in the sequence, or
     * a state whose hash is not the server's.
     */
    public fun heard(said: Said): List<Instance> {
        when (said) {
            is Said.Snapshot -> {
                val s = said.snapshot
                if (runner.closures.containsKey(s.instance.state)) {
                    val w =
                        instances.getOrPut(s.instance) {
                            WatchedInstance(
                                s.value, s.seq, true, false, ArrayList(), s.value, HashMap())
                        }
                    w.confirmed = s.value
                    w.seq = s.seq
                    w.synced = true
                    w.asked = false
                    w.counters = HashMap(s.counters)
                    replay(s.instance.state, w)
                    moved = true
                }
                return emptyList()
            }
            is Said.Refused -> {
                val r = said.refused
                val w = instances[r.instance] ?: return emptyList()
                if (w.pending.removeAll { it.id == r.id }) {
                    refused.add(r.id to r.reason)
                    replay(r.instance.state, w)
                    moved = true
                }
                return emptyList()
            }
            is Said.Applied -> {
                val a = said.applied
                val w = instances[a.instance] ?: return emptyList()
                if (!w.synced) return emptyList()
                when (val o = a.order) {
                    is Order.Ephemeral -> {
                        val last = w.counters[a.device]
                        if (last != null && last >= o.counter) return emptyList()
                        w.counters[a.device] = o.counter
                        try {
                            w.confirmed =
                                runner.run(
                                    a.instance.state, a.transition, w.confirmed, a.ctx(), a.input)
                            replay(a.instance.state, w)
                            moved = true
                        } catch (e: EvalFault) {
                            // Not one this replica takes: the server's next snapshot says.
                        }
                        return emptyList()
                    }
                    is Order.Ordered -> {
                        // Already confirmed — the answer to a call pushed again — leaves pending
                        // what it answers.
                        if (o.seq <= w.seq) {
                            val id = o.id
                            if (id != null && w.pending.removeAll { it.id == id }) {
                                replay(a.instance.state, w)
                                moved = true
                            }
                            return emptyList()
                        }
                        if (o.seq > w.seq + 1) return ask(w, a.instance)
                        val next =
                            try {
                                runner.run(
                                    a.instance.state, a.transition, w.confirmed, a.ctx(), a.input)
                            } catch (e: EvalFault) {
                                null
                            }
                        // Not the server's state: ask for it whole.
                        if (next == null ||
                            !runner.hash(a.instance.state, next).contentEquals(o.hash))
                            return ask(w, a.instance)
                        w.confirmed = next
                        w.seq = o.seq
                        o.id?.let { id -> w.pending.removeAll { it.id == id } }
                        replay(a.instance.state, w)
                        moved = true
                        return emptyList()
                    }
                }
            }
        }
    }

    /** Verdicts on this device's calls since the last ask. */
    public fun takeRefused(): List<Pair<Id, String>> {
        val out = ArrayList(refused)
        refused.clear()
        return out
    }

    // A snapshot asked for, once until it arrives.
    private fun ask(w: WatchedInstance, i: Instance): List<Instance> {
        if (w.asked) return emptyList()
        w.asked = true
        return listOf(i)
    }

    // The view again: the confirmed state with every pending call replayed on top, in order, under
    // what each was made with; one the state no longer takes is passed over, still pending, until
    // the server answers it.
    private fun replay(state: String, w: WatchedInstance) {
        var v = w.confirmed
        for (p in w.pending) {
            try {
                v = runner.run(state, p.transition, v, p.ctx, p.input)
            } catch (e: EvalFault) {
                // Passed over.
            }
        }
        w.view = v
    }
}

/** A live call refused by its own preview (`Refusal`, in the reference's `Result`). */
public class LiveRefusal(public val refusal: Refusal) :
    Exception(refusalText(refusal), null, false, false)
