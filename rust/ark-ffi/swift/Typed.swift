// The typed half of the bridge (`docs/plan-v5.md` §1.16, N3), beside the
// bindings UniFFI writes: what the Swift `arkdb gen` writes for a domain
// stands on — a row id, a view's rows read as their type with what it did
// (from their JSON, or a typed view's records as UniFFI hands them over),
// and JSON as the kernel reads and writes it.
import Foundation

/// A row id: its sixteen bytes in hex.
public struct Id: Hashable {
    public let hex: String
    public init(_ hex: String) { self.hex = hex }
    public static let none = Id(String(repeating: "0", count: 32))
}

/// A view's rows read as `T`, and what it did since last taken (its `Op`s
/// and the rows that came with them): each row its JSON read by `read` over
/// a `ViewHandle`, or — a typed view — rows UniFFI hands over as they are
/// (`typed`). A view made again says so as its changes: its rows removed
/// and inserted.
public final class LiveView<T> {
    let rowsOf: () -> [T]
    let takeOf: () -> Update<T>

    init(rows: @escaping () -> [T], take: @escaping () -> Update<T>) { rowsOf = rows; takeOf = take }

    public convenience init(_ handle: ViewHandle, _ read: @escaping (Any?) -> T) {
        self.init(rows: { handle.rows().map { read(J.parse($0)) } }, take: {
            let t = handle.take()
            return LiveView.taken(t.ops, t.rows.map { read(J.parse($0)) })
        })
    }

    /// A domain's typed view object (`<T>View`): its rows, and what it did
    /// with the rows that came with it.
    public static func typed(rows: @escaping () -> [T], take: @escaping () -> ([Op], [T])) -> LiveView<T> {
        LiveView(rows: rows, take: {
            let (ops, came) = take()
            return taken(ops, came)
        })
    }

    /// What a view did, `came` the rows that came with it: each insert's
    /// and update's.
    static func taken(_ ops: [Op], _ came: [T]) -> Update<T> {
        if ops.isEmpty { return .unchanged }
        var r = came.makeIterator()
        return .patched(ops.map { o -> Change<T> in
            switch o {
            case let .insert(at): return .insert(at: Int(at), row: r.next()!)
            case let .remove(at): return .remove(at: Int(at))
            case let .update(at): return .update(at: Int(at), row: r.next()!)
            }
        })
    }

    public func rows() -> [T] { rowsOf() }

    public func take() -> Update<T> { takeOf() }
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
