// The typed half of the bridge (`docs/plan-v5.md` §1.16, N3), beside the
// bindings UniFFI writes: what the Swift `arkdb gen` writes for a domain
// stands on — a row id, a value read from its bytes in the kernel's format
// (how a view's rows cross), a view's rows read as their type with what it
// did, and JSON as the kernel reads and writes it (how an input crosses,
// and the app's own JSON is read).
import Foundation

/// A row id: its sixteen bytes in hex.
public struct Id: Hashable {
    public let hex: String
    public init(_ hex: String) { self.hex = hex }
    public static let none = Id(String(repeating: "0", count: 32))
}

/// A value read from its bytes in the kernel's value format (`ark::format`,
/// postcard): an integer zigzagged into a varint, text and bytes after their
/// length, a list after its count, an option after a 0 or a 1, a sum's
/// variant as its index, a record's fields in order.
public final class Reader {
    private let b: Data
    private var at: Int

    public init(_ b: Data) { self.b = b; at = b.startIndex }

    private func byte() -> UInt8 { defer { at += 1 }; return b[at] }

    private func varint() -> UInt64 {
        var v: UInt64 = 0
        var shift: UInt64 = 0
        while true {
            let x = byte()
            v |= UInt64(x & 0x7f) << shift
            if x < 0x80 { return v }
            shift += 7
        }
    }

    private func bytes() -> Data {
        let n = Int(varint())
        defer { at += n }
        return b[at..<at + n]
    }

    public func variant() -> Int { Int(varint()) }
    public func int() -> Int64 { let v = varint(); return Int64(bitPattern: v >> 1) ^ -Int64(bitPattern: v & 1) }
    public func bool() -> Bool { byte() != 0 }
    public func text() -> String { String(decoding: bytes(), as: UTF8.self) }
    public func hex() -> String { bytes().map { String(format: "%02x", $0) }.joined() }
    public func id() -> Id { Id(hex()) }
    public func opt<T>(_ f: (Reader) -> T) -> T? { byte() == 0 ? nil : f(self) }
    public func list<T>(_ f: (Reader) -> T) -> [T] { (0..<Int(varint())).map { _ in f(self) } }
}

/// A view's rows read as `T`, and what it did since last taken (its `Op`s
/// and the rows that came with them): each row its bytes read by `read`. A
/// view made again says so as its changes: its rows removed and inserted.
public final class LiveView<T> {
    let handle: ViewHandle
    let read: (Reader) -> T

    public init(_ handle: ViewHandle, _ read: @escaping (Reader) -> T) { self.handle = handle; self.read = read }

    public func rows() -> [T] { handle.rows().map { read(Reader($0)) } }

    public func take() -> Update<T> {
        let t = handle.take()
        if t.ops.isEmpty { return .unchanged }
        var r = t.rows.makeIterator()
        return .patched(t.ops.map { o -> Change<T> in
            switch o {
            case let .insert(at): return .insert(at: Int(at), row: read(Reader(r.next()!)))
            case let .remove(at): return .remove(at: Int(at))
            case let .update(at): return .update(at: Int(at), row: read(Reader(r.next()!)))
            }
        })
    }
}

public enum Update<T> {
    case unchanged
    case patched([Change<T>])
}

public enum Change<T> {
    case insert(at: Int, row: T)
    case remove(at: Int)
    case update(at: Int, row: T)
}

extension Array {
    /// `changes` made to the list a screen holds.
    public mutating func splice(_ changes: [Change<Element>]) {
        for c in changes {
            switch c {
            case let .insert(at, row): insert(row, at: at)
            case let .remove(at): remove(at: at)
            case let .update(at, row): self[at] = row
            }
        }
    }
}

/// JSON as the kernel reads and writes it.
public enum J {
    public static func parse(_ s: String) -> Any? { try? JSONSerialization.jsonObject(with: Data(s.utf8), options: .fragmentsAllowed) }
    public static func int(_ j: Any?) -> Int64 { (j as? Int64) ?? (j as? Int).map(Int64.init) ?? (j as! NSNumber).int64Value }
    public static func text(_ j: Any?) -> String { j as! String }
    public static func bool(_ j: Any?) -> Bool { (j as? Bool) ?? (j as! NSNumber).boolValue }
    public static func opt<T>(_ j: Any?, _ f: (Any?) -> T) -> T? { j == nil || j is NSNull ? nil : f(j) }
    public static func list<T>(_ j: Any?, _ f: (Any?) -> T) -> [T] { (j as! [Any]).map { f($0) } }
    public static func sum(_ j: Any?) -> (String, [String: Any]?) {
        if let s = j as? String { return (s, nil) }
        let o = j as! [String: Any]
        let k = o.keys.first!
        return (k, o[k] as? [String: Any])
    }
    public static func obj(_ parts: [String]) -> String { "{" + parts.joined(separator: ",") + "}" }
    public static func wopt<T>(_ v: T?, _ f: (T) -> String) -> String { v.map(f) ?? "null" }
    public static func wlist<T>(_ v: [T], _ f: (T) -> String) -> String { "[" + v.map(f).joined(separator: ",") + "]" }
    public static func q(_ s: String) -> String {
        var out = "\""
        for u in s.unicodeScalars {
            switch u {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case "\u{8}": out += "\\b"
            case "\u{c}": out += "\\f"
            default: if u.value < 0x20 { out += String(format: "\\u%04x", u.value) } else { out.unicodeScalars.append(u) }
            }
        }
        return out + "\""
    }
}
