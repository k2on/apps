// Where a peer's replica is kept between runs, as
// `rust/ark-client/src/storage.rs` keeps it — record for record and byte for
// byte, so that a Swift device and a Rust device write the same bytes for
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
//     log       the local history of a peer alone (`ArkDB.Journal`,
//     log.1 …   `Layout.alone()`): the fork, then pages of what it sequenced
//
// The reference's module docs say why each record is shaped as it is and
// why a stop anywhere leaves something `load` reads correctly; `Peer` here
// writes them in the reference's order and by its rules.
import ArkDB
import Foundation

/// A record that does not read as one (`ark_client::Error::Corrupt`), or a storage that failed.
public struct StorageError: Error, CustomStringConvertible {
    public let what: String
    public init(_ what: String) { self.what = what }
    public var description: String { what }
}

/// A key-value place for a peer's files (`storage::Storage`). `key` is a
/// short file name such as `replica`. A `save` replaces its record whole or
/// not at all. There is no listing: pages are found by trying `facts.1`,
/// `facts.2`, … in turn. Every storage is also where a log's records can be
/// kept (`Keys`): the alone log is written through the same `save` and
/// `remove` as the replica.
public protocol Storage: Keys {}

/// In memory (`storage::Memory`): for tests, and for "reopening" from what a peer left behind.
public final class MemoryStorage: Storage {
    private var map = [String: [UInt8]]()
    private let lock = NSLock()

    public init() {}

    public func load(_ key: String) -> [UInt8]? {
        lock.lock()
        defer { lock.unlock() }
        return map[key]
    }

    public func save(_ key: String, _ bytes: [UInt8]) {
        lock.lock()
        defer { lock.unlock() }
        map[key] = bytes
    }

    public func remove(_ key: String) {
        lock.lock()
        defer { lock.unlock() }
        map[key] = nil
    }

    /// Every key held, sorted, for a test to look at.
    public func keys() -> [String] {
        lock.lock()
        defer { lock.unlock() }
        return map.keys.sorted()
    }

    /// A second storage holding what this one holds now.
    public func copy() -> MemoryStorage {
        let c = MemoryStorage()
        lock.lock()
        c.map = map
        lock.unlock()
        return c
    }
}

/// A directory, one file per key (`storage::Dir`), each written whole to a
/// temporary name beside it, synced, renamed over the old one, and the
/// directory synced after — `fsync(2)` on the directory's descriptor, as the
/// reference's `sync_dir` does it, which Linux, macOS and iOS all allow: a
/// crash, of the program or of the machine, leaves the old record or the new
/// one, never part of either, and never the new name over nothing.
public final class DirStorage: Storage {
    public let dir: String
    /// Whether the last save synced the directory after its rename, as the reference does.
    public private(set) var syncsDirectory = false

    public init(_ dir: String) { self.dir = dir }

    private func path(_ key: String) -> String { (dir as NSString).appendingPathComponent(key) }

    public func load(_ key: String) -> [UInt8]? {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path(key), isDirectory: &isDir), !isDir.boolValue,
            let d = FileManager.default.contents(atPath: path(key))
        else { return nil }
        return [UInt8](d)
    }

    public func save(_ key: String, _ bytes: [UInt8]) throws {
        do {
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        } catch {
            throw StorageError("cannot make \(dir): \(error)")
        }
        let tmp = path(key + ".tmp")
        let fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0o644)
        if fd < 0 { throw StorageError("cannot write \(tmp)") }
        var off = 0
        while off < bytes.count {
            let n = bytes.withUnsafeBufferPointer { p in write(fd, p.baseAddress! + off, bytes.count - off) }
            if n <= 0 {
                close(fd)
                throw StorageError("cannot write \(tmp)")
            }
            off += n
        }
        if fsync(fd) != 0 {
            close(fd)
            throw StorageError("cannot sync \(tmp)")
        }
        close(fd)
        if rename(tmp, path(key)) != 0 { throw StorageError("cannot rename \(tmp) to \(path(key))") }
        syncDir()
    }

    // `sync_dir`: the rename made durable.
    private func syncDir() {
        let fd = open(dir, O_RDONLY)
        if fd < 0 {
            syncsDirectory = false
            return
        }
        syncsDirectory = fsync(fd) == 0
        close(fd)
    }

    public func remove(_ key: String) throws {
        if FileManager.default.fileExists(atPath: path(key)) {
            do {
                try FileManager.default.removeItem(atPath: path(key))
            } catch {
                throw StorageError("cannot remove \(path(key))")
            }
        }
    }
}

/// Where a replica last shared a log with a server (`storage::Fork`,
/// `docs/plan-alone.md` §1): that log and the cursor it was at — `nil` and 0
/// for a peer that never had a server.
public struct Fork: Equatable {
    public var logId: Id?
    public var cursor: Seq
    public init(_ logId: Id? = nil, _ cursor: Seq = 0) {
        self.logId = logId
        self.cursor = cursor
    }

    func value() -> Value {
        var f: [(String, Value)] = [("cursor", .int(cursor))]
        if let l = logId { f.append(("log", .id(l))) }
        return .record(f)
    }
}

/// What is durable about the log, as it is read back (`storage::ReplicaFile`):
/// the snapshot with every page after it applied, and the intents not yet answered.
public final class ReplicaFile {
    public let fork: Fork
    public var cursor: Seq
    public let confirmed: MemoryStore
    public var pending: [LogEntry]
    public var user: String
    public var session: String
    public let partial: [String: [String]]?

    init(_ fork: Fork, _ cursor: Seq, _ confirmed: MemoryStore, _ pending: [LogEntry], _ user: String, _ session: String, _ partial: [String: [String]]?) {
        self.fork = fork
        self.cursor = cursor
        self.confirmed = confirmed
        self.pending = pending
        self.user = user
        self.session = session
        self.partial = partial
    }

    public static let key = "replica"
    public static let pendingKey = "pending"
    public static let whoKey = "who"

    /// The key of the journal's `n`th page, counting from 1.
    public static func pageKey(_ n: Int) -> String { "facts.\(n)" }

    /// The key of the pending intents' `n`th page, counting from 1.
    public static func pendingPageKey(_ n: Int) -> String { "pending.\(n)" }
}

/// One page of the journal (`storage::JournalPage`): the changes at each sequence `from`..`to`.
public struct JournalPage {
    public let from: Seq
    public let to: Seq
    public let facts: [FactList]
}

/// One page of the pending intents (`storage::PendingPage`).
public struct PendingPage {
    public let gen: Int64
    public let add: [LogEntry]
    public let drop: [Id]
}

/// What the pending intents' records looked like when read (`storage::PendingStored`).
public struct PendingStored {
    public var gen: Int64 = 0
    public var snapshotBytes = 0
    public var pages = 0
    public var pageBytes = 0
    public var found = 0
    public var clean = false
    public init() {}
}

/// What `load` found in a storage (`storage::Stored`): the replica, and what the journal was.
public struct Stored {
    public let file: ReplicaFile
    public let logId: Id?
    public let snapshotCursor: Seq
    public let snapshotBytes: Int
    public let pages: Int
    public let pageBytes: Int
    public let found: Int
    public let clean: Bool
    public let pending: PendingStored
}

/// The records, encoded and decoded as `storage.rs` encodes and decodes them.
public enum Records {
    private static func decode(_ bytes: [UInt8], _ bad: (String) -> StorageError) throws -> Record {
        let v: Value
        do {
            v = try Canon.decode(bytes)
        } catch let e as DecodeError {
            throw bad("\(e)")
        }
        guard case .record(let m) = v else { throw bad("not a struct") }
        return m
    }

    /// `encode_replica_held`: the `replica` record's bytes.
    public static func encodeReplica(_ cursor: Seq, _ logId: Id?, _ fork: Fork, _ partial: [String: [String]]?, _ confirmed: MemoryStore, _ user: String, _ session: String) -> [UInt8] {
        var f: [(String, Value)] = [("t", .text("replica")), ("cursor", .int(cursor)), ("fork", fork.value()), ("confirmed", confirmed.storeValue()), ("user", .text(user)), ("session", .text(session)), ("hashing", .int(HASH_VERSION))]
        if let l = logId { f.append(("log", .id(l))) }
        if let p = partial { f.append(("partial", .record(Record(p.map { ($0.key, Value.list($0.value.map { .text($0) })) })))) }
        return Canon.encode(.record(f))
    }

    /// `decode_replica`: the `replica` record, and the log it names.
    public static func decodeReplica(_ bytes: [UInt8], _ schema: Schema) throws -> (ReplicaFile, Id?) {
        func bad(_ w: String) -> StorageError { StorageError("a replica file: \(w)") }
        let m = try decode(bytes, bad)
        if m["t"] != .text("replica") { throw bad("not a replica") }
        guard case .int(let cursor)? = m["cursor"] else { throw bad("no cursor") }
        guard case .record(let rows)? = m["confirmed"] else { throw bad("no confirmed store") }
        for (t, rs) in rows.pairs {
            guard case .list(let list) = rs else { throw bad("rows of \(t)") }
            if list.contains(where: { $0.recordValue == nil }) { throw bad("a row of \(t)") }
        }
        let confirmed0 = MemoryStore.fromValue(schema, m["confirmed"]!)
        var pending = [LogEntry]()
        switch m["pending"] {
        case nil: break
        case .list(let ps)?:
            do {
                pending = try ps.map(entryFromValue)
            } catch let e as FactError {
                throw bad(e.what)
            }
        default: throw bad("pending is not a list")
        }
        func optional(_ k: String) throws -> String {
            switch m[k] {
            case nil: return ""
            case .text(let s)?: return s
            default: throw bad("\(k) is not text")
            }
        }
        var logId: Id? = nil
        switch m["log"] {
        case nil: break
        case .id(let i)?: logId = i
        default: throw bad("log is not an id")
        }
        switch m["hashing"] {
        case nil: break
        case .int(let h)? where h >= 1 && h <= HASH_VERSION: break
        case let h?: throw bad("a state hash by a construction this build does not know: \(h.show())")
        }
        var fork = Fork()
        switch m["fork"] {
        case nil: break
        case .record(let f)?:
            var fl: Id? = nil
            switch f["log"] {
            case nil: break
            case .id(let i)?: fl = i
            default: throw bad("a fork's log is not an id")
            }
            guard case .int(let fc)? = f["cursor"] else { throw bad("a fork with no cursor") }
            fork = Fork(fl, fc)
        default: throw bad("fork is not a struct")
        }
        var partial: [String: [String]]? = nil
        switch m["partial"] {
        case nil: break
        case .record(let p)?:
            var out = [String: [String]]()
            for (t, cs) in p.pairs {
                guard case .list(let l) = cs else { throw bad("partial: a table's columns are not a list") }
                out[t] = try l.map { c in
                    guard case .text(let s) = c else { throw bad("partial: a column is not text") }
                    return s
                }
            }
            partial = out
        default: throw bad("partial is not a struct")
        }
        var confirmed = confirmed0
        if let p = partial {
            let device = deviceSchema(schema, p)
            let st = MemoryStore.empty(device)
            for t in device.tables { for r in confirmed0.scan(t.name) { st.applyChange(.add(t.name, r)) } }
            confirmed = st
        }
        return (ReplicaFile(fork, cursor, confirmed, pending, try optional("user"), try optional("session"), partial), logId)
    }

    /// `encode_page`: a journal page's bytes, for a contiguous run of sequences.
    public static func encodePage(_ run: [(Seq, FactList)]) -> [UInt8] {
        Canon.encode(.record([("t", .text("facts")), ("from", .int(run.first?.0 ?? 0)), ("to", .int(run.last?.0 ?? 0)), ("facts", .list(run.map { factsValue($0.1) }))]))
    }

    /// `decode_page`: a page back, whole — a run that does not add up is corrupt.
    public static func decodePage(_ bytes: [UInt8]) throws -> JournalPage {
        func bad(_ w: String) -> StorageError { StorageError("a journal page: \(w)") }
        let m = try decode(bytes, bad)
        if m["t"] != .text("facts") { throw bad("not a page") }
        guard case .int(let from)? = m["from"] else { throw bad("no from") }
        guard case .int(let to)? = m["to"] else { throw bad("no to") }
        guard case .list(let runs)? = m["facts"] else { throw bad("no facts") }
        let facts = try runs.map { f -> FactList in
            guard case .list(let cs) = f else { throw bad("facts are not lists") }
            do {
                return try cs.map(changeFromValue)
            } catch let e as FactError {
                throw bad(e.what)
            }
        }
        if from < 1 || to < from || Int64(facts.count) != to - from + 1 { throw bad("\(facts.count) lists of facts for \(from)..\(to)") }
        return JournalPage(from: from, to: to, facts: facts)
    }

    /// `encode_who`.
    public static func encodeWho(_ user: String, _ session: String) -> [UInt8] {
        Canon.encode(.record([("t", .text("who")), ("user", .text(user)), ("session", .text(session))]))
    }

    /// `decode_who`: user, session.
    public static func decodeWho(_ bytes: [UInt8]) throws -> (String, String) {
        func bad(_ w: String) -> StorageError { StorageError("a who file: \(w)") }
        let m = try decode(bytes, bad)
        if m["t"] != .text("who") { throw bad("not a login") }
        guard case .text(let u)? = m["user"] else { throw bad("no user") }
        guard case .text(let s)? = m["session"] else { throw bad("no session") }
        return (u, s)
    }

    /// `encode_pending`: the `pending` record as written before pages had it — generation 0.
    public static func encodePending(_ pending: [LogEntry]) -> [UInt8] {
        Canon.encode(.record([("t", .text("pending")), ("entries", .list(pending.map(entryValue)))]))
    }

    /// `encode_pending_snapshot`: a snapshot of generation `gen`.
    public static func encodePendingSnapshot(_ gen: Int64, _ pending: [LogEntry]) -> [UInt8] {
        Canon.encode(.record([("t", .text("pending")), ("gen", .int(gen)), ("entries", .list(pending.map(entryValue)))]))
    }

    /// `decode_pending_snapshot`: its generation (0 where it has none) and its intents.
    public static func decodePendingSnapshot(_ bytes: [UInt8]) throws -> (Int64, [LogEntry]) {
        func bad(_ w: String) -> StorageError { StorageError("a pending file: \(w)") }
        let m = try decode(bytes, bad)
        if m["t"] != .text("pending") { throw bad("not pending intents") }
        var gen: Int64 = 0
        switch m["gen"] {
        case nil: break
        case .int(let g)?: gen = g
        default: throw bad("gen is not an int")
        }
        guard case .list(let ps)? = m["entries"] else { throw bad("no entries") }
        do {
            return (gen, try ps.map(entryFromValue))
        } catch let e as FactError {
            throw bad(e.what)
        }
    }

    /// `encode_pending_page`.
    public static func encodePendingPage(_ gen: Int64, _ add: [LogEntry], _ drop: [Id]) -> [UInt8] {
        Canon.encode(.record([("t", .text("pending-ops")), ("gen", .int(gen)), ("add", .list(add.map(entryValue))), ("drop", .list(drop.map { .id($0) }))]))
    }

    /// `decode_pending_page`.
    public static func decodePendingPage(_ bytes: [UInt8]) throws -> PendingPage {
        func bad(_ w: String) -> StorageError { StorageError("a pending page: \(w)") }
        let m = try decode(bytes, bad)
        if m["t"] != .text("pending-ops") { throw bad("not a pending page") }
        guard case .int(let gen)? = m["gen"] else { throw bad("no gen") }
        guard case .list(let add)? = m["add"], case .list(let drop)? = m["drop"] else { throw bad("no add or drop") }
        let adds: [LogEntry]
        do {
            adds = try add.map(entryFromValue)
        } catch let e as FactError {
            throw bad(e.what)
        }
        let drops = try drop.map { d -> Id in
            guard case .id(let i) = d else { throw bad("a drop is not an id") }
            return i
        }
        return PendingPage(gen: gen, add: adds, drop: drops)
    }

    /// `load_pending`: the snapshot, then its pages in order — skipping pages
    /// of an older generation, stopping at the first that does not decode or
    /// adds an intent already held; `nil` for the intents where there is no
    /// `pending` record.
    public static func loadPending(_ storage: Storage) throws -> ([LogEntry]?, PendingStored) {
        var st = PendingStored()
        var entries: [LogEntry]? = nil
        var gen: Int64 = 0
        if let snap = storage.load(ReplicaFile.pendingKey) {
            let (g, es) = try decodePendingSnapshot(snap)
            gen = g
            entries = es
            st.snapshotBytes = snap.count
        }
        st.gen = gen
        st.clean = entries != nil
        var torn = entries == nil
        var held = Set(entries?.map { $0.id } ?? [])
        while let bytes = storage.load(ReplicaFile.pendingPageKey(st.found + 1)) {
            st.found += 1
            if torn { continue }
            let p = try? decodePendingPage(bytes)
            if let p = p, p.gen < gen {
                st.clean = false
            } else if let p = p, p.gen == gen, p.add.allSatisfy({ !held.contains($0.id) || p.drop.contains($0.id) }) {
                let gone = Set(p.drop)
                if !gone.isEmpty {
                    entries!.removeAll { gone.contains($0.id) }
                    held.subtract(gone)
                }
                for e in p.add { held.insert(e.id) }
                entries!.append(contentsOf: p.add)
                st.pages += 1
                st.pageBytes += bytes.count
            } else {
                torn = true
                st.clean = false
            }
        }
        return (entries, st)
    }

    /// `count_pages`: how many `facts.<n>` records a storage holds, from 1 to the first absent.
    public static func countPages(_ storage: Storage) -> Int {
        var n = 0
        while storage.load(ReplicaFile.pageKey(n + 1)) != nil { n += 1 }
        return n
    }

    /// `Stored::load`: the snapshot, the journal after it — skipping pages at
    /// or below the snapshot's cursor, stopping at the first that does not
    /// decode or does not start at the next sequence — the login and the
    /// intents; `nil` where there is no snapshot. Reads, never writes.
    public static func load(_ storage: Storage, _ schema: Schema) throws -> Stored? {
        guard let bytes = storage.load(ReplicaFile.key) else { return nil }
        let (file, logId) = try decodeReplica(bytes, schema)
        let snapshotCursor = file.cursor
        var pages = 0
        var pageBytes = 0
        var found = 0
        var clean = true
        var torn = false
        while let b = storage.load(ReplicaFile.pageKey(found + 1)) {
            found += 1
            if torn { continue }
            let p = try? decodePage(b)
            if let p = p, p.to <= snapshotCursor {
                clean = false
            } else if let p = p, p.from == file.cursor + 1 {
                for f in p.facts { file.confirmed.applyChanges(f) }
                file.cursor = p.to
                pages += 1
                pageBytes += b.count
            } else {
                torn = true
                clean = false
            }
        }
        if let w = storage.load(ReplicaFile.whoKey) {
            let (u, s) = try decodeWho(w)
            file.user = u
            file.session = s
        }
        let (entries, pending) = try loadPending(storage)
        if let es = entries { file.pending = es }
        return Stored(file: file, logId: logId, snapshotCursor: snapshotCursor, snapshotBytes: bytes.count, pages: pages, pageBytes: pageBytes, found: found, clean: clean, pending: pending)
    }

    /// The alone log's layout, as `ark::journal::Layout::alone` names it.
    public static let alone = Layout.alone()
}
