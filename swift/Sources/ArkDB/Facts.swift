// A change in the protocol's fact form (`rust/ark/src/protocol.rs`,
// `change_value` and `change_from_value`): `{"t": "add", "table", "row"}`,
// `{"t": "remove", "table", "row"}`, `{"t": "edit", "table", "old",
// "new"}` — how the vectors write a mutator's changes and a view's
// batches, and how an authority's facts cross the wire.

/// A frame or a fact whose shape is wrong (`protocol::DecodeError`).
public struct FactError: Error, CustomStringConvertible {
    public let what: String
    public init(_ what: String) { self.what = what }
    public var description: String { what }
}

public enum Facts {
    /// `change_value`.
    public static func changeValue(_ c: Change) -> Value {
        switch c {
        case .add(let t, let r): return .record([("t", .text("add")), ("table", .text(t)), ("row", .record(r))])
        case .remove(let t, let r): return .record([("t", .text("remove")), ("table", .text(t)), ("row", .record(r))])
        case .edit(let t, let o, let n): return .record([("t", .text("edit")), ("table", .text(t)), ("old", .record(o)), ("new", .record(n))])
        }
    }

    /// `change_from_value`.
    public static func changeFromValue(_ v: Value) throws -> Change {
        guard case .record(let m) = v else { throw FactError("a change is a struct") }
        func need(_ k: String) throws -> Value {
            guard let x = m[k] else { throw FactError("missing field \(k)") }
            return x
        }
        func text(_ k: String) throws -> String {
            guard case .text(let s) = try need(k) else { throw FactError("\(k) is not text") }
            return s
        }
        func row(_ k: String) throws -> Row {
            guard case .record(let r) = try need(k) else { throw FactError("\(k) is not a row") }
            return r
        }
        let t = try text("t")
        let table = try text("table")
        switch t {
        case "add": return .add(table, try row("row"))
        case "remove": return .remove(table, try row("row"))
        case "edit": return .edit(table, try row("old"), try row("new"))
        default: throw FactError("unknown change \(t)")
        }
    }
}
