import Foundation

/// A position in the log. The first entry is 1; 0 is "nothing".
public typealias Seq = Int64

/// §10 An intent, as recorded. The sequence is the key it is stored under
/// once the authority has assigned it, never a field.
public struct Entry: Equatable {
    /// Chosen by the originating peer; the dedupe key.
    public var id: Id
    public var actor: String
    public var session: String
    /// The closure that authored it (§8.3).
    public var fn: FnHash
    public var args: Args
    public var autos: Args
    public init(id: Id, actor: String, session: String, fn: FnHash, args: Args, autos: Args) {
        self.id = id; self.actor = actor; self.session = session; self.fn = fn; self.args = args; self.autos = autos
    }
}

/// What applying an entry changed, in order.
public typealias Facts = [Change]

/// The state at a sequence, and its hash.
public struct Snapshot {
    public var seq: Seq
    public var store: MemoryStore
    public var hash: [UInt8]
    public init(seq: Seq, store: MemoryStore) {
        self.seq = seq
        self.store = store
        self.hash = Hash.stateHash(store)
    }
}

public struct LogItem: Equatable {
    public var entry: Entry
    public var facts: Facts
}

/// What a peer at a cursor is sent next.
public enum Page {
    /// The entries after the cursor, at most a batch, and whether more follow.
    case entries([(Seq, Entry, Facts)], Bool)
    /// The cursor is below the horizon: start again from the snapshot.
    case belowHorizon(Snapshot)
}

/// The module's history: a snapshot and the entries above it, each with the
/// facts its application produced.
public struct Log {
    public var base: Snapshot
    /// Every sequence above `base`, contiguous.
    public var entries: [Seq: LogItem]
    /// Every entry id ever sequenced, kept below the horizon too.
    public var ids: [Id: Seq]

    public init(schema: Schema) {
        base = Snapshot(seq: 0, store: MemoryStore(schema: schema))
        entries = [:]
        ids = [:]
    }

    /// The last sequence assigned.
    public var headSeq: Seq { return entries.keys.max() ?? base.seq }

    /// The sequence the log stands on: entries at or below it are gone.
    public var horizon: Seq { return base.seq }

    /// The retained sequences in order.
    public var sequences: [Seq] { return entries.keys.sorted() }

    /// §10.1 Append an entry the authority has applied; the sequence is
    /// `head + 1` and nothing else ever assigns one.
    public mutating func append(_ e: Entry, _ facts: Facts) -> Seq {
        let n = headSeq + 1
        entries[n] = LogItem(entry: e, facts: facts)
        ids[e.id] = n
        return n
    }

    /// The sequence an entry id was given, if it ever was.
    public func seqOf(_ i: Id) -> Seq? { return ids[i] }

    public func entriesAfter(_ cursor: Seq, _ limit: Int) -> Page {
        if cursor < horizon { return .belowHorizon(base) }
        let after = sequences.filter { $0 > cursor }.map { ($0, entries[$0]!.entry, entries[$0]!.facts) }
        return .entries(Array(after.prefix(limit)), after.count > limit)
    }

    /// §10.2 The state at any retained sequence, from the snapshot and the
    /// facts alone — no function is run.
    public func stateAt(_ n: Seq) -> MemoryStore? {
        if n < horizon || n > headSeq { return nil }
        let st = base.store.clone()
        for s in sequences where s <= n { st.applyChanges(entries[s]!.facts) }
        return st
    }

    /// §10.3 Move the horizon up to a sequence; false if it is not retained.
    public mutating func compactTo(_ n: Seq) -> Bool {
        guard let st = stateAt(n) else { return false }
        base = Snapshot(seq: n, store: st)
        for s in sequences where s <= n { entries[s] = nil }
        return true
    }

    /// The function hashes the retained entries name.
    public var namedHashes: Set<FnHash> {
        return Set(entries.values.map { $0.entry.fn })
    }

    /// Whether the entries run without a gap from the snapshot to the head.
    public var contiguous: Bool {
        let want = horizon < headSeq ? Array((horizon + 1)...headSeq) : []
        return sequences == want
    }
}
