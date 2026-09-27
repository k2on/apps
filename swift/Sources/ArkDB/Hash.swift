import Foundation

/// The 32-byte hash an entry names its function by.
public typealias FnHash = [UInt8]

/// §8.3 A function with the helpers it was verified against: a complete,
/// self-contained program the interpreter runs without any module.
public struct Closure: Equatable {
    public var fn: Function
    /// Every helper the function reaches, in declaration order, normalised.
    public var helpers: [Function]
    public init(fn: Function, helpers: [Function]) { self.fn = fn; self.helpers = helpers }
}

/// §8 The two hashes every runtime reproduces byte for byte.
public enum Hash {
    /// The closure of a function within a module.
    public static func closure(_ m: Module, _ fn: Function) -> Closure {
        var seen: [String] = []
        var todo = Encode.calls(fn)
        while let n = todo.first {
            todo.removeFirst()
            if seen.contains(n) { continue }
            if let h = m.lookupFunction(n) {
                seen.append(n)
                todo = Encode.calls(h) + todo
            }
        }
        let helpers = m.functions.filter { seen.contains($0.name) }.map(Encode.normalize)
        return Closure(fn: Encode.normalize(fn), helpers: helpers)
    }

    /// A closure as a value: `{ t: "closure", fn, helpers }`.
    public static func closureValue(_ c: Closure) -> Value {
        return .record([
            "t": .text("closure"),
            "fn": Encode.functionValue([:], c.fn),
            "helpers": .list(c.helpers.map { Encode.functionValue([:], $0) }),
        ])
    }

    /// Every function of a module, by the hash of its closure.
    public static func closures(_ m: Module) -> [FnHash: Closure] {
        var out: [FnHash: Closure] = [:]
        for fn in m.functions {
            let c = closure(m, fn)
            out[functionHash(c)] = c
        }
        return out
    }

    /// §8.1 The state hash: over a list with, for every table of the schema
    /// in schema order, the table's name and its rows in key order.
    public static func stateHash(_ st: MemoryStore) -> [UInt8] {
        let v = Value.list(st.tableNames.map { t in
            .list([.text(t), .list(st.scan(t).map { .record($0) })])
        })
        return Sha256.hash(Canon.encode(v))
    }

    /// §8.2 The hash of a function: its normalised form, names excluded,
    /// with the hashes of the helpers it calls directly.
    public static func functionHash(_ c: Closure) -> FnHash {
        var deps: [String: Value] = [:]
        for n in Encode.calls(c.fn) {
            if let h = c.helpers.first(where: { $0.name == n }) {
                deps[n] = .bytes(functionHash(Closure(fn: h, helpers: c.helpers)))
            }
        }
        return Sha256.hash(Canon.encode(Encode.functionValue(deps, c.fn)))
    }

    /// The hash of a whole module, normalised.
    public static func moduleHash(_ m: Module) -> [UInt8] {
        return Sha256.hash(Canon.encode(Encode.toValue(Encode.normalizeModule(m))))
    }
}
