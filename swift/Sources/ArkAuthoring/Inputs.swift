import Foundation
import ArkDB

// §1.3 and §2.5: a procedure's input is a struct of terms with a schema —
// each field's type and checks, and refinements over the whole.

/// A procedure's input.
public protocol Input: Codable {
    static var schema: Object<Self> { get }
}

extension Input {
    /// The input as the arguments an entry carries: every field, natively.
    public var args: Args {
        var out: Args = [:]
        for (name, t) in take(self) { out[name] = t.value ?? .null }
        return out
    }
}

/// One check as the builder holds it: an IR check, or a refinement kept as
/// the closure the author wrote.
enum CheckSpec {
    case ir(Check)
    case refine((Repr) -> Repr, String?)
}

struct FieldSpec {
    let name: String
    let ty: Ty
    let checks: [CheckSpec]
}

/// The schema of an input: its fields in order, and whole-input refinements.
public struct Object<I> {
    var fields: [FieldSpec] = []
    var refines: [((I) -> Repr, String?)] = []

    /// A field: its IR name and its builder.
    public func field<V: Term>(_ name: String, _ f: FieldBuilder<V>) -> Object<I> {
        var me = self
        me.fields.append(FieldSpec(name: name, ty: V.ty, checks: f.checks))
        return me
    }

    /// A check over the whole input, run after every field's.
    public func refine(_ p: @escaping (I) -> Bool, _ why: String? = nil) -> Object<I> {
        var me = self
        me.refines.append(({ p($0).repr }, why))
        return me
    }
}

/// An empty schema, to add fields to.
public func object<I>() -> Object<I> { return Object() }

/// A field's type and its checks, in order.
public struct FieldBuilder<V: Term> {
    var checks: [CheckSpec] = []

    func with(_ c: CheckSpec) -> FieldBuilder<V> {
        var me = self
        me.checks.append(c)
        return me
    }

    /// Text: normalise before every later check and before the body.
    public func trim() -> FieldBuilder<V> { return with(.ir(.trim)) }
    /// Text: at least `n` code points.
    public func min(_ n: Swift.Int, _ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.minLen(n, why))) }
    /// Text: at most `n` code points.
    public func max(_ n: Swift.Int, _ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.maxLen(n, why))) }
    /// Int: `lo <= v <= hi`.
    public func range(_ lo: Swift.Int, _ hi: Swift.Int, _ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.range(lo, hi, why))) }
    public func atLeast(_ lo: Swift.Int, _ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.range(lo, nil, why))) }
    public func atMost(_ hi: Swift.Int, _ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.range(nil, hi, why))) }
    /// List: at least one element.
    public func nonEmpty(_ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.nonEmpty(why))) }
    /// Id: a row with that key exists in the procedure's scope.
    public func exists(_ why: String? = nil) -> FieldBuilder<V> { return with(.ir(.exists(why))) }
    /// Any: the predicate holds of the value.
    public func refine(_ p: @escaping (V) -> Bool, _ why: String? = nil) -> FieldBuilder<V> {
        return with(.refine({ p(V(repr: $0)).repr }, why))
    }
}

public func text() -> FieldBuilder<Text> { return FieldBuilder() }
public func int() -> FieldBuilder<Int> { return FieldBuilder() }
public func bool() -> FieldBuilder<Bool> { return FieldBuilder() }
public func bytes() -> FieldBuilder<Bytes> { return FieldBuilder() }
public func id<T: Row>(_ t: T.Type) -> FieldBuilder<Id<T>> { return FieldBuilder() }
/// A field that may be absent; its checks apply to a present value.
public func opt<V: Term>(_ f: FieldBuilder<V>) -> FieldBuilder<Opt<V>> { return FieldBuilder(checks: f.checks) }
public func list<V: Term>(_ f: FieldBuilder<V>) -> FieldBuilder<List<V>> { return FieldBuilder(checks: f.checks) }

/// An input type, erased: its fields, refinements, and how to build one.
struct InputSpec {
    let fields: [FieldSpec]
    let refines: [((Any) -> Repr, String?)]
    let make: (FieldSource) -> Any

    static func of<I: Input>(_ t: I.Type) -> InputSpec {
        let o = I.schema
        return InputSpec(fields: o.fields,
                         refines: o.refines.map { r in ({ r.0($0 as! I) }, r.1) },
                         make: { build(I.self, $0) })
    }

    static let none = InputSpec(fields: [], refines: [], make: { _ in () })

    /// The input built from arguments by name: `EArg` under Emit, the
    /// entry's values under Native.
    func arguments(_ args: Args?) -> Any {
        return make(ByName { t, name in
            if let a = args { return t.init(repr: .v(a[name] ?? .null)) }
            return t.init(repr: .e(.arg(name)))
        })
    }
}
