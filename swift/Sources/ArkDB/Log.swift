// §10 The log, as `rust/ark/src/log.rs` defines it: the module's history,
// an append-only sequence of entries, each an intent with what applying it
// changed kept beside it, standing on a snapshot.
//
// An entry is an intent: who authored it, the function by hash, the
// arguments, the autos. The facts beside it are derived, never
// authoritative, and are what a peer takes for an entry it cannot replay.
// A client holds a log only when it is its own authority (alone,
// `docs/plan-alone.md`), and reads one back from its own storage
// (`Journal.swift`); compaction and the ids folded below a horizon are a
// server's, and are here only as far as reading a log written by one needs.

/// A position in the log (`log::Seq`). The first entry is 1; 0 is "nothing".
public typealias Seq = Int64

/// What applying an entry changed, in order (`log::Facts`).
public typealias FactList = [Change]

/// An intent, as recorded (`log::Entry`). The sequence is not a field: it
/// is the key an entry is stored under once the authority assigned it.
/// `roles` are the roles its author held, frozen with it
/// (`docs/plan-guards.md` D1), in code point order.
public struct LogEntry: Hashable, CustomStringConvertible {
    public let id: Id
    public let actor: String
    public let session: String
    public let roles: [String]
    public let fnHash: [UInt8]
    public let args: [String: Value]
    public let autos: [String: Value]

    public init(_ id: Id, _ actor: String, _ session: String, _ roles: [String], _ fnHash: [UInt8], _ args: [String: Value], _ autos: [String: Value]) {
        self.id = id
        self.actor = actor
        self.session = session
        self.roles = Ctx.sortedRoles(roles)
        self.fnHash = fnHash
        self.args = args
        self.autos = autos
    }

    /// The same intent, authored as `actor` under `session`.
    public func by(_ actor: String, _ session: String) -> LogEntry {
        LogEntry(id, actor, session, roles, fnHash, args, autos)
    }

    public static func == (a: LogEntry, b: LogEntry) -> Bool {
        a.id == b.id && textEquals(a.actor, b.actor) && textEquals(a.session, b.session) && a.roles == b.roles && a.fnHash == b.fnHash
            && argsEqual(a.args, b.args) && argsEqual(a.autos, b.autos)
    }

    public func hash(into h: inout Hasher) { h.combine(id) }

    public var description: String { "LogEntry(\(id.text) by \(actor)/\(session) \(Hex.encode(fnHash)))" }
}

/// Two argument maps the same: the same names, each with an equal value.
public func argsEqual(_ a: [String: Value], _ b: [String: Value]) -> Bool {
    if a.count != b.count { return false }
    for (k, v) in a {
        guard let w = b[k], w == v else { return false }
    }
    return true
}

/// Arguments as the struct they are on the wire.
public func argsRecord(_ a: [String: Value]) -> Record { Record(a.map { ($0.key, $0.value) }) }

/// A log (`log::Log`): the snapshot it stands on — its sequence, its store,
/// its hash and the log's identity — every entry above it with its facts,
/// and every id it sequenced: exactly above the horizon, and below it by an
/// 8-byte key (`below`, `log::Below`), as a server keeps them.
public final class Log {
    public var baseSeq: Seq
    public var baseStore: MemoryStore
    public var baseHash: [UInt8]
    public var logId: Id?
    public var entries = SortedMap<Seq, (LogEntry, FactList)>()
    public var ids = [Id: Seq]()
    /// `(key, seq)` of the ids below the horizon, in unsigned key order (`log::Below`).
    public var below = [(UInt64, Seq)]()

    public init(_ baseSeq: Seq, _ baseStore: MemoryStore, _ baseHash: [UInt8], _ logId: Id?) {
        self.baseSeq = baseSeq
        self.baseStore = baseStore
        self.baseHash = baseHash
        self.logId = logId
    }

    /// `Log::empty`: nothing in it, standing on the empty store at 0.
    public static func empty(_ sch: Schema) -> Log {
        let st = MemoryStore.empty(sch)
        return Log(0, st, Hash.stateHash(st), nil)
    }

    /// `head_seq`: the last sequence assigned.
    public func headSeq() -> Seq { entries.last?.key ?? baseSeq }

    /// `horizon`: the sequence the log stands on.
    public func horizon() -> Seq { baseSeq }

    /// §10.1 `append`: an entry the authority applied, at `head + 1`.
    @discardableResult
    public func append(_ e: LogEntry, _ facts: FactList) -> Seq {
        let n = headSeq() + 1
        ids[e.id] = n
        entries[n] = (e, facts)
        return n
    }

    /// `seq_of`: the sequence an id was given, exactly above the horizon and by key below.
    public func seqOf(_ id: Id) -> Seq? {
        if let n = ids[id] { return n }
        let k = Log.belowKey(id)
        return below.filter { $0.0 == k }.map { $0.1 }.min()
    }

    /// `Below::extend`: keys added, kept in unsigned key order.
    public func extendBelow(_ more: [(UInt64, Seq)]) {
        below.append(contentsOf: more)
        below.sort { $0.0 != $1.0 ? $0.0 < $1.0 : $0.1 < $1.1 }
    }

    /// `contiguous`: the entries run without a gap from the sequence after the snapshot.
    public func contiguous() -> Bool {
        var n = baseSeq
        for k in entries.keys {
            if k != n + 1 { return false }
            n = k
        }
        return true
    }

    /// §10.2 `state_at`: the state at a retained sequence, from the snapshot and the facts.
    public func stateAt(_ n: Seq) -> MemoryStore? {
        if n < horizon() || n > headSeq() { return nil }
        let st = baseStore.copy()
        for (k, ef) in entries where k <= n { for c in ef.1 { st.applyChange(Log.widened(st.schema, c)) } }
        return st
    }

    /// `take_entries`: every entry taken out, oldest first, and the log left
    /// standing at its head with the ids kept — what a peer alone's authority
    /// does after each append (`docs/plan-alone.md` §2).
    public func takeEntries() -> [(Seq, LogEntry, FactList)] {
        baseSeq = headSeq()
        let out = entries.entries.map { ($0.key, $0.value.0, $0.value.1) }
        entries.removeAll()
        return out
    }

    /// `Below::key`: the first 8 bytes of the id's SHA-256, big-endian.
    public static func belowKey(_ id: Id) -> UInt64 {
        let h = Sha256.hash(id.bytes)
        var k: UInt64 = 0
        for i in 0..<8 { k = (k << 8) | UInt64(h[i]) }
        return k
    }

    /// `widened`: a fact as a state of this schema holds it — a row that names
    /// nothing the table lacks but leaves out a nullable column laid out with
    /// it `Null`; any other change as it is (`docs/plan-db.md` D1).
    public static func widened(_ sch: Schema, _ c: Change) -> Change {
        guard let tbl = sch.lookupTable(c.table) else { return c }
        func short(_ r: Row) -> Bool { r.count < tbl.columns.count && r.names.allSatisfy { tbl.column($0) != nil } }
        func fill(_ r: Row) -> Row {
            if !short(r) { return r }
            if case .success(let p) = Rules.projectRow(tbl, r) { return p }
            return r
        }
        switch c {
        case .add(let t, let r): return short(r) ? .add(t, fill(r)) : c
        case .remove(let t, let r): return short(r) ? .remove(t, fill(r)) : c
        case .edit(let t, let o, let n): return short(o) || short(n) ? .edit(t, fill(o), fill(n)) : c
        }
    }
}
