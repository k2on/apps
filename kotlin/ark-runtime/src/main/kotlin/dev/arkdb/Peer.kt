// §11 The peer, as `rust/ark/src/peer.rs` defines it: the log machine every
// peer runs (`Replica`), and the authority role a peer takes for the log it
// sequences (`Authority`) — which a client takes only alone. Both are
// sans-io.
//
// A replica's view is always `replay(confirmed) then replay(pending)`.
// Confirmed state moves only forward; the only thing ever undone is the
// replica's own pending, replayed on top when confirmed entries land. That
// is the rebase. A replica that holds the closure an entry names replays
// the intent; one that does not asks for the facts; when it holds both it
// replays and compares, and a disagreement is recorded in `diverged` and
// resolved in the authority's favour. An intent of this replica's own is
// the exception, because it was already run here: what that run did is
// kept beside it, and confirming it applies that record rather than
// running it again (`docs/plan-perf.md` R2).
//
// One representation differs from the Rust and says nothing different. The
// Rust view shares with the confirmed store every table it has not written
// (`docs/plan-db.md` D7.3) and copies a table on its first write, keeping
// the copy until the next replay; here the view is its own store, copied
// whole from the confirmed store at open and at every replay, and moved by
// every change the confirmed store moves by while it shares that state. A
// shared table and a copy given the same changes hold the same rows, so
// what a view reports, every hash and every frame are the Rust's; what
// differs is a copy per replay, which a phone opening or adopting a
// snapshot pays once. The Rust's native procedures (`authoring::Procedure`)
// are an authoring vocabulary's, which a phone does not have: an entry is
// run by a raw write, by the closure its hash names, or not at all.
package dev.arkdb

import java.util.TreeMap

/** An entry and its facts as they wait in a replica's inbox (`peer::Inbox`). */
public class Inbox(public var entry: LogEntry? = null, public var facts: FactList? = null)

/**
 * What a view is told (`peer::Changes`): the changes to the optimistic store since it last asked —
 * every transition it made, a rebase's included — or that it was replaced whole and must
 * re-hydrate.
 */
public sealed class Changes {
    /** Oldest first. */
    public data class Applied(val changes: List<Change>) : Changes()

    public object Rebuilt : Changes()
}

/**
 * What whoever keeps the confirmed store durable is told (§11.1, `peer::Journal`): the changes it
 * moved by per sequence since they last asked, or that it was replaced and must be written whole.
 */
public sealed class Moved {
    public data class Run(val run: List<Pair<Seq, FactList>>) : Moved()

    public object Replaced : Moved()
}

/** An entry run by hash (`peer::run`): its changes, its verdict, or a bug. */
internal sealed class Applied {
    data class Ok(val changes: List<Change>) : Applied()

    data class Refused(val refusal: Refusal) : Applied()

    data class Bug(val error: EvalError) : Applied()
}

/** The key a closure is held under: its hash, as lowercase hex. */
internal fun hashKey(fh: ByteArray): String = Hex.encode(fh)

/**
 * The entry's author as every run of it after the first sees it (`peer::ctx_of`): the actor, the
 * login and the roles frozen in it.
 */
internal fun ctxOf(e: LogEntry): Ctx = Ctx(e.actor, e.session, e.roles, false)

/**
 * `bug_text`: a bug, as the Rust writes `format!("bug: {e:?}")` — the error's `Debug` form, which a
 * verdict made of it carries word for word.
 */
public fun bugText(e: EvalError): String = "bug: " + debugEvalError(e)

/** An `EvalError` as Rust's derived `Debug` writes it. */
public fun debugEvalError(e: EvalError): String =
    when (e) {
        is EvalError.UnknownFunction -> "UnknownFunction(${debugString(e.name)})"
        is EvalError.WrongKind -> "WrongKind(${debugString(e.name)}, ${e.kind.name})"
        is EvalError.MissingArg -> "MissingArg(${debugString(e.name)})"
        is EvalError.MissingAuto -> "MissingAuto(${debugString(e.name)})"
        is EvalError.UnboundVar -> "UnboundVar(${e.sym})"
        is EvalError.NoSuchField -> "NoSuchField(${debugString(e.name)})"
        is EvalError.TypeError -> "TypeError(${debugString(e.what)})"
        is EvalError.Arity -> "Arity(${debugString(e.what)})"
        is EvalError.NoReturn -> "NoReturn(${debugString(e.name)})"
        is EvalError.Impure -> "Impure(${debugString(e.what)})"
        is EvalError.UnknownTable -> "UnknownTable(${debugString(e.table)})"
        is EvalError.CompositeParentKey -> "CompositeParentKey(${debugString(e.table)})"
        is EvalError.NotProvided -> "NotProvided(${debugString(e.name)})"
    }

/**
 * `run`: an entry's function applied by its hash — a raw write natively (`docs/plan-guards.md` D4),
 * otherwise through the closure the hash names; `null` when neither is held.
 */
internal fun runEntry(
    schema: Schema,
    bodies: Map<String, Closure>,
    fh: ByteArray,
    ctx: Ctx,
    autos: Map<String, Value>,
    args: Map<String, Value>,
    store: Store,
): Applied? {
    val raw = Raw.of(fh)
    val c = if (raw == null) bodies[hashKey(fh)] ?: return null else null
    return try {
        val ran =
            if (raw != null) Raw.apply(raw, args, store)
            else Eval.applyClosure(schema, c!!, ctx, autos, args, store)
        when (ran) {
            is Ran.Ok -> Applied.Ok(ran.changes)
            is Ran.Refused -> Applied.Refused(ran.refusal)
        }
    } catch (v: EvalFault.Verdict) {
        Applied.Refused(v.refusal)
    } catch (b: EvalFault.Bug) {
        Applied.Bug(b.error)
    }
}

/**
 * `invert`: a change undone, exact because a change carries the whole row on each side of it
 * (`docs/plan-perf.md` R2).
 */
internal fun invert(c: Change): Change =
    when (c) {
        is Change.Add -> Change.Remove(c.table, c.row)
        is Change.Remove -> Change.Add(c.table, c.row)
        is Change.Edit -> Change.Edit(c.table, c.new, c.old)
    }

/**
 * `transitions`: whether `chs`, applied in order over `store`, is a transition at every step — an
 * `Add` onto an absent key, a `Remove` or an `Edit` from exactly the row there. Over an overlay.
 */
internal fun transitions(schema: Schema, store: Store, chs: List<Change>): Boolean {
    val over = Overlay(store)
    for (c in chs) {
        val tbl = schema.lookupTable(c.table) ?: return false
        val (row, before) =
            when (c) {
                is Change.Add -> c.row to null
                is Change.Remove -> c.row to c.row
                is Change.Edit -> c.new to c.old
            }
        if (over.get(c.table, tbl.keyOf(row)) != before) return false
        over.applyChange(c)
    }
    return true
}

/**
 * One peer's copy of the log (`peer::Replica`). The fields are the Rust's, each documented there;
 * `view` is its own store (the file's opening note).
 */
public class Replica(
    public var schema: Schema,
    /** The closures this peer can run, by the hash an entry names (as hex). */
    public val bodies: MutableMap<String, Closure>,
    /** The confirmed store: the tables exactly as the authority had them at `cursor`. */
    public var confirmed: MemoryStore,
    public var cursor: Seq,
    /** Intents authored here that no verdict has answered, in authoring order. */
    pending: List<LogEntry>,
) {
    /** The log `confirmed` is a prefix of, once the server said (§10, §12.4). */
    public var logId: Id? = null
    public val pending: MutableList<LogEntry> = ArrayList(pending)

    /** What each pending intent's run did to `view`, by its id (R2). Never durable. */
    public val recorded: HashMap<Id, FactList> = HashMap()

    /** The optimistic store: `confirmed` with `pending` replayed. Never durable. */
    public var view: MemoryStore = confirmed

    /** Confirmed entries received and not yet applied. */
    public val inbox: TreeMap<Seq, Inbox> = TreeMap()

    /** Verdicts against this peer's own intents, newest last. */
    public val rejections: MutableList<Pair<Id, Refusal>> = ArrayList()

    /** Sequences at which this peer's replay disagreed with the authority's facts. */
    public val diverged: MutableList<Seq> = ArrayList()

    public var rebuilt: Boolean = true
    public val changes: MutableList<Change> = ArrayList()
    public var replaced: Boolean = true

    /** What the confirmed store moved by since `takeConfirmed` last asked, per sequence. */
    public val journal: MutableList<Pair<Seq, FactList>> = ArrayList()

    /** The server runs another module than this peer's (§12 `behind`, `docs/plan-db.md` D1). */
    public var behind: Boolean = false

    /** The facts of pending intents this replica holds no closure for, where it knows them (D1). */
    public val known: HashMap<Id, FactList> = HashMap()

    /** `docs/plan-guards.md` D2 The union held: the tables held in part, with their columns. */
    public var partial: TreeMap<String, List<String>>? = null

    /** A partial replica's horizon of what it has been told (`upto`). */
    public var through: Seq = cursor

    init {
        replay()
    }

    /** `Replica::open`: from what was durable, the pending intents replayed on top. */
    public companion object {
        public fun open(
            schema: Schema,
            bodies: Map<String, Closure>,
            confirmed: MemoryStore,
            cursor: Seq,
            pending: List<LogEntry>,
        ): Replica = Replica(schema, TreeMap(bodies), confirmed, cursor, pending)
    }

    private fun run(e: LogEntry, store: Store): Applied? =
        runEntry(schema, bodies, e.fnHash, ctxOf(e), e.autos, e.args, store)

    /**
     * §11.2 `mutate`: an intent applied forward into the optimistic store and, if it is not
     * refused, recorded as pending; the entry, or the verdict, which changes nothing.
     */
    public fun mutate(
        id: Id,
        ctx: Ctx,
        fh: ByteArray,
        autos: Map<String, Value>,
        args: Map<String, Value>
    ): Pair<LogEntry?, Refusal?> {
        val raw = Raw.of(fh)
        if (raw != null) return null to Refusal.Forbidden(Raw.tableOf(raw, args))
        return when (val out = runEntry(schema, bodies, fh, ctx, autos, args, Overlay(view))) {
            null -> null to Refusal.Refused(unknownFunction(fh))
            is Applied.Bug -> null to Refusal.Refused(bugText(out.error))
            is Applied.Refused -> null to out.refusal
            is Applied.Ok -> {
                val e = LogEntry(id, ctx.user, ctx.session, ctx.roles, fh, args, autos)
                view.applyChanges(out.changes)
                if (recorded.size > pending.size) {
                    val live = pending.map { it.id }.toSet()
                    recorded.keys.retainAll(live)
                }
                recorded[e.id] = out.changes
                pending.add(e)
                changes.addAll(out.changes)
                e to null
            }
        }
    }

    /** `can_apply`: a closure is held for the hash, or it is a raw write. */
    public fun canApply(fh: ByteArray): Boolean = bodies.containsKey(hashKey(fh)) || Raw.isRaw(fh)

    /** D3 `is_private`: an entry naming this hash is taken by the authority's facts. */
    public fun isPrivate(fh: ByteArray): Boolean = bodies[hashKey(fh)]?.let(Hash::isPrivate) == true

    /** §11.3 `receive`: placed in the inbox, applied by the next `settle` (R8). */
    public fun receive(n: Seq, e: LogEntry) {
        if (n <= cursor) return
        inbox.getOrPut(n) { Inbox() }.entry = e
    }

    /** `receive_with`: an entry and its facts together, as a batch delivers them. */
    public fun receiveWith(n: Seq, e: LogEntry, f: FactList) {
        if (n <= cursor) return
        inbox[n] = Inbox(e, f)
    }

    /** `receive_batch`: a page handed in whole, as its own pump. */
    public fun receiveBatch(items: List<Triple<Seq, LogEntry, FactList?>>) {
        for ((n, e, f) in items) if (f != null) receiveWith(n, e, f) else receive(n, e)
        settle()
    }

    /** `receive_facts`: the facts of an entry, at its sequence. */
    public fun receiveFacts(n: Seq, f: FactList) {
        if (n <= cursor) return
        inbox.getOrPut(n) { Inbox() }.facts = f
    }

    /**
     * §11.4 `ack`: this peer's own intent was sequenced at `n`, placed as if it had arrived; one at
     * or below the cursor is confirmed by the store already holding it, and dropped.
     */
    public fun ack(id: Id, n: Seq) {
        val at = pending.indexOfFirst { it.id == id }
        if (at < 0) return
        if (n <= cursor) {
            val undo = pending.subList(at, pending.size).map { it.id }
            pending.removeAt(at)
            rebase(undo, emptyList(), at)
            return
        }
        receive(n, pending[at])
    }

    /**
     * §11.2b `sign_in`: every pending intent authored as nobody becomes `who`'s, and the view is
     * rebased by changes from the first rewritten.
     */
    public fun signIn(who: Ctx) {
        fun nobody(e: LogEntry) = e.actor.isEmpty() && e.session.isEmpty()
        val from = pending.indexOfFirst(::nobody)
        if (from < 0) return
        val undo = pending.subList(from, pending.size).map { it.id }
        for (i in from until pending.size) {
            if (nobody(pending[i])) pending[i] = pending[i].by(who.user, who.session)
        }
        rebase(undo, emptyList(), from)
    }

    /**
     * §11.5 `reject`: a verdict against an intent still pending — dropped, the verdict kept, and
     * the view rebased from it.
     */
    public fun reject(id: Id, why: Refusal) {
        val at = pending.indexOfFirst { it.id == id }
        if (at < 0) return
        rejections.add(id to why)
        val undo = pending.subList(at, pending.size).map { it.id }
        pending.removeAt(at)
        rebase(undo, emptyList(), at)
    }

    /**
     * `fork_back`, alone → server (`docs/plan-alone.md` §1): the local history since the fork taken
     * back out of the confirmed store and re-queued ahead of what is pending; the view told every
     * transition.
     */
    public fun forkBack(local: List<FactList>, fork: Seq, logId: Id?, requeue: List<LogEntry>) {
        val order = pending.map { it.id }
        val whole = order.all { recorded.containsKey(it) }
        for ((e, f) in requeue.zip(local)) if (!canApply(e.fnHash)) known[e.id] = f
        val told = ArrayList<Change>()
        if (whole) {
            for (id in order.asReversed()) {
                val rec = recorded.remove(id) ?: emptyList()
                for (c in rec.asReversed()) {
                    val back = invert(c)
                    view.applyChange(back)
                    told.add(back)
                }
            }
        }
        for (facts in local.asReversed()) {
            for (c in facts.asReversed()) {
                val back = invert(c)
                confirmed.applyChange(back)
                if (whole) view.applyChange(back)
                told.add(back)
            }
        }
        cursor = fork
        this.logId = logId
        inbox.clear()
        journal.clear()
        replaced = true
        val queued = requeue.map { it.id }.toSet()
        val rest = pending.filter { it.id !in queued }
        pending.clear()
        pending.addAll(requeue)
        pending.addAll(rest)
        if (!whole) return replay()
        told.addAll(runPending(0))
        changes.addAll(told)
    }

    /** `needs`: the sequences waiting on facts. A partial replica asks for none (D2). */
    public fun needs(): List<Seq> {
        if (partial != null) return emptyList()
        return inbox.entries
            .filter { (n, ib) ->
                val e = ib.entry
                e != null &&
                    ib.facts == null &&
                    (!canApply(e.fnHash) || n in diverged || isPrivate(e.fnHash))
            }
            .map { it.key }
    }

    /** §11.6 `settle`: the inbox applied, once per pump (R8). */
    public fun settle() {
        advance()
    }

    /** `take_changes`: what a view is told, and the slate wiped. */
    public fun takeChanges(): Changes {
        val out = if (rebuilt) Changes.Rebuilt else Changes.Applied(ArrayList(changes))
        rebuilt = false
        changes.clear()
        return out
    }

    /** `take_confirmed`: what the confirmed store has done since the last ask. */
    public fun takeConfirmed(): Moved {
        val run = ArrayList(journal)
        journal.clear()
        if (replaced) {
            replaced = false
            return Moved.Replaced
        }
        return Moved.Run(run)
    }

    /** `verify_at`: the cursor and the hash of the confirmed state there. */
    public fun verifyAt(): Pair<Seq, ByteArray> = cursor to Hash.stateHash(confirmed)

    // §11.6 `advance`: apply from the inbox in order for as long as the next
    // entry can be applied; then decide what the view owes.
    private fun advance() {
        val next = if (partial != null) through > cursor else inbox.containsKey(cursor + 1)
        if (!next) return
        val order = pending.map { it.id }
        val (acc, moved, others) = if (partial != null) confirmPartial() else confirmInbox()
        if (!moved) return
        if (order.isEmpty()) {
            land(acc)
            changes.addAll(acc)
        } else if (!others) {
            for (id in order.subList(0, order.size - pending.size)) recorded.remove(id)
        } else {
            rebase(order, acc, 0)
        }
    }

    // `confirm_inbox`: the advance's first half for a whole replica.
    private fun confirmInbox(): Triple<List<Change>, Boolean, Boolean> {
        val acc = ArrayList<Change>()
        var moved = false
        var others = false
        while (true) {
            val n = cursor + 1
            val ib = inbox[n] ?: break
            val e = ib.entry ?: break
            val mf = ib.facts
            val ownNext = pending.firstOrNull() == e
            val previewed = canApply(e.fnHash)
            val private = isPrivate(e.fnHash)
            val rec = recorded[e.id]
            val byRecord: Pair<FactList, Boolean>? =
                if (rec != null && ownNext && !others && previewed) {
                    val f = mf?.let(::here)
                    when {
                        f != null && private -> f to false
                        f == null && private -> null
                        f != null && f != rec -> f to true
                        else -> rec to false
                    }
                } else null
            val corrected =
                private && ownNext && byRecord != null && recorded[e.id] != byRecord.first
            val applied = byRecord ?: applyOne(n, e, mf)
            if (applied == null) {
                if (mf == null && canApply(e.fnHash) && !private && n !in diverged) diverged.add(n)
                break
            }
            val (chs, div) = applied
            confirmed.applyChanges(chs)
            journal.add(n to chs)
            cursor = n
            inbox.remove(n)
            if (ownNext) pending.removeAt(0) else pending.removeAll { it.id == e.id }
            known.remove(e.id)
            if (div) diverged.add(n)
            acc.addAll(chs)
            moved = true
            others = others || !ownNext || div || !previewed || corrected
        }
        return Triple(acc, moved, others)
    }

    // `confirm_partial`: every sequence up to `through`, by its facts (D2).
    private fun confirmPartial(): Triple<List<Change>, Boolean, Boolean> {
        val acc = ArrayList<Change>()
        var moved = false
        var others = false
        while (cursor < through) {
            val n = cursor + 1
            val ib = inbox.remove(n) ?: Inbox()
            val chs = ib.facts ?: emptyList()
            val e = ib.entry
            val ownNext = e != null && pending.firstOrNull() == e
            val agrees = ownNext && !others && canApply(e!!.fnHash) && recorded[e.id] == chs
            var dropped = false
            if (e != null) {
                val before = pending.size
                pending.removeAll { it.id == e.id }
                dropped = pending.size < before
                known.remove(e.id)
            }
            confirmed.applyChanges(chs)
            journal.add(n to chs)
            cursor = n
            moved = true
            if (!agrees && (dropped || chs.isNotEmpty())) others = true
            acc.addAll(chs)
        }
        return Triple(acc, moved, others)
    }

    // `apply_one`: one entry against the confirmed store — by intent when the
    // closure is held, by facts otherwise; both when both, compared.
    private fun applyOne(n: Seq, e: LogEntry, mf0: FactList?): Pair<FactList, Boolean>? {
        val mf = mf0?.let(::here)
        if (isPrivate(e.fnHash)) return mf?.let { it to false }
        if (canApply(e.fnHash) && n !in diverged) {
            val out = run(e, Overlay(confirmed))
            if (out is Applied.Ok) {
                return if (mf != null && mf != out.changes) mf to true else out.changes to false
            }
            return mf?.let { it to true }
        }
        return mf?.let { it to false }
    }

    // `here`: the authority's facts as this replica holds them (D1).
    private fun here(f: FactList): FactList {
        fun short(c: Change): Boolean {
            val tbl = schema.lookupTable(c.table) ?: return false
            val rows =
                when (c) {
                    is Change.Add -> listOf(c.row)
                    is Change.Remove -> listOf(c.row)
                    is Change.Edit -> listOf(c.old, c.new)
                }
            return rows.any { r ->
                r.fields.size < tbl.columns.size && r.fields.keys.all { tbl.column(it) != null }
            }
        }
        if (!behind && f.none(::short)) return f
        val out = ArrayList<Change>(f.size)
        for (c in f) {
            if (!behind && !short(c)) {
                out.add(c)
                continue
            }
            when (val p = project(schema, c)) {
                is Projected.Of -> out.add(p.change)
                is Projected.None -> {}
                is Projected.Refused -> out.add(c)
            }
        }
        return out
    }

    // `replay`: the view rebuilt whole from the confirmed store.
    private fun replay() {
        view = confirmed.copy()
        rebuilt = true
        changes.clear()
        recorded.clear()
        runPending(0)
    }

    // §R2 `rebase`, by changes: the records undone newest first, what landed
    // applied, then `pending[from..]` run again.
    private fun rebase(undo: List<Id>, landed: List<Change>, from: Int) {
        if (undo.any { !recorded.containsKey(it) }) return replay()
        val told = ArrayList<Change>()
        for (id in undo.asReversed()) {
            val rec = recorded.remove(id) ?: emptyList()
            for (c in rec.asReversed()) {
                val back = invert(c)
                view.applyChange(back)
                told.add(back)
            }
        }
        land(landed)
        told.addAll(landed)
        told.addAll(runPending(from))
        changes.addAll(told)
    }

    // `land`: what landed on the confirmed store, applied to the view.
    private fun land(landed: List<Change>) {
        view.applyChanges(landed)
    }

    // `run_pending`: `pending[from..]` run over the view in order, each
    // recorded; one now refused is dropped and its reason kept.
    private fun runPending(from: Int): List<Change> {
        val rest = ArrayList(pending.subList(from, pending.size))
        while (pending.size > from) pending.removeAt(pending.size - 1)
        val moved = ArrayList<Change>()
        for (e in rest) {
            when (val out = run(e, Overlay(view))) {
                null -> {
                    val k = known[e.id]
                    val chs = if (k != null && transitions(schema, view, k)) k else emptyList()
                    view.applyChanges(chs)
                    moved.addAll(chs)
                    recorded[e.id] = chs
                    pending.add(e)
                }
                is Applied.Ok -> {
                    view.applyChanges(out.changes)
                    moved.addAll(out.changes)
                    recorded[e.id] = out.changes
                    pending.add(e)
                }
                is Applied.Refused -> rejections.add(e.id to out.refusal)
                is Applied.Bug -> rejections.add(e.id to Refusal.Refused(bugText(out.error)))
            }
        }
        return moved
    }
}

/** What `store::project` makes of a fact under a schema. */
internal sealed class Projected {
    data class Of(val change: Change) : Projected()

    object None : Projected()

    data class Refused(val refusal: Refusal) : Projected()
}

/**
 * `store::project`: a fact laid out as `schema`'s table — dropped where the schema lacks the table,
 * refused where a row lacks a column the table requires.
 */
internal fun project(schema: Schema, c: Change): Projected {
    val tbl = schema.lookupTable(c.table) ?: return Projected.None
    fun p(r: Row): Row {
        val (row, why) = Rules.projectRow(tbl, r)
        if (why != null) throw ProjectRefused(why)
        return row!!
    }
    return try {
        Projected.Of(
            when (c) {
                is Change.Add -> Change.Add(c.table, p(c.row))
                is Change.Remove -> Change.Remove(c.table, p(c.row))
                is Change.Edit -> Change.Edit(c.table, p(c.old), p(c.new))
            })
    } catch (r: ProjectRefused) {
        Projected.Refused(r.refusal)
    }
}

private class ProjectRefused(val refusal: Refusal) : Exception(null, null, false, false)

/** The answer to a pushed intent (`peer::Sequenced`). */
public sealed class Sequenced {
    public data class Appended(val seq: Seq, val facts: FactList) : Sequenced()

    public data class Duplicate(val seq: Seq) : Sequenced()

    public data class Rejected(val refusal: Refusal) : Sequenced()
}

/**
 * The peer that sequences the log (`peer::Authority`), as a client is one: alone, over its own
 * closures, a private block never run (`private` false, `docs/plan-guards.md` D3) and a raw write
 * never authored (D4).
 */
public class Authority(public val schema: Schema, bodies: Map<String, Closure>) {
    public val bodies: MutableMap<String, Closure> = TreeMap(bodies)
    public var log: Log = Log.empty(schema)

    /** The state at the head of the log. */
    public var store: MemoryStore = MemoryStore.empty(schema)

    /** D3 This authority runs private blocks: a server's does, and a client's never. */
    public var private: Boolean = false

    /** `can_apply`. */
    public fun canApply(fh: ByteArray): Boolean = bodies.containsKey(hashKey(fh)) || Raw.isRaw(fh)

    /** D3 `is_private`. */
    public fun isPrivate(fh: ByteArray): Boolean = bodies[hashKey(fh)]?.let(Hash::isPrivate) == true

    /** §11.7 `sequence_entry`: deduped by id, applied to the head, appended with the facts. */
    public fun sequenceEntry(e: LogEntry): Sequenced = sequence(e, false)

    private fun sequence(e: LogEntry, rawOk: Boolean): Sequenced {
        log.seqOf(e.id)?.let {
            return Sequenced.Duplicate(it)
        }
        val raw = Raw.of(e.fnHash)
        if (raw != null && !rawOk)
            return Sequenced.Rejected(Refusal.Forbidden(Raw.tableOf(raw, e.args)))
        val out =
            runEntry(
                schema,
                bodies,
                e.fnHash,
                ctxOf(e).asAuthority(private),
                e.autos,
                e.args,
                Overlay(store))
        return when (out) {
            null -> Sequenced.Rejected(Refusal.Refused(unknownFunction(e.fnHash)))
            is Applied.Bug -> Sequenced.Rejected(Refusal.Refused(bugText(out.error)))
            is Applied.Refused -> Sequenced.Rejected(out.refusal)
            is Applied.Ok -> {
                val n = log.append(e, out.changes)
                store.applyChanges(out.changes)
                Sequenced.Appended(n, out.changes)
            }
        }
    }

    /**
     * §R2 `append_as`: an intent this process already ran, appended with the changes that run
     * produced, without running it again — a peer alone's authority with its replica's record.
     */
    public fun appendAs(e: LogEntry, facts: FactList): Sequenced {
        log.seqOf(e.id)?.let {
            return Sequenced.Duplicate(it)
        }
        store.applyChanges(facts)
        val n = log.append(e, facts)
        return Sequenced.Appended(n, facts)
    }
}

/**
 * §11.9 `local_commit`: a peer that is its own authority — everything pending sequenced, by the
 * replica's record where the authority's head is the replica's cursor and the intent the first
 * pending, and every answer delivered back, settled per intent.
 */
public fun localCommit(a: Authority, r: Replica) {
    for (e in ArrayList(r.pending)) {
        val rec = r.recorded[e.id]
        val recorded =
            if (rec != null && a.log.headSeq() == r.cursor && r.pending.firstOrNull() == e) rec
            else null
        val out =
            when {
                !a.canApply(e.fnHash) ->
                    Sequenced.Rejected(Refusal.Refused("no closure, and no authority to run it"))
                recorded != null -> a.appendAs(e, recorded)
                else -> a.sequenceEntry(e)
            }
        when (out) {
            is Sequenced.Appended -> {
                r.receiveFacts(out.seq, out.facts)
                r.ack(e.id, out.seq)
            }
            is Sequenced.Duplicate -> r.ack(e.id, out.seq)
            is Sequenced.Rejected -> r.reject(e.id, out.refusal)
        }
        r.settle()
    }
}
