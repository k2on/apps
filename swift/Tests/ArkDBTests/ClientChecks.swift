// The checks of the client's kinds — `protocol`, `rebase` (its scripted
// half) and `trace` — as spec/RUNNERS.md says them and
// rust/ark/tests/vectors.rs and rust/ark/tests/traces.rs read them. Each
// throws `Failed` with its first unmet claim, so a `falsify/` vector must
// fail the same check a vector passes.
import ArkDB
import Foundation

enum ClientChecks {
    // protocol ------------------------------------------------------------

    /// `check_protocol`: the bytes decode to the frame and the frame encodes
    /// to the bytes; the frame reads as a frame of its side and writes back
    /// as itself; and a server frame read as a client reads the socket —
    /// under the demo's schema and under none — is the frame as read.
    static func protocolFrame(_ path: String, _ v: Value) throws {
        let frame = try v.field("frame")
        let bytes = try hexBytes(try v.field("bytes"))
        try ensureEq(try Canon.decode(bytes), frame, "the bytes decode to the frame")
        try ensureBytes(Canon.encode(frame), bytes, "the frame encodes to the bytes")
        if (path.split(separator: "/").last ?? "").hasPrefix("client-") {
            let f = try ClientMsg.fromValue(frame)
            try ensureEq(f.toValue(), frame, "decode then encode")
            try ensureEq(try ClientMsg.fromValue(f.toValue()).toValue(), f.toValue(), "encode then decode")
        } else {
            let f = try ServerMsg.fromValue(frame)
            try ensureEq(f.toValue(), frame, "decode then encode")
            try ensureEq(try ServerMsg.fromValue(f.toValue()).toValue(), f.toValue(), "encode then decode")
            let demo = try Checks.moduleOf(try Checks.vector("rebase/three-peers.json").field("module")).schema
            for sch in [demo, Schema([])] {
                var want: Value
                var isSnap = false
                if case .snapshotOf(let seq, _, let rows, let logId, let module, let held) = f {
                    want = Snapshot.ofValues(sch, seq, rows, logId, module, held).toValue()
                    isSnap = true
                } else {
                    want = f.toValue()
                }
                var got: Value
                switch try ServerMsg.decodeFor(bytes, sch) {
                case .snap(let s):
                    if !isSnap { throw Failed("read as a client reads it: a snapshot") }
                    got = s.toValue()
                case .msg(let m):
                    if isSnap { throw Failed("read as a client reads it: not a snapshot") }
                    got = m.toValue()
                }
                try ensureEq(got, want, "read as a client reads it")
            }
        }
    }

    // rebase/three-peers ----------------------------------------------------

    /// The steps of `rebase/three-peers.json` a client does not take, named (spec/RUNNERS.md).
    static let serverOnly = [
        "the authority adopting eve's log by replaying it, and refusing it once its facts are touched (Authority::adopt is a server's)",
        "compaction: the horizon moved to 2, a page at 0 answered with the snapshot and at 2 with the tail, the state at the head from facts (a server's)",
    ]

    static func id(_ k: Int) -> Id {
        var b = [UInt8](repeating: 0, count: 16)
        b[15] = UInt8(k)
        return Id(bytes: b)!
    }

    static func claim(_ r: Replica) -> String {
        let (n, h) = r.verifyAt()
        return "\(n) \(Hex.encode(h))"
    }

    static func posOf(_ r: Replica, _ k: Int) -> Value? { r.view.get("item", [.id(id(1)), .text("t\(k)")])?["pos"] }

    static func push(_ a: Authority, _ e: LogEntry) throws -> (Seq, [Change]) {
        if case .appended(let n, let f) = a.sequenceEntry(e) { return (n, f) }
        throw Failed("push: \(a.sequenceEntry(e))")
    }

    static func bodiesOf(_ m: Module) -> [String: Closure] {
        var bs = [String: Closure]()
        for (k, v) in Hash.closures(m) { bs[k] = v }
        return bs
    }

    /// `rebase_three_peers`, a client's half: sequencing, a whole replica out
    /// of order, one by facts alone, and the scenario — the rebase, its
    /// transitions, a duplicate delivery, a divergence healed by facts, and a
    /// peer alone confirming its own intents by a local commit.
    static func threePeers(_ v: Value) throws {
        let m = try Checks.moduleOf(try v.field("module"))
        let sch = m.schema
        let bodies = bodiesOf(m)
        let entries = try v.field("entries").list().map { e -> (Seq, LogEntry) in
            let r = try e.record()
            let f = Record(r.pairs.filter { $0.0 != "seq" })
            return (try e.field("seq").int(), try entryFromValue(.record(f)))
        }
        let facts = try v.field("facts").list().map { try $0.list().map(changeFromValue) }
        let finalHash = try v.field("final_hash").text()

        // 1. The transcript through an authority.
        let auth = Authority(sch, bodies)
        for (k, (n, e)) in entries.enumerated() {
            guard case .appended(let s, let f) = auth.sequenceEntry(e) else { throw Failed("entry \(n)") }
            try ensureEq(s, n, "the sequence of entry \(n)")
            try ensureEq(Checks.changesValue(f), Checks.changesValue(facts[k]), "facts at \(n)")
        }
        try ensureEq(Hex.encode(Hash.stateHash(auth.store)), finalHash, "the authority's hash")
        try ensureEq(auth.store.storeValue(), try v.field("final_store"), "the authority's store")

        // 2. A whole replica, given every entry in reverse order.
        func fresh() -> Replica { Replica.open(sch, bodies, MemoryStore.empty(sch), 0, []) }
        let whole = fresh()
        for (n, e) in entries.reversed() { whole.receive(n, e) }
        whole.settle()
        try ensureEq(whole.cursor, Int64(entries.count), "the whole replica's cursor")
        try ensureEq(Hex.encode(whole.verifyAt().1), finalHash, "the whole replica's hash")
        try ensure(whole.diverged.isEmpty, "the whole replica diverged")

        // 3. A replica with no closures, by facts alone.
        let factsOnly = Replica.open(sch, [:], MemoryStore.empty(sch), 0, [])
        for (n, e) in entries { factsOnly.receive(n, e) }
        try ensureEq(factsOnly.needs(), Array(1...Int64(entries.count)), "what it needs")
        for (k, ne) in entries.enumerated() { factsOnly.receiveFacts(ne.0, facts[k]) }
        factsOnly.settle()
        try ensureEq(Hex.encode(factsOnly.verifyAt().1), finalHash, "by facts alone")
        try ensure(factsOnly.diverged.isEmpty, "by facts, diverged")

        // 4. The scenario.
        func hashOf(_ name: String) -> [UInt8] { Hex.decode(bodies.first { $0.value.function.name == name }!.key)! }
        let hCreate = hashOf("create_playlist")
        let hAdd = hashOf("add_to_playlist")
        func ctx(_ who: String) -> Ctx { Ctx(who, "\(who)-session") }
        let now = [String: Value]()
        func addArgs(_ k: Int) -> [String: Value] { ["playlist_id": .id(id(1)), "track_id": .text("t\(k)")] }
        func mutate(_ r: Replica, _ i: Int, _ who: String, _ fh: [UInt8], _ autos: [String: Value], _ args: [String: Value]) throws -> LogEntry {
            let (e, why) = r.mutate(id(i), ctx(who), fh, autos, args)
            guard let x = e else { throw Failed("mutate \(i) refused: \(String(describing: why))") }
            return x
        }
        let a = Authority(sch, bodies)
        let alice = fresh()
        let bob = fresh()
        let e1 = try mutate(alice, 101, "alice", hCreate, ["id": .id(id(1))], ["name": .text(" Favorites ")])
        let (s1, _) = try push(a, e1)
        alice.ack(e1.id, s1)
        alice.settle()
        bob.receive(s1, e1)
        bob.settle()
        try ensureEq(s1, 1, "the playlist was sequenced first")
        try ensureEq(alice.view.get("playlist", [.id(id(1))])?["name"], .text("Favorites"), "alice's name was trimmed")
        try ensureEq(claim(alice), claim(bob), "alice and bob agree after step 1")
        let e2 = try mutate(bob, 102, "bob", hAdd, now, addArgs(1))
        let (s2, _) = try push(a, e2)
        bob.ack(e2.id, s2)
        bob.settle()
        let e3 = try mutate(bob, 103, "bob", hAdd, now, addArgs(2))
        let (s3, _) = try push(a, e3)
        bob.ack(e3.id, s3)
        bob.settle()
        _ = alice.takeChanges()
        let e9 = try mutate(alice, 109, "alice", hAdd, now, addArgs(9))
        try ensureEq(posOf(alice, 9), try v.field("alice_alone_pos_of_9"), "alone, alice's track is first on her view")
        if case .applied(let cs) = alice.takeChanges() {
            try ensure(cs.count == 1, "a local mutation reports its changes")
        } else {
            throw Failed("a local mutation reports its changes, not a rebuild")
        }
        try ensureEq(posOf(bob, 1), .int(1), "bob's first")
        try ensureEq(posOf(bob, 2), .int(2), "bob's second")
        alice.receive(s2, e2)
        alice.settle()
        alice.receive(s3, e3)
        alice.settle()
        if case .applied(let cs) = alice.takeChanges() {
            try ensure(cs.count == 6, "the rebase is reported as its transitions: \(cs.count)")
        } else {
            throw Failed("the rebase is reported as its transitions, not a rebuild")
        }
        try ensureEq(posOf(alice, 9), try v.field("alice_after_rebase_pos_of_9"), "after the rebase alice's track is third")
        try ensureEq(claim(alice), claim(bob), "alice's confirmed state is bob's")
        let (s9, f9) = try push(a, e9)
        alice.receiveFacts(s9, f9)
        alice.ack(e9.id, s9)
        alice.settle()
        bob.receive(s9, e9)
        bob.settle()
        try ensure(alice.pending.isEmpty, "nothing is pending on alice once acked")
        if case .rebuilt = alice.takeChanges() { throw Failed("with nothing pending the ack costs no rebuild") }
        try ensure(alice.view == alice.confirmed, "alice's view is her confirmed store")
        try ensure(f9.contains { if case .add(_, let r) = $0 { return r["pos"] == .int(3) } else { return false } }, "the authority's facts say pos 3 too")
        try ensureEq(claim(alice), claim(bob), "alice and bob")
        try ensureEq(Hex.encode(bob.verifyAt().1), finalHash, "three replicas, one hash")
        // a duplicate delivery changes nothing
        func snapshot(_ r: Replica) -> String { "\(r.cursor) \(Hex.encode(r.verifyAt().1)) \(r.view.storeValue().show()) \(r.pending.count) \(r.inbox.count)" }
        let before = snapshot(bob)
        bob.receive(s2, e2)
        bob.settle()
        try ensureEq(snapshot(bob), before, "a duplicate delivery is a no-op")
        // dave's build of add_to_playlist steps by two; facts catch it
        let key = Hex.encode(hAdd)
        let right = bodies[key]!
        let wrongBody = right.function.body.map { s -> Stmt in
            if case .insert(let t, .structE(let fs), let on) = s, let i = fs.names.firstIndex(of: "pos"), case .op(.add, let args) = fs.exprs[i] {
                var pairs = Array(zip(fs.names, fs.exprs))
                pairs[i].1 = .op(.add, [args[0], .lit(.int(2))])
                return .insert(t, .structE(ExprFields(pairs)), on)
            }
            return s
        }
        try ensure(wrongBody != right.function.body, "dave's build is the right one")
        var wrong = bodies
        var wf = right.function
        wf.body = wrongBody
        wrong[key] = Closure(function: wf, helpers: right.helpers)
        let dave = Replica.open(sch, wrong, MemoryStore.empty(sch), 0, [])
        for (k, ne) in entries.enumerated() { dave.receiveWith(ne.0, ne.1, facts[k]) }
        dave.settle()
        try ensureEq(dave.diverged, [2, 3, 4], "a divergent runtime is detected")
        try ensureEq(Hex.encode(dave.verifyAt().1), finalHash, "and healed by the facts")
        // eve has no server: she is her own authority
        let eve = fresh()
        let eveAuth = Authority(sch, bodies)
        _ = try mutate(eve, 201, "eve", hCreate, ["id": .id(id(2))], ["name": .text("Road")])
        _ = try mutate(eve, 202, "eve", hAdd, now, ["playlist_id": .id(id(2)), "track_id": .text("t5")])
        localCommit(eveAuth, eve)
        try ensure(eve.cursor == 2 && eve.pending.isEmpty && eve.view == eve.confirmed, "alone, eve confirms her own intents")
        try ensureBytes(eve.verifyAt().1, Hash.stateHash(eveAuth.store), "and her state is her authority's")
    }

    // trace -----------------------------------------------------------------

    /// A trace file read back (`trace::read`): its module and its steps.
    static func readTrace(_ v: Value) throws -> (Module, [(Value, Value)]) {
        let m = try Checks.moduleOf(try v.field("module"))
        let steps = try v.field("steps").list().map { (try $0.field("in"), try $0.field("out")) }
        return (m, steps)
    }

    /// `trace::replay` over a trace vector, a difference a `Failed` naming the step and fields.
    static func trace(_ v: Value, _ durable: () -> Durable = { KeptStore() }) throws {
        let (m, steps) = try readTrace(v)
        do {
            try replay(m, steps, durable())
        } catch let e as TraceError {
            throw Failed(e.what)
        }
    }

    /// Every trace of a corpus directory (`packages.trace-corpus`) replayed:
    /// how many, the first failure of each that did not, and how long it took.
    static func corpus(_ dir: String, _ durable: @escaping () -> Durable = { KeptStore() }) throws -> (Int, [String], Int) {
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir) else { throw Failed("\(dir) is not a directory") }
        let files = names.filter { $0.hasSuffix(".json") }.sorted()
        var failed = [String]()
        let t0 = nowMs()
        for f in files {
            do {
                try trace(try readJson(dir + "/" + f), durable)
            } catch let e as Failed {
                failed.append("\(f): \(e.message)")
            } catch {
                failed.append("\(f): BUG \(type(of: error)): \(error)")
            }
        }
        return (files.count, failed, Int(nowMs() - t0))
    }
}
