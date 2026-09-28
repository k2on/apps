import Foundation
import ArkDB
import ArkAuthoring
import ArkDemo
import HarkenDomain

// The authoring vocabulary, both ways. `Emit` of the Swift demo (Tests/Demo,
// spec/AUTHORING.md Appendix B in Swift) must be the vector's bytes, and
// `Native` must agree with `Ark.Eval` over that emitted IR on every
// procedure, for every case below. The same agreement is held over harken's
// domain as the phone is built with it (../harken/domain/gen/swift).
//
// This file imports ArkDB and ArkAuthoring together, so the three names
// both define — `Id`, `Ctx`, `Module` — are always written qualified.

/// The Swift demo's procedures, by hash.
func demoNatives() -> [FnHash: Procedure] {
    var out: [FnHash: Procedure] = [:]
    for (h, p) in ArkDemo.module().procedures() { out[h] = p }
    return out
}

/// The demo module's bytes and procedures, for the client tests.
func demoAuthored() -> (bytes: [UInt8], procedures: [(FnHash, Procedure)]) {
    let m = ArkDemo.module()
    return (m.emit(), m.procedures())
}

/// Inputs as entry arguments, through the authored input types (their
/// memberwise initialisers are the domain module's own, so a test outside
/// it builds one from arguments and takes the arguments back).
func createArgs(_ name: String) -> Args { return ArkDemo.CreatePlaylist(args: ["name": .text(name)]).args }
func addArgs(_ playlist: ArkDB.Id, _ track: String) -> Args {
    return AddToPlaylist(args: ["playlist_id": .id(playlist), "track_id": .text(track)]).args
}

/// One case of the agreement: a procedure, who, its autos and input, and
/// the store it runs against.
struct Case {
    let label: String
    let function: String
    let ctx: ArkDB.Ctx
    let autos: Args
    let args: Args
    let store: MemoryStore
}

/// What a run came to, in a form two runs can be compared by.
func outcomeText(_ o: Eval.Outcome) -> String {
    switch o {
    case .refused(let r): return "refused " + r.text
    case .applied(let st, let chs):
        // Whole, not `brief`: a truncated rendering hid a bent position
        // the first time this was falsified.
        return "applied \(chs.count) changes " + Hex.encode(Sha256.hash(Canon.encode(.list(chs.map(Wire.changeValue))))) + " → " + Hex.encode(Hash.stateHash(st))
    }
}

func resultText(_ r: Result<Value, Refusal>) -> String {
    switch r {
    case .success(let v): return "value " + Hex.encode(Sha256.hash(Canon.encode(v)))
    case .failure(let why): return "refused " + why.text
    }
}

/// Native against `Ark.Eval` on every case: the same refusal, or the same
/// changes and the same resulting state (queries: the same value). Returns
/// the cases that disagreed.
func agreement(_ m: ArkDB.Module, _ natives: [FnHash: Procedure], _ cases: [Case]) throws -> [String] {
    var bad: [String] = []
    for c in cases {
        guard let fn = m.lookupFunction(c.function) else { bad.append("\(c.label): no function"); continue }
        let cl = Hash.closure(m, fn)
        guard let p = natives[Hash.functionHash(cl)] else { bad.append("\(c.label): no native procedure"); continue }
        if fn.kind == .mutator {
            let e = outcomeText(try Eval.applyClosure(m.schema, cl, c.ctx, c.autos, c.args, c.store))
            let n = outcomeText(try p.mutate!(c.ctx, c.autos, c.args, c.store))
            if e != n { bad.append("\(c.label): eval \(e), native \(n)") }
        } else {
            let e = resultText(try Eval.queryResult(m.schema, cl, c.args, c.store, ctx: c.ctx))
            let n = resultText(try p.query!(c.ctx, c.args, c.store))
            if e != n { bad.append("\(c.label): eval \(e), native \(n)") }
        }
    }
    return bad
}

func rawId(_ n: UInt8) -> ArkDB.Id {
    var b = [UInt8](repeating: 0, count: 16)
    b[15] = n
    return ArkDB.Id(bytes: b)!
}

/// A store after applying a sequence of (function, ctx, autos, args) through the interpreter.
func seeded(_ m: ArkDB.Module, _ steps: [(String, ArkDB.Ctx, Args, Args)]) throws -> MemoryStore {
    var st = MemoryStore(schema: m.schema)
    for (name, ctx, autos, args) in steps {
        guard case .applied(let s2, _) = try Eval.apply(m, name, ctx, autos, args, st) else { throw TestError("seed \(name) refused") }
        st = s2
    }
    return st
}

func demoCases(_ m: ArkDB.Module) throws -> [Case] {
    let alice = ArkDB.Ctx(user: "alice", session: "a1")
    let bob = ArkDB.Ctx(user: "bob", session: "b1")
    let p1 = Value.id(rawId(1)), p2 = Value.id(rawId(2))
    let empty = MemoryStore(schema: m.schema)
    let one = try seeded(m, [("create_playlist", alice, ["id": p1], ["name": .text("Favorites")])])
    let three = try seeded(m, [
        ("create_playlist", alice, ["id": p1], ["name": .text("Favorites")]),
        ("add_to_playlist", alice, [:], ["playlist_id": p1, "track_id": .text("t1")]),
        ("add_to_playlist", bob, [:], ["playlist_id": p1, "track_id": .text("t2")]),
    ])
    return [
        Case(label: "create: blank is refused", function: "create_playlist", ctx: alice, autos: ["id": p1], args: ["name": .text("   ")], store: empty),
        Case(label: "create: trimmed", function: "create_playlist", ctx: alice, autos: ["id": p1], args: ["name": .text("  Road ")], store: empty),
        Case(label: "create: same name again is a no-op", function: "create_playlist", ctx: alice, autos: ["id": p2], args: ["name": .text("Favorites")], store: one),
        Case(label: "create: another user's same name lands", function: "create_playlist", ctx: bob, autos: ["id": p2], args: ["name": .text("Favorites")], store: one),
        Case(label: "create: the same key again is a no-op", function: "create_playlist", ctx: bob, autos: ["id": p1], args: ["name": .text("Other")], store: one),
        Case(label: "add: no such playlist", function: "add_to_playlist", ctx: alice, autos: [:], args: ["playlist_id": p2, "track_id": .text("t1")], store: one),
        Case(label: "add: empty track", function: "add_to_playlist", ctx: alice, autos: [:], args: ["playlist_id": p1, "track_id": .text("")], store: one),
        Case(label: "add: first item at 1", function: "add_to_playlist", ctx: alice, autos: [:], args: ["playlist_id": p1, "track_id": .text("t9")], store: one),
        Case(label: "add: after the last", function: "add_to_playlist", ctx: alice, autos: [:], args: ["playlist_id": p1, "track_id": .text("t9")], store: three),
        Case(label: "add: the same track again is a no-op", function: "add_to_playlist", ctx: alice, autos: [:], args: ["playlist_id": p1, "track_id": .text("t2")], store: three),
        Case(label: "items: empty", function: "items", ctx: alice, autos: [:], args: ["playlist_id": p2], store: three),
        Case(label: "items: in position order", function: "items", ctx: alice, autos: [:], args: ["playlist_id": p1], store: three),
    ]
}

func harkenCases(_ m: ArkDB.Module) throws -> [Case] {
    let alice = ArkDB.Ctx(user: "alice", session: "a1")
    let bob = ArkDB.Ctx(user: "bob", session: "b1")
    let nobody = ArkDB.Ctx(user: "", session: "")
    let p1 = Value.id(rawId(1)), p2 = Value.id(rawId(2)), p3 = Value.id(rawId(3))
    let t1 = Value.id(rawId(0x11)), t2 = Value.id(rawId(0x12)), t3 = Value.id(rawId(0x13)), gone = Value.id(rawId(0x19))
    // The library the scanner would have made: add_song is not the phone's,
    // so its three rows are put in the store as they are.
    func store(_ steps: [(String, ArkDB.Ctx, Args, Args)]) throws -> MemoryStore {
        var st = MemoryStore(schema: m.schema)
        for (n, (id, title)) in [(t1, "Air"), (t2, "Aria"), (t3, "Spring")].enumerated() {
            st.applyChange(.add("media", ["id": id, "kind": .text("song"), "title": .text(title), "creator": .text("Bach"),
                                          "duration_ms": .int(1000), "file": .text("music/\(title).flac"), "pos": .int(Int64(n + 1)),
                                          "added_ms": .int(1), "user_id": .text("library")]))
        }
        for (name, ctx, autos, args) in steps {
            guard case .applied(let s2, _) = try Eval.apply(m, name, ctx, autos, args, st) else { throw TestError("seed \(name) refused") }
            st = s2
        }
        return st
    }
    let favorites: (String, ArkDB.Ctx, Args, Args) = ("create_playlist", alice, ["id": p1, "created_ms": .int(5)], ["name": .text("Favorites")])
    let empty = try store([])
    let one = try store([favorites])
    let renamed = try store([favorites, ("create_playlist", alice, ["id": p2, "created_ms": .int(6)], ["name": .text("Favorites")])])
    let two = try store([
        favorites,
        ("add_to_playlist", alice, ["added_ms": .int(6)], ["playlist_id": p1, "media_id": t2]),
        ("add_to_playlist", alice, ["added_ms": .int(7)], ["playlist_id": p1, "media_id": t1]),
    ])
    return [
        Case(label: "harken create: signed out, nobody's", function: "create_playlist", ctx: nobody, autos: ["id": p1, "created_ms": .int(1)], args: ["name": .text("x")], store: empty),
        Case(label: "harken create: blank", function: "create_playlist", ctx: alice, autos: ["id": p1, "created_ms": .int(1)], args: ["name": .text(" ")], store: empty),
        Case(label: "harken create: too long", function: "create_playlist", ctx: alice, autos: ["id": p1, "created_ms": .int(1)], args: ["name": .text(String(repeating: "a", count: 121))], store: empty),
        Case(label: "harken create: lands", function: "create_playlist", ctx: alice, autos: ["id": p1, "created_ms": .int(1)], args: ["name": .text(" Mine ")], store: empty),
        Case(label: "harken create: a name she has is numbered", function: "create_playlist", ctx: alice, autos: ["id": p2, "created_ms": .int(1)], args: ["name": .text("Favorites")], store: one),
        Case(label: "harken create: the smallest free number", function: "create_playlist", ctx: alice, autos: ["id": p3, "created_ms": .int(1)], args: ["name": .text("Favorites")], store: renamed),
        Case(label: "harken create: another's name is not taken", function: "create_playlist", ctx: bob, autos: ["id": p2, "created_ms": .int(1)], args: ["name": .text("Favorites")], store: one),
        Case(label: "harken create: the same key again", function: "create_playlist", ctx: alice, autos: ["id": p1, "created_ms": .int(1)], args: ["name": .text("Other")], store: one),
        Case(label: "harken add: not yours", function: "add_to_playlist", ctx: bob, autos: ["added_ms": .int(9)], args: ["playlist_id": p1, "media_id": t1], store: one),
        Case(label: "harken add: no such playlist", function: "add_to_playlist", ctx: alice, autos: ["added_ms": .int(9)], args: ["playlist_id": p2, "media_id": t1], store: one),
        Case(label: "harken add: nobody at hers", function: "add_to_playlist", ctx: nobody, autos: ["added_ms": .int(9)], args: ["playlist_id": p1, "media_id": t1], store: one),
        Case(label: "harken add: first at 1", function: "add_to_playlist", ctx: alice, autos: ["added_ms": .int(9)], args: ["playlist_id": p1, "media_id": t3], store: one),
        Case(label: "harken add: after the last", function: "add_to_playlist", ctx: alice, autos: ["added_ms": .int(9)], args: ["playlist_id": p1, "media_id": t3], store: two),
        Case(label: "harken add: already on it keeps its place", function: "add_to_playlist", ctx: alice, autos: ["added_ms": .int(9)], args: ["playlist_id": p1, "media_id": t2], store: two),
        Case(label: "harken add: not in the library", function: "add_to_playlist", ctx: alice, autos: ["added_ms": .int(9)], args: ["playlist_id": p1, "media_id": gone], store: two),
        Case(label: "harken remove: yours", function: "remove_from_playlist", ctx: alice, autos: [:], args: ["playlist_id": p1, "media_id": t1], store: two),
        Case(label: "harken remove: not yours", function: "remove_from_playlist", ctx: bob, autos: [:], args: ["playlist_id": p1, "media_id": t1], store: two),
        Case(label: "harken remove: not there", function: "remove_from_playlist", ctx: alice, autos: [:], args: ["playlist_id": p1, "media_id": gone], store: two),
        Case(label: "harken playlists", function: "playlists", ctx: alice, autos: [:], args: [:], store: renamed),
        Case(label: "harken playlists: nobody's", function: "playlists", ctx: nobody, autos: [:], args: [:], store: two),
        Case(label: "harken playlists: another's", function: "playlists", ctx: bob, autos: [:], args: [:], store: two),
        Case(label: "harken playlists_of: on one", function: "playlists_of", ctx: alice, autos: [:], args: ["media_id": t1], store: two),
        Case(label: "harken playlists_of: on none", function: "playlists_of", ctx: alice, autos: [:], args: ["media_id": t3], store: two),
        Case(label: "harken playlists_of: another's", function: "playlists_of", ctx: bob, autos: [:], args: ["media_id": t1], store: two),
        Case(label: "harken playlist: in playlist order", function: "playlist", ctx: alice, autos: [:], args: ["playlist_id": p1], store: two),
        Case(label: "harken playlist: not yours", function: "playlist", ctx: bob, autos: [:], args: ["playlist_id": p1], store: two),
        Case(label: "harken playlist: no such", function: "playlist", ctx: alice, autos: [:], args: ["playlist_id": p2], store: two),
        Case(label: "harken library: against a playlist", function: "library", ctx: alice, autos: [:], args: ["playlist_id": p1], store: two),
        Case(label: "harken library: against none", function: "library", ctx: nobody, autos: [:], args: ["playlist_id": p3], store: two),
    ]
}

func authoringTests(_ vectorModule: ArkDB.Module) throws {
    let demoObj = try loadJSON(vectors.appendingPathComponent("module").appendingPathComponent("demo.json"))
    let want = try hexBytes(demoObj, "bytes")

    run("authoring/demo-emit") {
        let m = ArkDemo.module()
        let mine = m.emit()
        if mine != want {
            let a = try Decode.fromValue(try Canon.decode(mine))
            for (x, y) in zip(a.functions, vectorModule.functions) where x != y {
                print("  differs: \(x.name)\n    emitted \(Encode.functionValue([:], x).brief)")
            }
            if a.schema != vectorModule.schema { print("  the schemas differ") }
            if a.routers != vectorModule.routers { print("  the routers differ: \(a.routers)") }
        }
        check("emit() of the Swift demo is spec/vectors/module/demo.json's bytes", mine == want)
        check("its hash is the vector's", Hex.encode(m.hash) == (try string(demoObj, "hash")))
        check("every procedure's hash is a closure of the vector's module",
              Set(m.procedures().map { $0.0 }) == Set(vectorModule.functions.map { Hash.functionHash(Hash.closure(vectorModule, $0)) }))
        check("emit is stable", m.emit() == mine && ArkDemo.module().emit() == mine)
    }

    run("authoring/demo-native-agrees-with-eval") {
        let m = try ArkDemo.module().ir()
        let cases = try demoCases(m)
        check("every demo procedure has a case", Set(cases.map { $0.function }) == Set(m.functions.map { $0.name }))
        let bad = try agreement(m, demoNatives(), cases)
        check("Native agrees with Ark.Eval on \(cases.count) cases over every demo procedure", bad.isEmpty, bad.joined(separator: "; "))
    }

    // The checker must be able to fail: a native add_to_playlist that steps
    // by two is caught on the case that appends after the last item.
    run("authoring/agreement-falsified") {
        let m = try ArkDemo.module().ir()
        var natives = demoNatives()
        let addFn = m.lookupFunction("add_to_playlist")!
        let h = Hash.functionHash(Hash.closure(m, addFn))
        let honest = natives[h]!
        natives[h] = Procedure(function: honest.function, mutate: { ctx, autos, args, st in
            guard case .applied(let s2, let chs) = try honest.mutate!(ctx, autos, args, st) else { return try honest.mutate!(ctx, autos, args, st) }
            var bent: [Change] = []
            for ch in chs {
                if case .add(let t, var row) = ch, case .int(let p)? = row["pos"] { row["pos"] = .int(p + 1); bent.append(.add(t, row)) } else { bent.append(ch) }
            }
            return .applied(s2.applying([]), bent)
        }, query: nil)
        let bad = try agreement(m, natives, try demoCases(m))
        check("a native procedure that disagrees is caught", !bad.isEmpty && bad.allSatisfy { $0.hasPrefix("add:") }, bad.joined(separator: "; "))
    }

    run("authoring/harken-emits-and-agrees") {
        let hm = HarkenDomain.module()
        let m = try hm.ir()
        let phoneNames = ["library", "create_playlist", "add_to_playlist", "remove_from_playlist", "playlists", "playlists_of", "playlist"]
        check("harken's phone domain verifies", Verify.verify(m).isSuccessV)
        check("routers: library, and playlists with its one middleware", m.routers == [ArkDB.Router(name: "library", uses: []),
                                                                                     ArkDB.Router(name: "playlists", uses: ["owned"])])
        check("the procedures are the seven the phone calls", Set(m.functions.filter { $0.kind.isProcedure }.map { $0.name }) == Set(phoneNames))
        check("each procedure runs the chain it was built from",
              m.lookupFunction("create_playlist")?.uses == [] && m.lookupFunction("add_to_playlist")?.uses == ["owned"]
              && m.lookupFunction("remove_from_playlist")?.uses == ["owned"] && m.lookupFunction("playlist")?.uses == ["owned"]
              && m.lookupFunction("playlists")?.uses == [] && m.lookupFunction("playlists_of")?.uses == [] && m.lookupFunction("library")?.uses == [])
        check("owned provides the row it read", m.lookupFunction("owned")?.ret == RowSchemaProbe.playlistRowTy(m))
        var natives: [FnHash: Procedure] = [:]
        for (h, p) in hm.procedures() { natives[h] = p }
        check("a native procedure per route", natives.count == 7)
        let cases = try harkenCases(m)
        check("every harken procedure has a case", Set(cases.map { $0.function }) == Set(phoneNames))
        let bad = try agreement(m, natives, cases)
        check("Native agrees with Ark.Eval on \(cases.count) cases over every harken procedure", bad.isEmpty, bad.joined(separator: "; "))
        // What the cases came to, so that agreeing on a wrong answer is not
        // enough: the renaming, the order, the refusals.
        func applied(_ label: String) throws -> Eval.Outcome {
            let c = cases.first { $0.label == label }!
            return try Eval.applyClosure(m.schema, Hash.closure(m, m.lookupFunction(c.function)!), c.ctx, c.autos, c.args, c.store)
        }
        func answered(_ label: String) throws -> String {
            let c = cases.first { $0.label == label }!
            return resultText(try Eval.queryResult(m.schema, Hash.closure(m, m.lookupFunction(c.function)!), c.args, c.store, ctx: c.ctx))
        }
        func named(_ o: Eval.Outcome) -> [Value] {
            guard case .applied(_, let chs) = o else { return [] }
            return chs.compactMap { if case .add("playlist", let row) = $0 { return row["name"] }; return nil }
        }
        check("a name she has becomes \"Favorites (1)\"", named(try applied("harken create: a name she has is numbered")) == [.text("Favorites (1)")])
        check("then \"Favorites (2)\"", named(try applied("harken create: the smallest free number")) == [.text("Favorites (2)")])
        check("another person's name is not hers", named(try applied("harken create: another's name is not taken")) == [.text("Favorites")])
        check("bob at alice's playlist is refused, and says why", outcomeText(try applied("harken add: not yours")) == "refused not your playlist")
        func titles(_ label: String) throws -> [Value] {
            let c = cases.first { $0.label == label }!
            guard case .success(let v) = try Eval.queryResult(m.schema, Hash.closure(m, m.lookupFunction(c.function)!), c.args, c.store, ctx: c.ctx) else { return [] }
            return try v.asList().map { $0.field("title") }
        }
        check("a playlist lists in its own order, not the library's", try titles("harken playlist: in playlist order") == [.text("Aria"), .text("Air")])
        check("the library lists in its order", try titles("harken library: against a playlist") == [.text("Air"), .text("Aria"), .text("Spring")])
        check("and a playlist that is not hers is refused", try answered("harken playlist: not yours").contains("not your playlist"))
        // Editing a middleware re-hashes every procedure behind it.
        var edited = m
        edited.functions = m.functions.map { f in
            guard f.name == "owned" else { return f }
            var g = f
            g.body = [g.body[0]] + g.body
            return g
        }
        func hashOf(_ mm: ArkDB.Module, _ n: String) -> [UInt8] { return Hash.functionHash(Hash.closure(mm, mm.lookupFunction(n)!)) }
        check("editing owned re-hashes what it guards, and nothing else",
              ["add_to_playlist", "remove_from_playlist", "playlist"].allSatisfy { hashOf(edited, $0) != hashOf(m, $0) }
              && ["create_playlist", "playlists", "playlists_of", "library"].allSatisfy { hashOf(edited, $0) == hashOf(m, $0) })
        // Against harken.ark: the phone's entries are accepted by a server
        // running the whole module only if every procedure hashes as there.
        let ark = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("harken/domain/harken.ark")
        let full = try Decode.fromValue(try Canon.decode([UInt8](try Data(contentsOf: ark))))
        check("harken.ark is a spec-\(specVersion) module", full.spec == specVersion)
        check("the phone's schema is harken.ark's", m.schema == full.schema)
        for f in m.functions where f.kind.isProcedure {
            guard let g = full.lookupFunction(f.name) else { check("harken.ark has \(f.name)", false); continue }
            check("\(f.name) hashes as harken.ark's (Rust) does", hashOf(m, f.name) == hashOf(full, g.name))
            if hashOf(m, f.name) != hashOf(full, g.name), ProcessInfo.processInfo.environment["ARK_DIFF"] != nil {
                for n in [f.name] + f.uses {
                    let x = m.lookupFunction(n)!, y = full.lookupFunction(n)!
                    if x != y { print("SWIFT \(n): \(Encode.functionValue([:], x))\nRUST  \(n): \(Encode.functionValue([:], y))") }
                }
            }
        }
    }

    run("authoring/form-validator") {
        let m = try ArkDemo.module().ir()
        let st = try seeded(m, [("create_playlist", ArkDB.Ctx(user: "alice", session: "a"), ["id": .id(rawId(1))], ["name": .text("Favorites")])])
        let c = Hash.closure(m, m.lookupFunction("create_playlist")!)
        let (msgs, vals) = try Eval.check(m.schema, c, ["name": .text("  ")], st)
        check("an empty name says why, under its field", msgs.count == 1 && msgs[0].0 == "name" && msgs[0].1 == "a playlist needs a name")
        check("and the value comes back trimmed", vals["name"] == .text(""))
        let (ok, okVals) = try Eval.check(m.schema, c, ["name": .text(" Road ")], st)
        check("a good name says nothing and is trimmed", ok.isEmpty && okVals["name"] == .text("Road"))
        let c2 = Hash.closure(m, m.lookupFunction("add_to_playlist")!)
        let (partial, _) = try Eval.check(m.schema, c2, ["playlist_id": .id(rawId(9))], st)
        check("a partial input checks only what is present", partial.map { $0.0 } == ["playlist_id"] && partial.first?.1 == "playlist_id: no such playlist")
    }

    run("authoring/verifier-rules") {
        let m = try HarkenDomain.module().ir()
        func complaints(_ f: (inout ArkDB.Module) -> Void) -> [String] {
            var mm = m
            f(&mm)
            if case .failure(let e) = Verify.verify(mm) { return e.errors.map { $0.complaint } }
            return []
        }
        func edit(_ mm: inout ArkDB.Module, _ n: String, _ g: (inout Function) -> Void) {
            mm.functions = mm.functions.map { f in var h = f; if f.name == n { g(&h) }; return h }
        }
        check("UsesNotOnRouter", complaints { edit(&$0, "create_playlist") { $0.uses = ["owned", "signed_in"] } }.contains("UsesNotOnRouter"))
        check("OnNotUnique", complaints { edit(&$0, "create_playlist") { f in
            f.body = f.body.map { if case .sInsert(let t, let e, _) = $0 { return .sInsert(t, e, ["name"]) }; return $0 } } }.contains { $0.hasPrefix("OnNotUnique") })
        check("NotProvided", complaints { edit(&$0, "playlists") { $0.body.insert(.sLet(99, .provided("owned")), at: 0) } }.contains { $0.hasPrefix("NotProvided") })
        check("MiddlewareInput", complaints { edit(&$0, "playlist") { $0.input = [] } }.contains { $0.hasPrefix("MiddlewareInput") })
        check("an exists check on an id of a table that is not there", complaints { edit(&$0, "add_to_playlist") { f in
            f.input = f.input.map { var x = $0; if x.name == "media_id" { x.field = Field(.id("nowhere"), checks: [.exists(nil)]) }; return x } } }.contains("UnknownTable nowhere"))
        check("an id column that is neither its table's key nor a reference", complaints {
            $0.schema.tables[0].columns.append(Column("stray_id", .id($0.schema.tables[0].name))) }.contains { $0.hasPrefix("BadSchema IdColumnWithoutRef") })
        check("routers: a use that is not middleware", complaints { $0.routers[1].uses.append("playlists") }.contains { $0.hasPrefix("NotMiddleware") })
        check("the module as emitted draws no complaint", complaints { _ in }.isEmpty)
    }
}

enum RowSchemaProbe {
    static func playlistRowTy(_ m: ArkDB.Module) -> Ty? { return m.schema.lookupTable("playlist")?.rowTy }
}

extension Result {
    var isSuccessV: Swift.Bool {
        if case .success = self { return true }
        return false
    }
}
