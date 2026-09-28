import Foundation
import ArkDB
import ArkDBClient

// ArkDBClient against the demo module as the Swift domain emits it: a
// session alone through the interpreter and through the native procedures,
// the link's backoff, and two sessions converging through an in-process
// authority. Same harness as the vectors: `check`, `run`.

/// The demo as the Swift domain emits it (Tests/Demo), with its native
/// procedures and the vector's hash to hold it to.
func demoModule() throws -> (bytes: [UInt8], hash: String, module: Module, procedures: [(FnHash, Procedure)]) {
    let obj = try loadJSON(vectors.appendingPathComponent("module").appendingPathComponent("demo.json"))
    let a = demoAuthored()
    let m = try Decode.fromValue(try Canon.decode(a.bytes))
    return (a.bytes, try string(obj, "hash"), m, a.procedures)
}

/// An empty directory under the temporary directory.
func freshDir(_ name: String) throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("arkdb-client-tests").appendingPathComponent(name)
    try? FileManager.default.removeItem(at: dir)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
}

func rows(_ s: Session, _ table: String) throws -> [Value] {
    return try s.run { db in db.select(Plan.from(table)) }.asList()
}

/// A playlist item as the tests read it: which track, at which position.
struct Pos: Equatable, CustomStringConvertible {
    let track: String
    let pos: Int64
    init(_ track: String, _ pos: Int64) { self.track = track; self.pos = pos }
    var description: String { return "\(track)@\(pos)" }
}

func positions(_ s: Session) throws -> [Pos] {
    return try s.run { db in db.select(Plan.from("item").orderBy("pos", Dir.asc)) }.asList()
        .map { Pos($0.field("track_id").asText(), $0.field("pos").asInt()) }
}

// MARK: - a transport that never opens

final class FailingTransport: LinkTransport {
    func open(_ events: @escaping (LinkEvent) -> Void) { events(.closed("refused")) }
    func send(_ frame: [UInt8]) {}
    func close() {}
}

final class CountingDial {
    var attempts = 0
    func dial(_ url: URL) -> LinkTransport {
        attempts += 1
        return FailingTransport()
    }
}

final class Silent: LinkDriven {
    var opened = 0
    var closed = 0
    func linkOpened() { opened += 1 }
    func linkClosed(_ reason: String?) { closed += 1 }
    func linkReceived(_ m: ServerMsg) {}
    func linkOutgoing() -> [ClientMsg] { return [] }
}

// MARK: - the tests

func clientTests() throws {
    let demo = try demoModule()

    run("client/authored-demo-is-the-vector's-module") {
        check("the Swift demo's bytes hash to the module vector's hash", Hex.encode(Hash.moduleHash(demo.module)) == demo.hash)
        check("every native procedure's hash is a closure of the module", Set(demo.procedures.map { $0.0 }) == Set(Hash.closures(demo.module).keys))
    }

    run("client/alone") {
        let dir = try freshDir("alone")
        let s = try Session.open(directory: dir, module: demo.bytes, user: "alice", server: nil)
        var seen: [(ScopeName, Changes)] = []
        s.subscribe { seen.append(($0, $1)) }
        check("the session stands alone", s.status.alone && !s.status.linked && s.status.cursors["demo"] == 0)

        let blank = s.mutate(name: "create_playlist", args: ["name": .text("   ")])
        check("a blank name is refused", blank == .refused("a playlist needs a name"), "\(blank.map { $0.text } ?? "nil")")
        check("a refusal changes nothing", s.status.pending == 0 && seen.isEmpty && s.status.cursors["demo"] == 0)
        check("the refusal is the last note", s.lastNote == "refused: a playlist needs a name")

        check("create_playlist", s.mutate(name: "create_playlist", args: createArgs("  Mine ")) == nil)
        let pls = try rows(s, "playlist")
        check("one playlist, trimmed", pls.count == 1 && pls.first?.field("name") == .text("Mine") && pls.first?.field("user_id") == .text("alice"))
        check("sequenced at once", s.status.cursors["demo"] == 1 && s.status.pending == 0)
        guard let pid = pls.first?.field("id").asId() else { throw TestError("no playlist id") }

        check("add_to_playlist", s.mutate(name: "add_to_playlist", args: addArgs(pid, "t123")) == nil)
        check("the item is at pos 1", try positions(s) == [Pos("t123", 1)])
        check("cursor 2, nothing pending", s.status.cursors["demo"] == 2 && s.status.pending == 0)
        let noop = s.mutate(name: "add_to_playlist", args: addArgs(pid, "t123"))
        let stillOne = try positions(s).count
        check("the same track again is a no-op, sequenced, not a refusal", noop == nil && s.status.cursors["demo"] == 3 && stillOne == 1)
        let missing = s.mutate(name: "add_to_playlist", args: addArgs(Id.nil_, "t7"))
        check("a missing playlist is refused by its exists check", missing == .refused("playlist_id: no such playlist") && s.status.cursors["demo"] == 3)
        let form = try s.validate(name: "create_playlist", partial: ["name": .text("  ")])
        check("the form validator says why, under the field", form.messages.count == 1 && form.messages[0].0 == "name" && form.messages[0].1 == "a playlist needs a name")
        check("and trims", try s.validate(name: "create_playlist", partial: ["name": .text(" x ")]).values["name"] == .text("x"))
        check("a query through the interpreter", try s.query(name: "items", args: ["playlist_id": .id(pid)]).asList().count == 1)

        let adds = seen.flatMap { pair -> [Change] in
            if case .applied(let cs) = pair.1 { return cs }
            return []
        }
        check("subscribers were told each add, as Applied", adds.count == 2
              && adds.contains { if case .add("playlist", _) = $0 { return true }; return false }
              && adds.contains { if case .add("item", _) = $0 { return true }; return false }, "\(seen)")
        check("nothing was reported as Rebuilt", !seen.contains { if case .rebuilt = $0.1 { return true }; return false })

        s.verify()
        check("the authority here agrees", s.status.lastAgree == Agreement(scope: "demo", seq: 3, ok: true))

        guard let (n1, h1) = s.stateHash("demo") else { throw TestError("no hash") }
        s.close()
        check("the replica file exists", FileManager.default.fileExists(atPath: dir.appendingPathComponent("demo.replica").path))

        let s2 = try Session.open(directory: dir, module: demo.bytes, user: "alice", server: nil)
        guard let (n2, h2) = s2.stateHash("demo") else { throw TestError("no hash after reopen") }
        check("reopened at the same cursor with the same hash", n1 == n2 && h1 == h2 && n2 == 3, "\(n1) \(n2)")
        check("the rows are back", try positions(s2) == [Pos("t123", 1)] && (try rows(s2, "playlist")).count == 1)
        let more = s2.mutate(name: "add_to_playlist", args: addArgs(pid, "t4"))
        let two = try positions(s2)
        check("and it goes on sequencing from there", more == nil && s2.status.cursors["demo"] == 4 && two == [Pos("t123", 1), Pos("t4", 2)])
        s2.verify()
        check("the resumed authority agrees", s2.status.lastAgree?.ok == true)
        s2.close()

        var mismatched = false
        do {
            _ = try Session.open(directory: dir, module: demo.bytes, user: "alice", server: URL(string: "mem://x")!, dial: MemoryExchange().dial)
        } catch SessionError.modeMismatch {
            mismatched = true
        }
        check("a directory opened alone refuses to be opened against a server", mismatched)
    }

    run("client/alone-native") {
        let dir = try freshDir("alone-native")
        let s = try Session.open(directory: dir, module: demo.bytes, procedures: demo.procedures, user: "bob", server: nil)
        check("create_playlist natively, by name", s.mutate(name: "create_playlist", args: createArgs("Gen")) == nil)
        let refused = s.mutate(name: "create_playlist", args: createArgs(""))
        check("the native check refuses with the vector's text", refused == .refused("a playlist needs a name"), "\(refused.map { $0.text } ?? "nil")")
        guard let pid = try rows(s, "playlist").first?.field("id").asId() else { throw TestError("no playlist") }
        check("add_to_playlist natively", s.mutate(name: "add_to_playlist", args: addArgs(pid, "t9")) == nil)
        var told: [Changes] = []
        s.subscribe { told.append($1) }
        check("the same item again is a no-op with nothing to report", s.mutate(name: "add_to_playlist", args: addArgs(pid, "t9")) == nil && told.isEmpty)
        check("the local authority — the interpreter — sequenced every entry the native procedures authored",
              s.status.cursors["demo"] == 3 && s.status.pending == 0 && s.status.rejections == 0)
        check("and the rows agree", try positions(s) == [Pos("t9", 1)])
        check("a query natively", try s.query(name: "items", args: ["playlist_id": .id(pid)]).asList().map { $0.field("track_id") } == [.text("t9")])
        s.verify()
        check("verified", s.status.lastAgree?.ok == true)
        s.close()
        let s2 = try Session.open(directory: dir, module: demo.bytes, procedures: demo.procedures, user: "bob", server: nil)
        check("reopened through native replay", try positions(s2) == [Pos("t9", 1)] && s2.status.cursors["demo"] == 3)
        s2.close()
    }

    run("client/link-backoff") {
        let d = CountingDial()
        let quiet = Silent()
        let link = Link(url: URL(string: "ws://nowhere/sync")!, dial: d.dial, driven: quiet)
        let t0 = Date(timeIntervalSince1970: 1_000_000)
        link.pump(now: t0)
        check("nothing dials before connect()", d.attempts == 0 && link.state == .idle)
        link.connect()
        link.pump(now: t0)
        check("connect dials at once", d.attempts == 1)
        link.pump(now: t0)
        check("a refused socket waits half a second", link.state == .waiting(until: t0.addingTimeInterval(0.5)), "\(link.state)")
        var now = t0
        var gaps: [TimeInterval] = []
        var lastDial = t0
        var attempts = d.attempts
        for _ in 0..<3000 {
            now = now.addingTimeInterval(0.05)
            link.pump(now: now)
            if d.attempts > attempts {
                attempts = d.attempts
                gaps.append(now.timeIntervalSince(lastDial))
                lastDial = now
            }
        }
        // A gap is the backoff plus a pump or two: the refusal is read on the
        // pump after the dial, and the wait starts there.
        let expected: [TimeInterval] = [0.5, 1, 2, 4, 8, 16, 30, 30, 30]
        let close = gaps.count >= expected.count && zip(gaps, expected).allSatisfy { abs($0 - $1) <= 0.11 }
        check("the gaps double from 0.5 s and stop at 30 s", close, "\(gaps)")
        check("the client was never told it was linked", quiet.opened == 0 && quiet.closed == 0)
        link.disconnect()
        let before = d.attempts
        link.pump(now: now.addingTimeInterval(100))
        check("disconnect stops the dialling", d.attempts == before && link.state == .idle)
    }

    run("client/link-frames") {
        // A real exchange, driven by hand: the hello goes out as bytes and the
        // batch comes back as bytes, through Link's codecs.
        let ex = MemoryExchange()
        ex.host(Authority(demo.module.schema, "demo", Hash.closures(demo.module)))
        final class Recorder: LinkDriven {
            var client: Client
            var received: [ServerMsg] = []
            init(_ c: Client) { client = c }
            func linkOpened() { client.connected() }
            func linkClosed(_ reason: String?) { client.disconnected() }
            func linkReceived(_ m: ServerMsg) { received.append(m); client.recv(m) }
            func linkOutgoing() -> [ClientMsg] { return client.takeOutgoing() }
        }
        var c = Client(schema: demo.module.schema, token: "carol")
        c.subscribe(.whole, Replica.open(demo.module.schema, "demo", Hash.closures(demo.module), MemoryStore(schema: demo.module.schema), 0, []))
        let rec = Recorder(c)
        let link = Link(url: URL(string: "mem://x")!, dial: ex.dial, driven: rec)
        link.connect()
        link.pump(); link.pump()
        check("open after two pumps", link.isOpen && rec.client.linked)
        let fh = Hash.functionHash(Hash.closure(demo.module, demo.module.lookupFunction("create_playlist")!))
        let r = rec.client.mutate("demo", Id(bytes: [UInt8](repeating: 1, count: 16))!, Ctx(user: "carol", session: "dev"), fh,
                                  ["id": .id(Id(bytes: [UInt8](repeating: 2, count: 16))!)], ["name": .text("Bytes")])
        check("authored", r.isSuccess)
        link.pump(); link.pump()
        check("the ack came back as a frame", rec.received.contains { if case .ack = $0 { return true }; return false }, "\(rec.received.count)")
        check("confirmed at 1", rec.client.scopes["demo"]?.replica.cursor == 1 && rec.client.scopes["demo"]?.replica.pending.isEmpty == true)
        check("no frame was undecodable", link.badFrames == 0 && ex.badFrames == 0)
        link.disconnect()
        check("disconnect tells the client", !rec.client.linked)
    }

    run("client/denied") {
        let ex = MemoryExchange(server: InProcessServer(authenticate: { _ in nil }))
        ex.host(Authority(demo.module.schema, "demo", Hash.closures(demo.module)))
        let s = try Session.open(directory: try freshDir("denied"), module: demo.bytes, user: "nobody", server: URL(string: "mem://x")!, dial: ex.dial)
        for _ in 0..<3 { s.pump() }
        check("the server's denial is the status", s.status.denied == "not signed in" && !s.status.linked, "\(s.status)")
        check("and the note", s.lastNote == "denied: not signed in")
        s.close()
    }

    run("client/converge") {
        let ex = MemoryExchange()
        ex.host(Authority(demo.module.schema, "demo", Hash.closures(demo.module)))
        let url = URL(string: "mem://exchange/sync")!
        let a = try Session.open(directory: try freshDir("converge-a"), module: demo.bytes, user: "alice", server: url, dial: ex.dial)
        let b = try Session.open(directory: try freshDir("converge-b"), module: demo.bytes, procedures: demo.procedures, user: "bob", server: url, dial: ex.dial)
        func settle() { for _ in 0..<6 { a.pump(); b.pump() } }
        settle()
        check("both linked", a.status.linked && b.status.linked && !a.status.alone, "\(a.status) \(b.status)")

        var aSeen: [Changes] = []
        var bSeen: [Changes] = []
        a.subscribe { aSeen.append($1) }
        b.subscribe { bSeen.append($1) }

        check("A creates a playlist", a.mutate(name: "create_playlist", args: createArgs("Shared")) == nil)
        check("pending until acked", a.status.pending == 1)
        settle()
        check("both at 1", a.status.cursors["demo"] == 1 && b.status.cursors["demo"] == 1 && a.status.pending == 0)
        check("B was told, as Applied", bSeen.contains { if case .applied(let cs) = $0 { return cs.contains { $0.table == "playlist" } }; return false })
        guard let pid = try rows(b, "playlist").first?.field("id").asId() else { throw TestError("B has no playlist") }

        a.goOffline()
        a.pump()
        check("A is offline", !a.status.linked && a.status.link == "idle")

        check("B adds meanwhile", b.mutate(name: "add_to_playlist", args: addArgs(pid, "t1")) == nil)
        settle()
        check("B's item is confirmed at 2", b.status.cursors["demo"] == 2 && b.status.pending == 0)
        check("A has not heard", a.status.cursors["demo"] == 1)

        aSeen = []
        check("A adds alone", a.mutate(name: "add_to_playlist", args: addArgs(pid, "t2")) == nil)
        check("A's optimistic view puts it first", try positions(a) == [Pos("t2", 1)] && a.status.pending == 1)

        a.goOnline()
        settle()
        check("both at 3 with nothing pending", a.status.cursors["demo"] == 3 && b.status.cursors["demo"] == 3
              && a.status.pending == 0 && b.status.pending == 0, "\(a.status) \(b.status)")
        let after = try positions(a)
        check("A's item landed last", after == [Pos("t1", 1), Pos("t2", 2)], "\(after)")
        check("B sees the same", try positions(b) == [Pos("t1", 1), Pos("t2", 2)])
        check("the confirmed hashes agree", a.stateHash("demo")! == b.stateHash("demo")!)
        check("A was told to rebuild", aSeen.contains { if case .rebuilt = $0 { return true }; return false }, "\(aSeen)")
        check("no rejections", a.status.rejections == 0 && b.status.rejections == 0)

        a.verify(); b.verify()
        settle()
        check("the authority agrees with both", a.status.lastAgree?.ok == true && b.status.lastAgree?.ok == true, "\(a.status.lastAgree.map { "\($0)" } ?? "nil")")

        // A server restart: every socket dropped, both reconnect with backoff.
        ex.dropAll()
        settle()
        check("both unlinked and waiting", !a.status.linked && !b.status.linked && a.status.link == "waiting")
        let later = Date().addingTimeInterval(1)
        for _ in 0..<6 { a.pump(now: later); b.pump(now: later) }
        check("both back", a.status.linked && b.status.linked, "\(a.status.link) \(b.status.link)")

        a.close(); b.close()
        let a2 = try Session.open(directory: a.directory, module: demo.bytes, user: "alice", server: url, dial: ex.dial)
        let reopened = try positions(a2)
        check("A reopens at 3 with the rows", a2.status.cursors["demo"] == 3 && reopened == [Pos("t1", 1), Pos("t2", 2)])
        a2.close()
    }
}
