// The typed half of the bridge (`docs/plan-v5.md` §1.16, N3), beside the
// bindings UniFFI writes: what the Swift `arkdb gen` writes for a domain
// stands on — a row id, a value read from its bytes in the kernel's format
// (how a view's rows cross), a view's rows read as their type and kept, and
// JSON as the kernel reads and writes it (how an input crosses, and the
// app's own JSON is read).
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

/// A view's rows read as `T` and kept: read when it is opened, and what the
/// view did since spliced in by `update()`. Each row is its bytes read by
/// `read`. A view made again says so as its changes: its rows removed and
/// inserted.
public final class LiveView<T> {
    let handle: ViewHandle
    let read: (Reader) -> T
    /// Its rows as of the last `update()`.
    public private(set) var rows: [T]
    public convenience init(_ handle: ViewHandle, _ read: @escaping (Reader) -> T) {
        self.init(handle, read, handle.rows().map { read(Reader($0)) })
    }
    init(_ handle: ViewHandle, _ read: @escaping (Reader) -> T, _ rows: [T]) { self.handle = handle; self.read = read; self.rows = rows }

    /// What the view did since, spliced into `rows`: whether anything moved.
    public func update() -> Bool {
        let t = handle.take()
        var r = t.rows.makeIterator()
        for o in t.ops {
            switch o {
            case let .insert(at): rows.insert(read(Reader(r.next()!)), at: Int(at))
            case let .remove(at): rows.remove(at: Int(at))
            case let .update(at): rows[Int(at)] = read(Reader(r.next()!))
            }
        }
        return !t.ops.isEmpty
    }

    /// This view with each row made `f` of it: what is held from now on, in place of this.
    public func map<U>(_ f: @escaping (T) -> U) -> LiveView<U> {
        let read = self.read
        return LiveView<U>(handle, { f(read($0)) }, rows.map(f))
    }
}

/// A view of a domain `D`'s, opened once its peer is there (`hydrate`) and
/// its rows kept as it moves: none until then, nor when it was refused.
public final class Kept<D, E> {
    private let open: (D) throws -> LiveView<E>?
    private var view: LiveView<E>? = nil
    public var rows: [E] { view?.rows ?? [] }
    public init(_ open: @escaping (D) throws -> LiveView<E>?) { self.open = open }
    public func hydrate(_ d: D) { view = (try? open(d)) ?? nil }
    /// What it did since last asked, spliced in; true when it moved.
    public func update() -> Bool { view?.update() ?? false }
}

/// Views opened by a key while something holds them, each held as many
/// times as it is held, kept like the rest and let go with the last.
public final class KeptBy<D, K: Hashable, E> {
    private let open: (D, K) throws -> LiveView<E>?
    private var held = [K: (view: Kept<D, E>, refs: Int)]()
    public init(_ open: @escaping (D, K) throws -> LiveView<E>?) { self.open = open }

    /// Held once more; its rows when it was read just now, none when it was
    /// held already or there is no peer yet (`hydrate` reads it then).
    public func hold(_ key: K, _ d: D?) -> [E]? {
        if let e = held[key] {
            held[key] = (e.view, e.refs + 1)
            return nil
        }
        let open = self.open
        let v = Kept<D, E> { try open($0, key) }
        held[key] = (v, 1)
        guard let d = d else { return nil }
        v.hydrate(d)
        return v.rows
    }

    /// Held once less; true when nothing holds it now and it is let go.
    public func release(_ key: K) -> Bool {
        guard let e = held[key] else { return false }
        if e.refs > 1 {
            held[key] = (e.view, e.refs - 1)
            return false
        }
        held[key] = nil
        return true
    }

    /// Every one read, now that the peer is there.
    public func hydrate(_ d: D) { for e in held.values { e.view.hydrate(d) } }

    /// Every one's rows, by its key.
    public var rows: [K: [E]] { held.mapValues { $0.view.rows } }

    /// Every one that moved, and its rows now.
    public func update() -> [(K, [E])] { held.compactMap { k, e in e.view.update() ? (k, e.view.rows) : nil } }

    public func clear() { held = [:] }
}

/// What the peer says when anything moved: `f`, on the peer's thread.
public func listener(_ f: @escaping () -> Void) -> Listener { Told(f) }

private final class Told: Listener, @unchecked Sendable {
    private let f: () -> Void
    init(_ f: @escaping () -> Void) { self.f = f }
    func changed() { f() }
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
