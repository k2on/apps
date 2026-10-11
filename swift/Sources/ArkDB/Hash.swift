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

    /// `module_hash`: the hash of a whole module, normalised.
    public static func moduleHash(_ m: Module) -> [UInt8] { Sha256.hash(Canon.encode(Encode.module(Normalize.module(m)))) }
}
