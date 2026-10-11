// §1.3 The canonical encoding (`rust/ark/src/canon.rs`).
//
// RFC 8949 §4.2.1 deterministic CBOR plus the mapping from Value onto
// CBOR's types: shortest heads, definite lengths, map keys sorted by the
// bytes they encode to, tag 37 over sixteen bytes for an id. `decode`
// accepts canonical bytes and nothing else.

/// Why some bytes are not the canonical encoding of any value; the first
/// rule the input broke, reading left to right (`canon::DecodeError`).
public enum DecodeError: Error, Equatable, CustomStringConvertible {
    case nonCanonicalHead
    case indefiniteLength
    case unsortedKeys
    case duplicateKey
    case nonTextKey
    case badTag
    case badId
    case float
    case badSimple
    case badUtf8
    case intOutOfRange
    case trailing
    case truncated

    public var description: String {
        switch self {
        case .nonCanonicalHead: return "NonCanonicalHead"
        case .indefiniteLength: return "IndefiniteLength"
        case .unsortedKeys: return "UnsortedKeys"
        case .duplicateKey: return "DuplicateKey"
        case .nonTextKey: return "NonTextKey"
        case .badTag: return "BadTag"
        case .badId: return "BadId"
        case .float: return "Float"
        case .badSimple: return "BadSimple"
        case .badUtf8: return "BadUtf8"
        case .intOutOfRange: return "IntOutOfRange"
        case .trailing: return "Trailing"
        case .truncated: return "Truncated"
        }
    }
}

public enum Canon {
    // MARK: Encoding

    /// The canonical bytes of a value (`canon::encode`). Total: every value has one.
    public static func encode(_ v: Value) -> [UInt8] {
        var out = [UInt8]()
        build(v, into: &out)
        return out
    }

    /// The canonical bytes of a value, appended.
    public static func encode(_ v: Value, into out: inout [UInt8]) { build(v, into: &out) }

    @inline(__always)
    static func header(_ major: UInt8, _ n: UInt64, into out: inout [UInt8]) {
        let mt = major << 5
        if n < 24 {
            out.append(mt | UInt8(n))
        } else if n < 0x100 {
            out.append(mt | 24)
            out.append(UInt8(n))
        } else if n < 0x10000 {
            out.append(mt | 25)
            out.append(UInt8(n >> 8))
            out.append(UInt8(n & 0xff))
        } else if n < 0x1_0000_0000 {
            out.append(mt | 26)
            for s in stride(from: 24, through: 0, by: -8) { out.append(UInt8((n >> UInt64(s)) & 0xff)) }
        } else {
            out.append(mt | 27)
            for s in stride(from: 56, through: 0, by: -8) { out.append(UInt8((n >> UInt64(s)) & 0xff)) }
        }
    }

    static func text(_ t: String, into out: inout [UInt8]) {
        var s = t
        s.withUTF8 { p in
            header(3, UInt64(p.count), into: &out)
            out.append(contentsOf: p)
        }
    }

    static func build(_ v: Value, into out: inout [UInt8]) {
        switch v {
        case .null: out.append(0xf6)
        case .bool(let b): out.append(b ? 0xf5 : 0xf4)
        case .int(let n):
            if n >= 0 {
                header(0, UInt64(n), into: &out)
            } else {
                header(1, UInt64(bitPattern: -1 - n), into: &out)
            }
        case .text(let t): text(t, into: &out)
        case .bytes(let b):
            header(2, UInt64(b.count), into: &out)
            out.append(contentsOf: b)
        case .id(let i):
            header(6, 37, into: &out)
            header(2, 16, into: &out)
            out.append(contentsOf: i.bytes)
        case .list(let xs):
            header(4, UInt64(xs.count), into: &out)
            for x in xs { build(x, into: &out) }
        case .record(let r):
            header(5, UInt64(r.count), into: &out)
            // Pairs sorted by the bytes the key encodes to: length first,
            // then bytewise — which is not the order of the names.
            var order = Array(0..<r.count)
            let lens = r.names.map { $0.utf8.count }
            order.sort { a, b in
                if lens[a] != lens[b] { return lens[a] < lens[b] }
                return compareText(r.names[a], r.names[b]) < 0
            }
            for i in order {
                text(r.names[i], into: &out)
                build(r.values[i], into: &out)
            }
        }
    }

    // MARK: Decoding

    /// A value from its canonical bytes, and only from those (`canon::decode`).
    public static func decode(_ input: [UInt8]) throws -> Value {
        var p = Parser(input: input, pos: 0)
        let v = try p.value()
        if p.pos != input.count { throw DecodeError.trailing }
        return v
    }

    /// Whether some bytes decode, and what they decode to encodes back to exactly them.
    public static func roundTrip(_ b: [UInt8]) -> Bool {
        guard let v = try? decode(b) else { return false }
        return encode(v) == b
    }

    static let int64Limit: UInt64 = UInt64(Int64.max)

    struct Parser {
        let input: [UInt8]
        var pos: Int

        mutating func byte() throws -> UInt8 {
            guard pos < input.count else { throw DecodeError.truncated }
            let b = input[pos]
            pos += 1
            return b
        }

        func peek() throws -> UInt8 {
            guard pos < input.count else { throw DecodeError.truncated }
            return input[pos]
        }

        /// The next n bytes; n is unsigned, since a length near 2^64 is a legal argument.
        mutating func chunk(_ n: UInt64) throws -> ArraySlice<UInt8> {
            if n > UInt64(input.count - pos) { throw DecodeError.truncated }
            let k = Int(n)
            let s = input[pos..<(pos + k)]
            pos += k
            return s
        }

        mutating func bigEndian(_ k: Int) throws -> UInt64 {
            var acc: UInt64 = 0
            for _ in 0..<k { acc = (acc << 8) | UInt64(try byte()) }
            return acc
        }

        /// The argument of a head with additional information ai, in shortest form only.
        mutating func argument(_ ai: UInt8) throws -> UInt64 {
            if ai < 24 { return UInt64(ai) }
            switch ai {
            case 24: return try wide(1, 24)
            case 25: return try wide(2, 0x100)
            case 26: return try wide(4, 0x10000)
            case 27: return try wide(8, 0x1_0000_0000)
            case 31: throw DecodeError.indefiniteLength
            default: throw DecodeError.nonCanonicalHead
            }
        }

        mutating func wide(_ k: Int, _ least: UInt64) throws -> UInt64 {
            let n = try bigEndian(k)
            if n < least { throw DecodeError.nonCanonicalHead }
            return n
        }

        mutating func text(_ n: UInt64) throws -> String {
            let b = try chunk(n)
            guard let s = Utf8.decodeStrict(b) else { throw DecodeError.badUtf8 }
            return s
        }

        mutating func value() throws -> Value {
            let ib = try byte()
            let ai = ib & 0x1f
            switch ib >> 5 {
            case 0:
                let n = try argument(ai)
                if n > Canon.int64Limit { throw DecodeError.intOutOfRange }
                return .int(Int64(n))
            case 1:
                let n = try argument(ai)
                if n > Canon.int64Limit { throw DecodeError.intOutOfRange }
                return .int(-1 - Int64(n))
            case 2:
                return .bytes(Array(try chunk(try argument(ai))))
            case 3:
                return .text(try text(try argument(ai)))
            case 4:
                var n = try argument(ai)
                var xs: [Value] = []
                // One at a time, so an absurd count fails on the input
                // running out rather than on allocation.
                while n != 0 {
                    xs.append(try value())
                    n -= 1
                }
                return .list(xs)
            case 5:
                return .record(try pairs(try argument(ai)))
            case 6:
                let t = try argument(ai)
                if t != 37 { throw DecodeError.badTag }
                return .id(try identifier())
            default:
                switch ai {
                case 20: return .bool(false)
                case 21: return .bool(true)
                case 22: return .null
                case 25, 26, 27: throw DecodeError.float
                case 31: throw DecodeError.indefiniteLength
                default: throw DecodeError.badSimple
                }
            }
        }

        /// n pairs whose keys are text strings in strictly increasing order of
        /// their encoded bytes.
        mutating func pairs(_ count: UInt64) throws -> Record {
            var out = [(String, Value)]()
            var prev: ArraySlice<UInt8>? = nil
            var n = count
            while n != 0 {
                let start = pos
                let k = try key()
                let raw = input[start..<pos]
                if let p = prev {
                    let c = compareSlices(raw, p)
                    if c == 0 { throw DecodeError.duplicateKey }
                    if c < 0 { throw DecodeError.unsortedKeys }
                }
                out.append((k, try value()))
                prev = raw
                n -= 1
            }
            return Record(out)
        }

        mutating func key() throws -> String {
            let ib = try peek()
            if ib >> 5 != 3 { throw DecodeError.nonTextKey }
            _ = try byte()
            return try text(try argument(ib & 0x1f))
        }

        mutating func identifier() throws -> Id {
            let ib = try byte()
            if ib >> 5 != 2 { throw DecodeError.badId }
            let n = try argument(ib & 0x1f)
            if n != 16 { throw DecodeError.badId }
            return Id(bytes: Array(try chunk(n)))!
        }
    }

    static func compareSlices(_ a: ArraySlice<UInt8>, _ b: ArraySlice<UInt8>) -> Int {
        var i = a.startIndex
        var j = b.startIndex
        while i < a.endIndex && j < b.endIndex {
            if a[i] != b[j] { return a[i] < b[j] ? -1 : 1 }
            i += 1
            j += 1
        }
        if i < a.endIndex { return 1 }
        if j < b.endIndex { return -1 }
        return 0
    }
}

/// UTF-8, strictly: overlong forms, surrogates and anything above U+10FFFF
/// refused. A Swift `String` holds Unicode scalars only, so encoding is total.
public enum Utf8 {
    public static func encode(_ s: String) -> [UInt8] { Array(s.utf8) }

    /// The text, or nil if the bytes are not valid UTF-8.
    public static func decodeStrict<C: Collection>(_ b: C) -> String? where C.Element == UInt8 {
        var ok = true
        for x in b where x >= 0x80 {
            ok = false
            break
        }
        if ok { return String(decoding: b, as: UTF8.self) }
        // Swift's decoder replaces what it cannot read with U+FFFD, so valid
        // input is exactly the input that comes back unchanged.
        let s = String(decoding: b, as: UTF8.self)
        return s.utf8.elementsEqual(b) ? s : nil
    }
}
