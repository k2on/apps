// The storage held to two things, as `kotlin/ark-client/.../StorageTests.kt`
// holds the Kotlin one:
//
// 1. The bytes. A fixture a Rust `ark_client::Peer` wrote — the demo module,
//    seeded autos, alone, reopened, joined to an in-process server, synced,
//    authoring online and offline, reopened again — is replayed into a Swift
//    `Peer` step by step, and after every step its storage must be the Rust
//    one's, key for key and byte for byte. The Rust peer's storage is also
//    opened by the Swift peer at each `open`, which must find the state the
//    Rust peer reported. The fixture is round 3's, read where it was
//    committed (`kotlin/ark-client/fixtures/rust-peer.json`), never copied.
// 2. The semantics. Every trace MANIFEST.json gives this runtime and, when
//    named, the corpus — replayed with the client's durable copy kept through a
//    `Storage` in these records (`StoredCopy`) rather than in memory: a
//    reopen opens what the records say, so a record that loses anything a
//    trace's reopen needs fails that trace at the step after it.
import ArkDB
import ArkDBClient
import Foundation

/// A client's durable copy kept through a `Storage` in the records
/// `Storage.swift` writes, by the writer a `Peer` writes them with: after
/// every step the replica is persisted, and a reopen opens what
/// `Records.load` reads back — the store, the cursor, the log, the union and
/// the pending intents all from the records.
final class StoredCopy: Durable {
    private(set) var storage = MemoryStorage()
    private var writer: ReplicaWriter
    private var fork = Fork()
    // The intents as last written: an intent rewritten in place — a sign-in
    // making nobody's intents somebody's, a join re-stamping the local
    // history — keeps its id, so the writer is told to write them whole, as
    // `Peer.signIn` and `Peer.join` tell it.
    private var wrote = [Id: LogEntry]()

    init() { writer = ReplicaWriter.fresh(storage) }

    func clear() {
        wrote = [:]
        storage = MemoryStorage()
        writer = ReplicaWriter.fresh(storage)
        fork = Fork()
    }

    func keep(_ c: Client, _ alone: Alone?) throws {
        if let a = alone { fork = Fork(a.forkLog, a.fork) }
        let pending = c.replica.pending
        if pending.contains(where: { e in wrote[e.id].map { !same($0, e) } ?? false }) { writer.forgetPending() }
        try writer.persist(c.replica, fork, Ctx.nobody)
        wrote = [:]
        for e in pending { wrote[e.id] = e }
    }

    // Kotlin's `LogEntry` is a data class, equal field for field; Swift's
    // `==` is by id, so the fields are compared here.
    private func same(_ a: LogEntry, _ b: LogEntry) -> Bool {
        a.id == b.id && a.actor == b.actor && a.session == b.session && a.roles == b.roles && a.fnHash == b.fnHash && argsEqual(a.args, b.args) && argsEqual(a.autos, b.autos)
    }

    func reopen(_ old: Client) throws -> Client {
        guard let st = try Records.load(storage, old.schema) else { throw Failed("a reopen with nothing stored") }
        let f = st.file
        return reopenFrom(old, f.confirmed, f.cursor, st.logId, f.partial, f.pending)
    }
}

enum StorageTests {
    static var problems = [String]()

    private static func differ(_ want: Record, _ got: MemoryStorage) throws -> String? {
        var keys = Set(got.keys())
        for (k, _) in want.pairs { keys.insert(k) }
        var bad = [String]()
        for k in keys.sorted() {
            let w = try want[k].map { try $0.bytes() }
            let g = got.load(k)
            if w == nil || g == nil || w! != g! {
                let ws = w.map { String(Hex.encode($0).prefix(120)) } ?? "absent"
                let gs = g.map { String(Hex.encode($0).prefix(120)) } ?? "absent"
                bad.append("\(k): rust \(ws), swift \(gs)")
            }
        }
        return bad.isEmpty ? nil : bad.joined(separator: "; ")
    }

    /// The Rust-written fixture, replayed into a Swift peer, storage compared after every step.
    static func fixture(_ path: String) throws -> Int {
        let v = try readJson(path)
        let domain = Domain(try Decode.module(try Canon.decode(try v.field("module").bytes())))
        let storage = MemoryStorage()
        var peer: Peer? = nil
        let steps = try v.field("steps").list()
        for (k, s) in steps.enumerated() {
            let what = try s.field("do").text()
            func args(_ x: Value) throws -> [String: Value] {
                var out = [String: Value]()
                for (n, a) in try x.record().pairs { out[n] = a }
                return out
            }
            switch what {
            case "open":
                try peer?.persist()
                let tok = try s.field("token")
                let token: String? = tok == .null ? nil : try tok.text()
                guard case .bool(let alone) = try s.field("alone") else { throw Failed("fixture step \(k): alone is not a bool") }
                let opts = Options(try s.field("user").text(), try s.field("session").text(), token, alone: alone, autos: .seeded(try s.field("seed").int()))
                let p = try Peer.open(domain, storage, opts)
                peer = p
                if let state = s.opt("state") {
                    let got = Value.record([
                        ("cursor", .int(p.cursor)),
                        ("hash", .bytes(Hash.stateHash(p.replica.confirmed))),
                        ("view_hash", .bytes(Hash.stateHash(p.replica.view))),
                        ("pending", .list(p.replica.pending.map { .id($0.id) })),
                    ])
                    if got != state {
                        problems.append("fixture step \(k): the Rust peer's storage opens as \(got.show()), it said \(state.show())")
                    }
                }
            case "mutate":
                var refused = false
                do {
                    try peer!.mutate(try s.field("name").text(), try args(try s.field("args")))
                } catch is Refused {
                    refused = true
                }
                let want = try s.field("refused") == .bool(true)
                if refused != want { problems.append("fixture step \(k): refused \(refused), the Rust peer \(want)") }
            case "persist":
                try peer!.persist()
            case "join":
                try peer!.join(try s.field("user").text(), try s.field("session").text(), try s.field("token").text())
            case "connected":
                peer!.connected()
                _ = peer!.takeOutgoingFrames()
            case "disconnected":
                peer!.disconnected()
            case "recv":
                for f in try s.field("frames").list() { try peer!.recvFrame(try f.bytes()) }
                try peer!.persist()
                _ = peer!.takeOutgoingFrames()
            default:
                problems.append("fixture step \(k): no such step \(what)")
            }
            if let d = try differ(try s.field("storage").record(), storage) {
                problems.append("fixture step \(k) (\(what)): \(d)")
                return k
            }
        }
        return steps.count
    }

    /// `DirStorage` on this machine: a record written, read back, replaced
    /// and removed, and the directory synced after every rename, as the
    /// reference's `Dir` syncs it.
    static func directory() throws {
        let base = NSTemporaryDirectory() + "/ark-dir-storage-\(getpid())"
        defer { try? FileManager.default.removeItem(atPath: base) }
        let dir = base + "/replica"
        let st = DirStorage(dir)
        let before = problems.count
        try st.save("pending", [1, 2, 3])
        try st.save("pending", [4])
        if st.load("pending") != [4] { problems.append("DirStorage: a replaced record does not read back") }
        if !st.syncsDirectory { problems.append("DirStorage: the directory was not synced") }
        if (try FileManager.default.contentsOfDirectory(atPath: dir)).contains(where: { $0.hasSuffix(".tmp") }) {
            problems.append("DirStorage: a temporary file was left behind")
        }
        try st.remove("pending")
        if st.load("pending") != nil { problems.append("DirStorage: a removed record reads back") }
        if problems.count == before { print("storage: DirStorage writes, replaces and removes, the directory synced") }
    }

    /// Every trace the manifest gives this runtime replayed with the durable
    /// copy kept in these records: what `MANIFEST.json` lists of kind `trace`
    /// on a side this runtime runs, each passed or failed as its `expect`
    /// says, and every other one skipped by name — the manifest is the list
    /// (spec/RUNNERS.md), so a trace of a side this runtime does not run is
    /// never replayed by walking a directory.
    static func traces(_ vectors: String, _ corpus: String?) throws {
        let mine = try Manifest.read(vectors).filter { $0.kind == "trace" }
        var ran = 0
        for t in mine {
            if !Manifest.sides.contains(t.side) {
                print("storage: skipped \(t.path) (\(t.side): \(Manifest.skipReason(t.side)))")
                continue
            }
            var failure: String? = nil
            do {
                try ClientChecks.trace(try readJson(vectors + "/" + t.path), { StoredCopy() })
            } catch {
                failure = "\(error)"
            }
            ran += 1
            if t.expect == "pass", let f = failure { problems.append("\(t.path) through storage: \(f)") }
            if t.expect == "fail" {
                if let f = failure {
                    print("storage: \(t.path) fails, as it must (\(String(f.prefix(120))))")
                } else {
                    problems.append("\(t.path) replayed through storage and must not")
                }
            }
        }
        print("storage: trace: \(ran) of \(mine.count) the manifest names replayed with the durable copy in storage records")
        if ran == 0 { problems.append("the manifest gave this runtime no trace") }
        if let c = corpus {
            let (n, failed, ms) = try ClientChecks.corpus(c, { StoredCopy() })
            print("storage: trace corpus \(c) through storage records: \(n - failed.count) of \(n) replayed in \(ms) ms")
            for f in failed.prefix(20) { problems.append("corpus through storage: \(String(f.prefix(2000)))") }
            if failed.count > 20 { problems.append("… and \(failed.count - 20) more") }
            if n == 0 { problems.append("the corpus at \(c) holds no trace") }
        }
    }

    /// All three; the problems found, empty when every record is the reference's.
    static func run(_ vectors: String, _ fixturePath: String, _ corpus: String?) -> [String] {
        problems = []
        do {
            let n = try fixture(fixturePath)
            if problems.isEmpty {
                print("storage: fixture \(fixturePath): \(n) steps, every record the Rust peer's, byte for byte")
            }
        } catch {
            problems.append("storage fixture: \(error)")
        }
        do { try directory() } catch { problems.append("DirStorage: \(error)") }
        do { try traces(vectors, corpus) } catch { problems.append("storage traces: \(error)") }
        return problems
    }
}
