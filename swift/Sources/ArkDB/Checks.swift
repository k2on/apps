import Foundation

/// What one field's checks came to: the normalised value, or a message.
public enum FieldCheck: Equatable {
    case ok(Value)
    case failed(String)
}

/// §1.3 (AUTHORING.md) The checks on one input field, as the interpreter,
/// a native procedure and the form validator all run them: in order, `trim`
/// normalising the value every later check and the body see, the first
/// failure's message ending the field. A field of an option type applies
/// its checks only to a present value.
public enum Checks {
    /// The message of a whole-input refinement with none of its own.
    public static let invalid = "invalid"

    /// `Ark.Eval.defaultMessage`: what a failing check says when its author
    /// gave no message. `table` is the table an `exists` check looked in.
    public static func defaultMessage(_ field: String, _ c: Check, table: TableName? = nil) -> String {
        switch c {
        case .trim: return field + ": invalid"
        case .minLen(let n, _): return "\(field): at least \(n) characters"
        case .maxLen(let n, _): return "\(field): at most \(n) characters"
        case .range(let lo?, let hi?, _): return "\(field): between \(lo) and \(hi)"
        case .range(let lo?, nil, _): return "\(field): at least \(lo)"
        case .range(nil, let hi?, _): return "\(field): at most \(hi)"
        case .range(nil, nil, _): return "\(field): invalid"
        case .nonEmpty: return "\(field): at least one"
        case .exists: return "\(field): no such \(table ?? "row")"
        case .refine: return "\(field): invalid"
        }
    }

    /// The message a check gives: its own, or the default.
    public static func message(_ field: String, _ c: Check, table: TableName? = nil) -> String {
        switch c {
        case .trim: return defaultMessage(field, c, table: table)
        case .minLen(_, let w), .maxLen(_, let w), .range(_, _, let w), .nonEmpty(let w), .exists(let w), .refine(_, let w):
            return w ?? defaultMessage(field, c, table: table)
        }
    }

    /// The table an id-typed field names, through an option.
    public static func idTable(_ t: Ty) -> TableName? {
        switch t {
        case .id(let tb): return tb
        case .option(let x): return idTable(x)
        default: return nil
        }
    }

    /// One field's checks over a present value: the normalised value, or
    /// the first failing check's message. `exists` answers whether a row
    /// with that single-column key is in the table; `refine` evaluates a
    /// refinement over the field's current value.
    public static func field(_ name: String, _ f: Field, _ v0: Value,
                             exists: (TableName, Value) throws -> Bool,
                             refine: (Expr, Value) throws -> Bool) rethrows -> FieldCheck {
        var v = v0
        if case .option = f.ty, v.isNull() { return .ok(v) }
        for c in f.checks {
            switch c {
            case .trim:
                if case .text = v { v = (try? Std.trim(v)) ?? v }
            case .minLen(let n, _):
                if case .text(let t) = v, t.unicodeScalars.count < n { return .failed(message(name, c)) }
            case .maxLen(let n, _):
                if case .text(let t) = v, t.unicodeScalars.count > n { return .failed(message(name, c)) }
            case .range(let lo, let hi, _):
                if case .int(let x) = v {
                    if let l = lo, x < Int64(l) { return .failed(message(name, c)) }
                    if let h = hi, x > Int64(h) { return .failed(message(name, c)) }
                }
            case .nonEmpty:
                if case .list(let xs) = v, xs.isEmpty { return .failed(message(name, c)) }
            case .exists:
                let tb = idTable(f.ty) ?? ""
                if !(try exists(tb, v)) { return .failed(message(name, c, table: tb)) }
            case .refine(let e, _):
                if !(try refine(e, v)) { return .failed(message(name, c)) }
            }
        }
        return .ok(v)
    }
}
