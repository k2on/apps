import Foundation

/// The name of a table, as declared in the schema.
public typealias TableName = String
/// The name of a field of a struct, or a column of a row.
public typealias FieldName = String

/// Sixteen bytes. The table an id names is a static type, never a run-time
/// tag. Constructed only through `init(bytes:)` or `init(uuid:)`, which
/// enforce the length.
public struct Id: Equatable, Hashable {
    public let bytes: [UInt8]

    /// An id from its bytes; `nil` unless there are exactly sixteen.
    public init?(bytes: [UInt8]) {
        guard bytes.count == 16 else { return nil }
        self.bytes = bytes
    }

    /// Parse 8-4-4-4-12 hex in either case; `nil` for anything else.
    public init?(uuid text: String) {
        guard let b = Id.parseUuid(text) else { return nil }
        self.bytes = b
    }

    /// Sixteen zero bytes.
    public static let nil_ = Id(bytes: [UInt8](repeating: 0, count: 16))!

    /// The canonical text: lowercase 8-4-4-4-12.
    public var uuid: String {
        let h = Hex.encode(bytes)
        func part(_ from: Int, _ to: Int) -> String {
            let s = h.utf8
            return String(decoding: Array(s)[from..<to], as: UTF8.self)
        }
        return part(0, 8) + "-" + part(8, 12) + "-" + part(12, 16) + "-" + part(16, 20) + "-" + part(20, 32)
    }

    static func parseUuid(_ t: String) -> [UInt8]? {
        let parts = t.split(separator: "-", omittingEmptySubsequences: false)
        guard parts.count == 5 else { return nil }
        let lens = [8, 4, 4, 4, 12]
        var hex = ""
        for (p, n) in zip(parts, lens) {
            let scalars = Array(p.unicodeScalars)
            guard scalars.count == n else { return nil }
            for c in scalars where !Hex.isHexDigit(c) { return nil }
            hex += String(p)
        }
        return Hex.decode(hex)
    }
}

/// A run-time value: the eight constructors, and nothing else.
///
/// `null` exists only as the absent case of an option; `record` is a named
/// struct whose field names are its identity.
public indirect enum Value {
    case null
    case bool(Bool)
    case int(Int64)
    case text(String)
    case bytes([UInt8])
    case id(Id)
    case list([Value])
    case record([String: Value])

    // Constructors, as GENERATED.md spells them ---------------------------

    public static func null() -> Value { return .null }

    /// Bytes from lowercase or uppercase hex, two digits per byte. A bug
    /// (fatal) if the text is not hex: generated code only ever writes a
    /// literal here.
    public static func bytesHex(_ hex: String) -> Value {
        guard let b = Hex.decode(hex) else { fatalError("Value.bytesHex: not hex: \(hex)") }
        return .bytes(b)
    }

    /// An id from 32 hex digits (with or without the 8-4-4-4-12 dashes).
    public static func idHex(_ hex: String) -> Value {
        if let i = Id(uuid: hex) { return .id(i) }
        if let b = Hex.decode(hex), let i = Id(bytes: b) { return .id(i) }
        fatalError("Value.idHex: not an id: \(hex)")
    }

    /// A record from pairs; a later pair with the same name wins.
    public static func record(_ pairs: [(String, Value)]) -> Value {
        var m: [String: Value] = [:]
        for (k, v) in pairs { m[k] = v }
        return .record(m)
    }

    /// An option: `Some v` is `v`, `None` is `null`.
    public static func opt(_ v: Value?) -> Value { return v ?? .null }

    // Accessors: fatal on a type mismatch, which is a bug and not a fault --

    public func isNull() -> Bool {
        if case .null = self { return true }
        return false
    }

    public func asBool() -> Bool {
        if case .bool(let b) = self { return b }
        fatalError("Value.asBool: expected Bool, got \(self.brief)")
    }

    public func asInt() -> Int64 {
        if case .int(let n) = self { return n }
        fatalError("Value.asInt: expected Int, got \(self.brief)")
    }

    public func asText() -> String {
        if case .text(let t) = self { return t }
        fatalError("Value.asText: expected Text, got \(self.brief)")
    }

    public func asBytes() -> [UInt8] {
        if case .bytes(let b) = self { return b }
        fatalError("Value.asBytes: expected Bytes, got \(self.brief)")
    }

    public func asId() -> Id {
        if case .id(let i) = self { return i }
        fatalError("Value.asId: expected Id, got \(self.brief)")
    }

    public func asList() -> [Value] {
        if case .list(let xs) = self { return xs }
        fatalError("Value.asList: expected List, got \(self.brief)")
    }

    public func asRecord() -> [String: Value] {
        if case .record(let m) = self { return m }
        fatalError("Value.asRecord: expected Struct, got \(self.brief)")
    }

    public func field(_ name: String) -> Value {
        guard case .record(let m) = self else {
            fatalError("Value.field(\(name)): expected Struct, got \(self.brief)")
        }
        guard let v = m[name] else { fatalError("Value.field: no such field \(name)") }
        return v
    }

    /// The rank of the value's type in the total order.
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

    /// A short rendering for messages; never part of any hash or wire form.
    public var brief: String {
        let s = describe()
        if s.count > 60 { return String(s.prefix(60)) }
        return s
    }

    func describe() -> String {
        switch self {
        case .null: return "null"
        case .bool(let b): return b ? "true" : "false"
        case .int(let n): return String(n)
        case .text(let t): return "\"" + t + "\""
        case .bytes(let b): return "0x" + Hex.encode(b)
        case .id(let i): return i.uuid
        case .list(let xs): return "[" + xs.map { $0.describe() }.joined(separator: ",") + "]"
        case .record(let m):
            return "{" + sortedFieldNames(m).map { "\($0):" + m[$0]!.describe() }.joined(separator: ",") + "}"
        }
    }
}

// MARK: - The total order

/// Code point order over text: UTF-8 byte order, and deliberately not what
/// `String.<` does.
public func compareText(_ a: String, _ b: String) -> Int {
    var ia = a.unicodeScalars.makeIterator()
    var ib = b.unicodeScalars.makeIterator()
    while true {
        let x = ia.next()
        let y = ib.next()
        switch (x, y) {
        case (nil, nil): return 0
        case (nil, _): return -1
        case (_, nil): return 1
        case (let x?, let y?):
            if x.value < y.value { return -1 }
            if x.value > y.value { return 1 }
        }
    }
}

func compareBytes(_ a: [UInt8], _ b: [UInt8]) -> Int {
    let n = min(a.count, b.count)
    var i = 0
    while i < n {
        if a[i] < b[i] { return -1 }
        if a[i] > b[i] { return 1 }
        i += 1
    }
    if a.count < b.count { return -1 }
    if a.count > b.count { return 1 }
    return 0
}

/// Field names in code point order, which is the order a struct's
/// association list is compared in.
func sortedFieldNames(_ m: [String: Value]) -> [String] {
    return m.keys.sorted { compareText($0, $1) < 0 }
}

/// §1.2 The one total order: -1, 0 or 1.
public func compareValue(_ a: Value, _ b: Value) -> Int {
    let ra = a.rank
    let rb = b.rank
    if ra != rb { return ra < rb ? -1 : 1 }
    switch (a, b) {
    case (.null, .null): return 0
    case (.bool(let x), .bool(let y)):
        if x == y { return 0 }
        return x ? 1 : -1
    case (.int(let x), .int(let y)):
        if x == y { return 0 }
        return x < y ? -1 : 1
    case (.text(let x), .text(let y)): return compareText(x, y)
    case (.bytes(let x), .bytes(let y)): return compareBytes(x, y)
    case (.id(let x), .id(let y)): return compareBytes(x.bytes, y.bytes)
    case (.list(let xs), .list(let ys)):
        let n = min(xs.count, ys.count)
        var i = 0
        while i < n {
            let c = compareValue(xs[i], ys[i])
            if c != 0 { return c }
            i += 1
        }
        if xs.count < ys.count { return -1 }
        if xs.count > ys.count { return 1 }
        return 0
    case (.record(let xs), .record(let ys)):
        let ka = sortedFieldNames(xs)
        let kb = sortedFieldNames(ys)
        let n = min(ka.count, kb.count)
        var i = 0
        while i < n {
            let c = compareText(ka[i], kb[i])
            if c != 0 { return c }
            let d = compareValue(xs[ka[i]]!, ys[kb[i]]!)
            if d != 0 { return d }
            i += 1
        }
        if ka.count < kb.count { return -1 }
        if ka.count > kb.count { return 1 }
        return 0
    default:
        fatalError("compareValue: rank mismatch is impossible")
    }
}

extension Value: Equatable, Comparable, Hashable {
    /// Structural equality: two texts are equal iff their scalars are.
    public static func == (a: Value, b: Value) -> Bool { return compareValue(a, b) == 0 }
    public static func < (a: Value, b: Value) -> Bool { return compareValue(a, b) < 0 }

    /// Hashes the canonical encoding, so that equal values hash equal
    /// whatever the host's `String` would have said.
    public func hash(into hasher: inout Hasher) {
        hasher.combine(Canon.encode(self))
    }
}

// MARK: - Hex

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

    static func isHexDigit(_ c: Unicode.Scalar) -> Bool {
        return (c.value >= 48 && c.value <= 57) || (c.value >= 97 && c.value <= 102) || (c.value >= 65 && c.value <= 70)
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
