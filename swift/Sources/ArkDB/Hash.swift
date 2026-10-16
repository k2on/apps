// §8 Hashes, as `rust/ark/src/hash.rs` defines them: SHA-256 over canonical
// CBOR — the state hash (§8.1), which moves with the store, and the
// function hash (§8.2).
//
// §8.1, exactly, with `enc` the canonical encoding and `‖` concatenation:
//
//     leaf(t, row)  = sha256(enc(Text t) ‖ enc(Struct row))
//     digest(t)     = Σ leaf(t, row) over the rows of t, mod 2^256
//     state_hash(s) = sha256(enc(List [List [Text t, Bytes digest(t)]
//                                      for every table t, in schema order]))
//
// A sum does not depend on order and moves by one addition and one
// subtraction per row written, so a store keeps its digests beside its
// rows (`MemoryStore`) and a `Verify` costs one SHA-256 over the tables.

/// The 32-byte hash an entry names its function by (`hash::FnHash`).
public typealias FnHash = [UInt8]

/// §8.3 A function with the helpers it was verified against: every helper
/// and middleware it reaches, normalised, in declaration order — a complete
/// program run without consulting any module.
public struct Closure: Hashable {
    public let function: Function
    public let helpers: [Function]
    public init(function: Function, helpers: [Function]) {
        self.function = function
        self.helpers = helpers
    }
}

/// §8.4 A live state with everything it runs (`hash::LiveClosure`): the
/// state, normalised; the declarations of every named type it reaches, in
/// code point order of their names; and every helper its init, scope and
/// transitions reach, in the module's order — a complete program run without
/// consulting any module.
public struct LiveClosure {
    public let state: LiveStateDecl
    public let decls: [Named<Ty>]
    public let helpers: [Function]
    /// The declarations as a map, which is how a named type is resolved.
    public let types: [String: Ty]
    /// `state_ty`: the state's type, resolved, its ephemeral markers kept.
    public let stateTy: Ty
    /// The fields of the state that are ephemeral.
    public let ephemeral: [String]

    public init(state: LiveStateDecl, decls: [Named<Ty>], helpers: [Function]) {
        self.state = state
        self.decls = decls
        self.helpers = helpers
        var m = [String: Ty]()
        for d in decls { m[d.name] = d.value }
        types = m
        stateTy = (try? Types.resolve(m, state.ty)) ?? state.ty
        ephemeral = Types.ephemeralFields(stateTy)
    }

    /// `kind`: a hook, and a name the state does not declare, are ordered.
    public func kind(_ name: String) -> Kind {
        guard let t = state.transitions.first(where: { $0.name == name }) else { return .ordered }
        return Ordering.classify(stateTy, t)
    }

    /// §8.4 `value_hash`: SHA-256 of the canonical encoding of an instance's
    /// ordered part, every ephemeral field left out — what an `applied` frame
    /// carries and a replica compares.
    public func valueHash(_ v: Value) -> [UInt8] { Sha256.hash(Canon.encode(Types.orderedPart(ephemeral, v))) }
}

/// §8.1 A row's leaf: `sha256(enc(Text table) ‖ enc(Struct row))` (`hash::leaf`).
public func leaf(_ table: String, _ row: Row) -> [UInt8] {
    var b = Canon.encode(.text(table))
    Canon.encode(.record(row), into: &b)
    return Sha256.hash(b)
}

/// §8.1 A table's digest: the sum of its rows' leaves modulo 2^256, 32
/// big-endian bytes, written out here byte by byte rather than through a
/// big-integer library (`hash::Digest`).
public struct Digest: Hashable, CustomStringConvertible {
    public var bytes: [UInt8]

    public init() { bytes = [UInt8](repeating: 0, count: 32) }
    public init(bytes: [UInt8]) { self.bytes = bytes }

    /// This digest with a leaf added, mod 2^256.
    public mutating func add(_ leaf: [UInt8]) {
        var carry: UInt16 = 0
        var i = 31
        while i >= 0 {
            let s = UInt16(bytes[i]) + UInt16(leaf[i]) + carry
            bytes[i] = UInt8(s & 0xff)
            carry = s >> 8
            i -= 1
        }
    }

    /// This digest with a leaf taken away, mod 2^256: the inverse of `add`.
    public mutating func sub(_ leaf: [UInt8]) {
        var borrow: Int16 = 0
        var i = 31
        while i >= 0 {
            var d = Int16(bytes[i]) - Int16(leaf[i]) - borrow
            if d < 0 {
                d += 256
                borrow = 1
            } else {
                borrow = 0
            }
            bytes[i] = UInt8(d)
            i -= 1
        }
    }

    public var description: String { "Digest(\(Hex.encode(bytes)))" }
}

public enum Hash {
    /// `table_digest`: the digest of some rows, summed one by one — the definition.
    public static func tableDigest<S: Sequence>(_ table: String, _ rows: S) -> Digest where S.Element == Row {
        var d = Digest()
        for r in rows { d.add(leaf(table, r)) }
        return d
    }

    /// `state_hash_of`: the SHA-256 of the list of `[name, digest]` pairs, in schema order.
    public static func stateHashOf(_ digests: [(String, Digest)]) -> [UInt8] {
        Sha256.hash(Canon.encode(.list(digests.map { .list([.text($0.0), .bytes($0.1.bytes)]) })))
    }

    /// §8.1 `state_hash`: every table of the store's schema, in order, with the
    /// digest the store keeps for it — or, for a store that keeps none, the
    /// digest of a scan.
    public static func stateHash(_ st: Store) -> [UInt8] {
        stateHashOf(st.schema.tables.map { t in (t.name, st.digest(t.name) ?? tableDigest(t.name, st.scan(t.name))) })
    }

    /// `closure`: a function normalised, with the helpers and middleware it
    /// reaches, normalised, in the module's order.
    public static func closure(_ m: Module, _ f: Function) -> Closure {
        var seen = [String]()
        var todo = Array(Encode.reaches(f).reversed())
        while let n = todo.popLast() {
            if seen.contains(n) { continue }
            guard let h = m.lookupFunction(n) else { continue }
            seen.append(n)
            todo.append(contentsOf: Encode.reaches(h).reversed())
        }
        return Closure(function: Normalize.function(f), helpers: m.functions.filter { seen.contains($0.name) }.map(Normalize.function))
    }

    /// `closures`: every function of a module by the hash of its closure, as lowercase hex.
    public static func closures(_ m: Module) -> SortedMap<String, Closure> {
        var out = SortedMap<String, Closure>()
        for f in m.functions {
            let c = closure(m, f)
            out[Hex.encode(functionHash(c))] = c
        }
        return out
    }

    /// §8.2 `function_hash`: over the function's normalised canonical form,
    /// names excluded, with the hashes of the helpers it calls and the
    /// middleware it uses directly — over the function as a client's module
    /// carries it, every private block stripped (`docs/plan-guards.md` D3).
    public static func functionHash(_ c: Closure) -> FnHash {
        let pub = strip(c.function)
        var deps = [(String, Value)]()
        for n in Encode.reaches(pub) {
            guard let h = c.helpers.first(where: { $0.name == n }) else { continue }
            deps.append((n, .bytes(functionHash(Closure(function: h, helpers: c.helpers)))))
        }
        return Sha256.hash(Canon.encode(Encode.function(deps, pub)))
    }

    /// D3 `stripped`: a closure as a client may hold it — its function stripped
    /// of private blocks and its helpers only those the stripped function
    /// reaches. The same hash as the closure it came from.
    public static func stripped(_ c: Closure) -> Closure {
        if !c.function.isPrivate { return c }
        let function = strip(c.function)
        var seen = [String]()
        var todo = Encode.reaches(function)
        while let n = todo.popLast() {
            if seen.contains(n) { continue }
            guard let h = c.helpers.first(where: { $0.name == n }) else { continue }
            todo.append(contentsOf: Encode.reaches(h))
            seen.append(n)
        }
        return Closure(function: Normalize.function(function), helpers: c.helpers.filter { seen.contains($0.name) })
    }

    /// D3 `is_private`: entries naming this closure are taken by the authority's facts.
    public static func isPrivate(_ c: Closure) -> Bool { c.function.isPrivate }

    /// D1 `reads_roles`: whether what a closure writes can depend on its
    /// author's roles — `has_role` in the function or a helper or provide it
    /// runs; not in a guard or a scope.
    public static func readsRoles(_ c: Closure) -> Bool {
        func asks(_ v: Value) -> Bool {
            switch v {
            case .record(let r): return r["t"] == .text("has_role") || r.values.contains(where: asks)
            case .list(let xs): return xs.contains(where: asks)
            default: return false
            }
        }
        return ([c.function] + c.helpers).filter { $0.kind != .guardFn && $0.kind != .scope }.contains { asks(Encode.function([], $0)) }
    }

    /// §8.4 `live_closure`: the closure of a live state within a module.
    public static func liveClosure(_ m: Module, _ st: LiveStateDecl) -> LiveClosure {
        var seen = [String]()
        var todo = Array(Encode.stateCalls(st).reversed())
        while let n = todo.popLast() {
            if seen.contains(n) { continue }
            guard let h = m.lookupFunction(n) else { continue }
            seen.append(n)
            todo.append(contentsOf: Encode.reaches(h).reversed())
        }
        let all = m.typeMap()
        var types = [String: Ty]()
        var want: [Ty] = [st.ty]
        var exprs: [Expr] = [st.initial, st.scope]
        for t in st.allTransitions {
            want.append(contentsOf: t.input.map { $0.value.ty })
            exprs.append(contentsOf: t.guards.map { $0.e })
            for s in t.body { stmtTypes(s, &want) }
        }
        for e in exprs { exprTypes(e, &want) }
        while let t = want.popLast() {
            namedIn(t) { n in
                if types[n] == nil, let d = all[n] {
                    types[n] = d
                    want.append(d)
                }
            }
        }
        let decls = types.map { Named($0.key, $0.value) }.sorted { compareText($0.name, $1.name) < 0 }
        return LiveClosure(state: Normalize.state(st), decls: decls, helpers: m.functions.filter { seen.contains($0.name) }.map(Normalize.function))
    }

    /// `live_closures`: every live state of a module, by name, with its closure.
    public static func liveClosures(_ m: Module) -> [String: LiveClosure] {
        var out = [String: LiveClosure]()
        for s in m.states where out[s.name] == nil { out[s.name] = liveClosure(m, s) }
        return out
    }

    private static func namedIn(_ t: Ty, _ f: (String) -> Void) {
        switch t {
        case .named(let n): f(n)
        case .option(let x), .list(let x), .nonEmpty(let x), .perDevice(let x), .ephemeral(let x): namedIn(x, f)
        case .structure(let fs): for x in fs.types { namedIn(x, f) }
        case .sum(let vs): for fs in vs.variants { for x in fs.types { namedIn(x, f) } }
        default: break
        }
    }

    private static func stmtTypes(_ s: Stmt, _ out: inout [Ty]) {
        switch s {
        case .letS(_, let e), .refuse(let e): exprTypes(e, &out)
        case .ret(let e): if let e = e { exprTypes(e, &out) }
        case .ifS(let c, let a, let b):
            exprTypes(c, &out)
            for x in a + b { stmtTypes(x, &out) }
        case .forS(_, let o, let body):
            exprTypes(o, &out)
            for x in body { stmtTypes(x, &out) }
        default: break
        }
    }

    private static func exprTypes(_ e: Expr, _ out: inout [Ty]) {
        switch e {
        case .variant(let t, _, _), .none(let t): out.append(t)
        default: break
        }
        for c in children(e) { exprTypes(c, &out) }
    }

    /// §8.4 `live_hash`: the hash of a live state's closure — the state's
    /// value, transitions normalised, with the hashes of the helpers it
    /// reaches as `deps`, and the declarations of the types it reaches.
    public static func liveHash(_ c: LiveClosure) -> FnHash {
        var deps = [(String, Value)]()
        for n in Encode.stateCalls(c.state) {
            guard let h = c.helpers.first(where: { $0.name == n }) else { continue }
            deps.append((n, .bytes(functionHash(Closure(function: h, helpers: c.helpers)))))
        }
        let v = Value.record(Record([
            ("t", .text("live_closure")),
            ("state", Encode.state(deps, c.state)),
            ("types", .list(c.decls.map { Encode.typeDecl($0.name, $0.value) })),
        ]))
        return Sha256.hash(Canon.encode(v))
    }

    /// §8.4 `state_type_hash`: the hash of a live state's type, resolved —
    /// what names the snapshot a server keeps of an instance. Ephemeral
    /// markers are part of it.
    public static func stateTypeHash(_ c: LiveClosure) -> [UInt8] { Sha256.hash(Canon.encode(Encode.ty(c.stateTy))) }

    /// `module_hash`: the hash of a whole module, normalised.
    public static func moduleHash(_ m: Module) -> [UInt8] { Sha256.hash(Canon.encode(Encode.module(Normalize.module(m)))) }
}
