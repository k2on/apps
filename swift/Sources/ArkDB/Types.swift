// §2.2 The type section, and a value held to a type (`rust/ark/src/types.rs`,
// `docs/plan-live.md`). A live state is made of records and sums the module
// declares by name (`Module.types`). Three things are asked of them, and all
// three are here: what a named type stands for (`resolve`), whether a value
// is one (`conforms`) — asked of an input, a snapshot and a transition's
// result, wherever a value crosses a boundary — and which of a state's
// fields are ephemeral, which the state's hash and what a server keeps leave
// out (`ephemeralFields`, `orderedPart`).
//
// A value of each type, exactly:
//
// | type | its values |
// |---|---|
// | `Bool`, `Int`, `Text`, `Bytes`, `Id t` | the value of that kind |
// | `Enum vs` | a `Text` that is one of `vs` |
// | `Option t` | `Null`, or a value of `t` |
// | `List t` | a list of values of `t` |
// | `NonEmpty t` | a list of values of `t` with at least one |
// | `Struct fs` | a struct of exactly the fields `fs`, each of its type |
// | `Sum vs` | a struct whose `t` names a variant of `vs`, and whose other fields are exactly that variant's |
// | `PerDevice t` | a struct keyed by device ids, each value of `t` |
// | `Ephemeral t` | a value of `t` |
// | `Named n` | a value of what `n` is declared as |

public enum Types {
    /// The tag a sum's value carries its variant's name under, as the IR's own nodes do.
    public static let tag = "t"

    // The most a type may nest named types: deeper is a cycle.
    private static let depth = 64

    /// Why a type does not resolve or a value is not of it: the reference's sentence.
    public struct TypeError: Error, CustomStringConvertible {
        public let why: String
        public var description: String { why }
    }

    /// `resolve`: `t` with every named type replaced by its declaration, at
    /// any depth. A `TypeError` names a type the section does not declare, or
    /// one that refers to itself.
    public static func resolve(_ types: [String: Ty], _ t: Ty) throws -> Ty {
        var seen = [String]()
        return try resolveAt(types, t, &seen)
    }

    private static func resolveAt(_ types: [String: Ty], _ t: Ty, _ seen: inout [String]) throws -> Ty {
        switch t {
        case .named(let n):
            if seen.contains(n) || seen.count > depth { throw TypeError(why: "the type \(n) refers to itself") }
            guard let d = types[n] else { throw TypeError(why: "no type \(n)") }
            seen.append(n)
            let r = try resolveAt(types, d, &seen)
            seen.removeLast()
            return r
        case .option(let x): return .option(try resolveAt(types, x, &seen))
        case .list(let x): return .list(try resolveAt(types, x, &seen))
        case .nonEmpty(let x): return .nonEmpty(try resolveAt(types, x, &seen))
        case .perDevice(let x): return .perDevice(try resolveAt(types, x, &seen))
        case .ephemeral(let x): return .ephemeral(try resolveAt(types, x, &seen))
        case .structure(let fs):
            var out = [(String, Ty)]()
            for (k, v) in fs.pairs { out.append((k, try resolveAt(types, v, &seen))) }
            return .structure(TyFields(out))
        case .sum(let vs):
            var out = [(String, TyFields)]()
            for (n, fs) in vs.pairs {
                var f = [(String, Ty)]()
                for (k, v) in fs.pairs { f.append((k, try resolveAt(types, v, &seen))) }
                out.append((n, TyFields(f)))
            }
            return .sum(TySum(out))
        default: return t
        }
    }

    /// `unmarked`: a resolved type with every `Ephemeral` marker taken off.
    public static func unmarked(_ t: Ty) -> Ty {
        switch t {
        case .ephemeral(let x): return unmarked(x)
        case .option(let x): return .option(unmarked(x))
        case .list(let x): return .list(unmarked(x))
        case .nonEmpty(let x): return .nonEmpty(unmarked(x))
        case .perDevice(let x): return .perDevice(unmarked(x))
        case .structure(let fs): return .structure(TyFields(fs.pairs.map { ($0.0, unmarked($0.1)) }))
        case .sum(let vs): return .sum(vs.mapTypes(unmarked))
        default: return t
        }
    }

    /// `ephemeral_fields`: the fields of a state of this resolved type marked
    /// `Ephemeral`, in code point order; none for a state that is not a record.
    public static func ephemeralFields(_ state: Ty) -> [String] {
        guard case .structure(let fs) = state else { return [] }
        return fs.pairs.compactMap { if case .ephemeral = $0.1 { return $0.0 } else { return nil } }
    }

    /// `ordered_part`: a state's value with its ephemeral fields taken out.
    public static func orderedPart(_ ephemeral: [String], _ v: Value) -> Value {
        guard case .record(var r) = v, !ephemeral.isEmpty else { return v }
        for f in ephemeral { r.remove(f) }
        return .record(r)
    }

    /// `with_empty_slots`: a value restored from its ordered part, every ephemeral field empty.
    public static func withEmptySlots(_ ephemeral: [String], _ v: Value) -> Value {
        guard case .record(var r) = v, !ephemeral.isEmpty else { return v }
        for f in ephemeral { r.set(f, .record(Record())) }
        return .record(r)
    }

    /// `conforms`: whether `v` is a value of `t`; a `TypeError` says where it
    /// is not, as a path and a sentence: `queue: an empty list where a
    /// non-empty one is declared`.
    public static func conforms(_ types: [String: Ty], _ t: Ty, _ v: Value) throws {
        var path = [String]()
        try conformAt(types, t, v, &path, 0)
    }

    /// `conforms` as a sentence, or nil where the value is one.
    public static func whyNot(_ types: [String: Ty], _ t: Ty, _ v: Value) -> String? {
        do {
            try conforms(types, t, v)
            return nil
        } catch let e as TypeError {
            return e.why
        } catch {
            return "\(error)"
        }
    }

    private static func at(_ path: [String], _ what: String) -> TypeError {
        TypeError(why: path.isEmpty ? what : "\(path.joined(separator: ".")): \(what)")
    }

    private static func conformAt(_ types: [String: Ty], _ t: Ty, _ v: Value, _ path: inout [String], _ d: Int) throws {
        func kind(_ want: String) -> TypeError { at(path, "expected \(want), found \(short(v))") }
        switch (t, v) {
        case (.named(let n), _):
            if d > depth { throw at(path, "the type \(n) refers to itself") }
            guard let decl = types[n] else { throw at(path, "no type \(n)") }
            try conformAt(types, decl, v, &path, d + 1)
        case (.ephemeral(let x), _): try conformAt(types, x, v, &path, d)
        case (.bool, .bool), (.int, .int), (.text, .text), (.bytes, .bytes), (.id, .id): return
        case (.bool, _): throw kind("a bool")
        case (.int, _): throw kind("an int")
        case (.text, _): throw kind("text")
        case (.bytes, _): throw kind("bytes")
        case (.id, _): throw kind("an id")
        case (.enumeration(let vs), _):
            if case .text(let x) = v, vs.contains(where: { textEquals($0, x) }) { return }
            throw at(path, "expected one of \(vs.joined(separator: ", ")), found \(short(v))")
        case (.option, .null): return
        case (.option(let x), _): try conformAt(types, x, v, &path, d)
        case (.list(let x), .list(let xs)): try elems(types, x, xs, &path, d)
        case (.nonEmpty(let x), .list(let xs)):
            if xs.isEmpty { throw at(path, "an empty list where a non-empty one is declared") }
            try elems(types, x, xs, &path, d)
        case (.list, _), (.nonEmpty, _): throw kind("a list")
        case (.structure(let fs), .record(let m)): try fields(types, fs, m, false, &path, d)
        case (.structure, _): throw kind("a record")
        case (.sum(let vs), .record(let m)):
            guard case .text(let name)? = m[tag] else { throw at(path, "a sum's value names its variant under t") }
            guard let fs = vs[name] else { throw at(path, "no variant \(name)") }
            path.append(name)
            try fields(types, fs, m, true, &path, d)
            path.removeLast()
        case (.sum, _): throw kind("a variant")
        case (.perDevice(let x), .record(let m)):
            for (k, e) in m.pairs {
                path.append("[\(k)]")
                try conformAt(types, x, e, &path, d)
                path.removeLast()
            }
        case (.perDevice, _): throw kind("a map of devices")
        }
    }

    private static func elems(_ types: [String: Ty], _ t: Ty, _ xs: [Value], _ path: inout [String], _ d: Int) throws {
        for (i, x) in xs.enumerated() {
            path.append("[\(i)]")
            try conformAt(types, t, x, &path, d)
            path.removeLast()
        }
    }

    private static func fields(_ types: [String: Ty], _ fs: TyFields, _ m: Record, _ tagged: Bool, _ path: inout [String], _ d: Int) throws {
        for k in m.names where !fs.has(k) && !(tagged && k == tag) {
            throw at(path, "no field \(k)")
        }
        for (k, t) in fs.pairs {
            guard let x = m[k] else { throw at(path, "missing field \(k)") }
            path.append(k)
            try conformAt(types, t, x, &path, d)
            path.removeLast()
        }
    }

    // A value named briefly, for a sentence.
    private static func short(_ v: Value) -> String {
        switch v {
        case .null: return "nothing"
        case .bool: return "a bool"
        case .int: return "an int"
        case .text: return "text"
        case .bytes: return "bytes"
        case .id: return "an id"
        case .list: return "a list"
        case .record: return "a struct"
        }
    }
}
