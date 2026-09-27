// §10 The log (Ark.Log): one scope's history, entries with their facts,
// standing on a snapshot.
package dev.arkdb

import java.util.TreeMap

/** A position in one scope's log. The first entry is 1; 0 is "nothing". */
public typealias Seq = Long

/** An intent, as recorded. The sequence is the key it is stored under, never a field. */
public data class Entry(
    val id: Id,
    val actor: String,
    val session: String,
    val fn: FnHash,
    val args: Args,
    val autos: Args,
)

/** The state of a scope at a sequence, and its hash. */
public class Snapshot(public val seq: Seq, public val store: MemoryStore, public val hash: ByteArray) {
    public companion object {
        public fun of(n: Seq, st: MemoryStore): Snapshot = Snapshot(n, st, Hash.stateHash(st))
    }
}

/** What a peer at a cursor is sent next. */
public sealed class Page {
    public data class Entries(val items: List<Triple<Seq, Entry, Facts>>, val hasMore: Boolean) : Page()
    public class BelowHorizon(public val snapshot: Snapshot) : Page()
}

public class Log(schema: Schema) {
    public var base: Snapshot = Snapshot.of(0, MemoryStore(schema))
        private set

    /** Every sequence above the base, contiguous. */
    public val entries: TreeMap<Seq, Pair<Entry, Facts>> = TreeMap()

    /** Every entry id ever sequenced, kept below the horizon too. */
    public val ids: HashMap<Id, Seq> = HashMap()

    /** The last sequence assigned. */
    public val headSeq: Seq get() = if (entries.isEmpty()) base.seq else entries.lastKey()

    /** The sequence the log stands on: entries at or below it are gone. */
    public val horizon: Seq get() = base.seq

    /** §10.1 Append an entry the authority has applied, with what it changed; the sequence is head + 1. */
    public fun append(e: Entry, facts: Facts): Seq {
        val n = headSeq + 1
        entries[n] = e to facts
        ids[e.id] = n
        return n
    }

    /** The sequence an entry id was given, if it ever was. */
    public fun seqOf(i: Id): Seq? = ids[i]

    public fun entriesAfter(cursor: Seq, limit: Int): Page {
        if (cursor < horizon) return Page.BelowHorizon(base)
        val after = entries.tailMap(cursor, false).map { (s, ef) -> Triple(s, ef.first, ef.second) }
        return Page.Entries(after.take(limit), after.size > limit)
    }

    /** §10.2 The state at any retained sequence, from the snapshot and the facts alone. */
    public fun stateAt(n: Seq): MemoryStore? {
        if (n < horizon || n > headSeq) return null
        val st = base.store.fork()
        for ((s, ef) in entries) if (s <= n) for (ch in ef.second) st.applyChange(ch)
        return st
    }

    /** §10.3 Move the horizon up to a sequence; false if it is not retained. Ids are kept. */
    public fun compactTo(n: Seq): Boolean {
        val st = stateAt(n) ?: return false
        base = Snapshot.of(n, st)
        entries.headMap(n, true).clear()
        return true
    }

    /** The function hashes the retained entries name. */
    public val namedHashes: Set<FnHash> get() = entries.values.map { it.first.fn }.toSet()

    /** Whether the entries run without a gap from the snapshot to the head. */
    public val contiguous: Boolean
        get() = entries.keys.toList() == ((horizon + 1)..headSeq).toList()
}
