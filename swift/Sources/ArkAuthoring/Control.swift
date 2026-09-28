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

func emitHelper(_ c: Collector, _ name: String, _ input: [NamedField], _ ret: Ty, _ body: (Emitter) -> Repr) {
    if c.helpers[name] != nil { return }
    let em = Emitter(c, .helper)
    c.helpers[name] = Function(name: name, kind: .helper, scope: nil, autos: [], input: input, ret: ret, body: [])
    let v = Ambient.with(.emit(em)) { body(em) }
    em.append(.sReturn(lift(v)))
    let fn = Function(name: name, kind: .helper, scope: nil, autos: [], input: input, ret: ret, body: em.blocks[0].stmts, names: em.names)
    c.helpers[name] = fn
    c.ready.append(fn)
}
