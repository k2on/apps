import Foundation
import ArkDB

// A runner over ../../spec/vectors, resolved relative to this file. It is an
// executable rather than XCTest because the nix devshell has no XCTest; it
// prints one line per check and exits non-zero if anything failed.

var passed = 0
var failed = 0
var currentCase = ""

func check(_ what: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
    if ok {
        passed += 1
    } else {
        failed += 1
        let d = detail()
        print("FAIL [\(currentCase)] \(what)" + (d.isEmpty ? "" : ": " + d))
    }
}

func run(_ name: String, _ body: () throws -> Void) {
    currentCase = name
    do {
        try body()
    } catch {
        failed += 1
        print("FAIL [\(name)] threw \(error)")
    }
}

struct TestError: Error, CustomStringConvertible {
    let description: String
    init(_ d: String) { description = d }
}

// MARK: - The vectors' JSON

func toValue(_ j: Any) throws -> Value {
    if j is NSNull { return .null }
    if let s = j as? String { return .text(s) }
    if let a = j as? [Any] { return .list(try a.map(toValue)) }
    if let d = j as? [String: Any] {
        if d.count == 1 {
            if let s = d["$int"] as? String {
                guard let n = Int64(s) else { throw TestError("bad $int \(s)") }
                return .int(n)
            }
            if let s = d["$bytes"] as? String {
                guard let b = Hex.decode(s) else { throw TestError("bad $bytes \(s)") }
                return .bytes(b)
            }
            if let s = d["$id"] as? String {
                guard let i = Id(uuid: s) else { throw TestError("bad $id \(s)") }
                return .id(i)
            }
        }
        var m: [String: Value] = [:]
        for (k, v) in d { m[k] = try toValue(v) }
        return .record(m)
    }
    if let b = j as? Bool { return .bool(b) }
    throw TestError("unexpected JSON \(type(of: j))")
}

func loadJSON(_ url: URL) throws -> [String: Any] {
    let data = try Data(contentsOf: url)
    guard let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw TestError("not an object: \(url.path)") }
    return obj
}

func value(_ obj: [String: Any], _ key: String) throws -> Value {
    guard let j = obj[key] else { throw TestError("missing \(key)") }
    return try toValue(j)
}

func string(_ obj: [String: Any], _ key: String) throws -> String {
    guard let s = obj[key] as? String else { throw TestError("missing string \(key)") }
    return s
}

func hexBytes(_ obj: [String: Any], _ key: String) throws -> [UInt8] {
    guard let b = Hex.decode(try string(obj, key)) else { throw TestError("bad hex in \(key)") }
    return b
}

/// A store from the vectors' `{table: [rows]}` shape.
func storeOf(_ v: Value, _ schema: Schema) throws -> MemoryStore {
    let st = MemoryStore(schema: schema)
    guard case .record(let m) = v else { throw TestError("store is not a struct") }
    for (t, rows) in m {
        guard case .list(let rs) = rows else { throw TestError("rows of \(t) are not a list") }
        for r in rs {
            guard case .record(let row) = r else { throw TestError("a row of \(t) is not a struct") }
            st.applyChange(.add(t, row))
        }
    }
    return st
}

func files(_ dir: URL) throws -> [URL] {
    let fm = FileManager.default
    return try fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        .filter { $0.pathExtension == "json" }
        .sorted { $0.lastPathComponent < $1.lastPathComponent }
}

let vectors = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("spec").appendingPathComponent("vectors")

// MARK: - codec/

func codecChecks(_ obj: [String: Any]) throws -> [String] {
    var problems: [String] = []
    let v = try value(obj, "value")
    let bytes = try hexBytes(obj, "bytes")
    let enc = Canon.encode(v)
    if enc != bytes { problems.append("encode gave \(Hex.encode(enc)), expected \(Hex.encode(bytes))") }
    let dec = try Canon.decode(bytes)
    if dec != v { problems.append("decode gave \(dec.brief), expected \(v.brief)") }
    if !Canon.roundTrip(bytes) { problems.append("bytes do not round-trip") }
    return problems
}

func codecVectors() throws {
    for f in try files(vectors.appendingPathComponent("codec")) {
        run("codec/" + f.lastPathComponent) {
            let problems = try codecChecks(try loadJSON(f))
            check("value <-> bytes", problems.isEmpty, problems.joined(separator: "; "))
        }
    }
    for f in try files(vectors.appendingPathComponent("codec/falsify")) {
        run("codec/falsify/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            check("is marked to fail", (obj["expect"] as? String) == "fail")
            var failedAsItMust = false
            do {
                failedAsItMust = !(try codecChecks(obj)).isEmpty
            } catch {
                failedAsItMust = true
            }
            check("the runner fails the falsified vector", failedAsItMust)
        }
    }
    // Inputs to refuse: one per rule of the decoder.
    run("codec/refusals") {
        func refuses(_ hex: String, _ want: DecodeError) {
            let b = Hex.decode(hex)!
            do {
                let v = try Canon.decode(b)
                check("refuse \(hex)", false, "decoded to \(v.brief)")
            } catch let e as DecodeError {
                check("refuse \(hex) with \(want)", e == want, "got \(e)")
            } catch {
                check("refuse \(hex)", false, "\(error)")
            }
        }
        refuses("1801", .nonCanonicalHead) // 1 with a one-byte head
        refuses("1900ff", .nonCanonicalHead) // 255 with a two-byte head
        refuses("5f", .indefiniteLength)
        refuses("ff", .indefiniteLength)
        refuses("a262616101616202", .unsortedKeys)
        refuses("a2616101616102", .duplicateKey)
        refuses("a10101", .nonTextKey)
        refuses("c101", .badTag)
        refuses("d8254100", .badId)
        refuses("f93c00", .float)
        refuses("f7", .badSimple)
        refuses("f800", .badSimple)
        refuses("61ff", .badUtf8)
        refuses("63eda080", .badUtf8) // a surrogate, encoded
        refuses("1b8000000000000000", .intOutOfRange)
        refuses("3b8000000000000000", .intOutOfRange)
        refuses("0000", .trailing)
        refuses("6261", .truncated)
        refuses("8201", .truncated)
    }
}

// MARK: - order/

func orderVectors() throws {
    for f in try files(vectors.appendingPathComponent("order")) {
        run("order/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            let input = try value(obj, "input").asList()
            let sorted = try value(obj, "sorted").asList()
            let mine = input.sorted { compareValue($0, $1) < 0 }
            check("sorted order", mine == sorted, "got \(Value.list(mine).brief)")
            check("e+combining acute is not e-acute", Value.text("e\u{301}") != Value.text("\u{e9}"))
            check("fullwidth tilde sorts before U+1F3B5", compareValue(.text("\u{FF5E}"), .text("\u{1F3B5}")) < 0)
        }
    }
}

// MARK: - hash/

func hashVectors(_ schema: Schema) throws {
    for f in try files(vectors.appendingPathComponent("hash")) {
        run("hash/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            let st = try storeOf(try value(obj, "store"), schema)
            let want = try string(obj, "hash")
            check("state hash", Hex.encode(Hash.stateHash(st)) == want, Hex.encode(Hash.stateHash(st)))
            // Empty tables contribute their name: a schema with one more
            // (empty) table hashes differently, and an empty store is not the
            // hash of an empty list.
            let empty = MemoryStore(schema: schema)
            var wider = schema
            wider.scopes[0].tables.append(Table("nothing", columns: [Column("id", .int)], key: ["id"]))
            check("an empty store hashes its tables' names", Hash.stateHash(empty) != Sha256.hash(Canon.encode(.list([]))))
            check("an empty table moves the hash", Hash.stateHash(MemoryStore(schema: wider)) != Hash.stateHash(empty))
            check("empty demo store hash", Hex.encode(Hash.stateHash(empty)) == Sha256.hex(Canon.encode(.list(schema.tableNames.map { .list([.text($0), .list([])]) }))))
            print("NOTE [hash] empty demo-schema store hashes to \(Hex.encode(Hash.stateHash(empty)))")
            check("sha256 of empty input", Sha256.hex([]) == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
            check("sha256 of abc", Sha256.hex(Array("abc".utf8)) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        }
    }
}

// MARK: - module/

func moduleVectors() throws -> Module {
    var found: Module? = nil
    for f in try files(vectors.appendingPathComponent("module")) {
        run("module/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            let mv = try value(obj, "module")
            let bytes = try hexBytes(obj, "bytes")
            let m = try Decode.fromValue(mv)
            check("decode of the bytes is the value", try Canon.decode(bytes) == mv)
            check("decode then encode is the value", Encode.toValue(m) == mv)
            check("decode then encode is the bytes", Canon.encode(Encode.toValue(m)) == bytes)
            check("module hash", Hex.encode(Hash.moduleHash(m)) == (try string(obj, "hash")))
            check("normalising again changes nothing", Encode.normalizeModule(m) == m)
            found = m
        }
    }
    guard let m = found else { throw TestError("no module vector") }
    return m
}

// MARK: - verify/

func verifyVectors() throws {
    for f in try files(vectors.appendingPathComponent("verify")) {
        run("verify/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            let mv = try value(obj, "module")
            let m = try Decode.fromValue(mv)
            let verifies = (obj["verifies"] as? Bool) ?? false
            check("the decoder accepts a module that verifies", verifies && Encode.toValue(m) == mv)
            switch Verify.verify(m) {
            case .success(let v): check("the v2 rules accept it, and it is already in verified form", verifies && v == m)
            case .failure(let e): check("the v2 rules accept it", !verifies, e.description)
            }
        }
    }
}

// MARK: - protocol/

func protocolVectors() throws {
    for f in try files(vectors.appendingPathComponent("protocol")) {
        run("protocol/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            let fv = try value(obj, "frame")
            let bytes = try hexBytes(obj, "bytes")
            check("frame bytes are canonical", Canon.encode(fv) == bytes)
            check("bytes decode to the frame", try Canon.decode(bytes) == fv)
            if f.lastPathComponent.hasPrefix("client-") {
                let msg = try Wire.clientFromValue(fv)
                check("client frame re-encodes byte for byte", Canon.encode(Wire.clientValue(msg)) == bytes)
            } else {
                let msg = try Wire.serverFromValue(fv)
                check("server frame re-encodes byte for byte", Canon.encode(Wire.serverValue(msg)) == bytes)
            }
        }
    }
}

// MARK: - eval/

// Every eval vector runs through the interpreter and through the Swift
// demo's native procedure of the same hash (AuthoringTests.swift), which
// must agree with it row for row and refusal for refusal.

func evalVectors() throws {
    for f in try files(vectors.appendingPathComponent("eval")) {
        run("eval/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            let m = try Decode.fromValue(try value(obj, "module"))
            if obj["steps"] != nil { try evalSteps(obj, m) }
            if obj["cases"] != nil, obj["store_before"] != nil { try evalCases(obj, m) }
            if obj["cases"] != nil, obj["store"] != nil { try formCases(obj, m) }
        }
    }
}

func evalSteps(_ obj: [String: Any], _ m: Module) throws {
    let name = try string(obj, "function")
    guard let fn = m.lookupFunction(name) else { throw TestError("no function \(name)") }
    let c = Hash.closure(m, fn)
    let fh = Hash.functionHash(c)
    check("function hash", Hex.encode(fh) == (try string(obj, "function_hash")), Hex.encode(fh))
    check("closures(module) holds it under its hash", Hash.closures(m)[fh]?.fn.name == name)
    let native = demoNatives()[fh]
    check("the Swift demo has this procedure natively, under the vector's hash", native?.mutate != nil)
    let ctxV = try value(obj, "ctx")
    let ctx = Ctx(user: ctxV.field("user").asText(), session: ctxV.field("session").asText())
    let autos = try value(obj, "autos").asRecord()
    guard let steps = obj["steps"] as? [[String: Any]] else { throw TestError("steps") }
    var st = try storeOf(try value(obj, "store_before"), m.schema)
    var nst = try storeOf(try value(obj, "store_before"), m.schema)
    for (i, step) in steps.enumerated() {
        let args = try value(step, "args").asRecord()
        let wantChanges = try value(step, "changes")
        let wantStore = try value(step, "store_after")
        let wantHash = try string(step, "hash_after")
        switch try Eval.applyClosure(m.schema, c, ctx, autos, args, st) {
        case .refused(let r): check("step \(i) applies", false, r.text)
        case .applied(let st2, let chs):
            check("step \(i) changes", Value.list(chs.map(Wire.changeValue)) == wantChanges, Value.list(chs.map(Wire.changeValue)).brief)
            check("step \(i) store_after", st2.asValue() == wantStore)
            check("step \(i) hash_after", Hex.encode(Hash.stateHash(st2)) == wantHash)
            check("step \(i) leaves the store it was given alone", st.asValue() != st2.asValue() || chs.isEmpty)
            st = st2
        }
        guard let run = native?.mutate else { continue }
        switch try run(ctx, autos, args, nst) {
        case .refused(let r): check("step \(i) applies natively", false, r.text)
        case .applied(let st2, let chs):
            check("step \(i) native changes", Value.list(chs.map(Wire.changeValue)) == wantChanges)
            check("step \(i) native store", st2.asValue() == wantStore)
            check("step \(i) native hash", Hex.encode(Hash.stateHash(st2)) == wantHash)
            nst = st2
        }
    }
}

/// `eval/checks.json`: each case applied, refused with the vector's text or not.
func evalCases(_ obj: [String: Any], _ m: Module) throws {
    let ctxV = try value(obj, "ctx")
    let ctx = Ctx(user: ctxV.field("user").asText(), session: ctxV.field("session").asText())
    guard let cases = obj["cases"] as? [[String: Any]] else { throw TestError("cases") }
    let natives = demoNatives()
    for cs in cases {
        let label = (cs["name"] as? String) ?? "?"
        let name = try string(cs, "function")
        guard let fn = m.lookupFunction(name) else { throw TestError("no function \(name)") }
        let c = Hash.closure(m, fn)
        let autos = try value(cs, "autos").asRecord()
        let args = try value(cs, "args").asRecord()
        let want = cs["refused"] as? String
        let st = try storeOf(try value(obj, "store_before"), m.schema)
        func verdict(_ o: Eval.Outcome) -> String? {
            if case .refused(let r) = o { return r.text }
            return nil
        }
        let got = verdict(try Eval.applyClosure(m.schema, c, ctx, autos, args, st))
        check("\(label): interpreter", got == want, "\(got ?? "applied") vs \(want ?? "applied")")
        if let run = natives[Hash.functionHash(c)]?.mutate {
            let n = verdict(try run(ctx, autos, args, st))
            check("\(label): native", n == want, "\(n ?? "applied") vs \(want ?? "applied")")
        } else {
            check("\(label): the demo has it natively", false)
        }
    }
}

/// `eval/form-check.json`: the form validator's messages and normalised input.
func formCases(_ obj: [String: Any], _ m: Module) throws {
    guard let cases = obj["cases"] as? [[String: Any]] else { throw TestError("cases") }
    for cs in cases {
        let label = (cs["name"] as? String) ?? "?"
        let name = try string(cs, "function")
        guard let fn = m.lookupFunction(name) else { throw TestError("no function \(name)") }
        let st = try storeOf(try value(obj, "store"), m.schema)
        let input = try value(cs, "input").asRecord()
        let (msgs, vals) = try Eval.check(m.schema, Hash.closure(m, fn), input, st)
        guard let wantMsgs = cs["messages"] as? [[String: Any]] else { throw TestError("messages") }
        let want = wantMsgs.map { (($0["field"] as? String) ?? "", ($0["message"] as? String) ?? "") }
        check("\(label): messages", msgs.map { "\($0.0): \($0.1)" } == want.map { "\($0.0): \($0.1)" }, "\(msgs)")
        check("\(label): normalised", Value.record(vals) == (try value(cs, "normalised")))
    }
}

// MARK: - views/

func viewVectors() throws {
    let pid = Value.id(Id(uuid: "00000000-0000-0000-0000-000000000001")!)
    let plans: [String: ViewPlan] = [
        "top-two-by-pos": ViewPlan(table: "item", filter: .fcmp("playlist_id", .eq, pid), order: [OrderBy("pos", .asc)], limit: 2, related: []),
        "playlist-with-items": ViewPlan(table: "playlist", filter: nil, order: [OrderBy("name", .asc)], limit: nil, related: [
            ViewRelated(name: "item", relation: Relation(parent: "playlist", child: "item", column: "playlist_id"),
                        plan: ViewPlan(table: "item", filter: nil, order: [OrderBy("pos", .desc)], limit: 3, related: [])),
        ]),
    ]
    for f in try files(vectors.appendingPathComponent("views")) {
        let name = f.deletingPathExtension().lastPathComponent
        run("views/" + f.lastPathComponent) {
            let obj = try loadJSON(f)
            guard let vp = plans[name] else { throw TestError("no plan written by hand for \(name)") }
            let m = try Decode.fromValue(try value(obj, "module"))
            let sch = m.schema
            let changeLists = try value(obj, "changes").asList()
            guard let steps = obj["steps"] as? [[String: Any]] else { throw TestError("steps") }
            check("one step per change list", changeLists.count == steps.count)
            let st = MemoryStore(schema: sch)
            var view = View.hydrate(sch, vp, st)
            var previous = view.rows
            for (i, (chs, step)) in zip(changeLists, steps).enumerated() {
                var patches: [Patch] = []
                for cv in chs.asList() {
                    let ch = try Wire.changeFromValue(cv)
                    st.applyChange(ch)
                    patches += view.push(sch, st, ch)
                }
                let wantPatches = try value(step, "patches")
                let wantRows = try value(step, "rows")
                check("step \(i) patches", Value.list(patches.map { $0.value }) == wantPatches, Value.list(patches.map { $0.value }).brief)
                check("step \(i) rows", Value.list(view.rows) == wantRows)
                check("step \(i) contract", View.contract(sch, vp, st, view))
                check("step \(i) splice", Patch.splice(patches, previous) == view.rows)
                previous = view.rows
            }
            // The same plan from an IR plan with literal right-hand sides.
            let irPlan: Plan
            if name == "top-two-by-pos" {
                irPlan = Plan.from("item").filter(Pred.cmp("playlist_id", CmpOp.eq, pid)).orderBy("pos", Dir.asc).limit(2)
            } else {
                irPlan = Plan.from("playlist").orderBy("name", Dir.asc)
                    .related("item", "playlist", "item", "playlist_id", Plan.from("item").orderBy("pos", Dir.desc).limit(3))
            }
            let evaluated = try ViewPlan.evalPlan(irPlan) { e in
                if case .lit(let v) = e { return v }
                throw TestError("not a literal")
            }
            check("evalPlan of the IR plan is the hand-written plan", evaluated == vp)
            check("select agrees with the view", st.select(irPlan) == .list(view.rows))
        }
    }
}

// MARK: - rebase/three-peers

func rebaseVectors() throws {
    let f = vectors.appendingPathComponent("rebase/three-peers.json")
    run("rebase/three-peers.json") {
        let obj = try loadJSON(f)
        let m = try Decode.fromValue(try value(obj, "module"))
        let sch = m.schema
        let bodies = Hash.closures(m)
        let scope = "demo"
        var entries: [(Seq, Entry)] = []
        for ev in try value(obj, "entries").asList() {
            let n = ev.field("seq").asInt()
            entries.append((n, try Wire.entryFromValue(ev)))
        }
        var facts: [Facts] = []
        for fv in try value(obj, "facts").asList() { facts.append(try fv.asList().map(Wire.changeFromValue)) }
        let finalHash = try string(obj, "final_hash")
        let finalStore = try value(obj, "final_store")
        check("four entries with their facts", entries.count == 4 && facts.count == 4)

        // The authority replays every entry in order and reaches the final hash.
        var auth = Authority(sch, scope, bodies)
        for (i, (n, e)) in entries.enumerated() {
            switch auth.sequenceEntry(e) {
            case .appended(let got, let fs):
                check("entry \(n) sequenced at \(n)", got == n, "\(got)")
                check("entry \(n) facts", fs == facts[i], Value.list(fs.map(Wire.changeValue)).brief)
            case .duplicate(let d): check("entry \(n) not a duplicate", false, "\(d)")
            case .rejected(let r): check("entry \(n) not rejected", false, r.text)
            }
        }
        check("authority final hash", Hex.encode(Hash.stateHash(auth.store)) == finalHash, Hex.encode(Hash.stateHash(auth.store)))
        check("authority final store", auth.store.asValue() == finalStore)
        check("the log's state at the head, from facts, is the head state", auth.log.stateAt(4).map { Hex.encode(Hash.stateHash($0)) } == finalHash)
        if case .duplicate(let n) = auth.sequenceEntry(entries[1].1) {
            check("a re-pushed intent is a duplicate at its sequence", n == 2)
        } else {
            check("a re-pushed intent is a duplicate", false)
        }

        // Bob: holds the code, receives every entry in order.
        var bob = Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, [])
        for (n, e) in entries { bob.receive(n, e) }
        check("bob reaches the final hash by replay", Hex.encode(bob.verifyAt().1) == finalHash)
        check("bob's cursor is 4", bob.cursor == 4)
        let bobBefore = bob
        bob.receive(2, entries[1].1)
        check("a duplicate delivery is a no-op", bob.cursor == bobBefore.cursor && bob.inbox.isEmpty && bob.verifyAt().1 == bobBefore.verifyAt().1)

        // Carol: no code at all, applies by facts.
        var carol = Replica.open(sch, scope, [:], MemoryStore(schema: sch), 0, [])
        for (n, e) in entries { carol.receive(n, e) }
        check("without closures carol asks for every entry's facts", carol.needs == [1, 2, 3, 4], "\(carol.needs)")
        for (i, (n, _)) in entries.enumerated() { carol.receiveFacts(n, facts[i]) }
        check("by facts alone carol reaches the same state", Hex.encode(carol.verifyAt().1) == finalHash && carol.diverged.isEmpty)

        // Alice: the rebase, step by step, as the vector's numbers say.
        let e1 = entries[0].1, e2 = entries[1].1, e3 = entries[2].1, e9 = entries[3].1
        var alice = Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, [])
        switch alice.mutate(e1.id, Ctx(user: e1.actor, session: e1.session), e1.fn, e1.autos, e1.args) {
        case .failure(let r): check("alice creates", false, r.text)
        case .success(let e): check("alice's entry is the vector's", e == e1)
        }
        let pidV = e9.args["playlist_id"]!
        check("alice's name was trimmed", alice.view.get("playlist", [pidV]).field("name") == .text("Favorites"))
        alice.ack(e1.id, 1)
        check("after the ack nothing is pending", alice.pending.isEmpty && alice.cursor == 1)
        _ = alice.takeChanges()
        let track9 = e9.args["track_id"]!
        func posOf(_ r: Replica, _ k: Value) -> Value {
            let row = r.view.get("item", [pidV, k])
            return row.isNull() ? .null : row.field("pos")
        }
        switch alice.mutate(e9.id, Ctx(user: e9.actor, session: e9.session), e9.fn, e9.autos, e9.args) {
        case .failure(let r): check("alice adds 9 alone", false, r.text)
        case .success(let e): check("alice's second entry is the vector's", e == e9)
        }
        check("alone, alice's track is first on her view", posOf(alice, track9) == (try value(obj, "alice_alone_pos_of_9")))
        if case .applied(let chs) = alice.takeChanges() {
            check("a local mutation reports its changes, not a rebuild", chs.count == 1)
        } else {
            check("a local mutation reports its changes, not a rebuild", false)
        }
        alice.receive(2, e2)
        alice.receive(3, e3)
        check("the rebase is reported as a rebuild", alice.takeChanges() == .rebuilt)
        check("after the rebase alice's track is third", posOf(alice, track9) == (try value(obj, "alice_after_rebase_pos_of_9")), posOf(alice, track9).brief)
        var bob3 = Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, [])
        for (n, e) in entries.prefix(3) { bob3.receive(n, e) }
        check("alice's confirmed state is bob's", alice.verifyAt() == bob3.verifyAt())
        alice.receiveFacts(4, facts[3])
        alice.ack(e9.id, 4)
        check("nothing is pending on alice once acked", alice.pending.isEmpty)
        if case .applied = alice.takeChanges() {
            check("with nothing pending the ack costs no rebuild", true)
        } else {
            check("with nothing pending the ack costs no rebuild", false)
        }
        check("alice's view is her confirmed store", alice.view.sameRows(as: alice.confirmed))
        check("three replicas, one hash", Hex.encode(alice.verifyAt().1) == finalHash)
        check("the authority's facts say pos 3 too", facts[3].contains { if case .add(_, let row) = $0 { return row["pos"] == .int(3) }; return false })

        // Dave's build steps by two; the facts catch it and heal it.
        let hAdd = e9.fn
        var wrong = bodies[hAdd]!
        wrong.fn.body = wrong.fn.body.map { s in
            if case .sInsert(let t, .structOf(var fs), let on) = s, case .op(.add, let xs)? = fs["pos"] {
                fs["pos"] = .op(.add, [xs[0], .lit(.int(2))])
                return .sInsert(t, .structOf(fs), on)
            }
            return s
        }
        var daveBodies = bodies
        daveBodies[hAdd] = wrong
        var dave = Replica.open(sch, scope, daveBodies, MemoryStore(schema: sch), 0, [])
        for (i, (n, e)) in entries.enumerated() { dave.receiveWith(n, e, facts[i]) }
        check("a divergent runtime is detected", dave.diverged == [2, 3, 4], "\(dave.diverged)")
        check("and healed by the facts", Hex.encode(dave.verifyAt().1) == finalHash)

        // Eve has no server: she is her own authority.
        var eve = Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, [])
        var eveAuth = Authority(sch, scope, bodies)
        let hCreate = e1.fn
        let id2 = Value.id(Id(uuid: "00000000-0000-0000-0000-000000000002")!)
        _ = eve.mutate(Id(uuid: "00000000-0000-0000-0000-0000000000c9")!, Ctx(user: "eve", session: "eve-session"), hCreate, ["id": id2], ["name": .text("Road")])
        _ = eve.mutate(Id(uuid: "00000000-0000-0000-0000-0000000000ca")!, Ctx(user: "eve", session: "eve-session"), hAdd, e9.autos, ["playlist_id": id2, "track_id": .text("t5")])
        check("eve has two pending", eve.pending.count == 2)
        localCommit(&eveAuth, &eve)
        check("alone, eve confirms her own intents", eve.cursor == 2 && eve.pending.isEmpty && eve.view.sameRows(as: eve.confirmed))
        check("and her state is her authority's", eve.verifyAt().1 == Hash.stateHash(eveAuth.store))

        // A refusal by the view is not recorded.
        var frank = Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, [])
        if case .failure(let why) = frank.mutate(Id.nil_, Ctx(user: "frank", session: "s"), hCreate, ["id": id2], ["name": .text(" ")]) {
            check("a refused intent is the mutator's verdict", why == .refused("a playlist needs a name"))
        } else {
            check("a refused intent is refused", false)
        }
        check("and records nothing", frank.pending.isEmpty && frank.rejections.isEmpty)

        // Compaction: the horizon moves to 2.
        var auth5 = auth
        check("compact to 2", auth5.compact(2))
        if case .belowHorizon(let sn) = auth5.page(0, 10) {
            check("a peer at 0 is sent the snapshot", sn.seq == 2)
        } else {
            check("a peer at 0 is sent the snapshot", false)
        }
        if case .entries(let es, let more) = auth5.page(2, 10) {
            check("a peer at 2 is sent the tail", es.map { $0.0 } == [3, 4] && !more)
        } else {
            check("a peer at 2 is sent the tail", false)
        }
        check("the state at the head, from facts, is the head state", auth5.log.stateAt(4).map { Hash.stateHash($0) } == Hash.stateHash(auth5.store))
        check("ids are kept below the horizon", auth5.log.seqOf(e1.id) == 1)

        // The client machine over the same entries.
        var client = Client.openClient(sch, "tok")
        client.subscribe(.whole, Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, []))
        client.connected()
        let outgoing = client.takeOutgoing()
        if outgoing.count == 1, case .hello(let subs, let tok, let spec) = outgoing[0] {
            check("connected says hello", subs == [Subscription(scope: scope, since: 0, mode: .whole)] && tok == "tok" && spec == specVersion)
        } else {
            check("connected says hello", false, "\(outgoing.count) frames")
        }
        client.recv(.batch(scope: scope, items: entries.map { BatchItem(seq: $0.0, entry: $0.1, facts: nil) }, hasMore: false))
        check("a whole client replays the batch", client.scopes[scope].map { Hex.encode($0.replica.verifyAt().1) } == finalHash)
        check("and needs no facts", client.takeOutgoing().isEmpty)
        client.verifyAll()
        let verifyOut = client.takeOutgoing()
        if verifyOut.count == 1, case .verify(let s, let n, let h) = verifyOut[0] {
            check("verify carries the cursor and the hash", s == scope && n == 4 && Hex.encode(h) == finalHash)
        } else {
            check("verify carries the cursor and the hash", false)
        }
        client.recv(.agree(scope: scope, seq: 4, hash: Hex.decode(finalHash)!, ok: true))
        check("agree is recorded", client.agreed.count == 1 && client.agreed[0].2)
        client.recv(.heard(frame: [1, 2, 3]))
        check("heard is taken", client.takeHeard() == [[1, 2, 3]])
        client.say([9])
        if case .say(let fr)? = client.takeOutgoing().first { check("say is queued while linked", fr == [9]) } else { check("say is queued while linked", false) }
        client.disconnected()
        client.say([9])
        check("say is dropped while unlinked", client.takeOutgoing().isEmpty)

        // A facts-mode client with no code asks for the facts and gets there.
        var byFacts = Client.openClient(sch, nil)
        byFacts.subscribe(.byFacts, Replica.open(sch, scope, [:], MemoryStore(schema: sch), 0, []))
        byFacts.connected()
        _ = byFacts.takeOutgoing()
        byFacts.recv(.batch(scope: scope, items: entries.map { BatchItem(seq: $0.0, entry: $0.1, facts: nil) }, hasMore: false))
        let asks = byFacts.takeOutgoing()
        if asks.count == 1, case .needFacts(let s, let ns) = asks[0] {
            check("a client without the code asks for facts", s == scope && ns == [1, 2, 3, 4])
        } else {
            check("a client without the code asks for facts", false, "\(asks.count)")
        }
        byFacts.recv(.factsFor(scope: scope, items: entries.enumerated().map { FactsItem(seq: $0.element.0, facts: facts[$0.offset]) }))
        check("and reaches the final hash", byFacts.scopes[scope].map { Hex.encode($0.replica.verifyAt().1) } == finalHash)

        // A client with pending work that is pushed on connect, acked, and one rejected.
        var pusher = Client.openClient(sch, "alice")
        pusher.subscribe(.whole, Replica.open(sch, scope, bodies, MemoryStore(schema: sch), 0, []))
        _ = pusher.mutate(scope, e1.id, Ctx(user: e1.actor, session: e1.session), e1.fn, e1.autos, e1.args)
        check("unlinked, nothing is queued", pusher.takeOutgoing().isEmpty)
        pusher.connected()
        let onConnect = pusher.takeOutgoing()
        check("connected pushes the pending intent after hello", onConnect.count == 2)
        if onConnect.count == 2, case .push(let s, let es) = onConnect[1] { check("the push carries the entry", s == scope && es == [e1]) }
        pusher.recv(.ack(scope: scope, ids: [e1.id], seqs: [1]))
        check("an ack confirms it", pusher.scopes[scope]!.replica.cursor == 1 && pusher.scopes[scope]!.replica.pending.isEmpty)
        _ = pusher.mutate(scope, e9.id, Ctx(user: e9.actor, session: e9.session), e9.fn, e9.autos, e9.args)
        pusher.recv(.reject(scope: scope, id: e9.id, reason: "not yours"))
        check("a reject drops the intent and keeps the verdict", pusher.scopes[scope]!.replica.pending.isEmpty && pusher.scopes[scope]!.replica.rejections.last?.why == .refused("not yours"))
        // A snapshot below the horizon replaces the confirmed store.
        let snapRows: [TableName: [Value]] = ["playlist": auth5.store.scan("playlist").map { .record($0) }, "item": auth5.log.base.store.scan("item").map { .record($0) }]
        pusher.recv(.snapshotOf(scope: scope, seq: 2, hash: auth5.log.base.hash, rows: snapRows))
        check("a snapshot moves the cursor to it", pusher.scopes[scope]!.replica.cursor == 2 && pusher.scopes[scope]!.replica.verifyAt().1 == auth5.log.base.hash)
        pusher.recv(.batch(scope: scope, items: [BatchItem(seq: 3, entry: e3, facts: nil), BatchItem(seq: 4, entry: e9, facts: nil)], hasMore: false))
        check("and the tail brings it to the head", Hex.encode(pusher.scopes[scope]!.replica.verifyAt().1) == finalHash)
        // Closures arriving unblock a replica without code.
        var late = Client.openClient(sch, nil)
        late.subscribe(.whole, Replica.open(sch, scope, [:], MemoryStore(schema: sch), 0, []))
        late.connected()
        _ = late.takeOutgoing()
        late.recv(.batch(scope: scope, items: entries.map { BatchItem(seq: $0.0, entry: $0.1, facts: nil) }, hasMore: false))
        _ = late.takeOutgoing()
        late.recv(.closures(items: bodies.map { ClosureItem(hash: $0.key, closure: $0.value) }))
        check("closures received over the wire unblock the inbox", Hex.encode(late.scopes[scope]!.replica.verifyAt().1) == finalHash)
        // …and a closure decoded from its wire form runs identically.
        let wireClosure = try Decode.closureFromValue(Hash.closureValue(bodies[hAdd]!))
        check("a closure survives the wire", Hash.functionHash(wireClosure) == hAdd)
    }
}

// MARK: - the store's own rules

func storeRules(_ schema0: Schema) {
    run("store/rules") {
        // A schema with a nullable unique column, a reference, and an enum.
        let sch = Schema(scopes: [Scope("s", tables: [
            Table("parent", columns: [Column("id", .id("parent")), Column("code", .text, nullable: true), Column("kind", .enumOf(["a", "b"]))], key: ["id"], indexes: [Index(["code"], unique: true)]),
            Table("child", columns: [Column("id", .id("child")), Column("parent_id", .id("parent"), nullable: true)], key: ["id"], refs: [Ref("parent_id", "parent")]),
        ])])
        let st = MemoryStore(schema: sch)
        let p1 = Id(uuid: "00000000-0000-0000-0000-000000000001")!
        let p2 = Id(uuid: "00000000-0000-0000-0000-000000000002")!
        check("an omitted nullable column is filled with null", st.tryPut("parent", ["id": .id(p1), "kind": .text("a")]) == .success(.add("parent", ["id": .id(p1), "code": .null, "kind": .text("a")])))
        check("a second null in a unique column does not clash", st.tryPut("parent", ["id": .id(p2), "code": .null, "kind": .text("b")]) == .success(.add("parent", ["id": .id(p2), "code": .null, "kind": .text("b")])))
        check("the same row again is no change", st.tryPut("parent", ["id": .id(p2), "kind": .text("b")]) == .success(nil))
        check("an edit reports both versions", st.tryPut("parent", ["id": .id(p2), "code": .text("x"), "kind": .text("b")]) == .success(.edit("parent", ["id": .id(p2), "code": .null, "kind": .text("b")], ["id": .id(p2), "code": .text("x"), "kind": .text("b")])))
        check("a unique clash is refused", st.tryPut("parent", ["id": .id(p1), "code": .text("x"), "kind": .text("a")]) == .failure(.uniqueViolation("parent", ["code"])))
        check("an omitted non-nullable column is malformed", { if case .failure(.malformedRow) = st.tryPut("parent", ["id": .id(p1)]) { return true }; return false }())
        check("an unknown column is malformed", { if case .failure(.malformedRow) = st.tryPut("parent", ["id": .id(p1), "kind": .text("a"), "extra": .int(1)]) { return true }; return false }())
        check("null in a non-nullable column", st.tryPut("parent", ["id": .id(p1), "kind": .null]) == .failure(.notNull("parent", "kind")))
        check("a value outside the enum is malformed", st.tryPut("parent", ["id": .id(p1), "kind": .text("c")]) == .failure(.malformedRow("parent", "kind has the wrong type")))
        check("no such table", st.tryPut("nowhere", [:]) == .failure(.noSuchTable("nowhere")))
        let c1 = Id(uuid: "00000000-0000-0000-0000-0000000000c1")!
        check("a missing parent is refused", st.tryPut("child", ["id": .id(c1), "parent_id": .id(Id.nil_)]) == .failure(.missingParent("child", "parent_id", "parent")))
        check("a null reference references nothing", st.tryPut("child", ["id": .id(c1)]) == .success(.add("child", ["id": .id(c1), "parent_id": .null])))
        check("a present parent is found", st.tryPut("child", ["id": .id(c1), "parent_id": .id(p1)]).isSuccess)
        check("a referenced row cannot be deleted", st.tryDelete("parent", [.id(p1)]) == .failure(.stillReferenced("parent", "child")))
        check("deleting a missing row is a no-op", st.tryDelete("parent", [.id(Id.nil_)]) == .success(nil))
        check("deleting reports the row", st.tryDelete("child", [.id(c1)]) == .success(.remove("child", ["id": .id(c1), "parent_id": .id(p1)])))
        check("and then the parent goes", st.tryDelete("parent", [.id(p1)]).isSuccess)
        check("the Store interface faults with the refusal's text", { do { try st.put("parent", .record(["id": .id(p2), "kind": .text("zzz")])); return false } catch let e as Fault { return e == .refuse("MalformedRow \"parent\" \"kind has the wrong type\"") } catch { return false } }())
        // Ops and Std faults.
        check("overflow", { do { _ = try Ops.add(.int(Int64.max), .int(1)); return false } catch let e as Fault { return e == .refuse("integer overflow") } catch { return false } }())
        check("division by zero", { do { _ = try Ops.div(.int(1), .int(0)); return false } catch let e as Fault { return e == .refuse("division by zero") } catch { return false } }())
        check("min / -1", { do { _ = try Ops.mod(.int(Int64.min), .int(-1)); return false } catch let e as Fault { return e == .refuse("integer overflow") } catch { return false } }())
        check("division truncates toward zero", (try? Ops.div(.int(-7), .int(2))) == .int(-3))
        check("remainder takes the dividend's sign", (try? Ops.mod(.int(-7), .int(2))) == .int(-1))
        check("NULL = NULL under the total order", Ops.cmp(.eq, .null, .null) == .bool(true) && Ops.cmp(.lt, .null, .int(0)) == .bool(true))
        check("sortBy is stable", (try? Ops.sortBy(.list([.list([.int(1), .text("a")]), .list([.int(0), .text("b")]), .list([.int(1), .text("c")]), .list([.int(0), .text("d")])])) { $0.asList()[0] }) == .list([.list([.int(0), .text("b")]), .list([.int(0), .text("d")]), .list([.int(1), .text("a")]), .list([.int(1), .text("c")])]))
        check("fold", (try? Ops.fold(.list([.int(1), .int(2), .int(3)]), .int(0)) { acc, x in try Ops.add(acc, x) }) == .int(6))
        check("trim uses White_Space", (try? Std.trim(.text("\u{3000} a b\u{a0}\t"))) == .text("a b"))
        check("lower is the simple mapping", (try? Std.lower(.text("İX"))) == .text("ix"))
        check("isAlnum on empty is false", (try? Std.isAlnum(.text(""))) == .bool(false) && (try? Std.isAlnum(.text("é9"))) == .bool(true) && (try? Std.isAlnum(.text("a b"))) == .bool(false))
        check("textLen counts code points", (try? Std.textLen(.text("e\u{301}🎵"))) == .int(3))
        check("chars", (try? Std.chars(.text("a🎵"))) == .list([.text("a"), .text("🎵")]))
        check("splitOnce", (try? Std.splitOnce(.text("a=b=c"), .text("="))) == .record(["before": .text("a"), "after": .text("b=c")]) && (try? Std.splitOnce(.text("abc"), .text(""))) == .null && (try? Std.splitOnce(.text("abc"), .text("x"))) == .null)
        check("fnv1a64 of empty is the offset basis", (try? Std.fnv1a64(.text(""))) == .int(Int64(bitPattern: 0xcbf29ce484222325)))
        check("fnv1a64 of a", (try? Std.fnv1a64(.text("a"))) == .int(Int64(bitPattern: 0xaf63dc4c8601ec8c)))
        check("textOfId / idOfText", (try? Std.idOfText(.text("00000000-0000-0000-0000-0000000000AB"))).flatMap { try? Std.textOfId($0) } == .text("00000000-0000-0000-0000-0000000000ab") && (try? Std.idOfText(.text("nope"))) == .null)
        check("hex", (try? Std.hex(.bytes([0, 255, 16]))) == .text("00ff10"))
        check("clamp refuses crossed bounds", { do { _ = try Std.clamp(.int(1), .int(5), .int(0)); return false } catch let e as Fault { return e == .refuse("clamp: lower bound above upper bound") } catch { return false } }())
        check("abs of min overflows", { do { _ = try Std.abs(.int(Int64.min)); return false } catch let e as Fault { return e == .refuse("integer overflow") } catch { return false } }())
        check("textOfInt", (try? Std.textOfInt(.int(-42))) == .text("-42"))
        check("contains under the total order", (try? Std.contains(.list([.text("é")]), .text("e\u{301}"))) == .bool(false))
        check("Value.record from pairs", Value.record([("b", .int(1)), ("a", .int(2))]) == .record(["a": .int(2), "b": .int(1)]))
        check("Value.opt", Value.opt(nil) == .null && Value.opt(.int(1)) == .int(1))
        check("Value.idHex and bytesHex", Value.idHex("00000000000000000000000000000001") == .id(p1) && Value.bytesHex("0a0b") == .bytes([10, 11]))
        check("Fault.refuse takes a Value", Fault.refuse(Value.text("x")) == Fault.refuse("x"))
        _ = schema0
    }
}

extension Result {
    var isSuccess: Bool {
        if case .success = self { return true }
        return false
    }
}

// MARK: - main

@main
struct VectorsTests {
    static func main() {
        setvbuf(stdout, nil, _IOLBF, 0)
        print("vectors at \(vectors.path)")
        do {
            try codecVectors()
            try orderVectors()
            let m = try moduleVectors()
            try hashVectors(m.schema)
            try verifyVectors()
            try protocolVectors()
            try evalVectors()
            try viewVectors()
            try rebaseVectors()
            storeRules(m.schema)
            try authoringTests(m)
            try clientTests()
            try phoneTests()
        } catch {
            failed += 1
            print("FAIL: \(error)")
        }
        print("rebase/fleet-seed-7.json: skipped (needs the server machine and the simulation, which the Swift runtime does not carry)")
        print("\(passed) checks passed, \(failed) failed")
        exit(failed == 0 ? 0 : 1)
    }
}
