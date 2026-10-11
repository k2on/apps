// Where a peer's replica is kept between runs, as
// `rust/ark-client/src/storage.rs` keeps it — record for record and byte for
// byte, so that a Kotlin device and a Rust device write the same bytes for
// the same state, and either opens what the other wrote.
//
// What is durable about the log is the confirmed store at its cursor and the
// intents still pending — exactly what `Replica.open` takes, and nothing
// optimistic — as canonical-CBOR records, each written whole or not at all:
//
//     replica   { t: "replica", cursor, confirmed: { table: [row…] },
//                 user, session, log, fork: { log, cursor },     the snapshot
//                 hashing, partial? }
//     facts.1   { t: "facts", from, to, facts: [[change…], …] }  the journal:
//     facts.2   …                                                a page per write
//     pending   { t: "pending", gen, entries: [entry…] }         the intents'
//     pending.1 { t: "pending-ops", gen, add: [entry…],          snapshot, and
//                 drop: [id…] }                                  their pages
//     who       { t: "who", user, session }
//     log       the local history of a peer alone (`dev.arkdb.Journal`,
//     log.1 …   `Layout.alone()`): the fork, then pages of what it sequenced
//
// The reference's module docs say why each record is shaped as it is and
// why a stop anywhere leaves something `load` reads correctly; `Peer` here
// writes them in the reference's order and by its rules.
package dev.arkdb.client

import dev.arkdb.Canon
import dev.arkdb.Change
import dev.arkdb.FactError
import dev.arkdb.FactList
import dev.arkdb.HASH_VERSION
import dev.arkdb.Id
import dev.arkdb.Keys
import dev.arkdb.Layout
import dev.arkdb.LogEntry
import dev.arkdb.MemoryStore
import dev.arkdb.Schema
import dev.arkdb.Seq
import dev.arkdb.Value
import dev.arkdb.changeFromValue
import dev.arkdb.deviceSchema
import dev.arkdb.entryFromValue
import dev.arkdb.entryValue
import dev.arkdb.factsValue
import java.io.File
import java.io.FileOutputStream
import java.util.TreeMap

/** A record that does not read as one (`ark_client::Error::Corrupt`), or a storage that failed. */
public class StorageError(what: String) : Exception(what)

/**
 * A key-value place for a peer's files (`storage::Storage`). `key` is a short file name such as
 * `replica`. A `save` replaces its record whole or not at all. There is no listing: pages are found
 * by trying `facts.1`, `facts.2`, … in turn. Every storage is also where a log's records can be
 * kept (`Keys`): the alone log is written through the same `save` and `remove` as the replica.
 */
public interface Storage : Keys

/** In memory (`storage::Memory`): for tests, and for "reopening" from what a peer left behind. */
public class MemoryStorage : Storage {
    private val map = TreeMap<String, ByteArray>()

    @Synchronized override fun load(key: String): ByteArray? = map[key]?.copyOf()

    @Synchronized
    override fun save(key: String, bytes: ByteArray) {
        map[key] = bytes.copyOf()
    }

    @Synchronized
    override fun remove(key: String) {
        map.remove(key)
    }

    /** Every key held, for a test to look at. */
    @Synchronized public fun keys(): List<String> = map.keys.toList()

    /** A second storage holding what this one holds now. */
    @Synchronized
    public fun copy(): MemoryStorage =
        MemoryStorage().also { c -> map.forEach { (k, v) -> c.map[k] = v.copyOf() } }
}

/**
 * A directory, one file per key (`storage::Dir`), each written whole to a temporary name beside it,
 * synced, and renamed over the old one: a crash leaves the old record or the new one, never part of
 * either. The reference also syncs the directory after the rename; `java.io` has no way to open
 * one, so here the rename's durability is the file system's.
 */
public class DirStorage(public val dir: File) : Storage {
    override fun load(key: String): ByteArray? {
        val f = File(dir, key)
        return if (f.isFile) f.readBytes() else null
    }

    override fun save(key: String, bytes: ByteArray) {
        if (!dir.isDirectory && !dir.mkdirs()) throw StorageError("cannot make $dir")
        val tmp = File(dir, "$key.tmp")
        FileOutputStream(tmp).use {
            it.write(bytes)
            it.fd.sync()
        }
        val to = File(dir, key)
        if (!tmp.renameTo(to)) {
            to.delete()
            if (!tmp.renameTo(to)) throw StorageError("cannot rename $tmp to $to")
        }
    }

    override fun remove(key: String) {
        val f = File(dir, key)
        if (f.exists() && !f.delete()) throw StorageError("cannot remove $f")
    }
}

/**
 * Where a replica last shared a log with a server (`storage::Fork`, `docs/plan-alone.md` §1): that
 * log and the cursor it was at — `null` and 0 for a peer that never had a server.
 */
public data class Fork(val logId: Id? = null, val cursor: Seq = 0) {
    internal fun value(): Value {
        val f = mutableListOf<Pair<String, Value>>("cursor" to Value.VInt(cursor))
        if (logId != null) f.add("log" to Value.VId(logId))
        return Value.record(f)
    }
}

/**
 * What is durable about the log, as it is read back (`storage::ReplicaFile`): the snapshot with
 * every page after it applied, and the intents not yet answered.
 */
public class ReplicaFile(
    public val fork: Fork,
    public var cursor: Seq,
    public val confirmed: MemoryStore,
    public var pending: List<LogEntry>,
    public var user: String,
    public var session: String,
    public val partial: Map<String, List<String>>?,
) {
    public companion object {
        public const val KEY: String = "replica"
        public const val PENDING: String = "pending"
        public const val WHO: String = "who"

        /** The key of the journal's `n`th page, counting from 1. */
        public fun pageKey(n: Int): String = "facts.$n"

        /** The key of the pending intents' `n`th page, counting from 1. */
        public fun pendingPageKey(n: Int): String = "pending.$n"
    }
}

/** One page of the journal (`storage::JournalPage`): the changes at each sequence `from`..`to`. */
public class JournalPage(
    public val from: Seq,
    public val to: Seq,
    public val facts: List<FactList>
)

/** One page of the pending intents (`storage::PendingPage`). */
public class PendingPage(
    public val gen: Long,
    public val add: List<LogEntry>,
    public val drop: List<Id>
)

/** What the pending intents' records looked like when read (`storage::PendingStored`). */
public data class PendingStored(
    val gen: Long = 0,
    val snapshotBytes: Int = 0,
    val pages: Int = 0,
    val pageBytes: Int = 0,
    val found: Int = 0,
    val clean: Boolean = false,
)

/** What `load` found in a storage (`storage::Stored`): the replica, and what the journal was. */
public class Stored(
    public val file: ReplicaFile,
    public val logId: Id?,
    public val snapshotCursor: Seq,
    public val snapshotBytes: Int,
    public val pages: Int,
    public val pageBytes: Int,
    public val found: Int,
    public val clean: Boolean,
    public val pending: PendingStored,
)

/** The records, encoded and decoded as `storage.rs` encodes and decodes them. */
public object Records {
    private fun corrupt(what: String): Nothing = throw StorageError(what)

    /** `encode_replica_held`: the `replica` record's bytes. */
    public fun encodeReplica(
        cursor: Seq,
        logId: Id?,
        fork: Fork,
        partial: Map<String, List<String>>?,
        confirmed: MemoryStore,
        user: String,
        session: String,
    ): ByteArray {
        val f =
            mutableListOf<Pair<String, Value>>(
                "t" to Value.VText("replica"),
                "cursor" to Value.VInt(cursor),
                "fork" to fork.value(),
                "confirmed" to confirmed.storeValue(),
                "user" to Value.VText(user),
                "session" to Value.VText(session),
                "hashing" to Value.VInt(HASH_VERSION))
        if (logId != null) f.add("log" to Value.VId(logId))
        if (partial != null)
            f.add(
                "partial" to
                    Value.VStruct(
                        partial.mapValues { (_, cs) -> Value.VList(cs.map { Value.VText(it) }) }))
        return Canon.encode(Value.record(f))
    }

    /** `decode_replica`: the `replica` record, and the log it names. */
    public fun decodeReplica(bytes: ByteArray, schema: Schema): Pair<ReplicaFile, Id?> {
        fun bad(w: String): Nothing = corrupt("a replica file: $w")
        val v =
            try {
                Canon.decode(bytes)
            } catch (e: Canon.DecodeError) {
                bad(e.why.name)
            }
        val m = (v as? Value.VStruct)?.fields ?: bad("not a struct")
        if (m["t"] != Value.VText("replica")) bad("not a replica")
        val cursor = (m["cursor"] as? Value.VInt)?.value ?: bad("no cursor")
        val rows = (m["confirmed"] as? Value.VStruct)?.fields ?: bad("no confirmed store")
        for ((t, rs) in rows) {
            val list = (rs as? Value.VList)?.items ?: bad("rows of $t")
            if (list.any { it !is Value.VStruct }) bad("a row of $t")
        }
        val confirmed0 = MemoryStore.fromValue(schema, m["confirmed"]!!)
        val pending =
            when (val p = m["pending"]) {
                null -> emptyList()
                is Value.VList ->
                    try {
                        p.items.map(::entryFromValue)
                    } catch (e: FactError) {
                        bad(e.message ?: "an entry")
                    }
                else -> bad("pending is not a list")
            }
        fun optional(k: String): String =
            when (val x = m[k]) {
                null -> ""
                is Value.VText -> x.value
                else -> bad("$k is not text")
            }
        val logId =
            when (val l = m["log"]) {
                null -> null
                is Value.VId -> l.value
                else -> bad("log is not an id")
            }
        when (val h = m["hashing"]) {
            null -> {}
            is Value.VInt ->
                if (h.value !in 1..HASH_VERSION)
                    bad("a state hash by a construction this build does not know: ${h.show()}")
            else -> bad("a state hash by a construction this build does not know: ${h.show()}")
        }
        val fork =
            when (val f = m["fork"]) {
                null -> Fork()
                is Value.VStruct ->
                    Fork(
                        when (val l = f.fields["log"]) {
                            null -> null
                            is Value.VId -> l.value
                            else -> bad("a fork's log is not an id")
                        },
                        (f.fields["cursor"] as? Value.VInt)?.value ?: bad("a fork with no cursor"))
                else -> bad("fork is not a struct")
            }
        val partial =
            when (val p = m["partial"]) {
                null -> null
                is Value.VStruct ->
                    TreeMap<String, List<String>>().apply {
                        for ((t, cs) in p.fields) {
                            val l =
                                (cs as? Value.VList)?.items
                                    ?: bad("partial: a table's columns are not a list")
                            put(
                                t,
                                l.map {
                                    (it as? Value.VText)?.value
                                        ?: bad("partial: a column is not text")
                                })
                        }
                    }
                else -> bad("partial is not a struct")
            }
        val confirmed =
            if (partial == null) confirmed0
            else {
                val device = deviceSchema(schema, partial)
                val st = MemoryStore.empty(device)
                for (t in device.tables) for (r in confirmed0.scan(t.name)) st.applyChange(
                    Change.Add(t.name, r))
                st
            }
        return ReplicaFile(
            fork, cursor, confirmed, pending, optional("user"), optional("session"), partial) to
            logId
    }

    /** `encode_page`: a journal page's bytes, for a contiguous run of sequences. */
    public fun encodePage(run: List<Pair<Seq, FactList>>): ByteArray {
        val from = run.firstOrNull()?.first ?: 0
        val to = run.lastOrNull()?.first ?: 0
        return Canon.encode(
            Value.record(
                "t" to Value.VText("facts"),
                "from" to Value.VInt(from),
                "to" to Value.VInt(to),
                "facts" to Value.VList(run.map { factsValue(it.second) })))
    }

    /** `decode_page`: a page back, whole — a run that does not add up is corrupt. */
    public fun decodePage(bytes: ByteArray): JournalPage {
        fun bad(w: String): Nothing = corrupt("a journal page: $w")
        val v =
            try {
                Canon.decode(bytes)
            } catch (e: Canon.DecodeError) {
                bad(e.why.name)
            }
        val m = (v as? Value.VStruct)?.fields ?: bad("not a struct")
        if (m["t"] != Value.VText("facts")) bad("not a page")
        val from = (m["from"] as? Value.VInt)?.value ?: bad("no from")
        val to = (m["to"] as? Value.VInt)?.value ?: bad("no to")
        val runs = (m["facts"] as? Value.VList)?.items ?: bad("no facts")
        val facts =
            runs.map { f ->
                val cs = (f as? Value.VList)?.items ?: bad("facts are not lists")
                try {
                    cs.map(::changeFromValue)
                } catch (e: FactError) {
                    bad(e.message ?: "a change")
                }
            }
        if (from < 1 || to < from || facts.size.toLong() != to - from + 1)
            bad("${facts.size} lists of facts for $from..$to")
        return JournalPage(from, to, facts)
    }

    /** `encode_who`. */
    public fun encodeWho(user: String, session: String): ByteArray =
        Canon.encode(
            Value.record(
                "t" to Value.VText("who"),
                "user" to Value.VText(user),
                "session" to Value.VText(session)))

    /** `decode_who`: user, session. */
    public fun decodeWho(bytes: ByteArray): Pair<String, String> {
        fun bad(w: String): Nothing = corrupt("a who file: $w")
        val v =
            try {
                Canon.decode(bytes)
            } catch (e: Canon.DecodeError) {
                bad(e.why.name)
            }
        val m = (v as? Value.VStruct)?.fields ?: bad("not a struct")
        if (m["t"] != Value.VText("who")) bad("not a login")
        return ((m["user"] as? Value.VText)?.value ?: bad("no user")) to
            ((m["session"] as? Value.VText)?.value ?: bad("no session"))
    }

    /** `encode_pending`: the `pending` record as written before pages had it — generation 0. */
    public fun encodePending(pending: List<LogEntry>): ByteArray =
        Canon.encode(
            Value.record(
                "t" to Value.VText("pending"), "entries" to Value.VList(pending.map(::entryValue))))

    /** `encode_pending_snapshot`: a snapshot of generation `gen`. */
    public fun encodePendingSnapshot(gen: Long, pending: List<LogEntry>): ByteArray =
        Canon.encode(
            Value.record(
                "t" to Value.VText("pending"),
                "gen" to Value.VInt(gen),
                "entries" to Value.VList(pending.map(::entryValue))))

    /** `decode_pending_snapshot`: its generation (0 where it has none) and its intents. */
    public fun decodePendingSnapshot(bytes: ByteArray): Pair<Long, List<LogEntry>> {
        fun bad(w: String): Nothing = corrupt("a pending file: $w")
        val v =
            try {
                Canon.decode(bytes)
            } catch (e: Canon.DecodeError) {
                bad(e.why.name)
            }
        val m = (v as? Value.VStruct)?.fields ?: bad("not a struct")
        if (m["t"] != Value.VText("pending")) bad("not pending intents")
        val gen =
            when (val g = m["gen"]) {
                null -> 0L
                is Value.VInt -> g.value
                else -> bad("gen is not an int")
            }
        val ps = (m["entries"] as? Value.VList)?.items ?: bad("no entries")
        return gen to
            try {
                ps.map(::entryFromValue)
            } catch (e: FactError) {
                bad(e.message ?: "an entry")
            }
    }

    /** `encode_pending_page`. */
    public fun encodePendingPage(gen: Long, add: List<LogEntry>, drop: List<Id>): ByteArray =
        Canon.encode(
            Value.record(
                "t" to Value.VText("pending-ops"),
                "gen" to Value.VInt(gen),
                "add" to Value.VList(add.map(::entryValue)),
                "drop" to Value.VList(drop.map { Value.VId(it) })))

    /** `decode_pending_page`. */
    public fun decodePendingPage(bytes: ByteArray): PendingPage {
        fun bad(w: String): Nothing = corrupt("a pending page: $w")
        val v =
            try {
                Canon.decode(bytes)
            } catch (e: Canon.DecodeError) {
                bad(e.why.name)
            }
        val m = (v as? Value.VStruct)?.fields ?: bad("not a struct")
        if (m["t"] != Value.VText("pending-ops")) bad("not a pending page")
        val gen = (m["gen"] as? Value.VInt)?.value ?: bad("no gen")
        val add = (m["add"] as? Value.VList)?.items ?: bad("no add or drop")
        val drop = (m["drop"] as? Value.VList)?.items ?: bad("no add or drop")
        return PendingPage(
            gen,
            try {
                add.map(::entryFromValue)
            } catch (e: FactError) {
                bad(e.message ?: "an entry")
            },
            drop.map { (it as? Value.VId)?.value ?: bad("a drop is not an id") })
    }

    /**
     * `load_pending`: the snapshot, then its pages in order — skipping pages of an older
     * generation, stopping at the first that does not decode or adds an intent already held; `null`
     * for the intents where there is no `pending` record.
     */
    public fun loadPending(storage: Storage): Pair<List<LogEntry>?, PendingStored> {
        var st = PendingStored()
        val snap = storage.load(ReplicaFile.PENDING)
        var entries: MutableList<LogEntry>? = null
        var gen = 0L
        if (snap != null) {
            val (g, es) = decodePendingSnapshot(snap)
            gen = g
            entries = ArrayList(es)
            st = st.copy(snapshotBytes = snap.size)
        }
        st = st.copy(gen = gen, clean = entries != null)
        var torn = entries == null
        val held = HashSet<Id>()
        entries?.forEach { held.add(it.id) }
        while (true) {
            val bytes = storage.load(ReplicaFile.pendingPageKey(st.found + 1)) ?: break
            st = st.copy(found = st.found + 1)
            if (torn) continue
            val list = entries!!
            val p =
                try {
                    decodePendingPage(bytes)
                } catch (_: StorageError) {
                    null
                }
            when {
                p != null && p.gen < gen -> st = st.copy(clean = false)
                p != null && p.gen == gen && p.add.all { it.id !in held || it.id in p.drop } -> {
                    val gone = p.drop.toSet()
                    if (gone.isNotEmpty()) {
                        list.removeAll { it.id in gone }
                        held.removeAll(gone)
                    }
                    p.add.forEach { held.add(it.id) }
                    list.addAll(p.add)
                    st = st.copy(pages = st.pages + 1, pageBytes = st.pageBytes + bytes.size)
                }
                else -> {
                    torn = true
                    st = st.copy(clean = false)
                }
            }
        }
        return entries to st
    }

    /** `count_pages`: how many `facts.<n>` records a storage holds, from 1 to the first absent. */
    public fun countPages(storage: Storage): Int {
        var n = 0
        while (storage.load(ReplicaFile.pageKey(n + 1)) != null) n += 1
        return n
    }

    /**
     * `Stored::load`: the snapshot, the journal after it — skipping pages at or below the
     * snapshot's cursor, stopping at the first that does not decode or does not start at the next
     * sequence — the login and the intents; `null` where there is no snapshot. Reads, never writes.
     */
    public fun load(storage: Storage, schema: Schema): Stored? {
        val bytes = storage.load(ReplicaFile.KEY) ?: return null
        val (file, logId) = decodeReplica(bytes, schema)
        val snapshotCursor = file.cursor
        var pages = 0
        var pageBytes = 0
        var found = 0
        var clean = true
        var torn = false
        while (true) {
            val b = storage.load(ReplicaFile.pageKey(found + 1)) ?: break
            found += 1
            if (torn) continue
            val p =
                try {
                    decodePage(b)
                } catch (_: StorageError) {
                    null
                }
            when {
                p != null && p.to <= snapshotCursor -> clean = false
                p != null && p.from == file.cursor + 1 -> {
                    for (f in p.facts) file.confirmed.applyChanges(f)
                    file.cursor = p.to
                    pages += 1
                    pageBytes += b.size
                }
                else -> {
                    torn = true
                    clean = false
                }
            }
        }
        storage.load(ReplicaFile.WHO)?.let {
            val (u, s) = decodeWho(it)
            file.user = u
            file.session = s
        }
        val (entries, pending) = loadPending(storage)
        if (entries != null) file.pending = entries
        return Stored(
            file, logId, snapshotCursor, bytes.size, pages, pageBytes, found, clean, pending)
    }

    /** The alone log's layout, as `ark::journal::Layout::alone` names it. */
    public val alone: Layout = Layout.alone()
}
