// §11 The peer (Ark.Peer): the replica and the authority. Both are
// sans-io: they take what arrived and give back what follows.
//
// The spec's replica is a value; this one is an object whose methods move
// it. Stores are still treated as values — every store a replica holds
// was produced by `Eval.applyClosure` or `applyChanges` on a fork and is
// never written in place — so `view = confirmed` shares as the spec does.
package dev.arkdb

import java.util.TreeMap

/** A confirmed entry received and not yet applied: out of order, or waiting for facts. */
public data class Inbox(val entry: Entry?, val facts: Facts?)

/** What a view is told: the changes since it last asked, or that it was rebuilt. */
public sealed class Changes {
    public data class Applied(val changes: List<Change>) : Changes()
    public object Rebuilt : Changes() {
        override fun toString(): String = "Rebuilt"
    }
}

/** One peer's copy of one scope. */
public class Replica private constructor(
    public val scope: String,
    public val schema: Schema,
    bodies: Map<FnHash, Closure>,
    confirmed: MemoryStore,
    cursor: Seq,
    pending: List<Entry>,
) {
    /** The closures this peer can run, keyed by the hash an entry names. */
    public var bodies: Map<FnHash, Closure> = bodies
        private set

    /** The confirmed store: the scope exactly as the authority had it at `cursor`. */
    public var confirmed: MemoryStore = confirmed
        private set

    public var cursor: Seq = cursor
        private set

    /** Intents authored here that no verdict has answered, in authoring order. */
    public var pending: List<Entry> = pending
        private set

    /** The optimistic store: `confirmed` with `pending` replayed. */
    public var view: MemoryStore = confirmed
        private set

    public val inbox: TreeMap<Seq, Inbox> = TreeMap()

    /** Verdicts against this peer's own intents, newest last. */
    public val rejections: MutableList<Pair<Id, Refusal>> = ArrayList()

    /** Sequences at which this peer's replay disagreed with the authority's facts. */
    public val diverged: MutableList<Seq> = ArrayList()

    private var rebuilt: Boolean = true
    private val changes: MutableList<Change> = ArrayList()

    public companion object {
        /** §11.1 Open a replica from what was durable; the pending intents are replayed on top. */
        public fun open(
            sch: Schema,
            scope: String,
            bodies: Map<FnHash, Closure>,
            confirmed: MemoryStore,
            cursor: Seq,
            pending: List<Entry>,
        ): Replica {
            val r = Replica(scope, sch, bodies, confirmed, cursor, pending)
            r.replay()
            return r
        }
    }

    /** Give the replica closures it did not have (new ones win), as a `Closures` frame does. */
    public fun learn(more: Map<FnHash, Closure>) {
        bodies = more + bodies.filterKeys { it !in more }
    }

    /**
     * §11.2 Author an intent: apply it forward into the optimistic store and,
     * if it is not refused, record it as pending. Throws `Fault.Refuse` on a
     * refusal, which changes nothing and records nothing.
     */
    public fun mutate(i: Id, ctx: Ctx, fh: FnHash, autos: Args, args: Args): Entry {
        val c = bodies[fh] ?: throw Fault.Refuse(Refusal.Refused("unknown function ${fh.hex}"))
        val applied = try {
            Eval.applyClosure(schema, c, ctx, autos, args, view)
        } catch (b: Fault.Bug) {
            throw Fault.Refuse(Refusal.Refused("bug: ${b.text}"))
        }
        return record(i, ctx, fh, autos, args, applied)
    }

    /**
     * §11.2 with generated code standing in for the interpreter: `body` is
     * the generated function for `fh`, run through a `TransactionStore` over
     * the optimistic store. The entry records the hash exactly as `mutate`
     * does, and the replica must still hold the closure, because a rebase
     * replays pending intents through the interpreter (§11.6) — the two are
     * held to agree by the conformance suite.
     */
    public fun mutateWith(i: Id, ctx: Ctx, fh: FnHash, autos: Args, args: Args, body: (Store) -> Unit): Entry {
        if (fh !in bodies) throw Fault.Refuse(Refusal.Refused("unknown function ${fh.hex}"))
        val applied = try {
            TransactionStore.run(view, body)
        } catch (b: Fault.Bug) {
            throw Fault.Refuse(Refusal.Refused("bug: ${b.text}"))
        }
        return record(i, ctx, fh, autos, args, applied)
    }

    // What both ways of authoring share: a verdict is thrown and changes
    // nothing; otherwise the entry is pending and the view has moved.
    private fun record(i: Id, ctx: Ctx, fh: FnHash, autos: Args, args: Args, applied: Eval.Applied): Entry =
        when (applied) {
            is Eval.Applied.Refused -> throw Fault.Refuse(applied.refusal)
            is Eval.Applied.Ok -> {
                val e = Entry(i, ctx.user, ctx.session, fh, args, autos)
                view = applied.store
                pending = pending + e
                changes.addAll(applied.changes)
                e
            }
        }

    /** §11.3 A confirmed entry arrives, at its sequence. */
    public fun receive(n: Seq, e: Entry) {
        if (n <= cursor) return
        val old = inbox[n]
        inbox[n] = if (old == null) Inbox(e, null) else old.copy(entry = e)
        advance()
    }

    /** An entry and its facts arrive together. */
    public fun receiveWith(n: Seq, e: Entry, f: Facts) {
        if (n <= cursor) return
        inbox[n] = Inbox(e, f)
        advance()
    }

    /** The facts of an entry arrive, at its sequence. */
    public fun receiveFacts(n: Seq, f: Facts) {
        if (n <= cursor) return
        val old = inbox[n]
        inbox[n] = if (old == null) Inbox(null, f) else old.copy(facts = f)
        advance()
    }

    /** §11.4 This peer's own intent was sequenced at n. */
    public fun ack(i: Id, n: Seq) {
        val e = pending.firstOrNull { it.id == i } ?: return
        receive(n, e)
    }

    /** §11.5 A verdict against this peer's own intent: dropped, kept for the app, the view rebuilt. */
    public fun reject(i: Id, why: Refusal) {
        pending = pending.filter { it.id != i }
        rejections.add(i to why)
        replay()
    }

    /** The sequences the replica is waiting on facts for. */
    public fun needs(): List<Seq> = inbox.entries.filter { (n, ib) ->
        ib.entry != null && ib.facts == null && (ib.entry.fn !in bodies || n in diverged)
    }.map { it.key }

    /** Try the inbox again. */
    public fun retry(): Unit = advance()

    /** What a view is told, and the slate wiped. */
    public fun takeChanges(): Changes {
        val out: Changes = if (rebuilt) Changes.Rebuilt else Changes.Applied(changes.toList())
        rebuilt = false
        changes.clear()
        return out
    }

    /** This replica's claim: its cursor and the hash of its confirmed state there. */
    public fun verifyAt(): Pair<Seq, ByteArray> = cursor to Hash.stateHash(confirmed)

    // §11.6 Advancing --------------------------------------------------------

    private class Step(val store: MemoryStore, val changes: List<Change>, val diverged: Boolean)

    private fun advance() {
        val pendingBefore = pending
        val acc = ArrayList<Change>()
        var moved = false
        var others = false
        while (true) {
            val n = cursor + 1
            val ib = inbox[n] ?: break
            val e = ib.entry ?: break
            val step = applyOne(n, e, ib.facts) ?: break
            val ownNext = pending.firstOrNull()?.id == e.id
            confirmed = step.store
            cursor = n
            inbox.remove(n)
            pending = pending.filter { it.id != e.id }
            if (step.diverged) diverged.add(n)
            acc.addAll(step.changes)
            moved = true
            others = others || !ownNext || step.diverged
        }
        when {
            !moved -> Unit
            pendingBefore.isEmpty() -> {
                view = confirmed
                changes.addAll(acc)
            }
            !others -> if (pending.isEmpty()) view = confirmed
            else -> replay()
        }
    }

    // One entry against the confirmed store: by intent when the closure is
    // held, by facts otherwise; both when both are present, comparing them.
    private fun applyOne(n: Seq, e: Entry, mf: Facts?): Step? {
        val c = bodies[e.fn]
        if (c != null && n !in diverged) {
            val applied = try {
                Eval.applyClosure(schema, c, Ctx(e.actor, e.session), e.autos, e.args, confirmed)
            } catch (b: Fault.Bug) {
                null
            }
            return if (applied is Eval.Applied.Ok) {
                if (mf != null && mf != applied.changes) Step(confirmed.applyChanges(mf), mf, true)
                else Step(applied.store, applied.changes, false)
            } else {
                if (mf != null) Step(confirmed.applyChanges(mf), mf, true) else null
            }
        }
        return if (mf != null) Step(confirmed.applyChanges(mf), mf, false) else null
    }

    // Rebuild the view: the confirmed store, then every pending intent in order.
    private fun replay() {
        view = confirmed
        rebuilt = true
        changes.clear()
        val kept = ArrayList<Entry>()
        for (e in pending) {
            val c = bodies[e.fn]
            if (c == null) {
                rejections.add(e.id to Refusal.Refused("no closure for a pending intent"))
                continue
            }
            try {
                when (val applied = Eval.applyClosure(schema, c, Ctx(e.actor, e.session), e.autos, e.args, view)) {
                    is Eval.Applied.Ok -> {
                        view = applied.store
                        kept.add(e)
                    }
                    is Eval.Applied.Refused -> rejections.add(e.id to applied.refusal)
                }
            } catch (b: Fault.Bug) {
                rejections.add(e.id to Refusal.Refused("bug: ${b.text}"))
            }
        }
        pending = kept
    }
}

/** The answer to a pushed intent. */
public sealed class Sequenced {
    public data class Appended(val seq: Seq, val facts: Facts) : Sequenced()
    public data class Duplicate(val seq: Seq) : Sequenced()
    public data class Rejected(val refusal: Refusal) : Sequenced()
}

/** Why a log offered for adoption was turned away. */
public sealed class AdoptError : Exception() {
    public object NotFromTheBeginning : AdoptError()
    public object Gap : AdoptError()
    public data class MissingClosure(val seq: Seq, val fn: FnHash) : AdoptError()
    public data class RefusedAt(val seq: Seq, val refusal: Refusal) : AdoptError()
    public data class FactsDiffer(val seq: Seq) : AdoptError()
    public object HashDiffers : AdoptError()
}

/** The peer that sequences a scope. */
public class Authority(public val scope: String, public val schema: Schema, bodies: Map<FnHash, Closure>) {
    /** Every closure ever accepted for this scope, by hash. */
    public var bodies: Map<FnHash, Closure> = bodies
        private set

    public val log: Log = Log(schema)

    /** The state at the head of the log. */
    public var store: MemoryStore = MemoryStore(schema)
        private set

    /** §11.7 Sequence an intent: dedupe by id, apply to the head state, append with the facts. */
    public fun sequenceEntry(e: Entry): Sequenced {
        log.seqOf(e.id)?.let { return Sequenced.Duplicate(it) }
        val c = bodies[e.fn] ?: return Sequenced.Rejected(Refusal.Refused("unknown function ${e.fn.hex}"))
        val applied = try {
            Eval.applyClosure(schema, c, Ctx(e.actor, e.session), e.autos, e.args, store)
        } catch (b: Fault.Bug) {
            return Sequenced.Rejected(Refusal.Refused("bug: ${b.text}"))
        }
        return when (applied) {
            is Eval.Applied.Refused -> Sequenced.Rejected(applied.refusal)
            is Eval.Applied.Ok -> {
                val n = log.append(e, applied.changes)
                store = applied.store
                Sequenced.Appended(n, applied.changes)
            }
        }
    }

    /** What a peer at a cursor is sent: a page, or the snapshot if it is below the horizon. */
    public fun page(cursor: Seq, limit: Int): Page = log.entriesAfter(cursor, limit)

    /** Move the horizon; false if the sequence is not retained. */
    public fun compact(n: Seq): Boolean = log.compactTo(n)

    /** Drop every closure that neither the current module nor a retained entry names. */
    public fun retire(current: Set<FnHash>) {
        val keep = current + log.namedHashes
        bodies = bodies.filterKeys { it in keep }
    }

    public companion object {
        /**
         * §11.8 Adopt a scope a peer sequenced alone: replay every intent from
         * the beginning, holding each to the facts the peer recorded.
         */
        public fun adopt(sch: Schema, scope: String, bodies: Map<FnHash, Closure>, l: Log): Authority {
            if (l.horizon != 0L || !l.base.store.isEmpty) throw AdoptError.NotFromTheBeginning
            if (!l.contiguous) throw AdoptError.Gap
            val a = Authority(scope, sch, bodies)
            for ((n, ef) in l.entries) {
                val (e, recorded) = ef
                when (val s = a.sequenceEntry(e)) {
                    is Sequenced.Appended -> {
                        if (s.seq != n) throw AdoptError.Gap
                        if (s.facts != recorded) throw AdoptError.FactsDiffer(n)
                    }
                    is Sequenced.Rejected -> throw AdoptError.RefusedAt(n, s.refusal)
                    is Sequenced.Duplicate -> throw AdoptError.Gap
                }
            }
            val claimed = l.stateAt(l.headSeq) ?: throw AdoptError.HashDiffers
            if (!Hash.stateHash(a.store).contentEquals(Hash.stateHash(claimed))) throw AdoptError.HashDiffers
            return a
        }

        /**
         * §11.9 A peer that is its own authority: everything pending is
         * sequenced and every answer delivered back, in order.
         */
        public fun localCommit(a: Authority, r: Replica) {
            for (e in r.pending.toList()) {
                when (val s = a.sequenceEntry(e)) {
                    is Sequenced.Appended -> {
                        r.receiveFacts(s.seq, s.facts)
                        r.ack(e.id, s.seq)
                    }
                    is Sequenced.Duplicate -> r.ack(e.id, s.seq)
                    is Sequenced.Rejected -> r.reject(e.id, s.refusal)
                }
            }
        }
    }
}
