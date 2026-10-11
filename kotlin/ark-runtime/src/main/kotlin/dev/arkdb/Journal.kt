// A log kept on a key/value storage, as `rust/ark/src/journal.rs` keeps it:
// a snapshot, and pages of records after it (§10; `docs/plan-alone.md` §2).
//
//     snapshot  canonical CBOR of `logToValue`:
//               { t: "log",
//                 base: { seq, hash, hashing, rows: { table: [row…] }, log },
//                 entries: [ { seq, entry, facts } … ],
//                 ids: [ { id, seq } …, { key: Bytes(8), seq } … ] }
//     page      records one after another, each a 4-byte big-endian length
//               and then the canonical CBOR of { seq, entry, facts }
//
// The shapes reuse the protocol's own encoders for an entry and a change,
// so a log on a disk carries exactly what a `batch` frame would, and a
// Kotlin device writes the bytes a Rust device writes for the same log.
// A client keeps one log this way — its local history alone, under
// `Layout.alone()`: the snapshot is the fork, with no entries, and each
// write a page of its own, `log.1`, `log.2`, …, merged into fewer pages as
// they accumulate and never folded into the snapshot, because they are the
// local history a join re-queues. The server's layout is here for reading
// a server's directory, which a client never writes.
//
// What a stop leaves behind is safe by the order of the writes, and read
// back as the reference reads it: the snapshot, then page 1, 2, … until one
// is absent; records at or below the head before any that extend it are
// skipped; the first record that is short, does not decode, or does not
// carry the next sequence is where the log ends, and `Journal.open` repairs
// what it read so the next write follows on.
package dev.arkdb

/**
 * A key/value place for a log's records (`journal::Keys`). `save` replaces its record whole or not
 * at all, which is what the rules above rest on.
 */
public interface Keys {
    public fun load(key: String): ByteArray?

    public fun save(key: String, bytes: ByteArray)

    public fun remove(key: String)

    /** Add `bytes` to the end of `key`, creating it; by default the record is rewritten whole. */
    public fun append(key: String, bytes: ByteArray) {
        save(key, (load(key) ?: ByteArray(0)) + bytes)
    }

    /** Cut `key` to its first `len` bytes; by default rewritten whole. */
    public fun truncate(key: String, len: Int) {
        val all = load(key) ?: ByteArray(0)
        save(key, all.copyOf(minOf(len, all.size)))
    }
}

/** How records reach a page (`journal::Paging`). */
public enum class Paging {
    /** One page, appended to: a file. */
    Append,

    /** A page per write, each saved whole under `{page}.{n}`. */
    Pages,
}

/** What a compaction does (`journal::Compaction`). */
public enum class Compaction {
    Snapshot,
    Merge,
}

/** Where a log's records are kept, and how they move (`journal::Layout`). */
public data class Layout(
    val snapshot: String,
    val page: String,
    val paging: Paging,
    val compaction: Compaction
) {
    /** The key of the `n`th page, counting from 1. */
    public fun pageKey(n: Int): String =
        when (paging) {
            Paging.Append -> page
            Paging.Pages -> "$page.$n"
        }

    public companion object {
        /** The server's data directory: `log.ark-log` and `log.ark-journal`. */
        public fun server(): Layout =
            Layout("log.ark-log", "log.ark-journal", Paging.Append, Compaction.Snapshot)

        /** A peer alone's local history: `log`, then `log.1`, `log.2`, … */
        public fun alone(): Layout = Layout("log", "log", Paging.Pages, Compaction.Merge)
    }
}

/** Which construction of the state hash this build writes (`hash::HASH_VERSION`). */
public const val HASH_VERSION: Long = 2

/**
 * `hash::state_hash_by`: the state hash by the construction `version` names — 1, the list of every
 * table's rows, as it was until `docs/plan-db.md` D3; 2, the additive digest — or `null`.
 */
public fun stateHashBy(version: Long, st: Store): ByteArray? =
    when (version) {
        1L ->
            Sha256.hash(
                Canon.encode(
                    Value.VList(
                        st.schema.tables.map { t ->
                            Value.VList(listOf(Value.VText(t.name), Value.VList(st.scan(t.name))))
                        })))
        HASH_VERSION -> Hash.stateHash(st)
        else -> null
    }

/** A journal that cannot be read, or written (`journal.rs`'s `Err(String)`). */
public class JournalError(what: String) : Exception(what)

/** What reading one page's records over a head found (`journal::Replayed`). */
public data class Replayed(val applied: Int, val stale: Int, val end: Int, val len: Int) {
    /** Whether everything after `end` is a torn tail. */
    public val torn: Boolean
        get() = end < len
}

/** What reading a log's records found (`journal::Opened`). */
public class Opened(
    public val snapshot: Log?,
    public val head: Seq,
    public val applied: Int,
    public val stale: Int,
    /** Bytes dropped as a torn tail, and the page they were in. */
    public val torn: Pair<Int, Int>?,
)

public object LogRecords {
    /** `record_value`: one entry as the snapshot and the pages both carry it. */
    public fun recordValue(n: Seq, e: LogEntry, facts: FactList): Value =
        Value.record("seq" to Value.VInt(n), "entry" to entryValue(e), "facts" to factsValue(facts))

    /** `encode_record`: a page record's bytes — the length, then the record. */
    public fun encodeRecord(n: Seq, e: LogEntry, facts: FactList): ByteArray {
        val body = Canon.encode(recordValue(n, e, facts))
        val out = ByteArray(body.size + 4)
        val len = body.size
        out[0] = (len ushr 24).toByte()
        out[1] = (len ushr 16).toByte()
        out[2] = (len ushr 8).toByte()
        out[3] = len.toByte()
        body.copyInto(out, 4)
        return out
    }

    /** `log_to_value`: a log as a value; the snapshot is `Canon.encode` of this. */
    public fun logToValue(log: Log): Value {
        val st = log.baseStore
        val rows =
            st.schema.tables.map { it.name }.distinct().associateWith { Value.VList(st.scan(it)) }
        val entries = log.entries.map { (n, ef) -> recordValue(n, ef.first, ef.second) }
        val ids =
            log.ids.entries
                .sortedWith { a, b -> a.key.compareTo(b.key) }
                .map { (id, n) -> Value.record("id" to Value.VId(id), "seq" to Value.VInt(n)) } +
                log.below.map { (k, n) ->
                    Value.record("key" to Value.VBytes(longBytes(k)), "seq" to Value.VInt(n))
                }
        val base =
            mutableListOf<Pair<String, Value>>(
                "seq" to Value.VInt(log.baseSeq),
                "hash" to Value.VBytes(log.baseHash),
                "hashing" to Value.VInt(HASH_VERSION),
                "rows" to Value.VStruct(rows))
        log.logId?.let { base.add("log" to Value.VId(it)) }
        return Value.record(
            "t" to Value.VText("log"),
            "base" to Value.record(base),
            "entries" to Value.VList(entries),
            "ids" to Value.VList(ids))
    }

    private fun longBytes(k: Long): ByteArray = ByteArray(8) { i -> (k ushr (56 - 8 * i)).toByte() }

    private fun fields(v: Value): Map<String, Value> =
        (v as? Value.VStruct)?.fields ?: throw JournalError("expected a struct, found ${v.show()}")

    private fun need(m: Map<String, Value>, k: String): Value =
        m[k] ?: throw JournalError("missing field $k")

    private fun int(v: Value): Long =
        (v as? Value.VInt)?.value ?: throw JournalError("expected an int, found ${v.show()}")

    private fun items(v: Value): List<Value> =
        (v as? Value.VList)?.items ?: throw JournalError("expected a list, found ${v.show()}")

    /** `record_from_value`: one entry back from its value. */
    public fun recordFromValue(v: Value): Triple<Seq, LogEntry, FactList> {
        val m = fields(v)
        val n = int(need(m, "seq"))
        val e =
            try {
                entryFromValue(need(m, "entry"))
            } catch (x: FactError) {
                throw JournalError("entry $n: ${x.message}")
            }
        val facts =
            try {
                items(need(m, "facts")).map(::changeFromValue)
            } catch (x: FactError) {
                throw JournalError("facts of $n: ${x.message}")
            }
        return Triple(n, e, facts)
    }

    /** `hashing_of`: which construction a snapshot's hash is by — 1 where it says none. */
    public fun hashingOf(base: Map<String, Value>): Long =
        when (val h = base["hashing"]) {
            null -> 1
            is Value.VInt ->
                if (h.value in 1..HASH_VERSION) h.value
                else
                    throw JournalError(
                        "a state hash by a construction this build does not know: ${h.show()}")
            else ->
                throw JournalError(
                    "a state hash by a construction this build does not know: ${h.show()}")
        }

    /** `log_from_value`: a log from its value, over the module's schema. */
    public fun logFromValue(schema: Schema, v: Value): Log {
        val m = fields(v)
        if (need(m, "t") != Value.VText("log")) throw JournalError("not a log file: t = ${m["t"]}")
        val base = fields(need(m, "base"))
        val store = MemoryStore.fromValue(schema, need(base, "rows"))
        val logId =
            when (val l = base["log"]) {
                null -> null
                is Value.VId -> l.value
                else -> throw JournalError("the log's identity is not an id: ${l.show()}")
            }
        val claimed = stateHashBy(hashingOf(base), store)
        val hash = (need(base, "hash") as? Value.VBytes)?.value
        if (claimed == null || hash == null || !hash.contentEquals(claimed))
            throw JournalError("the snapshot's hash does not match its rows")
        val log = Log(int(need(base, "seq")), store, Hash.stateHash(store), logId)
        for (item in items(need(m, "entries"))) {
            val (n, e, facts) = recordFromValue(item)
            log.entries[n] = e to facts
        }
        val keys = ArrayList<Pair<Long, Seq>>()
        for (item in items(need(m, "ids"))) {
            val im = fields(item)
            val n = int(need(im, "seq"))
            val id = im["id"]
            val key = im["key"]
            when {
                id is Value.VId -> log.ids[id.value] = n
                id == null && key is Value.VBytes && key.value.size == 8 -> {
                    var k = 0L
                    for (b in key.value) k = (k shl 8) or (b.toLong() and 0xff)
                    keys.add(k to n)
                }
                else -> throw JournalError("expected an id or an 8-byte key, found $id $key")
            }
        }
        log.extendBelow(keys)
        if (!log.contiguous())
            throw JournalError(
                "the entries do not run without a gap from ${log.horizon() + 1} to ${log.headSeq()}")
        return log
    }

    /** `decode_snapshot`: the snapshot's bytes back, as a log. */
    public fun decodeSnapshot(schema: Schema, bytes: ByteArray): Log =
        logFromValue(
            schema,
            try {
                Canon.decode(bytes)
            } catch (e: Canon.DecodeError) {
                throw JournalError("decoding: ${e.why}")
            })

    /**
     * `replay_page`: a page's records handed to `each`, in order, from the one after `head`,
     * stopping at the first that is short, does not decode, or does not carry the next sequence.
     */
    public fun replayPage(
        head0: Seq,
        bytes: ByteArray,
        each: (Seq, LogEntry, FactList) -> Unit
    ): Pair<Seq, Replayed> {
        var head = head0
        var applied = 0
        var stale = 0
        var at = 0
        while (at < bytes.size) {
            if (at + 4 > bytes.size) break
            val n =
                ((bytes[at].toInt() and 0xff) shl 24) or
                    ((bytes[at + 1].toInt() and 0xff) shl 16) or
                    ((bytes[at + 2].toInt() and 0xff) shl 8) or
                    (bytes[at + 3].toInt() and 0xff)
            if (n < 0 || at + 4 + n > bytes.size) break
            val rec =
                try {
                    recordFromValue(Canon.decode(bytes.copyOfRange(at + 4, at + 4 + n)))
                } catch (_: Canon.DecodeError) {
                    break
                } catch (_: JournalError) {
                    break
                }
            val (seq, e, facts) = rec
            if (seq == head + 1) {
                each(seq, e, facts)
                head = seq
                applied += 1
            } else if (seq <= head && applied == 0) {
                stale += 1
            } else break
            at += 4 + n
        }
        return head to Replayed(applied, stale, at, bytes.size)
    }

    /** Every page as read: the good length of each that extended the log, and what followed. */
    internal class Walk {
        val good = ArrayList<Int>()
        var after = 0
        var tornAt: Triple<Int, Int, Int>? = null
        var stale = 0
        var applied = 0
        var head: Seq = 0
    }

    internal fun readSnapshot(keys: Keys, layout: Layout, schema: Schema): Pair<Log, Int>? {
        val bytes = keys.load(layout.snapshot) ?: return null
        val log =
            try {
                decodeSnapshot(schema, bytes)
            } catch (e: JournalError) {
                throw JournalError("reading ${layout.snapshot}: ${e.message}")
            }
        return log to bytes.size
    }

    internal fun walk(
        keys: Keys,
        layout: Layout,
        head0: Seq,
        each: (Seq, LogEntry, FactList) -> Unit
    ): Walk {
        val w = Walk()
        var head = head0
        val read = ArrayList<Pair<Int, Boolean>>()
        var ended = false
        var n = 1
        while (true) {
            val bytes = keys.load(layout.pageKey(n)) ?: break
            if (ended) {
                w.after += 1
            } else {
                val (h, r) = replayPage(head, bytes, each)
                head = h
                w.applied += r.applied
                w.stale += r.stale
                if (r.torn) {
                    w.tornAt = Triple(n, r.end, r.len)
                    w.after += 1
                    ended = true
                } else {
                    read.add(r.len to (r.applied == 0 && r.stale > 0))
                }
            }
            if (layout.paging == Paging.Append) break
            n += 1
        }
        while (read.isNotEmpty() && read.last().second && w.tornAt == null) {
            read.removeAt(read.size - 1)
            w.after += 1
        }
        w.good.addAll(read.map { it.first })
        w.head = head
        return w
    }

    /** `journal::read`: the snapshot, then every record the pages add, handed to `each`. */
    public fun read(
        keys: Keys,
        layout: Layout,
        schema: Schema,
        each: (Seq, LogEntry, FactList) -> Unit
    ): Opened {
        val snapshot = readSnapshot(keys, layout, schema)
        val head = snapshot?.first?.headSeq() ?: 0
        val w = walk(keys, layout, head, each)
        return Opened(
            snapshot?.first,
            w.head,
            w.applied,
            w.stale,
            w.tornAt?.let { (n, end, len) -> n to (len - end) })
    }

    /** `journal::load`: a log back whole, the pages appended; `null` where there is neither. */
    public fun load(keys: Keys, layout: Layout, schema: Schema): Log? {
        val got = ArrayList<Pair<LogEntry, FactList>>()
        val o = read(keys, layout, schema) { _, e, f -> got.add(e to f) }
        if (o.snapshot == null && got.isEmpty()) return null
        val log = o.snapshot ?: Log.empty(schema)
        for ((e, f) in got) log.append(e, f)
        return log
    }
}

/**
 * A log open for writing (`journal::Journal`): what the storage holds of it, so that a write adds
 * what moved since and nothing else.
 */
public class Journal
private constructor(
    public val layout: Layout,
    private val pages: MutableList<Int>,
    private var snapshotBytes: Int,
    private var head: Seq,
    private var horizon: Seq,
    private var named: Id?,
    private var due: Boolean,
) {
    /** Every byte written, snapshots and pages both. */
    public var written: Long = 0

    /** The snapshot's size and the pages', as last written. */
    public fun sizes(): Pair<Int, Int> = snapshotBytes to pages.sum()

    /** How many pages are on the storage. */
    public fun pageCount(): Int = pages.size

    /** The head as last written. */
    public fun head(): Seq = head

    /**
     * `append`: records above the head, already encoded (`LogRecords.encodeRecord`), reaching `to`:
     * appended to the one page, or saved as a page of their own.
     */
    public fun append(keys: Keys, records: ByteArray, to: Seq) {
        if (records.isEmpty()) return
        when (layout.paging) {
            Paging.Append -> {
                try {
                    keys.append(layout.pageKey(1), records)
                } catch (e: Exception) {
                    due = true
                    throw e
                }
                if (pages.isEmpty()) pages.add(records.size) else pages[0] += records.size
            }
            Paging.Pages -> {
                keys.save(layout.pageKey(pages.size + 1), records)
                pages.add(records.size)
            }
        }
        written += records.size
        head = to
    }

    /**
     * `merge`: the newest page merged into the one before it for as long as that one is no larger —
     * the merged page saved whole first, then the newer removed. Nothing under
     * `Compaction.Snapshot`.
     */
    public fun merge(keys: Keys) {
        if (layout.compaction != Compaction.Merge) return
        while (pages.size >= 2) {
            val older = pages[pages.size - 2]
            val newer = pages[pages.size - 1]
            if (older > newer) break
            val k = pages.size
            val a = layout.pageKey(k - 1)
            val b = layout.pageKey(k)
            val both =
                (keys.load(a) ?: throw JournalError("$a is gone")) +
                    (keys.load(b) ?: throw JournalError("$b is gone"))
            keys.save(a, both)
            written += both.size
            keys.remove(b)
            pages.removeAt(pages.size - 1)
            pages[pages.size - 1] = both.size
        }
    }

    /**
     * `snapshot`: the log whole as the snapshot, then the pages emptied — so a stop between leaves
     * records the snapshot already holds, which a reader skips.
     */
    public fun snapshot(keys: Keys, log: Log) {
        val bytes = Canon.encode(LogRecords.logToValue(log))
        keys.save(layout.snapshot, bytes)
        written += bytes.size
        snapshotBytes = bytes.size
        head = log.headSeq()
        horizon = log.horizon()
        named = log.logId
        when (layout.paging) {
            Paging.Append ->
                if (pages.isNotEmpty()) {
                    keys.truncate(layout.pageKey(1), 0)
                    pages.clear()
                    pages.add(0)
                }
            Paging.Pages ->
                while (pages.isNotEmpty()) {
                    keys.remove(layout.pageKey(pages.size))
                    pages.removeAt(pages.size - 1)
                }
        }
        due = false
    }

    public companion object {
        /** `Journal::fresh`: nothing on the storage yet, whose first write is its snapshot. */
        public fun fresh(layout: Layout): Journal =
            Journal(layout, ArrayList(), 0, 0, 0, null, false)

        /**
         * `Journal::open`: read as `LogRecords.read` reads it, then repaired so that what is
         * written next follows on — a torn page cut back to its whole records, every page after the
         * good ones removed, newest first.
         */
        public fun open(
            keys: Keys,
            layout: Layout,
            schema: Schema,
            each: (Seq, LogEntry, FactList) -> Unit
        ): Pair<Journal, Opened> {
            val snapshot = LogRecords.readSnapshot(keys, layout, schema)
            val snapshotBytes = snapshot?.second ?: 0
            val baseHead = snapshot?.first?.headSeq() ?: 0
            val w = LogRecords.walk(keys, layout, baseHead, each)
            val good = w.good
            val after = w.after
            val tornAt = w.tornAt
            val head = w.head
            val applied = w.applied
            val stale = w.stale
            val pages = ArrayList(good)
            when (layout.paging) {
                Paging.Append -> {
                    if (tornAt != null) {
                        keys.truncate(layout.pageKey(1), tornAt.second)
                        pages.clear()
                        pages.add(tornAt.second)
                    } else if (after > 0) {
                        pages.clear()
                        pages.add(keys.load(layout.pageKey(1))?.size ?: 0)
                    }
                }
                Paging.Pages -> {
                    val firstAfter = good.size + 1
                    val last = good.size + after
                    for (n in last downTo firstAfter) {
                        val keep = tornAt?.takeIf { it.first == n && it.second > 0 }
                        if (keep != null) {
                            keys.truncate(layout.pageKey(n), keep.second)
                            pages.add(keep.second)
                        } else keys.remove(layout.pageKey(n))
                    }
                }
            }
            val j =
                Journal(
                    layout,
                    pages,
                    snapshotBytes,
                    head,
                    snapshot?.first?.horizon() ?: 0,
                    snapshot?.first?.logId,
                    false)
            val opened =
                Opened(
                    snapshot?.first,
                    head,
                    applied,
                    stale,
                    tornAt?.let { (n, end, len) -> n to (len - end) })
            return j to opened
        }

        /**
         * `Journal::create`: a log started on `keys` at `base` — a snapshot with nothing above it —
         * clearing any pages a log that was there left, newest first, then the snapshot saved.
         */
        public fun create(keys: Keys, layout: Layout, base: Log): Journal {
            clearPages(keys, layout)
            val j = fresh(layout)
            j.snapshot(keys, base)
            return j
        }

        /** `Journal::destroy`: the log removed — its pages newest first, then its snapshot. */
        public fun destroy(keys: Keys, layout: Layout) {
            clearPages(keys, layout)
            keys.remove(layout.snapshot)
        }

        private fun clearPages(keys: Keys, layout: Layout) {
            var n = 0
            while (keys.load(layout.pageKey(n + 1)) != null) {
                n += 1
                if (layout.paging == Paging.Append) break
            }
            for (k in n downTo 1) keys.remove(layout.pageKey(k))
        }
    }
}
