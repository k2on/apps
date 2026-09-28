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

/// A token `user:session`, as the tests' servers read one.
func loginOf(_ token: String?) -> Identity? {
    guard let t = token, let i = t.firstIndex(of: ":") else { return nil }
    return Identity(user: String(t[..<i]), session: String(t[t.index(after: i)...]))
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
        var seen: [Changes] = []
        s.subscribe { seen.append($0) }
        check("the session stands alone", s.status.alone && !s.status.linked && s.status.cursor == 0)

        let blank = s.mutate(name: "create_playlist", args: ["name": .text("   ")])
        check("a blank name is refused", blank == .refused("a playlist needs a name"), "\(blank.map { refusalText($0) } ?? "nil")")
        check("a refusal changes nothing", s.status.pending == 0 && seen.isEmpty && s.status.cursor == 0)
        check("the refusal is the last note", s.lastNote == "refused: a playlist needs a name")

        check("create_playlist", s.mutate(name: "create_playlist", args: createArgs("  Mine ")) == nil)
        let pls = try rows(s, "playlist")
        check("one playlist, trimmed", pls.count == 1 && pls.first?.field("name") == .text("Mine") && pls.first?.field("user_id") == .text("alice"))
        check("sequenced at once", s.status.cursor == 1 && s.status.pending == 0)
        guard let pid = pls.first?.field("id").asId() else { throw TestError("no playlist id") }

        check("add_to_playlist", s.mutate(name: "add_to_playlist", args: addArgs(pid, "t123")) == nil)
        check("the item is at pos 1", try positions(s) == [Pos("t123", 1)])
        check("cursor 2, nothing pending", s.status.cursor == 2 && s.status.pending == 0)
        let noop = s.mutate(name: "add_to_playlist", args: addArgs(pid, "t123"))
        let stillOne = try positions(s).count
        check("the same track again is a no-op, sequenced, not a refusal", noop == nil && s.status.cursor == 3 && stillOne == 1)
        let missing = s.mutate(name: "add_to_playlist", args: addArgs(Id.nil_, "t7"))
        check("a missing playlist is refused by its exists check", missing == .refused("playlist_id: no such playlist") && s.status.cursor == 3)
        let form = try s.validate(name: "create_playlist", partial: ["name": .text("  ")])
        check("the form validator says why, under the field", form.messages.count == 1 && form.messages[0].0 == "name" && form.messages[0].1 == "a playlist needs a name")
        check("and trims", try s.validate(name: "create_playlist", partial: ["name": .text(" x ")]).values["name"] == .text("x"))
        check("a query through the interpreter", try s.query(name: "items", args: ["playlist_id": .id(pid)]).asList().count == 1)

        let adds = seen.flatMap { pair -> [Change] in
            if case .applied(let cs) = pair { return cs }
            return []
        }
        check("subscribers were told each add, as Applied", adds.count == 2
              && adds.contains { if case .add("playlist", _) = $0 { return true }; return false }
              && adds.contains { if case .add("item", _) = $0 { return true }; return false }, "\(seen)")
        check("nothing was reported as Rebuilt", !seen.contains { if case .rebuilt = $0 { return true }; return false })

        s.verify()
        check("the authority here agrees", s.status.lastAgree == Agreement(seq: 3, ok: true))

        let (n1, h1) = s.stateHash()
        s.close()
        check("the replica file exists", FileManager.default.fileExists(atPath: dir.appendingPathComponent(ReplicaFile.fileName).path))

        let s2 = try Session.open(directory: dir, module: demo.bytes, user: "alice", server: nil)
        let (n2, h2) = s2.stateHash()
        check("reopened at the same cursor with the same hash", n1 == n2 && h1 == h2 && n2 == 3, "\(n1) \(n2)")
        check("the rows are back", try positions(s2) == [Pos("t123", 1)] && (try rows(s2, "playlist")).count == 1)
        let more = s2.mutate(name: "add_to_playlist", args: addArgs(pid, "t4"))
        let two = try positions(s2)
        check("and it goes on sequencing from there", more == nil && s2.status.cursor == 4 && two == [Pos("t123", 1), Pos("t4", 2)])
        s2.verify()
        check("the resumed authority agrees", s2.status.lastAgree?.ok == true)
        s2.close()

        var mismatched = false
        do {
            _ = try Session.open(directory: dir, module: demo.bytes, user: "alice", server: URL(string: "mem://x")!, dial: MemoryExchange(demo.module).dial)
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
        check("the native check refuses with the vector's text", refused == .refused("a playlist needs a name"), "\(refused.map { refusalText($0) } ?? "nil")")
        guard let pid = try rows(s, "playlist").first?.field("id").asId() else { throw TestError("no playlist") }
        check("add_to_playlist natively", s.mutate(name: "add_to_playlist", args: addArgs(pid, "t9")) == nil)
        var told: [Changes] = []
        s.subscribe { told.append($0) }
        check("the same item again is a no-op with nothing to report", s.mutate(name: "add_to_playlist", args: addArgs(pid, "t9")) == nil && told.isEmpty)
        check("the local authority — the interpreter — sequenced every entry the native procedures authored",
              s.status.cursor == 3 && s.status.pending == 0 && s.status.rejections == 0)
        check("and the rows agree", try positions(s) == [Pos("t9", 1)])
        check("a query natively", try s.query(name: "items", args: ["playlist_id": .id(pid)]).asList().map { $0.field("track_id") } == [.text("t9")])
        s.verify()
        check("verified", s.status.lastAgree?.ok == true)
        s.close()
        let s2 = try Session.open(directory: dir, module: demo.bytes, procedures: demo.procedures, user: "bob", server: nil)
        check("reopened through native replay", try positions(s2) == [Pos("t9", 1)] && s2.status.cursor == 3)
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
        let ex = MemoryExchange(demo.module)
        final class Recorder: LinkDriven {
            var client: Client
            var received: [ServerMsg] = []
            init(_ c: Client) { client = c }
            func linkOpened() { client.connected() }
            func linkClosed(_ reason: String?) { client.disconnected() }
            func linkReceived(_ m: ServerMsg) { received.append(m); client.recv(m) }
            func linkOutgoing() -> [ClientMsg] { return client.takeOutgoing() }
        }
        let rec = Recorder(Client(Replica.open(demo.module.schema, Hash.closures(demo.module), MemoryStore(schema: demo.module.schema), 0, []), .whole, token: "carol"))
        let link = Link(url: URL(string: "mem://x")!, dial: ex.dial, driven: rec)
        link.connect()
        link.pump(); link.pump()
        check("open after two pumps", link.isOpen && rec.client.linked)
        let fh = Hash.functionHash(Hash.closure(demo.module, demo.module.lookupFunction("create_playlist")!))
        let r = rec.client.mutate(Id(bytes: [UInt8](repeating: 1, count: 16))!, Ctx(user: "carol", session: "dev"), fh,
                                  ["id": .id(Id(bytes: [UInt8](repeating: 2, count: 16))!)], ["name": .text("Bytes")])
        check("authored", r.isSuccess)
        link.pump(); link.pump()
        check("the ack came back as a frame", rec.received.contains { if case .ack = $0 { return true }; return false }, "\(rec.received.count)")
        check("confirmed at 1", rec.client.replica.cursor == 1 && rec.client.replica.pending.isEmpty)
        check("no frame was undecodable", link.badFrames == 0 && ex.badFrames == 0)
        link.disconnect()
        check("disconnect tells the client", !rec.client.linked)
    }

    run("client/denied") {
        let ex = MemoryExchange(server: InProcessServer(Authority(demo.module.schema, Hash.closures(demo.module)), authenticate: { _ in nil }))
        let s = try Session.open(directory: try freshDir("denied"), module: demo.bytes, user: "nobody", server: URL(string: "mem://x")!, dial: ex.dial)
        for _ in 0..<3 { s.pump() }
        check("the server's denial is the status", s.status.denied == "not signed in" && !s.status.linked, "\(s.status)")
        check("and the note", s.lastNote == "denied: not signed in")
        s.close()
    }

    // §12.3 and §12.5 on the in-process server: an entry is held to the
    // login that pushed it unless the server knows the user owns the older
    // one, another user's entry is refused whatever the server knows, and
    // every refusal reaches the author as a sentence a screen can show.
    run("protocol/a-push-is-held-to-its-login-and-every-refusal-says-why") {
        let m = demo.module
        let create = Hash.functionHash(Hash.closure(m, m.lookupFunction("create_playlist")!))
        func serve(_ owns: Bool) -> InProcessServer {
            let sv = InProcessServer(Authority(m.schema, Hash.closures(m)), authenticate: loginOf)
            if owns { sv.withOwns { user, session in user == "alice" && session == "old" } }
            return sv
        }
        func entry(_ k: UInt8, _ actor: String, _ session: String, _ name: String) -> Entry {
            return Entry(id: rawId(k), actor: actor, session: session, fn: create, args: ["name": .text(name)], autos: ["id": .id(rawId(k))])
        }
        // The verdict each entry got, in order: the sequence, or the reason.
        func push(_ owns: Bool, _ entries: [Entry]) -> [String] {
            let sv = serve(owns)
            sv.recv(1, .hello(sub: Subscription(since: 0, mode: .whole), token: "alice:new", spec: specVersion))
            _ = sv.takeOutgoing()
            sv.recv(1, .push(entries: entries))
            return sv.takeOutgoing().flatMap { (_, f) -> [String] in
                switch f {
                case .ack(_, let seqs): return seqs.map { "ok \($0)" }
                case .reject(_, let why): return [why]
                default: return []
                }
            }
        }
        // Authored offline under an older login, pushed after signing in again.
        check("an older login is not yours by default", push(false, [entry(1, "alice", "old", "Mix")]) == ["not yours"])
        check("unless the server knows she owns it", push(true, [entry(1, "alice", "old", "Mix")]) == ["ok 1"])
        check("owning a session is only ever about the connection's own user", push(true, [entry(1, "bob", "old", "Mix")]) == ["not yours"])
        check("the login pushing is always its own", push(false, [entry(1, "alice", "new", "Mix")]) == ["ok 1"])
        let three = push(false, [entry(1, "alice", "new", "   "), entry(2, "alice", "new", "Mix"), entry(3, "alice", "new", "Mix")])
        check("a refusal is the author's own sentence; a duplicate under insert .on is sequenced, not refused",
              three == ["a playlist needs a name", "ok 1", "ok 2"], "\(three)")
        check("a constraint is named in a sentence",
              refusalText(.uniqueViolation("playlist", ["user_id", "name"])) == "playlist: another row has the same user_id, name"
              && refusalText(.missingParent("item", "playlist_id", "playlist")) == "item.playlist_id names no playlist"
              && refusalText(.notNull("playlist", "name")) == "playlist.name may not be empty"
              && refusalText(.stillReferenced("playlist", "item")) == "playlist: still referenced by item"
              && refusalText(.noSuchTable("nowhere")) == "no table nowhere"
              && refusalText(.malformedRow("item", "pos has the wrong type")) == "item: pos has the wrong type")
        let sv = serve(false)
        sv.recv(1, .hello(sub: Subscription(since: 0, mode: .whole), token: ":dev", spec: specVersion))
        check("a hello proving nobody is denied", { if case .denied("not signed in")? = sv.takeOutgoing().first?.1 { return true }; return false }())
        // Falsified: with the owns hook ignored, the second check fails.
    }

    // A screen asks, for any entry it authored, whether it is pending,
    // confirmed or rejected — and if rejected, the server's sentence.
    run("client/standing") {
        let m = demo.module
        let closures = Hash.closures(m)
        let create = Hash.functionHash(Hash.closure(m, m.lookupFunction("create_playlist")!))
        // A native create_playlist more lenient than the module's: a blank
        // name is authored here, and only the authority refuses it.
        let lenient = Procedure(function: m.lookupFunction("create_playlist")!, mutate: { ctx, autos, args, st in
            var a = args
            if case .text(let t)? = args["name"], t.trimmingCharacters(in: .whitespaces).isEmpty { a["name"] = .text("blank") }
            return try Eval.applyClosure(m.schema, closures[create]!, ctx, autos, a, st)
        }, query: nil)
        let server = InProcessServer(Authority(m.schema, closures), authenticate: loginOf)
        let ex = MemoryExchange(server: server)
        let url = URL(string: "mem://exchange/sync")!
        let dir = try freshDir("standing")
        // Authoring under the login "old" while the token proves "new".
        let a = try Session.open(directory: dir, module: demo.bytes, procedures: [(create, lenient)],
                                 user: "alice", session: "old", server: url, token: "alice:new", dial: ex.dial)
        func settle() { for _ in 0..<6 { a.pump() } }
        settle()
        guard case .success(let mine) = a.author(name: "create_playlist", args: createArgs("Mine")) else { throw TestError("not authored") }
        check("authored and not yet answered: pending", a.standing(mine) == .pending)
        settle()
        check("an older login's entry is rejected, and says why", a.standing(mine) == .rejected("not yours"), "\(a.standing(mine))")
        check("an id this peer never authored is unknown", a.standing(rawId(0x77)) == .unknown)

        server.withOwns { user, session in user == "alice" && session == "old" }
        guard case .success(let ours) = a.author(name: "create_playlist", args: createArgs("Ours")) else { throw TestError("not authored") }
        settle()
        check("once the server knows she owns the login, confirmed", a.standing(ours) == .confirmed && a.status.cursor == 1)

        guard case .success(let blank) = a.author(name: "create_playlist", args: createArgs("   ")) else { throw TestError("the lenient native refused") }
        check("the lenient native let a blank name through", a.standing(blank) == .pending)
        settle()
        check("the authority's refusal is the author's own sentence", a.standing(blank) == .rejected("a playlist needs a name"), "\(a.standing(blank))")
        check("and the view no longer holds it", try rows(a, "playlist").map { $0.field("name") } == [.text("Ours")])
        check("every verdict, in order, with its reason", a.rejections.map { $0.id } == [mine, blank]
              && a.rejections.map { $0.reason } == ["not yours", "a playlist needs a name"] && a.status.rejections == 2)
        check("a local refusal is answered at once, and is no entry", {
            if case .failure(let why) = a.author(name: "add_to_playlist", args: addArgs(Id.nil_, "t")) { return refusalText(why) == "playlist_id: no such playlist" }
            return false
        }())

        // Pending across a restart is still this peer's to ask about.
        a.goOffline()
        a.pump()
        guard case .success(let later) = a.author(name: "create_playlist", args: createArgs("Later")) else { throw TestError("not authored") }
        a.close()
        let a2 = try Session.open(directory: dir, module: demo.bytes, procedures: [(create, lenient)],
                                  user: "alice", session: "old", server: url, token: "alice:new", dial: ex.dial)
        check("reopened, the entry is still pending", a2.standing(later) == .pending)
        for _ in 0..<6 { a2.pump() }
        check("and confirmed once pushed", a2.standing(later) == .confirmed && a2.status.cursor == 2, "\(a2.standing(later)) \(a2.status)")
        a2.close()
    }

    // §11.2b A peer used for a while with no account, then signed in: all
    // of it is pushed as the person who signed in and accepted, and the rows
    // say whose they are. Pushed without signing in, every entry is refused.
    run("protocol/work-done-before-signing-in-becomes-the-signers") {
        let m = demo.module
        let bodies = Hash.closures(m)
        let create = Hash.functionHash(Hash.closure(m, m.lookupFunction("create_playlist")!))
        let add = Hash.functionHash(Hash.closure(m, m.lookupFunction("add_to_playlist")!))
        let pid = rawId(30)
        var local = Replica.open(m.schema, bodies, MemoryStore(schema: m.schema), 0, [])
        check("nobody authors", local.mutate(rawId(31), .nobody, create, ["id": .id(pid)], ["name": .text("Offline")]).isSuccess)
        for k in 0..<10 {
            _ = local.mutate(rawId(32 + UInt8(k)), .nobody, add, [:], ["playlist_id": .id(pid), "track_id": .text("t\(k)")])
        }
        check("eleven pending", local.pending.count == 11)
        // Everything the client says reaches the server; what came back.
        func run(_ c: Client) -> (acked: Int, refused: [String], server: InProcessServer) {
            var client = c
            let sv = InProcessServer(Authority(m.schema, bodies))
            client.connected()
            for f in client.takeOutgoing() { sv.recv(7, f) }
            var acked = 0
            var refused: [String] = []
            for (_, f) in sv.takeOutgoing() {
                if case .ack(let ids, _) = f { acked += ids.count }
                if case .reject(_, let why) = f { refused.append(why) }
            }
            return (acked, refused, sv)
        }
        var signed = Client.openClient(local, .whole, nil)
        signed.signIn(Ctx(user: "alice", session: "dev"), token: "alice")
        check("the optimistic view already says whose it is", signed.replica.view.getRow("playlist", [.id(pid)])?["user_id"] == .text("alice"))
        check("and so does every pending entry", signed.replica.pending.allSatisfy { $0.actor == "alice" && $0.session == "dev" })
        let yes = run(signed)
        check("signed in, all eleven are accepted", yes.acked == 11 && yes.refused.isEmpty, "\(yes.acked) \(yes.refused)")
        check("and the playlist is hers", yes.server.authority.store.scan("playlist").map { $0["user_id"] } == [.text("alice")])
        let no = run(Client.openClient(local, .whole, "alice"))
        check("unsigned, all eleven are refused as not yours", no.acked == 0 && no.refused.count == 11 && no.refused.allSatisfy { $0 == "not yours" })
        check("an entry authored as somebody is not re-stamped", {
            var r = Replica.open(m.schema, bodies, MemoryStore(schema: m.schema), 0, [])
            _ = r.mutate(rawId(50), Ctx(user: "bob", session: "b"), create, ["id": .id(rawId(51))], ["name": .text("Bob's")])
            r.signIn(Ctx(user: "alice", session: "dev"))
            return r.pending.map { $0.actor } == ["bob"]
        }())
    }

    // A session opened signed out authors as nobody, keeps its work pending
    // and on disk, and says nothing to any server; signing in makes it all
    // the signer's, connects, and every entry's standing follows it.
    run("client/signed-out-then-signed-in") {
        let m = demo.module
        let closures = Hash.closures(m)
        let create = Hash.functionHash(Hash.closure(m, m.lookupFunction("create_playlist")!))
        let lenient = Procedure(function: m.lookupFunction("create_playlist")!, mutate: { ctx, autos, args, st in
            var a = args
            if case .text(let t)? = args["name"], t.trimmingCharacters(in: .whitespaces).isEmpty { a["name"] = .text("blank") }
            return try Eval.applyClosure(m.schema, closures[create]!, ctx, autos, a, st)
        }, query: nil)
        let ex = MemoryExchange(m)
        let url = URL(string: "mem://exchange/sync")!
        let dir = try freshDir("signed-out")
        let s = try Session.openSignedOut(directory: dir, module: demo.bytes, procedures: [(create, lenient)], server: url, dial: ex.dial)
        for _ in 0..<4 { s.pump() }
        check("signed out: no connection", !s.status.signedIn && !s.status.linked && s.status.link == "idle", "\(s.status)")
        guard case .success(let mine) = s.author(name: "create_playlist", args: createArgs("Mine")) else { throw TestError("not authored") }
        guard let pid = try rows(s, "playlist").first?.field("id").asId() else { throw TestError("no playlist") }
        check("the row is nobody's for now", try rows(s, "playlist").first?.field("user_id") == .text(""))
        guard case .success(let item) = s.author(name: "add_to_playlist", args: addArgs(pid, "t1")) else { throw TestError("not authored") }
        guard case .success(let blank) = s.author(name: "create_playlist", args: createArgs("  ")) else { throw TestError("not authored") }
        s.goOnline()
        for _ in 0..<4 { s.pump() }
        check("pending, and going online signed out does nothing", s.status.pending == 3 && !s.status.linked
              && [mine, item, blank].allSatisfy { s.standing($0) == .pending })
        s.close()

        // Reopened, still signed out: the work was written down.
        let s2 = try Session.openSignedOut(directory: dir, module: demo.bytes, procedures: [(create, lenient)], server: url, dial: ex.dial)
        check("reopened with the work pending", try s2.status.pending == 3 && s2.standing(item) == .pending && (try positions(s2)) == [Pos("t1", 1)])
        s2.signIn(user: "alice")
        check("signing in makes the rows hers at once", try rows(s2, "playlist").map { $0.field("user_id") }.allSatisfy { $0 == .text("alice") }
              && s2.status.signedIn && s2.ctx == Ctx(user: "alice", session: "dev"))
        for _ in 0..<6 { s2.pump() }
        check("and pushed: two confirmed, one rejected with the server's sentence",
              s2.standing(mine) == .confirmed && s2.standing(item) == .confirmed && s2.standing(blank) == .rejected("a playlist needs a name"),
              "\(s2.standing(mine)) \(s2.standing(item)) \(s2.standing(blank))")
        check("the authority has them as hers", ex.server.authority.store.scan("playlist").map { $0["user_id"] } == [.text("alice")]
              && s2.status.cursor == 2 && s2.status.pending == 0)
        check("the verdict is listed with its reason", s2.rejections.map { $0.reason } == ["a playlist needs a name"])
        s2.close()

        // Signed out again elsewhere, and this time the app is opened signed
        // in: what nobody authored is the signer's all the same.
        let dir3 = try freshDir("signed-out-reopened-in")
        let s3 = try Session.openSignedOut(directory: dir3, module: demo.bytes, server: url, dial: ex.dial)
        guard case .success(let later) = s3.author(name: "create_playlist", args: createArgs("Later")) else { throw TestError("not authored") }
        s3.close()
        let s4 = try Session.open(directory: dir3, module: demo.bytes, user: "bob", server: url, dial: ex.dial)
        for _ in 0..<6 { s4.pump() }
        check("opened signed in, nobody's work is pushed as theirs", s4.standing(later) == .confirmed
              && ex.server.authority.store.scan("playlist").contains { $0["name"] == .text("Later") && $0["user_id"] == .text("bob") }, "\(s4.standing(later))")
        s4.close()
    }

    run("client/converge") {
        let ex = MemoryExchange(demo.module)
        let url = URL(string: "mem://exchange/sync")!
        let a = try Session.open(directory: try freshDir("converge-a"), module: demo.bytes, user: "alice", server: url, dial: ex.dial)
        let b = try Session.open(directory: try freshDir("converge-b"), module: demo.bytes, procedures: demo.procedures, user: "bob", server: url, dial: ex.dial)
        func settle() { for _ in 0..<6 { a.pump(); b.pump() } }
        settle()
        check("both linked", a.status.linked && b.status.linked && !a.status.alone, "\(a.status) \(b.status)")

        var aSeen: [Changes] = []
        var bSeen: [Changes] = []
        a.subscribe { aSeen.append($0) }
        b.subscribe { bSeen.append($0) }

        check("A creates a playlist", a.mutate(name: "create_playlist", args: createArgs("Shared")) == nil)
        check("pending until acked", a.status.pending == 1)
        settle()
        check("both at 1", a.status.cursor == 1 && b.status.cursor == 1 && a.status.pending == 0)
        check("B was told, as Applied", bSeen.contains { if case .applied(let cs) = $0 { return cs.contains { $0.table == "playlist" } }; return false })
        guard let pid = try rows(b, "playlist").first?.field("id").asId() else { throw TestError("B has no playlist") }

        a.goOffline()
        a.pump()
        check("A is offline", !a.status.linked && a.status.link == "idle")

        check("B adds meanwhile", b.mutate(name: "add_to_playlist", args: addArgs(pid, "t1")) == nil)
        settle()
        check("B's item is confirmed at 2", b.status.cursor == 2 && b.status.pending == 0)
        check("A has not heard", a.status.cursor == 1)

        aSeen = []
        check("A adds alone", a.mutate(name: "add_to_playlist", args: addArgs(pid, "t2")) == nil)
        check("A's optimistic view puts it first", try positions(a) == [Pos("t2", 1)] && a.status.pending == 1)

        a.goOnline()
        settle()
        check("both at 3 with nothing pending", a.status.cursor == 3 && b.status.cursor == 3
              && a.status.pending == 0 && b.status.pending == 0, "\(a.status) \(b.status)")
        let after = try positions(a)
        check("A's item landed last", after == [Pos("t1", 1), Pos("t2", 2)], "\(after)")
        check("B sees the same", try positions(b) == [Pos("t1", 1), Pos("t2", 2)])
        check("the confirmed hashes agree", a.stateHash() == b.stateHash())
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
        check("A reopens at 3 with the rows", a2.status.cursor == 3 && reopened == [Pos("t1", 1), Pos("t2", 2)])
        a2.close()
    }
}
