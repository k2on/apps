// The check of each vector kind, as spec/RUNNERS.md says it and
// rust/ark/tests/vectors.rs reads it: each throws `Failed` with its first
// unmet claim, so that one definition runs both ways — a vector must pass
// it and a `falsify/` vector must fail it.
import ArkDB

enum Checks {
    static var root = "../spec/vectors"

    /// A vector by its path under spec/vectors, parsed.
    static func vector(_ path: String) throws -> Value { try readJson(root + "/" + path) }

    static func moduleOf(_ v: Value) throws -> Module { try Decode.module(v) }

    // codec ---------------------------------------------------------------

    /// `check_codec`: the value encodes to the bytes, and the bytes decode to the value.
    static func codec(_ v: Value) throws {
        let value = try v.field("value")
        let bytes = try hexBytes(try v.field("bytes"))
        try ensureBytes(Canon.encode(value), bytes, "encode")
        try ensureEq(try Canon.decode(bytes), value, "decode")
    }

    // order ---------------------------------------------------------------

    /// `check_order`: the input sorted by the total order is the sorted list.
    static func order(_ v: Value) throws {
        let input = try v.field("input").list().sorted { compareValue($0, $1) < 0 }
        try ensureEq(Value.list(input), try v.field("sorted"), "sorted")
    }

    // hash ----------------------------------------------------------------

    // The sum of two 256-bit big-endian integers, modulo 2^256: written here
    // rather than taken from `Digest`, so that the runner checks the
    // construction (§8.1) and not the runtime's arithmetic against itself.
    private static func sum256(_ a: [UInt8], _ b: [UInt8]) -> [UInt8] {
        var out = [UInt8](repeating: 0, count: 32)
        var carry = 0
        for i in stride(from: 31, through: 0, by: -1) {
            let s = Int(a[i]) + Int(b[i]) + carry
            out[i] = UInt8(s & 0xff)
            carry = s >> 8
        }
        return out
    }

    /// `check_hash`: the store's own state hash; and, where the vector pins
    /// the construction part by part (`tables`), every leaf, every digest and
    /// the hash of the pairs, each from the canonical encoding and SHA-256 alone.
    static func hash(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let st = MemoryStore.fromValue(m.schema, try v.field("store"))
        let want = try v.field("hash").text()
        guard let tables = v.opt("tables") else {
            try ensureEq(Hex.encode(Hash.stateHash(st)), want, "state hash")
            return
        }
        let names = m.schema.tables.map { $0.name }
        let ts = try tables.list()
        try ensureEq(ts.count, names.count, "one entry per table of the schema")
        var pairs = [Value]()
        for (t, name) in zip(ts, names) {
            try ensureEq(try t.field("table").text(), name, "the tables in schema order")
            var digest = [UInt8](repeating: 0, count: 32)
            let rows = try t.field("rows").list()
            try ensureEq(rows.count, st.scan(name).count, "\(name): every row of the store")
            for r in rows {
                let l = Sha256.hash(Canon.encode(.text(name)) + Canon.encode(try r.field("row")))
                try ensureEq(Hex.encode(l), try r.field("leaf").text(), "\(name): a row's leaf")
                try ensureBytes(leaf(name, try r.field("row").record()), l, "\(name): the runtime's leaf")
                digest = sum256(digest, l)
            }
            try ensureEq(Hex.encode(digest), try t.field("digest").text(), "\(name): the digest is the sum of the leaves")
            pairs.append(.list([.text(name), .bytes(digest)]))
        }
        try ensureEq(Hex.encode(Sha256.hash(Canon.encode(.list(pairs)))), want, "the hash of the pairs")
        try ensureEq(Hex.encode(Hash.stateHash(st)), want, "state hash")
    }

    // module --------------------------------------------------------------

    /// `check_module`: the bytes decode to the module value; it reads as a
    /// module of spec 4 that writes back as itself and encodes to the bytes;
    /// its module hash; its schema well formed. A `stripped` vector also: the
    /// client's module beside it, the server's with its private blocks
    /// stripped and verified, at the public hash, and both naming every
    /// function by the listed hash.
    static func module(_ v: Value) throws {
        let value = try v.field("module")
        let bytes = try hexBytes(try v.field("bytes"))
        try ensureEq(try Canon.decode(bytes), value, "the bytes decode to the module value")
        let m = try moduleOf(value)
        try ensureEq(m.spec, SPEC_VERSION, "spec version")
        try ensureEq(Encode.module(m), value, "decode then encode is the identity")
        try ensureBytes(Canon.encode(Encode.module(m)), bytes, "the module encodes to the bytes")
        try ensureEq(Hex.encode(Hash.moduleHash(m)), try v.field("hash").text(), "module hash")
        try ensure(checkSchema(m.schema).isEmpty, "the schema is not well formed")
        guard let sval = v.opt("stripped") else { return }
        try ensureEq(try Canon.decode(try hexBytes(try v.field("strippedBytes"))), sval, "the stripped bytes decode to the stripped module")
        let stripped = try moduleOf(sval)
        // The one step a client may skip; this runtime takes it.
        let want = try Verify.verify(stripModule(m))
        try ensureEq(Encode.module(want), sval, "the stripped module is the module with its private blocks taken out")
        try ensureEq(Hex.encode(Hash.moduleHash(stripped)), try v.field("public").text(), "the public module hash")
        func names(_ m: Module) -> [String] {
            Hash.closures(m).entries.map { ($0.value.function.name, $0.key) }
                .sorted { compareText($0.0, $1.0) != 0 ? compareText($0.0, $1.0) < 0 : compareText($0.1, $1.1) < 0 }
                .map { "\($0.0) \($0.1)" }
        }
        let listed = try v.field("functions").list().map { p -> String in
            let xs = try p.list()
            return "\(try xs[0].text()) \(try xs[1].text())"
        }
        try ensureEq(names(m), listed, "the server's module names its functions by the listed hashes")
        try ensureEq(names(stripped), listed, "and so does the client's")
    }

    // verify --------------------------------------------------------------

    /// `check_verify`: it verifies, as itself; its schema, its spec; every function has a closure.
    static func verify(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        try ensure(try v.field("verifies") == .bool(true), "only accepting modules are checked here")
        try ensure(checkSchema(m.schema).isEmpty, "the schema is not well formed")
        try ensureEq(m.spec, SPEC_VERSION, "spec version")
        let verified = try Verify.verify(m)
        try ensureEq(Encode.module(verified), Encode.module(m), "a verified module is its own verified form")
        try ensureEq(Hash.closures(m).count, m.functions.count, "one closure per function")
    }

    // eval ------------------------------------------------------------------

    /// A vector's `ctx`: the user, the login, and the roles where it says any.
    static func ctxOf(_ c: Value) throws -> Ctx {
        let roles = try c.opt("roles")?.list().map { try $0.text() } ?? []
        return Ctx(try c.field("user").text(), try c.field("session").text(), roles: roles)
    }

    static func argsOf(_ v: Value) throws -> [String: Value] {
        var out = [String: Value]()
        for (k, x) in try v.record().pairs { out[k] = x }
        return out
    }

    static func changesValue(_ cs: [Change]) -> Value { .list(cs.map(Facts.changeValue)) }

    /// The function hashes `rebase/three-peers.json`'s entries name, as hex.
    static let named: Set<String> = {
        guard let v = try? vector("rebase/three-peers.json"), let es = try? v.field("entries").list() else { return [] }
        return Set(es.compactMap { try? Hex.encode($0.field("fn").bytes()) })
    }()

    // A bug is a failure of the vector, named; a verdict is the caller's to judge.
    private static func ran(_ what: String, _ f: () throws -> Ran) throws -> Ran {
        do {
            return try f()
        } catch EvalFault.bug(let e) {
            throw Failed("\(what): bug \(e)")
        }
    }

    /// `check_eval`: a mutator applied step by step — its changes, the store
    /// and its hash after each — and again through its closure over a second
    /// copy, which must reach the same store.
    static func evalApply(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let name = try v.field("function").text()
        guard let f = m.lookupFunction(name) else { throw Failed("no function \(name)") }
        let fh = Hex.encode(Hash.functionHash(Hash.closure(m, f)))
        try ensure(named.contains(fh), "the function hash is not the one the log names")
        try ensureEq(fh, try v.field("function_hash").text(), "function_hash")
        let bodies = Hash.closures(m)
        let ctx = try ctxOf(try v.field("ctx"))
        let autos = try argsOf(try v.field("autos"))
        let st = MemoryStore.fromValue(m.schema, try v.field("store_before"))
        let byClosure = st.copy()
        for (i, step) in try v.field("steps").list().enumerated() {
            let args = try argsOf(try step.field("args"))
            var changes: [Change]
            switch try ran("step \(i)", { try Eval.apply(m, name, ctx, autos, args, st) }) {
            case .ok(let cs): changes = cs
            case .refused(let r): throw Failed("step \(i): refused \(r.show())")
            }
            try ensureEq(changesValue(changes), try step.field("changes"), "step \(i) changes")
            try ensureEq(st.storeValue(), try step.field("store_after"), "step \(i) store")
            try ensureEq(Hex.encode(Hash.stateHash(st)), try step.field("hash_after").text(), "step \(i) hash")
            switch try ran("step \(i) closure", { try Eval.applyClosure(m.schema, bodies[fh]!, ctx, autos, args, byClosure) }) {
            case .ok(let again): try ensure(again == changes, "step \(i) through the closure")
            case .refused(let r): throw Failed("step \(i): closure refused \(r.show())")
            }
        }
        try ensure(byClosure == st, "the closure map reached another store")
    }

    /// `eval_cases`: one store, each case over a fresh copy — a mutator that
    /// must apply (its changes compared where given) or be refused by exactly
    /// a text; or the form validator's messages and normalised values over a
    /// partial input.
    static func evalCases(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let bodies = Hash.closures(m)
        func byName(_ n: String) throws -> Closure {
            guard let c = bodies.values.first(where: { $0.function.name == n }) else { throw Failed("no function \(n)") }
            return c
        }
        let ctx0 = try v.opt("ctx").map(ctxOf) ?? Ctx("alice", "session-1")
        let st = MemoryStore.fromValue(m.schema, try v.opt("store_before") ?? v.field("store"))
        for c in try v.field("cases").list() {
            let name = try c.field("name").text()
            let fn = try byName(try c.field("function").text())
            let ctx = try c.opt("ctx").map(ctxOf) ?? ctx0
            if let refused = c.opt("refused") {
                let got = try ran(name) { try Eval.applyClosure(m.schema, fn, ctx, try argsOf(try c.field("autos")), try argsOf(try c.field("args")), st.copy()) }
                switch (refused, got) {
                case (.null, .ok(let cs)):
                    if let want = c.opt("changes") { try ensureEq(changesValue(cs), want, "\(name) changes") }
                case (.text(let why), .refused(.refused(let w))):
                    try ensureEq(w, why, name)
                default:
                    throw Failed("\(name): wanted \(refused.show()), got \(got)")
                }
            } else {
                let got: Checked
                do {
                    got = try Eval.check(m.schema, fn, ctx, try argsOf(try c.field("input")), st)
                } catch EvalFault.bug(let e) {
                    throw Failed("\(name): bug \(e)")
                }
                let want = try c.field("messages").list().map { "\(try $0.field("field").text())|\(try $0.field("message").text())" }
                try ensureEq(got.messages.map { "\($0.0)|\($0.1)" }, want, "\(name) messages")
                try ensureEq(Value.record(Record(got.values.map { ($0.key, $0.value) })), try c.field("normalised"), "\(name) normalised")
            }
        }
    }

    /// `check_raw`: a raw write named by the hash of its name, applied step by
    /// step — its changes, or the refusal's text with the store unmoved — and
    /// the store and its hash after each.
    static func evalRaw(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let name = try v.field("raw").text()
        let fh = Raw.hashOf(name)
        try ensureEq(Hex.encode(fh), try v.field("function_hash").text(), "function_hash")
        guard let raw = Raw.of(fh) else { throw Failed("\(name) is no raw write") }
        let st = MemoryStore.fromValue(m.schema, try v.field("store_before"))
        for (i, step) in try v.field("steps").list().enumerated() {
            let args = try argsOf(try step.field("args"))
            let before = st.copy()
            let got = try ran("step \(i)") { try Raw.apply(raw, args, st) }
            let want = step.opt("refused")
            switch (got, want) {
            case (.ok(let cs), nil): try ensureEq(changesValue(cs), try step.field("changes"), "step \(i) changes")
            case (.refused(let r), let w?):
                try ensureEq(r.show(), try w.text(), "step \(i) refusal")
                try ensure(st == before, "step \(i): a refused write moved the store")
            default: throw Failed("step \(i): got \(got), wanted \(want?.show() ?? "nothing")")
            }
            try ensureEq(st.storeValue(), try step.field("store_after"), "step \(i) store")
            try ensureEq(Hex.encode(Hash.stateHash(st)), try step.field("hash_after").text(), "step \(i) hash")
        }
    }

    /// `check_live` (`eval/live-*`, `docs/plan-live.md`): a live state's
    /// closure and type hashes, every transition's kind, its init and the hash
    /// of it, and a life of it — each step run over the state the last left,
    /// as the engine runs it (`Runner.run`: a leave empties the leaver's
    /// ephemeral slots first), its new state and that state's hash with the
    /// ephemeral fields left out, or the refusal's words.
    static func evalLive(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let name = try v.field("live").text()
        guard let st = m.lookupState(name) else { throw Failed("the module has no state \(name)") }
        let c = Hash.liveClosure(m, st)
        try ensureEq(Hex.encode(Hash.liveHash(c)), try v.field("live_hash").text(), "the live hash")
        try ensureEq(Hex.encode(Hash.stateTypeHash(c)), try v.field("type_hash").text(), "the type hash")
        for (t, k) in try v.field("kinds").record().pairs {
            try ensureEq(Value.text(c.kind(t).rawValue), k, "the kind of \(t)")
        }
        let r = Runner.ofModule(m)
        var s = try r.initial(name)
        try ensureEq(s, try v.field("init"), "the init")
        try ensureEq(Hex.encode(r.hash(name, s)), try v.field("init_hash").text(), "the init's hash")
        for (k, step) in try v.field("steps").list().enumerated() {
            let t = try step.field("transition").text()
            let cv = try step.field("ctx")
            let ctx = LiveCtx(user: try cv.field("user").text(), session: try cv.field("session").text(), roles: try cv.field("roles").list().map { try $0.text() }, device: try cv.field("device").text(), now: try cv.field("now").int())
            let want = step.opt("refused")
            let got: Value
            do {
                got = try r.run(name, t, s, ctx, try argsOf(try step.field("input")))
            } catch EvalFault.verdict(let refusal) {
                guard case .refused(let why) = refusal else { throw Failed("step \(k) (\(t)): refused \(refusal)") }
                guard let w = want else { throw Failed("step \(k) (\(t)): refused \(why), the vector says it applies") }
                try ensureEq(why, try w.text(), "step \(k) (\(t)): the refusal")
                continue
            }
            if let w = want { throw Failed("step \(k) (\(t)): applied, the vector says refused \(w.show())") }
            try ensureEq(got, try step.field("state"), "step \(k) (\(t)): the state")
            try ensureEq(Hex.encode(r.hash(name, got)), try step.field("hash").text(), "step \(k) (\(t)): its hash")
            s = got
        }
    }

    // views -----------------------------------------------------------------

    /// `check_view`: a query kept up to date — its plan as the module writes
    /// it; hydrated over `store_before` in the vector's context and
    /// arguments, after its middleware; then each batch applied to the store
    /// whole and pushed as one, and after each the patches (where the step
    /// has them), the answer, the contract and the splice.
    static func views(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let sch = m.schema
        let name = try v.field("query").text()
        guard let f = m.lookupFunction(name) else { throw Failed("no query \(name)") }
        guard let plan = f.plan else { throw Failed("\(name) is not a plan") }
        let written = try v.field("module").field("functions").list().first { $0.opt("name") == .text(name) }?.opt("plan")
        try ensureEq(written ?? .null, try v.field("plan"), "the plan is the query's, as the module writes it")
        let ctx = try ctxOf(try v.field("ctx"))
        let c = Hash.closure(m, f)
        let st = MemoryStore.fromValue(sch, try v.field("store_before"))
        let args: [String: Value]
        let provided: [String: Value]
        do {
            (args, provided) = try Eval.middleware(sch, c, ctx, try argsOf(try v.field("args")), st)
        } catch let x as EvalFault {
            throw Failed("middleware: \(x)")
        }
        let view: MaintainedView
        do {
            view = try Views.hydrate(sch, plan, ViewEnv(c.helpers, ctx, args, provided), st)
        } catch let x as EvalFault {
            throw Failed("hydrate: \(x)")
        }
        try ensureEq(Value.list(view.rows()), try v.field("rows_before"), "the answer at hydrate")
        let batches = try v.field("batches").list()
        let steps = try v.field("steps").list()
        try ensureEq(batches.count, steps.count, "a step per batch")
        for (i, (batch, step)) in zip(batches, steps).enumerated() {
            let changes = try batch.list().map(Facts.changeFromValue)
            let before = view.rows()
            st.applyChanges(changes)
            let patches: [Patch]
            do {
                patches = try Views.pushAll(sch, st, changes, view)
            } catch let x as EvalFault {
                throw Failed("batch \(i): \(x)")
            }
            try ensure(Views.contract(sch, st, view), "batch \(i) breaks the contract")
            // A step the fuzzer wrote has no patches: only the answer, the contract and the splice.
            if let want = step.opt("patches") { try ensureEq(Value.list(patches.map(Views.patchValue)), want, "batch \(i) patches") }
            try ensureEq(Value.list(view.rows()), try step.field("rows"), "batch \(i) rows")
            try ensureEq(Views.splice(patches, before), view.rows(), "batch \(i) splice")
        }
    }
}
