// §10 The log, as `rust/ark/src/log.rs` defines it: the module's history,
// an append-only sequence of entries, each an intent with what applying it
// changed kept beside it, standing on a snapshot.
//
// An entry is an intent: who authored it, the function by hash, the
// arguments, the autos. The facts beside it are derived, never
// authoritative, and are what a peer takes for an entry it cannot replay.
// A client holds a log only when it is its own authority (alone,
// `docs/plan-alone.md`), and reads one back from its own storage
// (`Journal.kt`); compaction and the ids folded below a horizon are a
// server's, and are here only as far as reading a log written by one needs.
package dev.arkdb

import java.util.SortedSet
import java.util.TreeMap
import java.util.TreeSet

/** A position in the log (`log::Seq`). The first entry is 1; 0 is "nothing". */
public typealias Seq = Long

/** What applying an entry changed, in order (`log::Facts`). */
public typealias FactList = List<Change>

/**
 * An intent, as recorded (`log::Entry`). The sequence is not a field: it is the key an entry is
 * stored under once the authority assigned it. `roles` are the roles its author held, frozen with
 * it (`docs/plan-guards.md` D1), in code point order.
 */
public class LogEntry(
    public val id: Id,
    public val actor: String,
    public val session: String,
    roles: Collection<String>,
    fnHash: ByteArray,
    args: Map<String, Value>,
    autos: Map<String, Value>,
) {
    public val roles: SortedSet<String> = TreeSet(CodePointOrder).apply { addAll(roles) }
    public val fnHash: ByteArray = fnHash.copyOf()
    public val args: Map<String, Value> = sortedFields(args)
    public val autos: Map<String, Value> = sortedFields(autos)

    /** The same intent, authored as `actor` under `session`. */
    public fun by(actor: String, session: String): LogEntry =
        LogEntry(id, actor, session, roles, fnHash, args, autos)

    override fun equals(other: Any?): Boolean =
        other is LogEntry &&
            id == other.id &&
            actor == other.actor &&
            session == other.session &&
            roles == other.roles &&
            fnHash.contentEquals(other.fnHash) &&
            args == other.args &&
            autos == other.autos

    override fun hashCode(): Int = id.hashCode()

    override fun toString(): String =
        "LogEntry(${id.text} by $actor/$session ${Hex.encode(fnHash)} ${Value.VStruct(args).show()})"
}

/**
 * A log (`log::Log`): the snapshot it stands on — its sequence, its store, its hash and the log's
 * identity — every entry above it with its facts, and every id it sequenced: exactly above the
 * horizon, and below it by an 8-byte key (`below`, `log::Below`), as a server keeps them.
 */
public class Log(
    public var baseSeq: Seq,
    public var baseStore: MemoryStore,
    public var baseHash: ByteArray,
    public var logId: Id?,
) {
    public val entries: TreeMap<Seq, Pair<LogEntry, FactList>> = TreeMap()
    public val ids: HashMap<Id, Seq> = HashMap()

    /** `(key, seq)` of the ids below the horizon, in unsigned key order (`log::Below`). */
    public val below: MutableList<Pair<Long, Seq>> = ArrayList()

    /** `head_seq`: the last sequence assigned. */
    public fun headSeq(): Seq = if (entries.isEmpty()) baseSeq else entries.lastKey()

    /** `horizon`: the sequence the log stands on. */
    public fun horizon(): Seq = baseSeq

    /** §10.1 `append`: an entry the authority applied, at `head + 1`. */
    public fun append(e: LogEntry, facts: FactList): Seq {
        val n = headSeq() + 1
        ids[e.id] = n
        entries[n] = e to facts
        return n
    }

    /** `seq_of`: the sequence an id was given, exactly above the horizon and by key below. */
    public fun seqOf(id: Id): Seq? = ids[id] ?: belowSeq(belowKey(id))

    private fun belowSeq(key: Long): Seq? =
        below.filter { it.first == key }.minOfOrNull { it.second }

    /** `Below::extend`: keys added, kept in unsigned key order. */
    public fun extendBelow(more: List<Pair<Long, Seq>>) {
        below.addAll(more)
        below.sortWith { a, b ->
            val k = java.lang.Long.compareUnsigned(a.first, b.first)
            if (k != 0) k else a.second.compareTo(b.second)
        }
    }

    /** `contiguous`: the entries run without a gap from the sequence after the snapshot. */
    public fun contiguous(): Boolean {
        var n = baseSeq
        for (k in entries.keys) {
            if (k != n + 1) return false
            n = k
        }
        return true
    }

    /** §10.2 `state_at`: the state at a retained sequence, from the snapshot and the facts. */
    public fun stateAt(n: Seq): MemoryStore? {
        if (n < horizon() || n > headSeq()) return null
        val st = baseStore.copy()
        for ((_, ef) in entries.headMap(n, true)) for (c in ef.second) st.applyChange(
            widened(st.schema, c))
        return st
    }

    /**
     * `take_entries`: every entry taken out, oldest first, and the log left standing at its head
     * with the ids kept — what a peer alone's authority does after each append, its entries kept in
     * its journal and not in memory (`docs/plan-alone.md` §2).
     */
    public fun takeEntries(): List<Triple<Seq, LogEntry, FactList>> {
        baseSeq = headSeq()
        val out = entries.map { (n, ef) -> Triple(n, ef.first, ef.second) }
        entries.clear()
        return out
    }

    public companion object {
        /** `Log::empty`: nothing in it, standing on the empty store at 0. */
        public fun empty(sch: Schema): Log {
            val st = MemoryStore.empty(sch)
            return Log(0, st, Hash.stateHash(st), null)
        }

        /** `Below::key`: the first 8 bytes of the id's SHA-256, big-endian. */
        public fun belowKey(id: Id): Long {
            val h = Sha256.hash(id.bytes)
            var k = 0L
            for (i in 0 until 8) k = (k shl 8) or (h[i].toLong() and 0xff)
            return k
        }

        /**
         * `widened`: a fact as a state of this schema holds it — a row that names nothing the table
         * lacks but leaves out a nullable column laid out with it `Null`; any other change as it is
         * (`docs/plan-db.md` D1).
         */
        public fun widened(sch: Schema, c: Change): Change {
            val tbl = sch.lookupTable(c.table) ?: return c
            fun short(r: Row): Boolean =
                r.fields.size < tbl.columns.size && r.fields.keys.all { tbl.column(it) != null }
            fun fill(r: Row): Row = if (short(r)) Rules.projectRow(tbl, r).first ?: r else r
            return when (c) {
                is Change.Add -> if (short(c.row)) Change.Add(c.table, fill(c.row)) else c
                is Change.Remove -> if (short(c.row)) Change.Remove(c.table, fill(c.row)) else c
                is Change.Edit ->
                    if (short(c.old) || short(c.new)) Change.Edit(c.table, fill(c.old), fill(c.new))
                    else c
            }
        }
    }
}
