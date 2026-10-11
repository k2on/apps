// What a peer writes, and when, as `rust/ark-client/src/peer.rs` decides it
// (`persist`, `persist_confirmed`, `snapshot`, `persist_pending`,
// `compact_pending`, `snapshot_pending`): the confirmed store as a snapshot
// and pages of the journal after it, the pending intents as a snapshot of a
// generation and pages of what moved, and the login. One writer, so that a
// `Peer` and a trace runner keeping its client durable through a `Storage`
// write the same records by the same rules.
import ArkDB

/// What a storage holds of a replica, so that a write adds what moved since and nothing else.
public final class ReplicaWriter {
    private let storage: Storage
    // The journal: the snapshot's size, the pages after it and their size,
    // the cursor and log they reach, and whether the next write must be a
    // snapshot (`peer::Durable`).
    private var snapshotBytes: Int
    private var pages: Int
    private var pageBytes: Int
    private var cursor: Seq
    private var logId: Id?
    private var snapshotDue: Bool
    // The intents: the snapshot's generation and size, the pages after it
    // and their size (`peer::PendingFile`), and the ids as written, or `nil`
    // where the next write must be a snapshot.
    private var gen: Int64
    private var pendingBytes: Int
    private var pendingPages: Int
    private var pendingPageBytes: Int
    private var wrotePending: [Id]?
    private var wroteWho: Ctx?

    private init(_ storage: Storage, _ snapshotBytes: Int, _ pages: Int, _ pageBytes: Int, _ cursor: Seq, _ logId: Id?, _ snapshotDue: Bool, _ gen: Int64, _ pendingBytes: Int, _ pendingPages: Int, _ pendingPageBytes: Int, _ wrotePending: [Id]?, _ wroteWho: Ctx?) {
        self.storage = storage
        self.snapshotBytes = snapshotBytes
        self.pages = pages
        self.pageBytes = pageBytes
        self.cursor = cursor
        self.logId = logId
        self.snapshotDue = snapshotDue
        self.gen = gen
        self.pendingBytes = pendingBytes
        self.pendingPages = pendingPages
        self.pendingPageBytes = pendingPageBytes
        self.wrotePending = wrotePending
        self.wroteWho = wroteWho
    }

    /// A storage holding no replica: everything is written at the first write.
    public static func fresh(_ storage: Storage, _ pending: PendingStored = PendingStored()) -> ReplicaWriter {
        ReplicaWriter(storage, 0, Records.countPages(storage), 0, 0, nil, true, pending.gen, pending.snapshotBytes, pending.found, pending.pageBytes, nil, nil)
    }

    /// A storage `Records.load` read: written on from what it found.
    public static func of(_ storage: Storage, _ st: Stored) -> ReplicaWriter {
        ReplicaWriter(storage, st.snapshotBytes, st.found, st.pageBytes, st.file.cursor, st.logId, !st.clean, st.pending.gen, st.pending.snapshotBytes, st.pending.found, st.pending.pageBytes, st.pending.clean ? st.file.pending.map { $0.id } : nil, Ctx(st.file.user, st.file.session))
    }

    /// The log the snapshot on the storage names.
    public var namedLog: Id? { logId }

    /// The intents are written next as a snapshot, whatever moved.
    public func forgetPending() { wrotePending = nil }

    /// `persist`, after the local history: the intents, the login, the confirmed store.
    public func persist(_ r: Replica, _ fork: Fork, _ ctx: Ctx) throws {
        try persistPending(r.pending)
        try compactPending(r.pending)
        if wroteWho != ctx {
            try storage.save(ReplicaFile.whoKey, Records.encodeWho(ctx.user, ctx.session))
            wroteWho = ctx
        }
        try persistConfirmed(r, fork, ctx)
    }

    /// `persist_confirmed`: the journal since the last write, as one page; or a snapshot.
    public func persistConfirmed(_ r: Replica, _ fork: Fork, _ ctx: Ctx) throws {
        var run = [(Seq, FactList)]()
        switch r.takeConfirmed() {
        case .replaced: snapshotDue = true
        case .run(let rs): run = rs
        }
        let follows = run.isEmpty || run[0].0 == cursor + 1
        let renamed = r.logId != logId
        if snapshotDue || !follows || renamed { return try snapshot(r, fork, ctx) }
        guard let last = run.last else { return }
        let bytes = Records.encodePage(run)
        do {
            try storage.save(ReplicaFile.pageKey(pages + 1), bytes)
        } catch {
            snapshotDue = true
            throw error
        }
        pages += 1
        pageBytes += bytes.count
        cursor = last.0
        if pageBytes > snapshotBytes { try snapshot(r, fork, ctx) }
    }

    /// `snapshot`: the confirmed store whole at the cursor, then the pages it
    /// now holds removed, newest first — so a stop anywhere leaves only pages
    /// at or below the snapshot's cursor.
    public func snapshot(_ r: Replica, _ fork: Fork, _ ctx: Ctx) throws {
        snapshotDue = true
        let bytes = Records.encodeReplica(r.cursor, r.logId, fork, r.partialMap, r.confirmed, ctx.user, ctx.session)
        try storage.save(ReplicaFile.key, bytes)
        var n = pages
        while n >= 1 {
            try storage.remove(ReplicaFile.pageKey(n))
            pages = n - 1
            n -= 1
        }
        snapshotBytes = bytes.count
        pages = 0
        pageBytes = 0
        cursor = r.cursor
        logId = r.logId
        snapshotDue = false
    }

    /// `persist_pending`: the pending intents, if they moved since they were
    /// last written — one page of what moved, or a snapshot where the
    /// storage's copy is unknown, the move is not a page's shape, or nothing
    /// is left pending. Whether the list only grew is one comparison: the
    /// last intent written still in its place.
    public func persistPending(_ pending: [LogEntry]) throws {
        guard var held = wrotePending else { return try snapshotPending(pending) }
        let n = held.count
        let grew = n == 0 || (pending.count >= n && pending[n - 1].id == held[n - 1])
        if grew && n == pending.count { return }
        if pending.isEmpty { return try snapshotPending(pending) }
        let add: [LogEntry]
        let drop: [Id]
        if grew {
            add = Array(pending[n...])
            drop = []
        } else {
            let now = Set(pending.map { $0.id })
            let was = Set(held)
            add = pending.filter { !was.contains($0.id) }
            drop = held.filter { !now.contains($0) }
            let says = held.filter { now.contains($0) } + add.map { $0.id }
            if says != pending.map({ $0.id }) { return try snapshotPending(pending) }
        }
        let bytes = Records.encodePendingPage(gen, add, drop)
        do {
            try storage.save(ReplicaFile.pendingPageKey(pendingPages + 1), bytes)
        } catch {
            wrotePending = nil
            throw error
        }
        pendingPages += 1
        pendingPageBytes += bytes.count
        if !drop.isEmpty {
            let gone = Set(drop)
            held.removeAll { gone.contains($0) }
        }
        held.append(contentsOf: add.map { $0.id })
        wrotePending = held
    }

    /// `compact_pending`: once the pages outgrow the snapshot, or nothing is pending.
    public func compactPending(_ pending: [LogEntry]) throws {
        if pendingPages > 0 && (pendingPageBytes > pendingBytes || pending.isEmpty) { try snapshotPending(pending) }
    }

    /// `snapshot_pending`: the intents whole, as a snapshot of the next
    /// generation, then the pages it now holds removed, newest first — a stop
    /// leaves only pages of an older generation.
    public func snapshotPending(_ pending: [LogEntry]) throws {
        wrotePending = nil
        let next = gen + 1
        let bytes = Records.encodePendingSnapshot(next, pending)
        try storage.save(ReplicaFile.pendingKey, bytes)
        gen = next
        pendingBytes = bytes.count
        var k = pendingPages
        while k >= 1 {
            try storage.remove(ReplicaFile.pendingPageKey(k))
            pendingPages = k - 1
            k -= 1
        }
        pendingPageBytes = 0
        wrotePending = pending.map { $0.id }
    }
}
