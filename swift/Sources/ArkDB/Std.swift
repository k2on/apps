// §5 The standard library (`rust/ark/src/stdlib.rs`): everything a function
// may compute that is not an operator or a read. `trim`, `lower` and
// `is_alnum` consult the pinned Unicode tables and nothing of the platform.
// Text is walked by code point, and searched by UTF-8 bytes — never by
// `String`'s own `hasPrefix` or `range(of:)`, which match canonical
// equivalents.

/// A fault a standard function reports (`stdlib::StdError`): `arity` and
/// `typeMismatch` are bugs.
public enum StdError: Error {
    case arity(StdFn, Int)
    case typeMismatch(StdFn)
    /// A deterministic verdict.
    case fault(String)
}

public enum Std {
    /// `stdlib::std`: a standard function applied to evaluated arguments, or a `StdError`.
    public static func apply(_ f: StdFn, _ args: [Value]) throws -> Value {
        if args.count != f.arity { throw StdError.arity(f, args.count) }
        let a = args.count > 0 ? args[0] : .null
        let b = args.count > 1 ? args[1] : .null
        let c = args.count > 2 ? args[2] : .null
        let mismatch = StdError.typeMismatch(f)
        func text(_ v: Value) throws -> String {
            guard case .text(let s) = v else { throw mismatch }
            return s
        }
        func int(_ v: Value) throws -> Int64 {
            guard case .int(let n) = v else { throw mismatch }
            return n
        }
        func list(_ v: Value) throws -> [Value] {
            guard case .list(let xs) = v else { throw mismatch }
            return xs
        }
        func bytes(_ v: Value) throws -> [UInt8] {
            guard case .bytes(let b) = v else { throw mismatch }
            return b
        }
        switch f {
        case .Trim: return .text(trim(try text(a)))
        case .IsEmpty: return .bool(try text(a).isEmpty)
        case .Concat:
            var out = ""
            for x in try list(a) { out += try text(x) }
            return .text(out)
        case .Lower: return .text(fold(try text(a)))
        case .IsAlnum:
            let t = try text(a)
            return .bool(!t.isEmpty && t.unicodeScalars.allSatisfy { UnicodeTables.isAlphanumeric($0.value) })
        case .Chars: return .list(try text(a).unicodeScalars.map { .text(String($0)) })
        case .TextLen: return .int(Int64(try text(a).unicodeScalars.count))
        case .StartsWith:
            let s = Array(try text(a).utf8)
            let p = Array(try text(b).utf8)
            return .bool(s.count >= p.count && Array(s[0..<p.count]) == p)
        case .SplitOnce:
            let s = try text(a)
            let sep = try text(b)
            if sep.isEmpty { return .null }
            guard let at = textIndex(s, sep) else { return .null }
            let u = Array(s.utf8)
            let n = sep.utf8.count
            return .record([("before", .text(String(decoding: u[0..<at], as: UTF8.self))), ("after", .text(String(decoding: u[(at + n)...], as: UTF8.self)))])
        case .TextOfInt: return .text(String(try int(a)))
        case .Hex: return .text(ArkDB.Hex.encode(try bytes(a)))
        case .Min: return .int(Swift.min(try int(a), try int(b)))
        case .Max: return .int(Swift.max(try int(a), try int(b)))
        case .Clamp:
            let x = try int(a)
            let lo = try int(b)
            let hi = try int(c)
            if lo > hi { throw StdError.fault("clamp: lower bound above upper bound") }
            return .int(Swift.max(lo, Swift.min(hi, x)))
        case .Abs:
            let n = try int(a)
            if n == Int64.min { throw StdError.fault("integer overflow") }
            return .int(Swift.abs(n))
        case .Fnv1a64: return .int(fnv1a64(Array(try text(a).utf8)))
        case .Sha256: return .bytes(ArkDB.Sha256.hash(try bytes(a)))
        case .IdOfText: return Id.ofText(try text(a)).map { .id($0) } ?? .null
        case .TextOfId:
            guard case .id(let i) = a else { throw mismatch }
            return .text(i.text)
        case .NilId: return .id(Id.nilId)
        case .Utf8: return .bytes(Array(try text(a).utf8))
        case .First: return try list(a).first ?? .null
        case .Last: return try list(a).last ?? .null
        case .Len: return .int(Int64(try list(a).count))
        case .Contains: return .bool(try list(a).contains(b))
        case .Reverse: return .list(try list(a).reversed())
        case .ListConcat: return .list(try list(a) + list(b))
        case .Insert:
            var xs = try list(a)
            let at = try place("insert", try int(b), xs.count, end: true)
            xs.insert(c, at: at)
            return .list(xs)
        case .RemoveAt:
            var xs = try list(a)
            let at = try place("remove_at", try int(b), xs.count, end: false)
            xs.remove(at: at)
            return .list(xs)
        case .Move:
            var xs = try list(a)
            let from = try place("move", try int(b), xs.count, end: false)
            let to = try place("move", try int(c), xs.count, end: false)
            xs.insert(xs.remove(at: from), at: to)
            return .list(xs)
        case .Slice:
            let xs = try list(a)
            let start = try int(b)
            let end = try int(c)
            if start < 0 || start > end || end > Int64(xs.count) {
                throw StdError.fault("slice: \(start)..\(end) out of range for a list of \(xs.count)")
            }
            return .list(Array(xs[Int(start)..<Int(end)]))
        case .IsSome: return .bool(!a.isNull)
        case .UnwrapOr: return a.isNull ? b : a
        case .Unwrap:
            if a.isNull { throw StdError.fault("unwrapped none") }
            return a
        }
    }

    /// `stdlib::place` (§5.1): an index into a list of `n`, `0..<n` — or
    /// `0...n` where the end is a place too, as `insert`'s is — or the
    /// refusal `<f>: index <i> out of range for a list of <n>`. Never clamped.
    static func place(_ f: String, _ i: Int64, _ n: Int, end: Bool) throws -> Int {
        let bound = Int64(n) + (end ? 1 : 0)
        if i < 0 || i >= bound { throw StdError.fault("\(f): index \(i) out of range for a list of \(n)") }
        return Int(i)
    }

    /// `trim_matches(is_white_space)`: white space by the pinned tables, off both ends.
    public static func trim(_ t: String) -> String {
        let us = Array(t.unicodeScalars)
        var start = 0
        var end = us.count
        while start < end && UnicodeTables.isWhiteSpace(us[start].value) { start += 1 }
        while end > start && UnicodeTables.isWhiteSpace(us[end - 1].value) { end -= 1 }
        if start == 0 && end == us.count { return t }
        var v = String.UnicodeScalarView()
        v.append(contentsOf: us[start..<end])
        return String(v)
    }

    /// FNV-1a, 64-bit, over the bytes in order, as the two's-complement `Int` it is stored as.
    public static func fnv1a64(_ bytes: [UInt8]) -> Int64 {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        for b in bytes {
            h ^= UInt64(b)
            h = h &* 0x0000_0100_0000_01b3
        }
        return Int64(bitPattern: h)
    }
}
