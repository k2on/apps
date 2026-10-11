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
// Swift device writes the bytes a Rust device writes for the same log. A
// client keeps one log this way — its local history alone, under
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

/// A key/value place for a log's records (`journal::Keys`). `save` replaces
/// its record whole or not at all, which is what the rules above rest on.
public protocol Keys: AnyObject {
    func load(_ key: String) -> [UInt8]?
    func save(_ key: String, _ bytes: [UInt8]) throws
    func remove(_ key: String) throws
    /// Add `bytes` to the end of `key`, creating it.
    func append(_ key: String, _ bytes: [UInt8]) throws
    /// Cut `key` to its first `len` bytes.
    func truncate(_ key: String, _ len: Int) throws
}

extension Keys {
    /// By default the record is rewritten whole.
    public func append(_ key: String, _ bytes: [UInt8]) throws { try save(key, (load(key) ?? []) + bytes) }

    /// By default rewritten whole.
    public func truncate(_ key: String, _ len: Int) throws {
        let all = load(key) ?? []
        try save(key, Array(all.prefix(len)))
    }
}

/// How records reach a page (`journal::Paging`).
public enum Paging {
    /// One page, appended to: a file.
    case append
    /// A page per write, each saved whole under `{page}.{n}`.
    case pages
}

/// What a compaction does (`journal::Compaction`).
public enum Compaction {
    case snapshot
    case merge
}

/// Where a log's records are kept, and how they move (`journal::Layout`).
public struct Layout {
    public let snapshot: String
    public let page: String
    public let paging: Paging
    public let compaction: Compaction

    /// The key of the `n`th page, counting from 1.
    public func pageKey(_ n: Int) -> String { paging == .append ? page : "\(page).\(n)" }

    /// The server's data directory: `log.ark-log` and `log.ark-journal`.
    public static func server() -> Layout { Layout(snapshot: "log.ark-log", page: "log.ark-journal", paging: .append, compaction: .snapshot) }

    /// A peer alone's local history: `log`, then `log.1`, `log.2`, …
    public static func alone() -> Layout { Layout(snapshot: "log", page: "log", paging: .pages, compaction: .merge) }
}

/// Which construction of the state hash this build writes (`hash::HASH_VERSION`).
public let HASH_VERSION: Int64 = 2

/// `hash::state_hash_by`: the state hash by the construction `version`
/// names — 1, the list of every table's rows, as it was until
/// `docs/plan-db.md` D3; 2, the additive digest — or `nil`.
public func stateHashBy(_ version: Int64, _ st: Store) -> [UInt8]? {
    switch version {
    case 1: return Sha256.hash(Canon.encode(.list(st.schema.tables.map { t in .list([.text(t.name), .list(st.scan(t.name).map { .record($0) })]) })))
    case HASH_VERSION: return Hash.stateHash(st)
    default: return nil
    }
}

/// A journal that cannot be read, or written (`journal.rs`'s `Err(String)`).
public struct JournalError: Error, CustomStringConvertible {
    public let what: String
    public init(_ what: String) { self.what = what }
    public var description: String { what }
}

/// What reading one page's records over a head found (`journal::Replayed`).
public struct Replayed {
    public let applied: Int
    public let stale: Int
    public let end: Int
    public let len: Int
    /// Whether everything after `end` is a torn tail.
    public var torn: Bool { end < len }
}

/// What reading a log's records found (`journal::Opened`).
public struct Opened {
    public let snapshot: Log?
    public let head: Seq
    public let applied: Int
    public let stale: Int
    /// Bytes dropped as a torn tail, and the page they were in.
    public let torn: (Int, Int)?
}

public enum LogRecords {
    /// `record_value`: one entry as the snapshot and the pages both carry it.
    public static func recordValue(_ n: Seq, _ e: LogEntry, _ facts: FactList) -> Value {
        .record([("seq", .int(n)), ("entry", entryValue(e)), ("facts", factsValue(facts))])
    }

    /// `encode_record`: a page record's bytes — the length, then the record.
    public static func encodeRecord(_ n: Seq, _ e: LogEntry, _ facts: FactList) -> [UInt8] {
        let body = Canon.encode(recordValue(n, e, facts))
        let len = UInt32(body.count)
        return [UInt8(len >> 24), UInt8((len >> 16) & 0xff), UInt8((len >> 8) & 0xff), UInt8(len & 0xff)] + body
    }

    /// `log_to_value`: a log as a value; the snapshot is `Canon.encode` of this.
    public static func logToValue(_ log: Log) -> Value {
        let st = log.baseStore
        var rows = [(String, Value)]()
        var seen = Set<String>()
        for t in st.schema.tables where seen.insert(t.name).inserted { rows.append((t.name, .list(st.scan(t.name).map { .record($0) }))) }
        let entries = log.entries.entries.map { recordValue($0.key, $0.value.0, $0.value.1) }
        let ids = log.ids.sorted { $0.key < $1.key }.map { Value.record([("id", .id($0.key)), ("seq", .int($0.value))]) }
            + log.below.map { Value.record([("key", .bytes(longBytes($0.0))), ("seq", .int($0.1))]) }
        var base: [(String, Value)] = [("seq", .int(log.baseSeq)), ("hash", .bytes(log.baseHash)), ("hashing", .int(HASH_VERSION)), ("rows", .record(Record(rows)))]
        if let l = log.logId { base.append(("log", .id(l))) }
        return .record([("t", .text("log")), ("base", .record(base)), ("entries", .list(entries)), ("ids", .list(ids))])
    }

    private static func longBytes(_ k: UInt64) -> [UInt8] { (0..<8).map { UInt8((k >> (56 - 8 * UInt64($0))) & 0xff) } }

    private static func fields(_ v: Value) throws -> Record {
        guard case .record(let r) = v else { throw JournalError("expected a struct, found \(v.show())") }
        return r
    }

    private static func need(_ m: Record, _ k: String) throws -> Value {
        guard let v = m[k] else { throw JournalError("missing field \(k)") }
        return v
    }

    private static func int(_ v: Value) throws -> Int64 {
        guard case .int(let n) = v else { throw JournalError("expected an int, found \(v.show())") }
        return n
    }

    private static func items(_ v: Value) throws -> [Value] {
        guard case .list(let xs) = v else { throw JournalError("expected a list, found \(v.show())") }
        return xs
    }

    /// `record_from_value`: one entry back from its value.
    public static func recordFromValue(_ v: Value) throws -> (Seq, LogEntry, FactList) {
        let m = try fields(v)
        let n = try int(try need(m, "seq"))
        let e: LogEntry
        do {
            e = try entryFromValue(try need(m, "entry"))
        } catch let x as FactError {
            throw JournalError("entry \(n): \(x.what)")
        }
        let facts: FactList
        do {
            facts = try items(try need(m, "facts")).map(changeFromValue)
        } catch let x as FactError {
            throw JournalError("facts of \(n): \(x.what)")
        }
        return (n, e, facts)
    }

    /// `hashing_of`: which construction a snapshot's hash is by — 1 where it says none.
    public static func hashingOf(_ base: Record) throws -> Int64 {
        guard let h = base["hashing"] else { return 1 }
        if case .int(let n) = h, n >= 1 && n <= HASH_VERSION { return n }
        throw JournalError("a state hash by a construction this build does not know: \(h.show())")
    }

    /// `log_from_value`: a log from its value, over the module's schema.
    public static func logFromValue(_ schema: Schema, _ v: Value) throws -> Log {
        let m = try fields(v)
        if try need(m, "t") != .text("log") { throw JournalError("not a log file: t = \(m["t"]?.show() ?? "absent")") }
        let base = try fields(try need(m, "base"))
        let store = MemoryStore.fromValue(schema, try need(base, "rows"))
        var logId: Id? = nil
        if let l = base["log"] {
            guard case .id(let i) = l else { throw JournalError("the log's identity is not an id: \(l.show())") }
            logId = i
        }
        let claimed = stateHashBy(try hashingOf(base), store)
        guard case .bytes(let hash)? = base["hash"], let c = claimed, c == hash else { throw JournalError("the snapshot's hash does not match its rows") }
        let log = Log(try int(try need(base, "seq")), store, Hash.stateHash(store), logId)
        for item in try items(try need(m, "entries")) {
            let (n, e, facts) = try recordFromValue(item)
            log.entries[n] = (e, facts)
        }
        var keys = [(UInt64, Seq)]()
        for item in try items(try need(m, "ids")) {
            let im = try fields(item)
            let n = try int(try need(im, "seq"))
            switch (im["id"], im["key"]) {
            case (.id(let i)?, _): log.ids[i] = n
            case (nil, .bytes(let k)?) where k.count == 8:
                var x: UInt64 = 0
                for b in k { x = (x << 8) | UInt64(b) }
                keys.append((x, n))
            default: throw JournalError("expected an id or an 8-byte key")
            }
        }
        log.extendBelow(keys)
        if !log.contiguous() { throw JournalError("the entries do not run without a gap from \(log.horizon() + 1) to \(log.headSeq())") }
        return log
    }

    /// `decode_snapshot`: the snapshot's bytes back, as a log.
    public static func decodeSnapshot(_ schema: Schema, _ bytes: [UInt8]) throws -> Log {
        let v: Value
        do {
            v = try Canon.decode(bytes)
        } catch let e as DecodeError {
            throw JournalError("decoding: \(e)")
        }
        return try logFromValue(schema, v)
    }

    /// `replay_page`: a page's records handed to `each`, in order, from the
    /// one after `head`, stopping at the first that is short, does not
    /// decode, or does not carry the next sequence.
    public static func replayPage(_ head0: Seq, _ bytes: [UInt8], _ each: (Seq, LogEntry, FactList) throws -> Void) rethrows -> (Seq, Replayed) {
        var head = head0
        var applied = 0
        var stale = 0
        var at = 0
        while at < bytes.count {
            if at + 4 > bytes.count { break }
            let n = Int(bytes[at]) << 24 | Int(bytes[at + 1]) << 16 | Int(bytes[at + 2]) << 8 | Int(bytes[at + 3])
            if at + 4 + n > bytes.count { break }
            guard let v = try? Canon.decode(Array(bytes[(at + 4)..<(at + 4 + n)])), let rec = try? recordFromValue(v) else { break }
            let (seq, e, facts) = rec
            if seq == head + 1 {
                try each(seq, e, facts)
                head = seq
                applied += 1
            } else if seq <= head && applied == 0 {
                stale += 1
            } else {
                break
            }
            at += 4 + n
        }
        return (head, Replayed(applied: applied, stale: stale, end: at, len: bytes.count))
    }

    /// Every page as read: the good length of each that extended the log, and what followed.
    struct Walk {
        var good = [Int]()
        var after = 0
        var tornAt: (Int, Int, Int)? = nil
        var stale = 0
        var applied = 0
        var head: Seq = 0
    }

    static func readSnapshot(_ keys: Keys, _ layout: Layout, _ schema: Schema) throws -> (Log, Int)? {
        guard let bytes = keys.load(layout.snapshot) else { return nil }
        do {
            return (try decodeSnapshot(schema, bytes), bytes.count)
        } catch let e as JournalError {
            throw JournalError("reading \(layout.snapshot): \(e.what)")
        }
    }

    static func walk(_ keys: Keys, _ layout: Layout, _ head0: Seq, _ each: (Seq, LogEntry, FactList) throws -> Void) rethrows -> Walk {
        var w = Walk()
        var head = head0
        var read = [(Int, Bool)]()
        var ended = false
        var n = 1
        while true {
            guard let bytes = keys.load(layout.pageKey(n)) else { break }
            if ended {
                w.after += 1
            } else {
                let (h, r) = try replayPage(head, bytes, each)
                head = h
                w.applied += r.applied
                w.stale += r.stale
                if r.torn {
                    w.tornAt = (n, r.end, r.len)
                    w.after += 1
                    ended = true
                } else {
                    read.append((r.len, r.applied == 0 && r.stale > 0))
                }
            }
            if layout.paging == .append { break }
            n += 1
        }
        while let last = read.last, last.1, w.tornAt == nil {
            read.removeLast()
            w.after += 1
        }
        w.good = read.map { $0.0 }
        w.head = head
        return w
    }

    /// `journal::read`: the snapshot, then every record the pages add, handed to `each`.
    public static func read(_ keys: Keys, _ layout: Layout, _ schema: Schema, _ each: (Seq, LogEntry, FactList) throws -> Void) throws -> Opened {
        let snapshot = try readSnapshot(keys, layout, schema)
        let w = try walk(keys, layout, snapshot?.0.headSeq() ?? 0, each)
        return Opened(snapshot: snapshot?.0, head: w.head, applied: w.applied, stale: w.stale, torn: w.tornAt.map { ($0.0, $0.2 - $0.1) })
    }

    /// `journal::load`: a log back whole, the pages appended; `nil` where there is neither.
    public static func load(_ keys: Keys, _ layout: Layout, _ schema: Schema) throws -> Log? {
        var got = [(LogEntry, FactList)]()
        let o = try read(keys, layout, schema) { _, e, f in got.append((e, f)) }
        if o.snapshot == nil && got.isEmpty { return nil }
        let log = o.snapshot ?? Log.empty(schema)
        for (e, f) in got { log.append(e, f) }
        return log
    }
}

/// A log open for writing (`journal::Journal`): what the storage holds of
/// it, so that a write adds what moved since and nothing else.
public final class Journal {
    public let layout: Layout
    private var pages: [Int]
    private var snapshotBytes: Int
    private var head: Seq
    private var horizon: Seq
    private var named: Id?
    private var due: Bool
    /// Every byte written, snapshots and pages both.
    public var written: Int64 = 0

    private init(_ layout: Layout, _ pages: [Int], _ snapshotBytes: Int, _ head: Seq, _ horizon: Seq, _ named: Id?, _ due: Bool) {
        self.layout = layout
        self.pages = pages
        self.snapshotBytes = snapshotBytes
        self.head = head
        self.horizon = horizon
        self.named = named
        self.due = due
    }

    /// The snapshot's size and the pages', as last written.
    public func sizes() -> (Int, Int) { (snapshotBytes, pages.reduce(0, +)) }

    /// How many pages are on the storage.
    public func pageCount() -> Int { pages.count }

    /// The head as last written.
    public func headSeq() -> Seq { head }

    /// `append`: records above the head, already encoded, reaching `to`:
    /// appended to the one page, or saved as a page of their own.
    public func append(_ keys: Keys, _ records: [UInt8], _ to: Seq) throws {
        if records.isEmpty { return }
        switch layout.paging {
        case .append:
            do {
                try keys.append(layout.pageKey(1), records)
            } catch {
                due = true
                throw error
            }
            if pages.isEmpty { pages.append(records.count) } else { pages[0] += records.count }
        case .pages:
            try keys.save(layout.pageKey(pages.count + 1), records)
            pages.append(records.count)
        }
        written += Int64(records.count)
        head = to
    }

    /// `merge`: the newest page merged into the one before it for as long as
    /// that one is no larger — the merged page saved whole first, then the
    /// newer removed. Nothing under `Compaction.snapshot`.
    public func merge(_ keys: Keys) throws {
        if layout.compaction != .merge { return }
        while pages.count >= 2 {
            let older = pages[pages.count - 2]
            let newer = pages[pages.count - 1]
            if older > newer { break }
            let k = pages.count
            let a = layout.pageKey(k - 1)
            let b = layout.pageKey(k)
            guard let x = keys.load(a) else { throw JournalError("\(a) is gone") }
            guard let y = keys.load(b) else { throw JournalError("\(b) is gone") }
            let both = x + y
            try keys.save(a, both)
            written += Int64(both.count)
            try keys.remove(b)
            pages.removeLast()
            pages[pages.count - 1] = both.count
        }
    }

    /// `snapshot`: the log whole as the snapshot, then the pages emptied — so
    /// a stop between leaves records the snapshot already holds, which a reader skips.
    public func snapshot(_ keys: Keys, _ log: Log) throws {
        let bytes = Canon.encode(LogRecords.logToValue(log))
        try keys.save(layout.snapshot, bytes)
        written += Int64(bytes.count)
        snapshotBytes = bytes.count
        head = log.headSeq()
        horizon = log.horizon()
        named = log.logId
        switch layout.paging {
        case .append:
            if !pages.isEmpty {
                try keys.truncate(layout.pageKey(1), 0)
                pages = [0]
            }
        case .pages:
            while !pages.isEmpty {
                try keys.remove(layout.pageKey(pages.count))
                pages.removeLast()
            }
        }
        due = false
    }

    /// `Journal::fresh`: nothing on the storage yet, whose first write is its snapshot.
    public static func fresh(_ layout: Layout) -> Journal { Journal(layout, [], 0, 0, 0, nil, false) }

    /// `Journal::open`: read as `LogRecords.read` reads it, then repaired so
    /// that what is written next follows on — a torn page cut back to its
    /// whole records, every page after the good ones removed, newest first.
    public static func open(_ keys: Keys, _ layout: Layout, _ schema: Schema, _ each: (Seq, LogEntry, FactList) throws -> Void) throws -> (Journal, Opened) {
        let snapshot = try LogRecords.readSnapshot(keys, layout, schema)
        let w = try LogRecords.walk(keys, layout, snapshot?.0.headSeq() ?? 0, each)
        var pages = w.good
        switch layout.paging {
        case .append:
            if let t = w.tornAt {
                try keys.truncate(layout.pageKey(1), t.1)
                pages = [t.1]
            } else if w.after > 0 {
                pages = [keys.load(layout.pageKey(1))?.count ?? 0]
            }
        case .pages:
            let firstAfter = w.good.count + 1
            let last = w.good.count + w.after
            if last >= firstAfter {
                for n in stride(from: last, through: firstAfter, by: -1) {
                    if let t = w.tornAt, t.0 == n, t.1 > 0 {
                        try keys.truncate(layout.pageKey(n), t.1)
                        pages.append(t.1)
                    } else {
                        try keys.remove(layout.pageKey(n))
                    }
                }
            }
        }
        let j = Journal(layout, pages, snapshot?.1 ?? 0, w.head, snapshot?.0.horizon() ?? 0, snapshot?.0.logId, false)
        let opened = Opened(snapshot: snapshot?.0, head: w.head, applied: w.applied, stale: w.stale, torn: w.tornAt.map { ($0.0, $0.2 - $0.1) })
        return (j, opened)
    }

    /// `Journal::create`: a log started on `keys` at `base` — a snapshot with
    /// nothing above it — clearing any pages a log that was there left,
    /// newest first, then the snapshot saved.
    public static func create(_ keys: Keys, _ layout: Layout, _ base: Log) throws -> Journal {
        try clearPages(keys, layout)
        let j = fresh(layout)
        try j.snapshot(keys, base)
        return j
    }

    /// `Journal::destroy`: the log removed — its pages newest first, then its snapshot.
    public static func destroy(_ keys: Keys, _ layout: Layout) throws {
        try clearPages(keys, layout)
        try keys.remove(layout.snapshot)
    }

    private static func clearPages(_ keys: Keys, _ layout: Layout) throws {
        var n = 0
        while keys.load(layout.pageKey(n + 1)) != nil {
            n += 1
            if layout.paging == .append { break }
        }
        if n >= 1 { for k in stride(from: n, through: 1, by: -1) { try keys.remove(layout.pageKey(k)) } }
    }
}
