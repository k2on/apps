import Foundation

/// §5 The standard library, one static function per `StdFn`, each over
/// `Value`s, each `throws` a `Fault`: a type mismatch or arity error is a
/// `bug` (a verified module never produces one), a deterministic verdict —
/// an overflow, a clamp with its bounds crossed — is a `refuse`.
///
/// The three Unicode functions consult `UnicodeTables` and nothing of the
/// platform; text is handled as Unicode scalars throughout.
public enum Std {
    // MARK: text

    public static func trim(_ a: Value) throws -> Value {
        let t = try text(.trim, a)
        var s = Array(t.unicodeScalars)
        while let f = s.first, UnicodeTables.isWhiteSpace(f) { s.removeFirst() }
        while let l = s.last, UnicodeTables.isWhiteSpace(l) { s.removeLast() }
        return .text(fromScalars(s))
    }

    public static func isEmpty(_ a: Value) throws -> Value {
        return .bool(try text(.isEmpty, a).unicodeScalars.isEmpty)
    }

    public static func concat(_ a: Value) throws -> Value {
        let xs = try list(.concat, a)
        var out = ""
        for x in xs { out += try text(.concat, x) }
        return .text(out)
    }

    public static func lower(_ a: Value) throws -> Value {
        let t = try text(.lower, a)
        return .text(fromScalars(t.unicodeScalars.map { UnicodeTables.toLowerSimple($0) }))
    }

    public static func isAlnum(_ a: Value) throws -> Value {
        let s = try text(.isAlnum, a).unicodeScalars
        return .bool(!s.isEmpty && s.allSatisfy { UnicodeTables.isAlphanumeric($0) })
    }

    public static func chars(_ a: Value) throws -> Value {
        let t = try text(.chars, a)
        return .list(t.unicodeScalars.map { .text(String(Character($0))) })
    }

    public static func textLen(_ a: Value) throws -> Value {
        return .int(Int64(try text(.textLen, a).unicodeScalars.count))
    }

    public static func startsWith(_ a: Value, _ b: Value) throws -> Value {
        let t = Array(try text(.startsWith, a).unicodeScalars)
        let p = Array(try text(.startsWith, b).unicodeScalars)
        return .bool(t.count >= p.count && Array(t[0..<p.count]) == p)
    }

    /// At the first occurrence: `{before, after}` or null.
    public static func splitOnce(_ a: Value, _ b: Value) throws -> Value {
        let t = Array(try text(.splitOnce, a).unicodeScalars)
        let sep = Array(try text(.splitOnce, b).unicodeScalars)
        if sep.isEmpty { return .null }
        if t.count < sep.count { return .null }
        var i = 0
        while i + sep.count <= t.count {
            if Array(t[i..<(i + sep.count)]) == sep {
                return .record([
                    "before": .text(fromScalars(Array(t[0..<i]))),
                    "after": .text(fromScalars(Array(t[(i + sep.count)...]))),
                ])
            }
            i += 1
        }
        return .null
    }

    public static func textOfInt(_ a: Value) throws -> Value {
        return .text(String(try int(.textOfInt, a)))
    }

    public static func hex(_ a: Value) throws -> Value {
        return .text(Hex.encode(try bytes(.hex, a)))
    }

    // MARK: int

    public static func min(_ a: Value, _ b: Value) throws -> Value {
        return .int(Swift.min(try int(.min, a), try int(.min, b)))
    }

    public static func max(_ a: Value, _ b: Value) throws -> Value {
        return .int(Swift.max(try int(.max, a), try int(.max, b)))
    }

    public static func clamp(_ a: Value, _ lo: Value, _ hi: Value) throws -> Value {
        let x = try int(.clamp, a), l = try int(.clamp, lo), h = try int(.clamp, hi)
        if l > h { throw Fault.refuse("clamp: lower bound above upper bound") }
        return .int(Swift.max(l, Swift.min(h, x)))
    }

    public static func abs(_ a: Value) throws -> Value {
        let n = try int(.abs, a)
        if n == Int64.min { throw Fault.refuse("integer overflow") }
        return .int(n < 0 ? -n : n)
    }

    // MARK: hash

    /// FNV-1a, 64-bit, over the text's UTF-8 bytes; the u64 reinterpreted.
    public static func fnv1a64(_ a: Value) throws -> Value {
        return .int(Int64(bitPattern: fnv1a64(Array(try text(.fnv1a64, a).utf8))))
    }

    public static func fnv1a64(_ bytes: [UInt8]) -> UInt64 {
        var h: UInt64 = 0xcbf29ce484222325
        for b in bytes {
            h ^= UInt64(b)
            h = h &* 0x00000100000001b3
        }
        return h
    }

    public static func sha256(_ a: Value) throws -> Value {
        return .bytes(Sha256.hash(try bytes(.sha256, a)))
    }

    // MARK: id

    public static func idOfText(_ a: Value) throws -> Value {
        if let i = Id(uuid: try text(.idOfText, a)) { return .id(i) }
        return .null
    }

    public static func textOfId(_ a: Value) throws -> Value {
        return .text(try id(.textOfId, a).uuid)
    }

    public static func nilId() throws -> Value {
        return .id(Id.nil_)
    }

    public static func utf8(_ a: Value) throws -> Value {
        return .bytes(Array(try text(.utf8, a).utf8))
    }

    // MARK: list and option

    public static func first(_ a: Value) throws -> Value {
        return try list(.first, a).first ?? .null
    }

    public static func last(_ a: Value) throws -> Value {
        return try list(.last, a).last ?? .null
    }

    public static func len(_ a: Value) throws -> Value {
        return .int(Int64(try list(.len, a).count))
    }

    public static func contains(_ a: Value, _ v: Value) throws -> Value {
        return .bool(try list(.contains, a).contains { compareValue(v, $0) == 0 })
    }

    public static func reverse(_ a: Value) throws -> Value {
        return .list(Array(try list(.reverse, a).reversed()))
    }

    public static func isSome(_ a: Value) throws -> Value {
        return .bool(!a.isNull())
    }

    public static func unwrapOr(_ a: Value, _ d: Value) throws -> Value {
        return a.isNull() ? d : a
    }

    /// The option's value; `None` is the refusal "unwrapped none".
    public static func unwrap(_ a: Value) throws -> Value {
        if a.isNull() { throw Fault.refuse("unwrapped none") }
        return a
    }

    // MARK: dispatch, for the interpreter

    /// Apply a standard function to already-evaluated arguments.
    public static func call(_ f: StdFn, _ args: [Value]) throws -> Value {
        if args.count != f.arity { throw Fault.bug("Arity \(f.rawValue)/\(args.count)") }
        switch f {
        case .trim: return try trim(args[0])
        case .isEmpty: return try isEmpty(args[0])
        case .concat: return try concat(args[0])
        case .lower: return try lower(args[0])
        case .isAlnum: return try isAlnum(args[0])
        case .chars: return try chars(args[0])
        case .textLen: return try textLen(args[0])
        case .startsWith: return try startsWith(args[0], args[1])
        case .splitOnce: return try splitOnce(args[0], args[1])
        case .textOfInt: return try textOfInt(args[0])
        case .hex: return try hex(args[0])
        case .min: return try min(args[0], args[1])
        case .max: return try max(args[0], args[1])
        case .clamp: return try clamp(args[0], args[1], args[2])
        case .abs: return try abs(args[0])
        case .fnv1a64: return try fnv1a64(args[0])
        case .sha256: return try sha256(args[0])
        case .idOfText: return try idOfText(args[0])
        case .textOfId: return try textOfId(args[0])
        case .nilId: return try nilId()
        case .utf8: return try utf8(args[0])
        case .first: return try first(args[0])
        case .last: return try last(args[0])
        case .len: return try len(args[0])
        case .contains: return try contains(args[0], args[1])
        case .reverse: return try reverse(args[0])
        case .isSome: return try isSome(args[0])
        case .unwrapOr: return try unwrapOr(args[0], args[1])
        case .unwrap: return try unwrap(args[0])
        }
    }

    // MARK: helpers

    static func fromScalars(_ s: [Unicode.Scalar]) -> String {
        var v = String.UnicodeScalarView()
        v.append(contentsOf: s)
        return String(v)
    }

    static func mismatch(_ f: StdFn) -> Fault { return .bug("TypeMismatch \(f.rawValue)") }

    static func text(_ f: StdFn, _ v: Value) throws -> String {
        if case .text(let t) = v { return t }
        throw mismatch(f)
    }

    static func int(_ f: StdFn, _ v: Value) throws -> Int64 {
        if case .int(let n) = v { return n }
        throw mismatch(f)
    }

    static func bytes(_ f: StdFn, _ v: Value) throws -> [UInt8] {
        if case .bytes(let b) = v { return b }
        throw mismatch(f)
    }

    static func id(_ f: StdFn, _ v: Value) throws -> Id {
        if case .id(let i) = v { return i }
        throw mismatch(f)
    }

    static func list(_ f: StdFn, _ v: Value) throws -> [Value] {
        if case .list(let xs) = v { return xs }
        throw mismatch(f)
    }
}
