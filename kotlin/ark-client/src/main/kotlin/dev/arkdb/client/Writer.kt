// What a peer writes, and when, as `rust/ark-client/src/peer.rs` decides it
// (`persist`, `persist_confirmed`, `snapshot`, `persist_pending`,
// `compact_pending`, `snapshot_pending`): the confirmed store as a snapshot
// and pages of the journal after it, the pending intents as a snapshot of a
// generation and pages of what moved, and the login. One writer, so that a
// `Peer` and a trace runner keeping its client durable through a `Storage`
// (`StoredCopy`) write the same records by the same rules.
package dev.arkdb.client

import dev.arkdb.Ctx
import dev.arkdb.Id
import dev.arkdb.LogEntry
import dev.arkdb.Moved
import dev.arkdb.Replica
import dev.arkdb.Seq

/** What a storage holds of a replica, so that a write adds what moved since and nothing else. */
public class ReplicaWriter
private constructor(
    private val storage: Storage,
    // The journal: the snapshot's size, the pages after it and their size,
    // the cursor and log they reach, and whether the next write must be a
    // snapshot (`peer::Durable`).
    private var snapshotBytes: Int,
    private var pages: Int,
    private var pageBytes: Int,
    private var cursor: Seq,
    private var logId: Id?,
    private var snapshotDue: Boolean,
    // The intents: the snapshot's generation and size, the pages after it
    // and their size (`peer::PendingFile`), and the ids as written, or null
    // where the next write must be a snapshot.
    private var gen: Long,
    private var pendingBytes: Int,
    private var pendingPages: Int,
    private var pendingPageBytes: Int,
    private var wrotePending: MutableList<Id>?,
    private var wroteWho: Ctx?,
) {
    public companion object {
        /** A storage holding no replica: everything is written at the first write. */
        public fun fresh(
            storage: Storage,
            pending: PendingStored = PendingStored()
        ): ReplicaWriter =
            ReplicaWriter(
                storage,
                0,
                Records.countPages(storage),
                0,
                0,
                null,
                true,
                pending.gen,
                pending.snapshotBytes,
                pending.found,
                pending.pageBytes,
                null,
                null)

        /** A storage `Records.load` read: written on from what it found. */
        public fun of(storage: Storage, st: Stored): ReplicaWriter =
            ReplicaWriter(
                storage,
                st.snapshotBytes,
                st.found,
                st.pageBytes,
                st.file.cursor,
                st.logId,
                !st.clean,
                st.pending.gen,
                st.pending.snapshotBytes,
                st.pending.found,
                st.pending.pageBytes,
                if (st.pending.clean) st.file.pending.map { it.id }.toMutableList() else null,
                Ctx(st.file.user, st.file.session))
    }

    /** The log the snapshot on the storage names. */
    public val namedLog: Id?
        get() = logId

    /** The intents are written next as a snapshot, whatever moved. */
    public fun forgetPending() {
        wrotePending = null
    }

    /** `persist`, after the local history: the intents, the login, the confirmed store. */
    public fun persist(r: Replica, fork: Fork, ctx: Ctx) {
        persistPending(r.pending)
        compactPending(r.pending)
        if (wroteWho != ctx) {
            storage.save(ReplicaFile.WHO, Records.encodeWho(ctx.user, ctx.session))
            wroteWho = ctx
        }
        persistConfirmed(r, fork, ctx)
    }

    /** `persist_confirmed`: the journal since the last write, as one page; or a snapshot. */
    public fun persistConfirmed(r: Replica, fork: Fork, ctx: Ctx) {
        val run =
            when (val m = r.takeConfirmed()) {
                is Moved.Replaced -> {
                    snapshotDue = true
                    emptyList()
                }
                is Moved.Run -> m.run
            }
        val follows = run.isEmpty() || run.first().first == cursor + 1
        val renamed = r.logId != logId
        if (snapshotDue || !follows || renamed) return snapshot(r, fork, ctx)
        if (run.isEmpty()) return
        val to = run.last().first
        val bytes = Records.encodePage(run)
        try {
            storage.save(ReplicaFile.pageKey(pages + 1), bytes)
        } catch (e: Exception) {
            snapshotDue = true
            throw e
        }
        pages += 1
        pageBytes += bytes.size
        cursor = to
        if (pageBytes > snapshotBytes) snapshot(r, fork, ctx)
    }

    /**
     * `snapshot`: the confirmed store whole at the cursor, then the pages it now holds removed,
     * newest first — so a stop anywhere leaves only pages at or below the snapshot's cursor.
     */
    public fun snapshot(r: Replica, fork: Fork, ctx: Ctx) {
        snapshotDue = true
        val bytes =
            Records.encodeReplica(
                r.cursor, r.logId, fork, r.partial, r.confirmed, ctx.user, ctx.session)
        storage.save(ReplicaFile.KEY, bytes)
        for (n in pages downTo 1) {
            storage.remove(ReplicaFile.pageKey(n))
            pages = n - 1
        }
        snapshotBytes = bytes.size
        pages = 0
        pageBytes = 0
        cursor = r.cursor
        logId = r.logId
        snapshotDue = false
    }

    /**
     * `persist_pending`: the pending intents, if they moved since they were last written — one page
     * of what moved, or a snapshot where the storage's copy is unknown, the move is not a page's
     * shape, or nothing is left pending. Whether the list only grew is one comparison: the last
     * intent written still in its place.
     */
    public fun persistPending(pending: List<LogEntry>) {
        val held = wrotePending ?: return snapshotPending(pending)
        val n = held.size
        val grew = n == 0 || (pending.size >= n && pending[n - 1].id == held[n - 1])
        if (grew && n == pending.size) return
        if (pending.isEmpty()) return snapshotPending(pending)
        val add: List<LogEntry>
        val drop: List<Id>
        if (grew) {
            add = pending.subList(n, pending.size).toList()
            drop = emptyList()
        } else {
            val now = pending.map { it.id }.toSet()
            val was = held.toSet()
            add = pending.filter { it.id !in was }
            drop = held.filter { it !in now }
            val says = held.filter { it in now } + add.map { it.id }
            if (says != pending.map { it.id }) return snapshotPending(pending)
        }
        val bytes = Records.encodePendingPage(gen, add, drop)
        try {
            storage.save(ReplicaFile.pendingPageKey(pendingPages + 1), bytes)
        } catch (e: Exception) {
            wrotePending = null
            throw e
        }
        pendingPages += 1
        pendingPageBytes += bytes.size
        if (drop.isNotEmpty()) {
            val gone = drop.toSet()
            held.removeAll { it in gone }
        }
        held.addAll(add.map { it.id })
    }

    /** `compact_pending`: once the pages outgrow the snapshot, or nothing is pending. */
    public fun compactPending(pending: List<LogEntry>) {
        if (pendingPages > 0 && (pendingPageBytes > pendingBytes || pending.isEmpty()))
            snapshotPending(pending)
    }

    /**
     * `snapshot_pending`: the intents whole, as a snapshot of the next generation, then the pages
     * it now holds removed, newest first — a stop leaves only pages of an older generation.
     */
    public fun snapshotPending(pending: List<LogEntry>) {
        wrotePending = null
        val next = gen + 1
        val bytes = Records.encodePendingSnapshot(next, pending)
        val ids = pending.map { it.id }.toMutableList()
        storage.save(ReplicaFile.PENDING, bytes)
        gen = next
        pendingBytes = bytes.size
        for (k in pendingPages downTo 1) {
            storage.remove(ReplicaFile.pendingPageKey(k))
            pendingPages = k - 1
        }
        pendingPageBytes = 0
        wrotePending = ids
    }
}
