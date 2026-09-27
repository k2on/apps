import Foundation

/// Why some bytes are not the canonical encoding of any value; the first
/// rule the input broke, reading left to right. Mirrors `Ark.Canon.DecodeError`.
public enum DecodeError: Error, Equatable {
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
}

/// §1.3 The canonical encoding: RFC 8949 §4.2.1 deterministic CBOR plus the
/// mapping from `Value` onto CBOR's types, exactly as `Ark.Canon`.
public enum Canon {
    // MARK: Encoding

    public static func encode(_ v: Value) -> [UInt8] {
        var out = [UInt8]()
        build(v, into: &out)
        return out
    }

    /// The head of a data item: the major type in the top three bits and
    /// the argument in the shortest form that holds it.
    static func header(_ major: UInt8, _ n: UInt64, into out: inout [UInt8]) {
        let mt = major << 5
        if n < 24 {
            out.append(mt | UInt8(n))
        } else if n < 0x100 {
            out.append(mt | 24)
            out.append(UInt8(n))
        } else if n < 0x10000 {
            out.append(mt | 25)
            out.append(UInt8(n >> 8)); out.append(UInt8(n & 0xff))
        } else if n < 0x1_0000_0000 {
            out.append(mt | 26)
            for s in stride(from: 24, through: 0, by: -8) { out.append(UInt8((n >> UInt64(s)) & 0xff)) }
        } else {
            out.append(mt | 27)
            for s in stride(from: 56, through: 0, by: -8) { out.append(UInt8((n >> UInt64(s)) & 0xff)) }
        }
    }

    static func string(_ major: UInt8, _ b: [UInt8], into out: inout [UInt8]) {
        header(major, UInt64(b.count), into: &out)
        out.append(contentsOf: b)
    }

    static func build(_ v: Value, into out: inout [UInt8]) {
        switch v {
        case .null: out.append(0xf6)
        case .bool(false): out.append(0xf4)
        case .bool(true): out.append(0xf5)
        case .int(let n):
            if n >= 0 {
                header(0, UInt64(n), into: &out)
            } else {
                // -1 - n, computed in Int64: in range exactly when n is negative.
                header(1, UInt64((-1) - n), into: &out)
            }
        case .text(let t): string(3, Array(t.utf8), into: &out)
        case .bytes(let b): string(2, b, into: &out)
        case .id(let i):
            header(6, 37, into: &out)
            string(2, i.bytes, into: &out)
        case .list(let xs):
            header(4, UInt64(xs.count), into: &out)
            for x in xs { build(x, into: &out) }
        case .record(let m):
            header(5, UInt64(m.count), into: &out)
            // Sorted by the bytes the key encodes to, bytewise.
            var pairs: [([UInt8], Value)] = []
            pairs.reserveCapacity(m.count)
            for (k, x) in m { pairs.append((encode(.text(k)), x)) }
            pairs.sort { compareBytes($0.0, $1.0) < 0 }
            for (kb, x) in pairs {
                out.append(contentsOf: kb)
                build(x, into: &out)
            }
        }
    }

    // MARK: Decoding

    /// A value from its canonical bytes, and only from those.
    public static func decode(_ input: [UInt8]) throws -> Value {
        var p = Parser(input: input, pos: 0)
        let v = try p.value()
        if p.pos != input.count { throw DecodeError.trailing }
        return v
    }

    /// Whether some bytes are a canonical encoding: they decode, and what
    /// they decode to encodes back to exactly them.
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

        mutating func chunk(_ n: UInt64) throws -> [UInt8] {
            if n > UInt64(input.count - pos) { throw DecodeError.truncated }
            let k = Int(n)
            let s = Array(input[pos..<(pos + k)])
            pos += k
            return s
        }

        mutating func bigEndian(_ k: Int) throws -> UInt64 {
            var acc: UInt64 = 0
            for _ in 0..<k { acc = (acc << 8) | UInt64(try byte()) }
            return acc
        }

        /// The argument of a head, insisting on the shortest form.
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
            let s = String(decoding: b, as: UTF8.self)
            // Valid UTF-8 round-trips exactly; anything invalid was replaced.
            if Array(s.utf8) != b { throw DecodeError.badUtf8 }
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
                return .int((-1) - Int64(n))
            case 2:
                let n = try argument(ai)
                return .bytes(try chunk(n))
            case 3:
                let n = try argument(ai)
                return .text(try text(n))
            case 4:
                let n = try argument(ai)
                var xs: [Value] = []
                var i: UInt64 = 0
                while i < n {
                    xs.append(try value())
                    i += 1
                }
                return .list(xs)
            case 5:
                let n = try argument(ai)
                return .record(try pairs(n))
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

        /// `n` key–value pairs whose keys are text strings in strictly
        /// increasing order of their encoded bytes.
        mutating func pairs(_ n: UInt64) throws -> [String: Value] {
            var m: [String: Value] = [:]
            var prev: [UInt8]? = nil
            var i: UInt64 = 0
            while i < n {
                let start = pos
                let k = try key()
                let raw = Array(input[start..<pos])
                if let p = prev {
                    let c = compareBytes(raw, p)
                    if c == 0 { throw DecodeError.duplicateKey }
                    if c < 0 { throw DecodeError.unsortedKeys }
                }
                let v = try value()
                m[k] = v
                prev = raw
                i += 1
            }
            return m
        }

        mutating func key() throws -> String {
            let ib = try peek()
            if ib >> 5 != 3 { throw DecodeError.nonTextKey }
            _ = try byte()
            let n = try argument(ib & 0x1f)
            return try text(n)
        }

        mutating func identifier() throws -> Id {
            let ib = try byte()
            if ib >> 5 != 2 { throw DecodeError.badId }
            let n = try argument(ib & 0x1f)
            if n != 16 { throw DecodeError.badId }
            let b = try chunk(n)
            guard let i = Id(bytes: b) else { throw DecodeError.badId }
            return i
        }
    }
}
