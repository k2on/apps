// The JSON dialect ArkDB values cross text in (`rust/ark/src/json.rs`): the
// vectors are written in it, and so is every line a `harken-peer` reads and
// answers.
//
// JSON cannot say bytes, ids or 64-bit integers, so those three are wrapped —
// `{"$bytes": hex}`, `{"$id": "8-4-4-4-12"}`, `{"$int": "decimal"}` — and
// text, booleans and `null` are themselves. A value is printed on one line
// with no spaces, a struct's keys in code-point order: `Value.show`, which is
// this dialect already.
//
// `decode` reads it back and is a little wider than `json` prints, as the
// reference's is: a bare JSON integer is an `Int` too, because a person typing
// a command writes `3`, and whitespace anywhere JSON allows it is skipped. A
// fraction or an exponent is refused rather than rounded — there are no floats
// in the value model (§3). An object is a wrapper only when it has exactly one
// key and that key is one of the three; `{"$int": 3}` is an error.

/// Why a text is not the dialect: what was wrong, and the offset it was noticed at (`json::JsonError`).
public struct JsonError: Error, CustomStringConvertible {
    public let at: Int
    public let why: String
    public var description: String { "\(why) at offset \(at)" }
}

public enum Json {
    /// Nesting deeper than this is refused rather than recursed into.
    static let depth = 128

    /// A value, on one line (`json::json`).
    public static func json(_ v: Value) -> String { v.show() }

    /// A JSON string: only `"`, `\` and the C0 controls escaped (`json::quoted`).
    public static func quoted(_ s: String) -> String { Value.quote(s) }

    /// Already-printed JSON, as an array on one line (`json::array`).
    public static func array(_ items: [String]) -> String { "[" + items.joined(separator: ",") + "]" }

    /// One value in the dialect, and nothing after it but whitespace (`json::decode`).
    public static func decode(_ text: String) throws -> Value { try decode(bytes: Array(text.utf8)) }

    /// The same, over UTF-8 bytes.
    public static func decode(bytes: [UInt8]) throws -> Value {
        return try bytes.withUnsafeBufferPointer { buf in
            var p = Parser(s: buf, at: 0)
            let v = try p.value(0)
            p.skip()
            if p.at != buf.count { throw p.fail("something after the value") }
            return v
        }
    }

    struct Parser {
        let s: UnsafeBufferPointer<UInt8>
        var at: Int

        func fail(_ why: String) -> JsonError { JsonError(at: at, why: why) }

        @inline(__always) func peek() -> Int { at < s.count ? Int(s[at]) : -1 }

        mutating func skip() {
            while at < s.count {
                let c = s[at]
                if c == 32 || c == 9 || c == 10 || c == 13 { at += 1 } else { break }
            }
        }

        mutating func eat(_ c: UInt8) throws {
            skip()
            if peek() == Int(c) { at += 1 } else { throw fail("expected `\(Character(Unicode.Scalar(c)))`") }
        }

        mutating func word(_ w: String, _ v: Value) throws -> Value {
            let b = Array(w.utf8)
            if at + b.count <= s.count && (0..<b.count).allSatisfy({ s[at + $0] == b[$0] }) {
                at += b.count
                return v
            }
            throw fail("not a value")
        }

        mutating func value(_ depth: Int) throws -> Value {
            if depth > Json.depth { throw fail("nested too deeply") }
            skip()
            let c = peek()
            switch c {
            case -1: throw fail("expected a value, found the end")
            case 110: return try word("null", .null)
            case 116: return try word("true", .bool(true))
            case 102: return try word("false", .bool(false))
            case 34: return .text(try string())
            case 91: return try arr(depth)
            case 123: return try obj(depth)
            default:
                if c == 45 || (c >= 48 && c <= 57) { return try number() }
                throw fail("not a value")
            }
        }

        mutating func arr(_ depth: Int) throws -> Value {
            at += 1
            var xs = [Value]()
            skip()
            if peek() == 93 {
                at += 1
                return .list(xs)
            }
            while true {
                xs.append(try value(depth + 1))
                skip()
                switch peek() {
                case 44: at += 1
                case 93:
                    at += 1
                    return .list(xs)
                default: throw fail("expected `,` or `]`")
                }
            }
        }

        mutating func obj(_ depth: Int) throws -> Value {
            let start = at
            at += 1
            var pairs = [(String, Value)]()
            var seen = Set<CodePoints>()
            skip()
            if peek() == 125 {
                at += 1
                return .record(Record())
            }
            while true {
                skip()
                if peek() != 34 { throw fail("expected a key") }
                let k = try string()
                try eat(58)
                let v = try value(depth + 1)
                if !seen.insert(CodePoints(k)).inserted { throw fail("a key twice") }
                pairs.append((k, v))
                skip()
                switch peek() {
                case 44: at += 1
                case 125:
                    at += 1
                    if pairs.count == 1 { if let w = try wrapper(pairs[0].0, pairs[0].1, start) { return w } }
                    return .record(Record(pairs))
                default: throw fail("expected `,` or `}`")
                }
            }
        }

        func wrapper(_ k: String, _ v: Value, _ start: Int) throws -> Value? {
            func wrapped(_ what: String, _ ok: Value?) throws -> Value {
                guard let x = ok else { throw JsonError(at: start, why: "`\(k)` wraps \(what)") }
                return x
            }
            let t = v.textValue
            switch k {
            case "$int": return try wrapped(t == nil ? "text" : "a decimal integer", t.flatMap(Json.decimal).map { .int($0) })
            case "$bytes": return try wrapped(t == nil ? "text" : "hex", t.flatMap { Hex.decode($0) }.map { .bytes($0) })
            case "$id": return try wrapped(t == nil ? "text" : "an 8-4-4-4-12 id", t.flatMap { Id.ofText($0) }.map { .id($0) })
            default: return nil
            }
        }

        mutating func number() throws -> Value {
            let start = at
            if peek() == 45 { at += 1 }
            while peek() >= 48 && peek() <= 57 { at += 1 }
            if peek() == 46 || peek() == 101 || peek() == 69 { throw fail("a fraction or an exponent: the value model has no floats") }
            let digits = String(decoding: UnsafeBufferPointer(rebasing: s[start..<at]), as: UTF8.self)
            guard let n = Int64(digits) else { throw JsonError(at: start, why: "not a 64-bit integer") }
            return .int(n)
        }

        mutating func hex4() throws -> UInt32 {
            if at + 4 > s.count { throw fail("a short \\u escape") }
            var n: UInt32 = 0
            for i in 0..<4 {
                guard let d = Hex.nibble(s[at + i]) else { throw fail("a \\u escape that is not hex") }
                n = n << 4 | UInt32(d)
            }
            at += 4
            return n
        }

        mutating func string() throws -> String {
            at += 1
            let start = at
            // The common case: no escape, read as one slice.
            while at < s.count {
                let c = s[at]
                if c == 34 {
                    let slice = UnsafeBufferPointer(rebasing: s[start..<at])
                    at += 1
                    guard let t = Utf8.decodeStrict(slice) else { throw fail("a string that is not UTF-8") }
                    return t
                }
                if c == 92 || c < 32 { break }
                at += 1
            }
            var out = Array(UnsafeBufferPointer(rebasing: s[start..<at]))
            while true {
                let c = peek()
                switch c {
                case -1: throw fail("a string with no end")
                case 34:
                    at += 1
                    guard let t = Utf8.decodeStrict(out) else { throw fail("a string that is not UTF-8") }
                    return t
                case 92:
                    at += 1
                    if at >= s.count { throw fail("an escape with no end") }
                    let e = peek()
                    at += 1
                    var code: UInt32
                    switch e {
                    case 34: code = 34
                    case 92: code = 92
                    case 47: code = 47
                    case 98: code = 8
                    case 102: code = 12
                    case 110: code = 10
                    case 114: code = 13
                    case 116: code = 9
                    case 117:
                        let hi = try hex4()
                        if hi >= 0xd800 && hi < 0xdc00 {
                            if !(peek() == 92 && at + 1 < s.count && s[at + 1] == 117) { throw fail("half a surrogate pair") }
                            at += 2
                            let lo = try hex4()
                            if !(lo >= 0xdc00 && lo < 0xe000) { throw fail("half a surrogate pair") }
                            code = 0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)
                        } else if hi >= 0xdc00 && hi < 0xe000 {
                            throw fail("a \\u escape that is no character")
                        } else {
                            code = hi
                        }
                    default: throw fail("an unknown escape")
                    }
                    out.append(contentsOf: Array(String(Unicode.Scalar(code)!).utf8))
                default:
                    if c < 32 { throw fail("a control character in a string") }
                    out.append(UInt8(c))
                    at += 1
                }
            }
        }
    }

    /// Rust's `i64::from_str`: an optional sign, then digits only.
    static func decimal(_ t: String) -> Int64? {
        var body = Substring(t)
        if body.hasPrefix("-") || body.hasPrefix("+") { body = body.dropFirst() }
        if body.isEmpty || !body.utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }) { return nil }
        return Int64(t)
    }
}
