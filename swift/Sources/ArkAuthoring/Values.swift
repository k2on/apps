import Foundation
import ArkDB

/// §2.1 A value of the vocabulary: under `Emit` it holds an expression,
/// under `Native` a value. Distinct from the host's types, so that a host
/// `if` on one does not compile.
public protocol Term {
    /// The IR type.
    static var ty: Ty { get }
    var repr: Repr { get }
    init(repr: Repr)
}

/// The scalar and container terms (every `Term` but a row): each one is a
/// single `Repr`, and is `Codable` only so that a row or an input made of
/// them gets its coding synthesised; the coders here never call these.
public protocol Scalar: Term, Codable {}

struct NotCodable: Error {}

extension Scalar {
    public init(from decoder: Decoder) throws { throw NotCodable() }
    public func encode(to encoder: Encoder) throws { throw NotCodable() }
}

// MARK: - the types

public struct Bool: Scalar, ExpressibleByBooleanLiteral {
    public static var ty: Ty { return .bool }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
    public init(booleanLiteral b: Swift.Bool) { repr = .v(.bool(b)) }
    public init(_ b: Swift.Bool) { repr = .v(.bool(b)) }
}

public struct Int: Scalar, ExpressibleByIntegerLiteral {
    public static var ty: Ty { return .int }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
    public init(integerLiteral n: Int64) { repr = .v(.int(n)) }
    public init(_ n: Int64) { repr = .v(.int(n)) }
}

public struct Text: Scalar, ExpressibleByStringLiteral {
    public static var ty: Ty { return .text }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
    public init(stringLiteral s: String) { repr = .v(.text(s)) }
    public init(_ s: String) { repr = .v(.text(s)) }
}

public struct Bytes: Scalar {
    public static var ty: Ty { return .bytes }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
    public init(_ b: [UInt8]) { repr = .v(.bytes(b)) }
}

/// An id of a row of `T`'s table. The table is the static type alone.
public struct Id<T: Row>: Scalar {
    public static var ty: Ty { return .id(T.NAME) }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
    public init(_ raw: ArkDB.Id) { repr = .v(.id(raw)) }
}

/// An option: flat, as values are — `some(x)` is `x`, none is null.
public struct Opt<T: Term>: Scalar {
    public static var ty: Ty { return .option(T.ty) }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
}

public struct List<T: Term>: Scalar {
    public static var ty: Ty { return .list(T.ty) }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
}

// MARK: - reading a native value back out, for an app

extension Term {
    /// The value, under `Native` or for a term built from one; nil for an
    /// expression.
    public var value: Value? {
        if case .v(let v) = repr { return v }
        return nil
    }
}

extension Bool { public var bool: Swift.Bool? { if case .bool(let b)? = value { return b }; return nil } }
extension Int { public var int: Int64? { if case .int(let n)? = value { return n }; return nil } }
extension Text { public var string: String? { if case .text(let t)? = value { return t }; return nil } }
extension Bytes { public var bytes: [UInt8]? { if case .bytes(let b)? = value { return b }; return nil } }
extension Id { public var raw: ArkDB.Id? { if case .id(let i)? = value { return i }; return nil } }
extension Opt {
    /// The present value, natively; nil for none (or an expression).
    public var get: T? {
        guard let v = value, !v.isNull() else { return nil }
        return T(repr: .v(v))
    }
}
extension List {
    /// The elements, natively.
    public var items: [T] {
        guard case .list(let xs)? = value else { return [] }
        return xs.map { T(repr: .v($0)) }
    }
}

// MARK: - §2.2 operations

extension Term {
    public func eq(_ b: Self) -> Bool { return cmp(.eq, b) }
    public func ne(_ b: Self) -> Bool { return cmp(.ne, b) }
    public func lt(_ b: Self) -> Bool { return cmp(.lt, b) }
    public func le(_ b: Self) -> Bool { return cmp(.le, b) }
    public func gt(_ b: Self) -> Bool { return cmp(.gt, b) }
    public func ge(_ b: Self) -> Bool { return cmp(.ge, b) }

    func cmp(_ o: CmpOp, _ b: Self) -> Bool {
        return op(.cmp(o, lift(repr), lift(b.repr))) { n in .bool(Ops.compare(o, n.value(self.repr), n.value(b.repr))) }
    }
}

func arith(_ o: Op, _ a: Repr, _ b: Repr) -> Int {
    return op(.op(o, [lift(a), lift(b)])) { n in try Ops.arith(o, n.value(a), n.value(b)) }
}

extension Int {
    public func add(_ b: Int) -> Int { return arith(.add, repr, b.repr) }
    public func sub(_ b: Int) -> Int { return arith(.sub, repr, b.repr) }
    public func mul(_ b: Int) -> Int { return arith(.mul, repr, b.repr) }
    public func div(_ b: Int) -> Int { return arith(.div, repr, b.repr) }
    public func rem(_ b: Int) -> Int { return arith(.mod, repr, b.repr) }
    public func neg() -> Int { return op(.op(.neg, [lift(repr)])) { n in try Ops.neg(n.value(self.repr)) } }
    public func min(_ b: Int) -> Int { return std(.min, [repr, b.repr]) }
    public func max(_ b: Int) -> Int { return std(.max, [repr, b.repr]) }
    public func clamp(_ lo: Int, _ hi: Int) -> Int { return std(.clamp, [repr, lo.repr, hi.repr]) }
    public func abs() -> Int { return std(.abs, [repr]) }
    public func toText() -> Text { return std(.textOfInt, [repr]) }
}

extension Bool {
    /// Short-circuits, as `EOp And` does: `b` is not evaluated when this is false.
    public func and(_ b: @autoclosure () -> Bool) -> Bool {
        switch Ambient.mode {
        case .emit: return Bool(repr: .e(.op(.and, [lift(repr), lift(b().repr)])))
        case .native(let n):
            if n.stopped { return false }
            return n.value(repr) == .bool(true) ? b() : false
        }
    }

    /// Short-circuits, as `EOp Or` does.
    public func or(_ b: @autoclosure () -> Bool) -> Bool {
        switch Ambient.mode {
        case .emit: return Bool(repr: .e(.op(.or, [lift(repr), lift(b().repr)])))
        case .native(let n):
            if n.stopped { return false }
            return n.value(repr) == .bool(true) ? true : b()
        }
    }

    public func not() -> Bool { return op(.op(.not, [lift(repr)])) { n in Ops.not(n.value(self.repr)) } }
}

extension Text {
    public func trim() -> Text { return std(.trim, [repr]) }
    public func isEmpty() -> Bool { return std(.isEmpty, [repr]) }
    public func lower() -> Text { return std(.lower, [repr]) }
    public func len() -> Int { return std(.textLen, [repr]) }
    public func startsWith(_ p: Text) -> Bool { return std(.startsWith, [repr, p.repr]) }
    public func splitOnce(_ p: Text) -> Opt<Split> { return std(.splitOnce, [repr, p.repr]) }
    public func chars() -> List<Text> { return std(.chars, [repr]) }
    public func isAlnum() -> Bool { return std(.isAlnum, [repr]) }
    public func utf8() -> Bytes { return std(.utf8, [repr]) }
    public func fnv1a64() -> Int { return std(.fnv1a64, [repr]) }
}

/// What `splitOnce` answers: the text before the first separator and after it.
public struct Split: Term {
    public static var ty: Ty { return .structOf(["before": .text, "after": .text]) }
    public let repr: Repr
    public init(repr: Repr) { self.repr = repr }
    public var before: Text { return field(repr, "before") }
    public var after: Text { return field(repr, "after") }
}

/// A field of a struct-valued term.
func field<T: Term>(_ r: Repr, _ name: FieldName) -> T {
    switch r {
    case .e(let e): return T(repr: .e(.field(e, name)))
    case .v(.record(let m)): return T(repr: .v(m[name] ?? .null))
    case .v: return T(repr: .v(zero(T.ty)))
    }
}

extension Bytes {
    public func hex() -> Text { return std(.hex, [repr]) }
    public func sha256() -> Bytes { return std(.sha256, [repr]) }
}

extension Id {
    public func toText() -> Text { return std(.textOfId, [repr]) }
}

/// The id a text spells, or none.
public func idOfText<T: Row>(_ t: Text) -> Opt<Id<T>> { return std(.idOfText, [t.repr]) }

/// The nil id.
public func nilId<T: Row>() -> Id<T> { return std(.nilId, []) }

/// Texts joined.
public func concat(_ xs: List<Text>) -> Text { return std(.concat, [xs.repr]) }

/// A list of terms.
public func list<T: Term>(_ xs: [T]) -> List<T> {
    return op(.list(xs.map { lift($0.repr) })) { n in .list(xs.map { n.value($0.repr) }) }
}

/// An option holding a value.
public func some<T: Term>(_ x: T) -> Opt<T> {
    return op(.some(lift(x.repr))) { n in n.value(x.repr) }
}

/// The empty option of a type.
public func none<T: Term>(_ t: T.Type) -> Opt<T> {
    return op(.none(T.ty)) { _ in .null }
}

extension Opt {
    public func isSome() -> Bool { return std(.isSome, [repr]) }
    public func unwrapOr(_ d: T) -> T { return std(.unwrapOr, [repr, d.repr]) }
    /// The value, or the refusal "unwrapped none".
    public func unwrap() -> T { return std(.unwrap, [repr]) }

    /// `EMatch opt x (f x) d`: `d` is evaluated only when this is none.
    public func mapOr<U: Term>(_ d: @autoclosure () -> U, _ f: (T) -> U) -> U {
        switch Ambient.mode {
        case .emit(let em):
            let x = em.fresh()
            let body = f(T(repr: .e(.variable(x))))
            return U(repr: .e(.match(lift(repr), x, lift(body.repr), lift(d().repr))))
        case .native(let n):
            if n.stopped { return U(repr: .v(zero(U.ty))) }
            let v = n.value(repr)
            return v.isNull() ? d() : f(T(repr: .v(v)))
        }
    }

    /// `EMatch opt x (ESome (f x)) (ENone U)`.
    public func map<U: Term>(_ f: (T) -> U) -> Opt<U> {
        switch Ambient.mode {
        case .emit(let em):
            let x = em.fresh()
            let body = f(T(repr: .e(.variable(x))))
            return Opt<U>(repr: .e(.match(lift(repr), x, .some(lift(body.repr)), .none(U.ty))))
        case .native(let n):
            if n.stopped { return Opt<U>(repr: .v(.null)) }
            let v = n.value(repr)
            return v.isNull() ? Opt<U>(repr: .v(.null)) : Opt<U>(repr: .v(n.value(f(T(repr: .v(v))).repr)))
        }
    }

    /// `EMatch opt x (EIf (p x) (ESome x) (ENone T)) (ENone T)`.
    public func filter(_ p: (T) -> Bool) -> Opt<T> {
        switch Ambient.mode {
        case .emit(let em):
            let x = em.fresh()
            let c = p(T(repr: .e(.variable(x))))
            return Opt(repr: .e(.match(lift(repr), x, .ife(lift(c.repr), .some(.variable(x)), .none(T.ty)), .none(T.ty))))
        case .native(let n):
            if n.stopped { return Opt(repr: .v(.null)) }
            let v = n.value(repr)
            if v.isNull() { return self }
            return n.value(p(T(repr: .v(v))).repr) == .bool(true) ? Opt(repr: .v(v)) : Opt(repr: .v(.null))
        }
    }

    /// The value, or a refusal with this message: `SLet s opt`, `SIf
    /// (IsSome s) [] [SRefuse msg]`, and the value `EStd Unwrap [EVar s]`.
    public func orRefuse(_ why: Text) -> T {
        switch Ambient.mode {
        case .emit(let em):
            let s = em.fresh()
            em.append(.sLet(s, lift(repr)))
            em.append(.sIf(.std(.isSome, [.variable(s)]), [], [.sRefuse(lift(why.repr))]))
            return T(repr: .e(.std(.unwrap, [.variable(s)])))
        case .native(let n):
            n.flush()
            if n.stopped { return T(repr: .v(zero(T.ty))) }
            let v = n.value(repr)
            if v.isNull() {
                if case .text(let t) = n.value(why.repr) { n.refuse(.refused(t)) } else { n.bug("orRefuse: a message must be text") }
                return T(repr: .v(zero(T.ty)))
            }
            return T(repr: .v(v))
        }
    }
}

extension List {
    func binder<U: Term>(_ mk: (Expr, Sym, Expr) -> Expr, _ f: (T) -> U, _ native: (Native, [Value]) -> Value) -> Repr {
        switch Ambient.mode {
        case .emit(let em):
            let x = em.fresh()
            let body = f(T(repr: .e(.variable(x))))
            return .e(mk(lift(repr), x, lift(body.repr)))
        case .native(let n):
            if n.stopped { return .v(.null) }
            guard case .list(let xs) = n.value(repr) else { n.bug("TypeError expected List"); return .v(.list([])) }
            return .v(native(n, xs))
        }
    }

    public func map<U: Term>(_ f: (T) -> U) -> List<U> {
        return List<U>(repr: binder({ .map($0, $1, $2) }, f) { n, xs in .list(xs.map { n.value(f(T(repr: .v($0))).repr) }) }).orZero()
    }

    public func filter(_ p: (T) -> Bool) -> List<T> {
        return List(repr: binder({ .filter($0, $1, $2) }, p) { n, xs in .list(xs.filter { n.value(p(T(repr: .v($0))).repr) == .bool(true) }) }).orZero()
    }

    public func any(_ p: (T) -> Bool) -> Bool {
        return Bool(repr: binder({ .any($0, $1, $2) }, p) { n, xs in
            var r = false
            for x in xs where n.value(p(T(repr: .v(x))).repr) == .bool(true) { r = true }
            return .bool(r)
        }).orZero()
    }

    public func all(_ p: (T) -> Bool) -> Bool {
        return Bool(repr: binder({ .all($0, $1, $2) }, p) { n, xs in
            var r = true
            for x in xs where n.value(p(T(repr: .v(x))).repr) != .bool(true) { r = false }
            return .bool(r)
        }).orZero()
    }

    /// Stable, under the total order of the key.
    public func sortBy<K: Term>(_ key: (T) -> K) -> List<T> {
        return List(repr: binder({ .sortBy($0, $1, $2) }, key) { n, xs in
            let keyed = xs.map { ($0, n.value(key(T(repr: .v($0))).repr)) }
            return .list(stableSortPairs(keyed).map { $0.0 })
        }).orZero()
    }

    public func fold<A: Term>(_ initial: A, _ f: (A, T) -> A) -> A {
        switch Ambient.mode {
        case .emit(let em):
            let acc = em.fresh()
            let x = em.fresh()
            let body = f(A(repr: .e(.variable(acc))), T(repr: .e(.variable(x))))
            return A(repr: .e(.fold(lift(repr), lift(initial.repr), acc, x, lift(body.repr))))
        case .native(let n):
            if n.stopped { return A(repr: .v(zero(A.ty))) }
            guard case .list(let xs) = n.value(repr) else { n.bug("TypeError expected List"); return A(repr: .v(zero(A.ty))) }
            var a = initial
            for x in xs { a = A(repr: .v(n.value(f(a, T(repr: .v(x))).repr))) }
            return a
        }
    }

    public func first() -> Opt<T> { return std(.first, [repr]) }
    public func last() -> Opt<T> { return std(.last, [repr]) }
    public func len() -> Int { return std(.len, [repr]) }
    public func contains(_ x: T) -> Bool { return std(.contains, [repr, x.repr]) }
    public func reverse() -> List<T> { return std(.reverse, [repr]) }
}

extension Term {
    /// A stopped native body's placeholder is `.v(.null)`; give it the zero
    /// of this type instead, so that nothing downstream mistakes its shape.
    func orZero() -> Self {
        if case .v(.null) = repr, !(Self.ty.isOption) { return Self(repr: .v(zero(Self.ty))) }
        return self
    }
}

extension Ty {
    var isOption: Swift.Bool { if case .option = self { return true }; return false }
}

func stableSortPairs(_ xs: [(Value, Value)]) -> [(Value, Value)] {
    let indexed = xs.enumerated().map { ($0.offset, $0.element) }
    return indexed.sorted { a, b in
        let c = compareValue(a.1.1, b.1.1)
        return c != 0 ? c < 0 : a.0 < b.0
    }.map { $0.1 }
}
