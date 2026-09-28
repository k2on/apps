import Foundation
import ArkDB

// §1.1–1.2 and §2.5: routers, middleware, procedures, and the module.

/// A middleware declared on a router: a guard or a provide.
final class MiddlewareDecl {
    let name: String
    let kind: FnKind
    let input: InputSpec
    let ret: Ty?
    let run: (Ctx, Any, Any) -> Repr?
    init(_ name: String, _ kind: FnKind, _ input: InputSpec, _ ret: Ty?, _ run: @escaping (Ctx, Any, Any) -> Repr?) {
        self.name = name; self.kind = kind; self.input = input; self.ret = ret; self.run = run
    }
}

/// A procedure: a mutation or a query, on the chain it was built from.
final class RouteDecl {
    let name: String
    let kind: FnKind
    let chain: [MiddlewareDecl]
    let input: InputSpec
    let ret: Ty?
    let run: (Ctx, Any, Any, [Repr]) -> Repr?
    init(_ name: String, _ kind: FnKind, _ chain: [MiddlewareDecl], _ input: InputSpec, _ ret: Ty?, _ run: @escaping (Ctx, Any, Any, [Repr]) -> Repr?) {
        self.name = name; self.kind = kind; self.chain = chain; self.input = input; self.ret = ret; self.run = run
    }
}

/// What every chain built from one `router(...)` shares.
public final class RouterCore {
    let name: String
    let scope: ArkDB.Scope
    let makeDb: () -> Any
    var middleware: [MiddlewareDecl] = []
    var routes: [RouteDecl] = []
    init(_ name: String, _ scope: ArkDB.Scope, _ makeDb: @escaping () -> Any) {
        self.name = name; self.scope = scope; self.makeDb = makeDb
    }

    func declare(_ m: MiddlewareDecl) {
        precondition(!middleware.contains { $0.name == m.name }, "ArkAuthoring: middleware \(m.name) declared twice")
        middleware.append(m)
    }
}

/// A router, or a chain of middleware built on one.
public protocol AnyRouter {
    var core: RouterCore { get }
}

/// A procedure, ready to be named in `routes(...)`.
public struct Route {
    let core: RouterCore
    let decl: RouteDecl
}

/// A router over scope `S`: `router(Playlists.self, "playlists")`.
public func router<S: Scope>(_ s: S.Type, _ name: String) -> Router<S> {
    return Router(core: RouterCore(name, RowSchema.scope(S.self), { RowSchema.make(S.self) }), chain: [])
}

func provided<P: Term>(_ ps: [Repr], _ i: Swift.Int) -> P { return P(repr: ps[i]) }

/// A chain with no provided value.
public struct Router<S: Scope>: AnyRouter {
    public let core: RouterCore
    let chain: [MiddlewareDecl]

    /// A guard: runs before the body, may refuse, returns nothing.
    public func `guard`(_ name: String, _ body: @escaping (Ctx, S) -> Effect) -> Router<S> {
        let m = MiddlewareDecl(name, .guard_, .none, nil) { ctx, db, _ in _ = body(ctx, db as! S); return nil }
        core.declare(m)
        return Router(core: core, chain: chain + [m])
    }

    /// A provide: runs before the body, may refuse, and hands the body a value.
    public func provide<I: Input, P: Term>(_ name: String, _ body: @escaping (Ctx, S, I) -> P) -> Router1<S, P> {
        let m = MiddlewareDecl(name, .provide, InputSpec.of(I.self), P.ty) { ctx, db, i in body(ctx, db as! S, i as! I).repr }
        core.declare(m)
        return Router1(core: core, chain: chain + [m])
    }

    public func input<I: Input>(_ t: I.Type) -> Proc<S, I> { return Proc(core: core, chain: chain) }

    public func mutation(_ name: String, _ body: @escaping (Ctx, S, ()) -> Effect) -> Route {
        return Route(core: core, decl: RouteDecl(name, .mutator, chain, .none, nil) { ctx, db, _, _ in _ = body(ctx, db as! S, ()); return nil })
    }

    public func query<T: Term>(_ name: String, _ body: @escaping (Ctx, S, ()) -> T) -> Route {
        return Route(core: core, decl: RouteDecl(name, .query, chain, .none, T.ty) { ctx, db, _, _ in body(ctx, db as! S, ()).repr })
    }

    /// The router's procedures, in order.
    public func routes(_ rs: Route...) -> Router<S> {
        for r in rs {
            precondition(r.core === core, "ArkAuthoring: \(r.decl.name) is not on router \(core.name)")
            core.routes.append(r.decl)
        }
        return Router(core: core, chain: [])
    }
}

/// A chain whose body is handed one provided value.
public struct Router1<S: Scope, P: Term>: AnyRouter {
    public let core: RouterCore
    let chain: [MiddlewareDecl]

    public func `guard`(_ name: String, _ body: @escaping (Ctx, S) -> Effect) -> Router1<S, P> {
        let m = MiddlewareDecl(name, .guard_, .none, nil) { ctx, db, _ in _ = body(ctx, db as! S); return nil }
        core.declare(m)
        return Router1(core: core, chain: chain + [m])
    }

    public func provide<I: Input, Q: Term>(_ name: String, _ body: @escaping (Ctx, S, I) -> Q) -> Router2<S, P, Q> {
        let m = MiddlewareDecl(name, .provide, InputSpec.of(I.self), Q.ty) { ctx, db, i in body(ctx, db as! S, i as! I).repr }
        core.declare(m)
        return Router2(core: core, chain: chain + [m])
    }

    public func input<I: Input>(_ t: I.Type) -> Proc1<S, I, P> { return Proc1(core: core, chain: chain) }

    public func mutation(_ name: String, _ body: @escaping (Ctx, S, (), P) -> Effect) -> Route {
        return Route(core: core, decl: RouteDecl(name, .mutator, chain, .none, nil) { ctx, db, _, ps in _ = body(ctx, db as! S, (), provided(ps, 0)); return nil })
    }

    public func query<T: Term>(_ name: String, _ body: @escaping (Ctx, S, (), P) -> T) -> Route {
        return Route(core: core, decl: RouteDecl(name, .query, chain, .none, T.ty) { ctx, db, _, ps in body(ctx, db as! S, (), provided(ps, 0)).repr })
    }
}

/// A chain whose body is handed two provided values, oldest first.
public struct Router2<S: Scope, P: Term, Q: Term>: AnyRouter {
    public let core: RouterCore
    let chain: [MiddlewareDecl]

    public func `guard`(_ name: String, _ body: @escaping (Ctx, S) -> Effect) -> Router2<S, P, Q> {
        let m = MiddlewareDecl(name, .guard_, .none, nil) { ctx, db, _ in _ = body(ctx, db as! S); return nil }
        core.declare(m)
        return Router2(core: core, chain: chain + [m])
    }

    public func input<I: Input>(_ t: I.Type) -> Proc2<S, I, P, Q> { return Proc2(core: core, chain: chain) }

    public func mutation(_ name: String, _ body: @escaping (Ctx, S, (), P, Q) -> Effect) -> Route {
        return Route(core: core, decl: RouteDecl(name, .mutator, chain, .none, nil) { ctx, db, _, ps in _ = body(ctx, db as! S, (), provided(ps, 0), provided(ps, 1)); return nil })
    }

    public func query<T: Term>(_ name: String, _ body: @escaping (Ctx, S, (), P, Q) -> T) -> Route {
        return Route(core: core, decl: RouteDecl(name, .query, chain, .none, T.ty) { ctx, db, _, ps in body(ctx, db as! S, (), provided(ps, 0), provided(ps, 1)).repr })
    }
}

/// A procedure with an input, on a chain with no provided value.
public struct Proc<S: Scope, I: Input> {
    let core: RouterCore
    let chain: [MiddlewareDecl]

    public func mutation(_ name: String, _ body: @escaping (Ctx, S, I) -> Effect) -> Route {
        return Route(core: core, decl: RouteDecl(name, .mutator, chain, InputSpec.of(I.self), nil) { ctx, db, i, _ in _ = body(ctx, db as! S, i as! I); return nil })
    }

    public func query<T: Term>(_ name: String, _ body: @escaping (Ctx, S, I) -> T) -> Route {
        return Route(core: core, decl: RouteDecl(name, .query, chain, InputSpec.of(I.self), T.ty) { ctx, db, i, _ in body(ctx, db as! S, i as! I).repr })
    }
}

/// A procedure with an input, on a chain with one provided value.
public struct Proc1<S: Scope, I: Input, P: Term> {
    let core: RouterCore
    let chain: [MiddlewareDecl]

    public func mutation(_ name: String, _ body: @escaping (Ctx, S, I, P) -> Effect) -> Route {
        return Route(core: core, decl: RouteDecl(name, .mutator, chain, InputSpec.of(I.self), nil) { ctx, db, i, ps in _ = body(ctx, db as! S, i as! I, provided(ps, 0)); return nil })
    }

    public func query<T: Term>(_ name: String, _ body: @escaping (Ctx, S, I, P) -> T) -> Route {
        return Route(core: core, decl: RouteDecl(name, .query, chain, InputSpec.of(I.self), T.ty) { ctx, db, i, ps in body(ctx, db as! S, i as! I, provided(ps, 0)).repr })
    }
}

/// A procedure with an input, on a chain with two provided values.
public struct Proc2<S: Scope, I: Input, P: Term, Q: Term> {
    let core: RouterCore
    let chain: [MiddlewareDecl]

    public func mutation(_ name: String, _ body: @escaping (Ctx, S, I, P, Q) -> Effect) -> Route {
        return Route(core: core, decl: RouteDecl(name, .mutator, chain, InputSpec.of(I.self), nil) { ctx, db, i, ps in _ = body(ctx, db as! S, i as! I, provided(ps, 0), provided(ps, 1)); return nil })
    }

    public func query<T: Term>(_ name: String, _ body: @escaping (Ctx, S, I, P, Q) -> T) -> Route {
        return Route(core: core, decl: RouteDecl(name, .query, chain, InputSpec.of(I.self), T.ty) { ctx, db, i, ps in body(ctx, db as! S, i as! I, provided(ps, 0), provided(ps, 1)).repr })
    }
}
