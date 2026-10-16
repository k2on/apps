// §1 Values (`rust/ark/src/value.rs`).
//
// The eight run-time values and the one total order over them. Everything
// here must agree byte for byte with the Rust and Kotlin runtimes, in
// memory, on the wire, in an index key and under a hash.
//
// Text is compared by Unicode code point, which is UTF-8 byte order, and
// never by `String.==` or `String.<`: Swift's string equality is canonical
// equivalence, under which "é" precomposed and "e" with a combining accent
// are one string where they are two values. Every comparison, equality and
// hash of a value's text here is of its UTF-8 bytes.
import Foundation

/// The name of a table, as declared in the schema.
public typealias TableName = String

/// Code point order over two texts: -1, 0 or 1 (`value.rs`, the `Ord` of
/// `Value::Text`, which compares `str`s bytewise).
@inline(__always)
public func compareText(_ a: String, _ b: String) -> Int {
    var x = a
    var y = b
    return x.withUTF8 { pa in
        y.withUTF8 { pb in
            let n = Swift.min(pa.count, pb.count)
            if n > 0 {
                let c = memcmp(pa.baseAddress!, pb.baseAddress!, n)
                if c != 0 { return c < 0 ? -1 : 1 }
            }
            return pa.count < pb.count ? -1 : (pa.count > pb.count ? 1 : 0)
        }
    }
}

/// Whether two texts are the same code points.
@inline(__always)
public func textEquals(_ a: String, _ b: String) -> Bool {
    var x = a
    var y = b
    return x.withUTF8 { pa in
        y.withUTF8 { pb in
            pa.count == pb.count && (pa.count == 0 || memcmp(pa.baseAddress!, pb.baseAddress!, pa.count) == 0)
        }
    }
}

/// Unsigned lexicographic order over bytes, a prefix first.
public func compareBytes(_ a: [UInt8], _ b: [UInt8]) -> Int {
    let n = Swift.min(a.count, b.count)
    if n > 0 {
        let c = a.withUnsafeBufferPointer { pa in b.withUnsafeBufferPointer { pb in memcmp(pa.baseAddress!, pb.baseAddress!, n) } }
        if c != 0 { return c < 0 ? -1 : 1 }
    }
    return a.count < b.count ? -1 : (a.count > b.count ? 1 : 0)
}

/// A text wrapped so that it orders, equates and hashes by code point: a key
/// for the sorted collections that hold names.
public struct CodePoints: Comparable, Hashable, CustomStringConvertible {
    public let s: String
    public init(_ s: String) { self.s = s }
    public static func < (a: CodePoints, b: CodePoints) -> Bool { compareText(a.s, b.s) < 0 }
    public static func == (a: CodePoints, b: CodePoints) -> Bool { textEquals(a.s, b.s) }
    public func hash(into h: inout Hasher) {
        var x = s
        x.withUTF8 { h.combine(bytes: UnsafeRawBufferPointer($0)) }
    }
    public var description: String { s }
}

/// Lowercase hex, two digits per byte, and back.
public enum Hex {
    static let digits: [UInt8] = Array("0123456789abcdef".utf8)

    public static func encode(_ b: [UInt8]) -> String {
        var out = [UInt8]()
        out.reserveCapacity(b.count * 2)
        for w in b {
            out.append(digits[Int(w >> 4)])
            out.append(digits[Int(w & 0x0f)])
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// Whether a character is an ASCII hex digit.
    public static func isHexDigit(_ c: UInt8) -> Bool {
        (c >= 48 && c <= 57) || (c >= 97 && c <= 102) || (c >= 65 && c <= 70)
    }

    static func nibble(_ c: UInt8) -> UInt8? {
        switch c {
        case 48...57: return c - 48
        case 97...102: return c - 97 + 10
        case 65...70: return c - 65 + 10
        default: return nil
        }
    }

    /// Bytes from hex in either case; `nil` if not hex or of odd length.
    public static func decode(_ s: String) -> [UInt8]? {
        let u = Array(s.utf8)
        guard u.count % 2 == 0 else { return nil }
        var out = [UInt8]()
        out.reserveCapacity(u.count / 2)
        var i = 0
        while i < u.count {
            guard let hi = nibble(u[i]), let lo = nibble(u[i + 1]) else { return nil }
            out.append(hi << 4 | lo)
            i += 2
        }
        return out
    }
}

/// Sixteen bytes: the run-time form of an id (`value.rs`, `Id`). The table it
/// names is a static type.
public struct Id: Hashable, Comparable, CustomStringConvertible {
    public let bytes: [UInt8]

    /// An id from its bytes; `nil` unless there are exactly sixteen.
    public init?(bytes: [UInt8]) {
        guard bytes.count == 16 else { return nil }
        self.bytes = bytes
    }

    /// The nil id: sixteen zero bytes.
    public static let nilId = Id(bytes: [UInt8](repeating: 0, count: 16))!

    /// Thirty-two hex digits, in either case.
    public static func ofHex(_ hex: String) -> Id? { Hex.decode(hex).flatMap { Id(bytes: $0) } }

    /// 8-4-4-4-12 hex in either case; `nil` for anything else (`Id::parse`).
    public static func ofText(_ text: String) -> Id? {
        let u = Array(text.utf8)
        guard u.count == 36 else { return nil }
        var hex = [UInt8]()
        for (i, c) in u.enumerated() {
            if i == 8 || i == 13 || i == 18 || i == 23 {
                if c != 45 { return nil }
            } else {
                if !Hex.isHexDigit(c) { return nil }
                hex.append(c)
            }
        }
        return ofHex(String(decoding: hex, as: UTF8.self))
    }

    /// Lowercase 8-4-4-4-12, as `text_of_id` spells it.
    public var text: String {
        let h = Array(Hex.encode(bytes).utf8)
        var out = [UInt8]()
        out.reserveCapacity(36)
        for (i, c) in h.enumerated() {
            if i == 8 || i == 12 || i == 16 || i == 20 { out.append(45) }
            out.append(c)
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// Thirty-two lowercase hex digits, no dashes.
    public var hex: String { Hex.encode(bytes) }

    public var description: String { text }

    public static func < (a: Id, b: Id) -> Bool { compareBytes(a.bytes, b.bytes) < 0 }
}

/// A struct's fields, in code point order of their names: a row is one of
/// every column of its table (`value.rs`, `Value::Struct`). Two records with
/// the same fields and values are equal whatever order they were built in.
public struct Record: Hashable {
    public private(set) var names: [String]
    public private(set) var values: [Value]

    public init() {
        names = []
        values = []
    }

    /// From pairs in any order; a later pair of a name wins.
    public init(_ pairs: [(String, Value)]) {
        var idx = Array(pairs.indices)
        idx.sort { a, b in
            let c = compareText(pairs[a].0, pairs[b].0)
            return c != 0 ? c < 0 : a < b
        }
        var ns = [String]()
        var vs = [Value]()
        ns.reserveCapacity(pairs.count)
        vs.reserveCapacity(pairs.count)
        for i in idx {
            let (k, v) = pairs[i]
            if let last = ns.last, textEquals(last, k) {
                vs[vs.count - 1] = v
            } else {
                ns.append(k)
                vs.append(v)
            }
        }
        names = ns
        values = vs
    }

    /// From names already in code point order, each once.
    init(sorted names: [String], _ values: [Value]) {
        self.names = names
        self.values = values
    }

    public var count: Int { names.count }
    public var isEmpty: Bool { names.isEmpty }

    @inline(__always)
    func index(_ name: String) -> Int? {
        var lo = 0
        var hi = names.count
        while lo < hi {
            let mid = (lo + hi) >> 1
            let c = compareText(names[mid], name)
            if c == 0 { return mid }
            if c < 0 { lo = mid + 1 } else { hi = mid }
        }
        return nil
    }

    /// The field, or `nil` where the struct lacks it.
    public subscript(name: String) -> Value? {
        guard let i = index(name) else { return nil }
        return values[i]
    }

    /// The field, or `Null` where the struct lacks it — how a row's column is read.
    public func get(_ name: String) -> Value { self[name] ?? .null }

    public func has(_ name: String) -> Bool { index(name) != nil }

    /// This record with a field set, added where it was missing.
    public mutating func set(_ name: String, _ v: Value) {
        var lo = 0
        var hi = names.count
        while lo < hi {
            let mid = (lo + hi) >> 1
            let c = compareText(names[mid], name)
            if c == 0 {
                values[mid] = v
                return
            }
            if c < 0 { lo = mid + 1 } else { hi = mid }
        }
        names.insert(name, at: lo)
        values.insert(v, at: lo)
    }

    /// This record without a field, where it had one.
    public mutating func remove(_ name: String) {
        guard let i = index(name) else { return }
        names.remove(at: i)
        values.remove(at: i)
    }

    /// The fields in code point order of their names.
    public var pairs: [(String, Value)] { Array(zip(names, values)) }

    /// A record with each value mapped.
    public func mapValues(_ f: (Value) throws -> Value) rethrows -> Record {
        Record(sorted: names, try values.map(f))
    }

    public static func == (a: Record, b: Record) -> Bool {
        if a.names.count != b.names.count { return false }
        for i in 0..<a.names.count {
            if !textEquals(a.names[i], b.names[i]) || a.values[i] != b.values[i] { return false }
        }
        return true
    }

    public func hash(into h: inout Hasher) {
        h.combine(names.count)
        for i in 0..<names.count {
            CodePoints(names[i]).hash(into: &h)
            values[i].hash(into: &h)
        }
    }
}

/// A row: every column of its table, by name. Never partial once stored.
public typealias Row = Record

/// Arguments (or autos) by name.
public typealias Args = [String: Value]

/// A run-time value (`value.rs`, `Value`). `null` is the absent case of an
/// option; an option is flat, so `Some v` is `v`.
public enum Value: Hashable, Comparable, CustomStringConvertible {
    case null
    case bool(Bool)
    case int(Int64)
    case text(String)
    case bytes([UInt8])
    case id(Id)
    case list([Value])
    case record(Record)

    /// A struct from pairs.
    public static func record(_ pairs: [(String, Value)]) -> Value { .record(Record(pairs)) }

    /// An option: `Some v` is `v`, `None` is `null`.
    public static func opt(_ v: Value?) -> Value { v ?? .null }

    /// `Null < Bool < Int < Text < Bytes < Id < List < Struct`.
    public var rank: Int {
        switch self {
        case .null: return 0
        case .bool: return 1
        case .int: return 2
        case .text: return 3
        case .bytes: return 4
        case .id: return 5
        case .list: return 6
        case .record: return 7
        }
    }

    public var isNull: Bool {
        if case .null = self { return true }
        return false
    }

    // Accessors: a verified module never mismatches, so these answer nil and
    // the caller decides whether that is a bug.

    public var boolValue: Bool? {
        if case .bool(let b) = self { return b }
        return nil
    }

    public var intValue: Int64? {
        if case .int(let n) = self { return n }
        return nil
    }

    public var textValue: String? {
        if case .text(let t) = self { return t }
        return nil
    }

    public var bytesValue: [UInt8]? {
        if case .bytes(let b) = self { return b }
        return nil
    }

    public var idValue: Id? {
        if case .id(let i) = self { return i }
        return nil
    }

    public var listValue: [Value]? {
        if case .list(let xs) = self { return xs }
        return nil
    }

    public var recordValue: Record? {
        if case .record(let r) = self { return r }
        return nil
    }

    public static func == (a: Value, b: Value) -> Bool {
        switch (a, b) {
        case (.null, .null): return true
        case (.bool(let x), .bool(let y)): return x == y
        case (.int(let x), .int(let y)): return x == y
        case (.text(let x), .text(let y)): return textEquals(x, y)
        case (.bytes(let x), .bytes(let y)): return x == y
        case (.id(let x), .id(let y)): return x.bytes == y.bytes
        case (.list(let x), .list(let y)): return x == y
        case (.record(let x), .record(let y)): return x == y
        default: return false
        }
    }

    public func hash(into h: inout Hasher) {
        h.combine(rank)
        switch self {
        case .null: break
        case .bool(let b): h.combine(b)
        case .int(let n): h.combine(n)
        case .text(let t): CodePoints(t).hash(into: &h)
        case .bytes(let b): h.combine(b)
        case .id(let i): h.combine(i.bytes)
        case .list(let xs):
            h.combine(xs.count)
            for x in xs { x.hash(into: &h) }
        case .record(let r): r.hash(into: &h)
        }
    }

    public static func < (a: Value, b: Value) -> Bool { compareValue(a, b) < 0 }

    /// A printable form: the vectors' JSON, with the `$int`/`$bytes`/`$id`
    /// wrappers (`json.rs`, how a value is printed).
    public func show() -> String {
        var out = ""
        show(into: &out)
        return out
    }

    func show(into out: inout String) {
        switch self {
        case .null: out += "null"
        case .bool(let b): out += b ? "true" : "false"
        case .int(let n): out += "{\"$int\":\"\(n)\"}"
        case .text(let t): out += Value.quote(t)
        case .bytes(let b): out += "{\"$bytes\":\"\(Hex.encode(b))\"}"
        case .id(let i): out += "{\"$id\":\"\(i.text)\"}"
        case .list(let xs):
            out += "["
            for (i, x) in xs.enumerated() {
                if i > 0 { out += "," }
                x.show(into: &out)
            }
            out += "]"
        case .record(let r):
            out += "{"
            for i in 0..<r.count {
                if i > 0 { out += "," }
                out += Value.quote(r.names[i])
                out += ":"
                r.values[i].show(into: &out)
            }
            out += "}"
        }
    }

    public var description: String { show() }

    /// A JSON string: `"` and `\` escaped, and every control character below
    /// U+0020 as `\u00xx`.
    public static func quote(_ s: String) -> String {
        var out = [UInt8]()
        out.reserveCapacity(s.utf8.count + 2)
        out.append(34)
        let hexd = Array("0123456789abcdef".utf8)
        for b in s.utf8 {
            switch b {
            case 34: out.append(92); out.append(34)
            case 92: out.append(92); out.append(92)
            case 0..<32:
                out.append(contentsOf: Array("\\u00".utf8))
                out.append(hexd[Int(b >> 4)])
                out.append(hexd[Int(b & 15)])
            default: out.append(b)
            }
        }
        out.append(34)
        return String(decoding: out, as: UTF8.self)
    }
}

/// §1.2 The total order (`value.rs`, `impl Ord for Value`). Values of
/// different types order by rank; within a type: ints numerically, text by
/// code point, bytes and ids bytewise, lists lexicographically (a prefix
/// first), structs as the association list sorted by field name, names
/// before values.
public func compareValue(_ a: Value, _ b: Value) -> Int {
    switch (a, b) {
    case (.null, .null): return 0
    case (.bool(let x), .bool(let y)): return x == y ? 0 : (x ? 1 : -1)
    case (.int(let x), .int(let y)): return x == y ? 0 : (x < y ? -1 : 1)
    case (.text(let x), .text(let y)): return compareText(x, y)
    case (.bytes(let x), .bytes(let y)): return compareBytes(x, y)
    case (.id(let x), .id(let y)): return compareBytes(x.bytes, y.bytes)
    case (.list(let xs), .list(let ys)): return compareKeys(xs, ys)
    case (.record(let x), .record(let y)):
        let n = Swift.min(x.count, y.count)
        for i in 0..<n {
            let c = compareText(x.names[i], y.names[i])
            if c != 0 { return c }
            let d = compareValue(x.values[i], y.values[i])
            if d != 0 { return d }
        }
        return x.count < y.count ? -1 : (x.count > y.count ? 1 : 0)
    default:
        return a.rank < b.rank ? -1 : 1
    }
}

/// A key's order: the key columns' values, lexicographically, a prefix first.
public func compareKeys(_ a: [Value], _ b: [Value]) -> Int {
    let n = Swift.min(a.count, b.count)
    for i in 0..<n {
        let c = compareValue(a[i], b[i])
        if c != 0 { return c }
    }
    return a.count < b.count ? -1 : (a.count > b.count ? 1 : 0)
}

/// A key — the key columns' values — as a sorted collection's key, in key order.
extension Array: Comparable where Element == Value {
    public static func < (a: [Value], b: [Value]) -> Bool { compareKeys(a, b) < 0 }
}
