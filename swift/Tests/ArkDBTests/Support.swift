// What every check is written with: a claim that does not hold is `Failed`,
// as `ensure!` returns an `Err` in the reference (`rust/ark/tests/vectors.rs`).
import ArkDB
import Foundation

/// A claim a check makes that does not hold.
struct Failed: Error, CustomStringConvertible {
    let message: String
    init(_ message: String) { self.message = message }
    var description: String { message }
}

func ensure(_ ok: Bool, _ why: @autoclosure () -> String) throws {
    if !ok { throw Failed(why()) }
}

func ensureEq<T: Equatable>(_ got: T, _ want: T, _ why: String) throws {
    if got != want { throw Failed("\(why): \(show(got)) != \(show(want))") }
}

func ensureBytes(_ got: [UInt8], _ want: [UInt8], _ why: String) throws {
    if got != want { throw Failed("\(why): \(Hex.encode(got)) != \(Hex.encode(want))") }
}

func show(_ x: Any) -> String {
    switch x {
    case let v as Value: return v.show()
    case let b as [UInt8]: return Hex.encode(b)
    case let vs as [Value]: return "[" + vs.map { $0.show() }.joined(separator: ",") + "]"
    default: return "\(x)"
    }
}

extension Value {
    /// A field of a struct, or `Failed`.
    func field(_ name: String) throws -> Value {
        guard case .record(let r) = self else { throw Failed("expected a struct with \(name), got \(show().prefix(60))") }
        guard let v = r[name] else { throw Failed("no field \"\(name)\" in \(show().prefix(80))") }
        return v
    }

    /// A field of a struct, or nil where it lacks it.
    func opt(_ name: String) -> Value? { recordValue?[name] }

    func list() throws -> [Value] {
        guard case .list(let xs) = self else { throw Failed("expected a list, got \(show().prefix(60))") }
        return xs
    }

    func text() throws -> String {
        guard case .text(let s) = self else { throw Failed("expected text, got \(show().prefix(60))") }
        return s
    }

    func int() throws -> Int64 {
        guard case .int(let n) = self else { throw Failed("expected an int, got \(show().prefix(60))") }
        return n
    }

    func bytes() throws -> [UInt8] {
        guard case .bytes(let b) = self else { throw Failed("expected bytes, got \(show().prefix(60))") }
        return b
    }

    func record() throws -> Record {
        guard case .record(let r) = self else { throw Failed("expected a struct, got \(show().prefix(60))") }
        return r
    }
}

/// A file's bytes, or `Failed`.
func readFile(_ path: String) throws -> [UInt8] {
    guard let d = FileManager.default.contents(atPath: path) else { throw Failed("cannot read \(path)") }
    return [UInt8](d)
}

/// A file in the vectors' JSON, parsed.
func readJson(_ path: String) throws -> Value { try Json.decode(bytes: try readFile(path)) }

/// Hex text as bytes, or `Failed`.
func hexBytes(_ v: Value) throws -> [UInt8] {
    guard let b = Hex.decode(try v.text()) else { throw Failed("not hex: \(v.show().prefix(60))") }
    return b
}

/// Milliseconds on a monotonic clock.
func nowMs() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1_000_000 }
