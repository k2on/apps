import Foundation
import ArkDB

// §2.4 Control: every construct returns a value and takes its bodies as
// closures. Under Emit each closure runs once, recording a block; under
// Native a closure runs only when its branch is taken.

/// Who authored the entry, and the non-determinism it was allowed, drawn
/// at its origin and frozen in the log.
public final class Ctx {
    init() {}

    /// The user the authority verified.
    public var user: Text {
        switch Ambient.mode {
        case .emit: return Text(repr: .e(.ctxUser))
        case .native(let n): return Text(repr: .v(.text(n.ctx.user)))
        }
    }

    /// The login the entry was authored under.
    public var session: Text {
        switch Ambient.mode {
        case .emit: return Text(repr: .e(.ctxSession))
        case .native(let n): return Text(repr: .v(.text(n.ctx.session)))
        }
    }

    /// The clock at the origin, in milliseconds, under this name.
    public func now(_ name: String) -> Int { return draw(name, .now) }

    /// A fresh id for `T`'s table, under this name.
    public func newId<T: Row>(_ name: String) -> Id<T> { return draw(name, .newId(T.NAME)) }

    func draw<T: Term>(_ name: String, _ a: Auto) -> T {
        switch Ambient.mode {
        case .emit(let em):
            em.auto(name, a)
            return T(repr: .e(.auto(name)))
        case .native(let n):
            guard let v = n.autos[name] else { n.bug("MissingAuto \(name)"); return T(repr: .v(zero(T.ty))) }
            return T(repr: .v(v))
        }
    }
}

func branch(_ c: Bool, then: () -> Effect, else otherwise: () -> Effect) -> Effect {
    switch Ambient.mode {
    case .emit(let em):
        let a = em.record { _ = then() }
        let b = em.record { _ = otherwise() }
        em.append(.sIf(lift(c.repr), a, b))
        return .none
    case .native(let n):
        n.flush()
        if n.stopped { return .none }
        _ = n.value(c.repr) == .bool(true) ? then() : otherwise()
        n.flush()
        return .none
    }
}

/// `SIf c body []`.
@discardableResult
public func when(_ c: Bool, _ body: () -> Effect) -> Effect {
    return branch(c, then: body, else: { .none })
}

/// `SIf c [] body`.
@discardableResult
public func unless(_ c: Bool, _ body: () -> Effect) -> Effect {
    return branch(c, then: { .none }, else: body)
}

/// `SIf c a b`.
@discardableResult
public func ifElse(_ c: Bool, then: () -> Effect, else otherwise: () -> Effect) -> Effect {
    return branch(c, then: then, else: otherwise)
}

/// `EIf c a b`: only the taken arm is evaluated.
public func pick<T: Term>(_ c: Bool, _ a: @autoclosure () -> T, _ b: @autoclosure () -> T) -> T {
    switch Ambient.mode {
    case .emit: return T(repr: .e(.ife(lift(c.repr), lift(a().repr), lift(b().repr))))
    case .native(let n):
        if n.stopped { return T(repr: .v(zero(T.ty))) }
        return n.value(c.repr) == .bool(true) ? a() : b()
    }
}

/// `SFor x xs body`.
@discardableResult
public func forEach<T: Term>(_ xs: List<T>, _ body: (T) -> Effect) -> Effect {
    switch Ambient.mode {
    case .emit(let em):
        let x = em.fresh()
        let b = em.record { _ = body(T(repr: .e(.variable(x)))) }
        em.append(.sFor(x, lift(xs.repr), b))
        return .none
    case .native(let n):
        n.flush()
        if n.stopped { return .none }
        guard case .list(let vs) = n.value(xs.repr) else { n.bug("TypeError expected List"); return .none }
        for v in vs {
            if n.stopped { break }
            _ = body(T(repr: .v(v)))
            n.flush()
        }
        return .none
    }
}

/// `SRefuse msg`: the entry is refused with this text, and nothing it did stands.
@discardableResult
public func refuse(_ why: Text) -> Effect {
    switch Ambient.mode {
    case .emit(let em):
        em.append(.sRefuse(lift(why.repr)))
        return .none
    case .native(let n):
        n.flush()
        if n.stopped { return .none }
        if case .text(let t) = n.value(why.repr) { n.refuse(.refused(t)) } else { n.bug("refuse: not text") }
        return .none
    }
}

// MARK: - helpers

/// A helper: a pure function of the vocabulary's types, emitted once when
/// first called (`ECall`), and called directly under Native.
/// `let double = helper("double", "x") { (x: Int) in x.mul(2) }`.
public func helper<A: Term, R: Term>(_ name: String, _ p: String, _ body: @escaping (A) -> R) -> (A) -> R {
    return { a in
        switch Ambient.mode {
        case .emit(let em):
            emitHelper(em.collector, name, [NamedField(p, A.ty)], R.ty) { hem in
                body(A(repr: .e(.arg(p)))).repr
            }
            return R(repr: .e(.call(name, [lift(a.repr)])))
        case .native: return body(a)
        }
    }
}

/// A helper of two arguments.
public func helper<A: Term, B: Term, R: Term>(_ name: String, _ p: String, _ q: String, _ body: @escaping (A, B) -> R) -> (A, B) -> R {
    return { a, b in
        switch Ambient.mode {
        case .emit(let em):
            emitHelper(em.collector, name, [NamedField(p, A.ty), NamedField(q, B.ty)], R.ty) { _ in
                body(A(repr: .e(.arg(p))), B(repr: .e(.arg(q)))).repr
            }
            return R(repr: .e(.call(name, [lift(a.repr), lift(b.repr)])))
        case .native: return body(a, b)
        }
    }
}

/// A helper called with its arguments named: the idiom a domain writes a
/// helper in, as an ordinary function whose body is this call —
///
///     public func movementKey(_ workId: Text, _ no: Int) -> Text {
///         helper("movement_key", ("work_id", workId), ("no", no)) { workId, no in
///             concat(list([workId, "#", no.toText()]))
///         }
///     }
///
/// Under Emit it is `ECall name args`, and the first call in a module's
/// emit records the helper itself — its body over `EArg`s of the named
/// parameters — immediately before the first function that calls it;
/// under Native it is the body, run on the values. The names are the
/// helper's parameters in the IR (`fnInput`), in order. A helper reads
/// nothing, writes nothing and refuses nothing.
public func helper<A: Term, R: Term>(_ name: String, _ a: (String, A), _ body: (A) -> R) -> R {
    switch Ambient.mode {
    case .emit(let em):
        emitHelper(em.collector, name, [NamedField(a.0, A.ty)], R.ty) { _ in body(A(repr: .e(.arg(a.0)))).repr }
        return R(repr: .e(.call(name, [lift(a.1.repr)])))
    case .native: return body(a.1)
    }
}

/// A helper of 2 named arguments.
public func helper<A: Term, B: Term, R: Term>(_ name: String, _ a: (String, A), _ b: (String, B), _ body: (A, B) -> R) -> R {
    switch Ambient.mode {
    case .emit(let em):
        emitHelper(em.collector, name, [NamedField(a.0, A.ty), NamedField(b.0, B.ty)], R.ty) { _ in body(A(repr: .e(.arg(a.0))), B(repr: .e(.arg(b.0)))).repr }
        return R(repr: .e(.call(name, [lift(a.1.repr), lift(b.1.repr)])))
    case .native: return body(a.1, b.1)
    }
}

/// A helper of 3 named arguments.
public func helper<A: Term, B: Term, C: Term, R: Term>(_ name: String, _ a: (String, A), _ b: (String, B), _ c: (String, C), _ body: (A, B, C) -> R) -> R {
    switch Ambient.mode {
    case .emit(let em):
        emitHelper(em.collector, name, [NamedField(a.0, A.ty), NamedField(b.0, B.ty), NamedField(c.0, C.ty)], R.ty) { _ in body(A(repr: .e(.arg(a.0))), B(repr: .e(.arg(b.0))), C(repr: .e(.arg(c.0)))).repr }
        return R(repr: .e(.call(name, [lift(a.1.repr), lift(b.1.repr), lift(c.1.repr)])))
    case .native: return body(a.1, b.1, c.1)
    }
}

/// A helper of 4 named arguments.
public func helper<A: Term, B: Term, C: Term, D: Term, R: Term>(_ name: String, _ a: (String, A), _ b: (String, B), _ c: (String, C), _ d: (String, D), _ body: (A, B, C, D) -> R) -> R {
    switch Ambient.mode {
    case .emit(let em):
        emitHelper(em.collector, name, [NamedField(a.0, A.ty), NamedField(b.0, B.ty), NamedField(c.0, C.ty), NamedField(d.0, D.ty)], R.ty) { _ in body(A(repr: .e(.arg(a.0))), B(repr: .e(.arg(b.0))), C(repr: .e(.arg(c.0))), D(repr: .e(.arg(d.0)))).repr }
        return R(repr: .e(.call(name, [lift(a.1.repr), lift(b.1.repr), lift(c.1.repr), lift(d.1.repr)])))
    case .native: return body(a.1, b.1, c.1, d.1)
    }
}

/// A helper of 5 named arguments.
public func helper<A: Term, B: Term, C: Term, D: Term, E: Term, R: Term>(_ name: String, _ a: (String, A), _ b: (String, B), _ c: (String, C), _ d: (String, D), _ e: (String, E), _ body: (A, B, C, D, E) -> R) -> R {
    switch Ambient.mode {
    case .emit(let em):
        emitHelper(em.collector, name, [NamedField(a.0, A.ty), NamedField(b.0, B.ty), NamedField(c.0, C.ty), NamedField(d.0, D.ty), NamedField(e.0, E.ty)], R.ty) { _ in body(A(repr: .e(.arg(a.0))), B(repr: .e(.arg(b.0))), C(repr: .e(.arg(c.0))), D(repr: .e(.arg(d.0))), E(repr: .e(.arg(e.0)))).repr }
        return R(repr: .e(.call(name, [lift(a.1.repr), lift(b.1.repr), lift(c.1.repr), lift(d.1.repr), lift(e.1.repr)])))
    case .native: return body(a.1, b.1, c.1, d.1, e.1)
    }
}

/// A helper of 6 named arguments.
public func helper<A: Term, B: Term, C: Term, D: Term, E: Term, F: Term, R: Term>(_ name: String, _ a: (String, A), _ b: (String, B), _ c: (String, C), _ d: (String, D), _ e: (String, E), _ f: (String, F), _ body: (A, B, C, D, E, F) -> R) -> R {
    switch Ambient.mode {
    case .emit(let em):
        emitHelper(em.collector, name, [NamedField(a.0, A.ty), NamedField(b.0, B.ty), NamedField(c.0, C.ty), NamedField(d.0, D.ty), NamedField(e.0, E.ty), NamedField(f.0, F.ty)], R.ty) { _ in body(A(repr: .e(.arg(a.0))), B(repr: .e(.arg(b.0))), C(repr: .e(.arg(c.0))), D(repr: .e(.arg(d.0))), E(repr: .e(.arg(e.0))), F(repr: .e(.arg(f.0)))).repr }
        return R(repr: .e(.call(name, [lift(a.1.repr), lift(b.1.repr), lift(c.1.repr), lift(d.1.repr), lift(e.1.repr), lift(f.1.repr)])))
    case .native: return body(a.1, b.1, c.1, d.1, e.1, f.1)
    }
}

func emitHelper(_ c: Collector, _ name: String, _ input: [NamedField], _ ret: Ty, _ body: (Emitter) -> Repr) {
    if c.helpers[name] != nil { return }
    let em = Emitter(c, .helper)
    c.helpers[name] = Function(name: name, kind: .helper, autos: [], input: input, ret: ret, body: [])
    let v = Ambient.with(.emit(em)) { body(em) }
    em.append(.sReturn(lift(v)))
    let fn = Function(name: name, kind: .helper, autos: [], input: input, ret: ret, body: em.blocks[0].stmts, names: em.names)
    c.helpers[name] = fn
    c.ready.append(fn)
}

// MARK: - evaluate

/// Run pure vocabulary natively, outside any procedure: the value `f`
/// computes, or the refusal it reached; a bug is thrown. What lets a host
/// compute what a helper computes — a derived key, say — with the helper's
/// own definition rather than a copy of it. There is no store to read.
public func evaluate<T: Term>(_ f: () -> T) throws -> Result<Value, Refusal> {
    let n = Native(MemoryStore(schema: ArkDB.Schema(tables: [])), ArkDB.Ctx(user: "", session: ""), [:], writable: false)
    let r = Ambient.with(.native(n)) { f().repr }
    switch n.stop {
    case .refused(let why)?: return .failure(why)
    case .bug(let b)?: throw Fault.bug(b)
    case nil: return .success(n.value(r))
    }
}
