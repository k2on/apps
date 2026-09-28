import Foundation
import ArkDB

/// A domain: its routers, in order. `emit()` is its module's bytes;
/// `procedures()` is every procedure as native code, by function hash,
/// for a runtime to apply what it holds natively.
public final class Module {
    let routers: [RouterCore]
    private var built: ArkDB.Module?
    private let lock = NSLock()

    public init(_ routers: AnyRouter...) {
        self.routers = routers.map { $0.core }
    }

    public init(routers: [AnyRouter]) {
        self.routers = routers.map { $0.core }
    }

    /// The module as `Emit` records it, verified: orders completed and every
    /// function normalised (AUTHORING.md §6). Throws the verifier's
    /// complaints.
    public func ir() throws -> ArkDB.Module {
        lock.lock()
        defer { lock.unlock() }
        if let m = built { return m }
        let m = try Emission.module(routers)
        built = m
        return m
    }

    /// The module's canonical bytes (`.ark`). A module that does not verify
    /// is a bug in the domain, and stops here.
    public func emit() -> [UInt8] {
        return Canon.encode(Encode.toValue(verified))
    }

    /// The hash of the whole module.
    public var hash: [UInt8] { return Hash.moduleHash(verified) }

    var verified: ArkDB.Module {
        do {
            return try ir()
        } catch {
            fatalError("ArkAuthoring: the module does not verify: \(error)")
        }
    }

    /// Every procedure, natively, by the hash of its closure — what a
    /// replica applies an entry through when it names one of them.
    public func procedures() -> [(FnHash, Procedure)] {
        let m = verified
        var out: [(FnHash, Procedure)] = []
        for core in routers {
            for d in core.routes {
                guard let fn = m.lookupFunction(d.name) else { continue }
                let h = Hash.functionHash(Hash.closure(m, fn))
                let mutate: ((ArkDB.Ctx, Args, Args, MemoryStore) throws -> Eval.Outcome)? = d.kind == .mutator ? { ctx, autos, args, st in
                    let (n, _) = try Native.run(core, d, ctx, autos, args, st.clone(), writable: true)
                    switch n.stop {
                    case .refused(let r)?: return .refused(r)
                    case .bug(let b)?: throw Fault.bug(b)
                    case nil: return .applied(n.store, n.changes)
                    }
                } : nil
                let query: ((ArkDB.Ctx, Args, MemoryStore) throws -> Result<Value, Refusal>)? = d.kind == .query ? { ctx, args, st in
                    let (n, r) = try Native.run(core, d, ctx, [:], args, st, writable: false)
                    switch n.stop {
                    case .refused(let why)?: return .failure(why)
                    case .bug(let b)?: throw Fault.bug(b)
                    case nil: return .success(r.map(n.value) ?? .null)
                    }
                } : nil
                out.append((h, Procedure(function: fn, mutate: mutate, query: query)))
            }
        }
        return out
    }
}

// MARK: - Emit

enum Emission {
    static func module(_ routers: [RouterCore]) throws -> ArkDB.Module {
        let c = Collector()
        var functions: [Function] = []
        var irRouters: [ArkDB.Router] = []
        func add(_ f: Function) {
            functions += c.ready
            c.ready = []
            functions.append(f)
        }
        // One set of tables: every router is over the same `Tables` type.
        if let first = routers.first {
            for core in routers where core.over != first.over {
                throw VerifyErrors(errors: [VerifyError(core.name, "TablesMismatch \(core.overName) \(first.overName)")])
            }
        }
        let tables = routers.first?.tables() ?? []
        for core in routers {
            irRouters.append(ArkDB.Router(name: core.name, uses: core.middleware.map { $0.name }))
            for m in core.middleware { add(middleware(c, core, m)) }
            for d in core.routes { add(route(c, core, d)) }
        }
        let m = ArkDB.Module(spec: specVersion, schema: ArkDB.Schema(tables: tables), functions: functions, routers: irRouters, live: [])
        switch Verify.verify(m) {
        case .success(let v): return v
        case .failure(let e): throw e
        }
    }

    /// A field's checks as IR: a refinement's closure runs once, over the
    /// field's `EArg`.
    static func fields(_ spec: InputSpec, stripped: Swift.Bool) -> [NamedField] {
        return spec.fields.map { f in
            if stripped { return NamedField(f.name, f.ty) }
            let checks: [Check] = f.checks.map { c in
                switch c {
                case .ir(let k): return k
                case .refine(let p, let why): return .refine(lift(p(.e(.arg(f.name)))), why)
                }
            }
            return NamedField(f.name, Field(f.ty, checks: checks))
        }
    }

    static func middleware(_ c: Collector, _ core: RouterCore, _ m: MiddlewareDecl) -> Function {
        let em = Emitter(c, m.kind)
        let input = Ambient.with(.emit(em)) { fields(m.input, stripped: true) }
        Ambient.with(.emit(em)) {
            let r = m.run(Ctx(), core.makeDb(), m.input.arguments(nil))
            if m.kind == .provide, let r = r { em.append(.sReturn(lift(r))) }
        }
        return Function(name: m.name, kind: m.kind, router: nil, uses: [], autos: em.autos,
                        input: input, refine: [], ret: m.ret, body: em.blocks[0].stmts, names: em.names)
    }

    static func route(_ c: Collector, _ core: RouterCore, _ d: RouteDecl) -> Function {
        let em = Emitter(c, d.kind)
        var input: [NamedField] = []
        var refine: [Refine] = []
        Ambient.with(.emit(em)) {
            input = fields(d.input, stripped: false)
            let whole = d.input.arguments(nil)
            refine = d.input.refines.map { Refine(lift($0.0(whole)), $0.1) }
            let provided = d.chain.filter { $0.kind == .provide }.map { Repr.e(.provided($0.name)) }
            let r = d.run(Ctx(), core.makeDb(), d.input.arguments(nil), provided)
            if d.kind == .query, let r = r { em.append(.sReturn(lift(r))) }
        }
        return Function(name: d.name, kind: d.kind, router: core.name, uses: d.chain.map { $0.name },
                        autos: em.autos, input: input, refine: refine, ret: d.ret, body: em.blocks[0].stmts, names: em.names)
    }
}

// MARK: - Native

extension Native {
    /// Run a procedure natively: the input's checks, the chain's
    /// middleware, the body — the same order `Eval.applyClosure` keeps, in
    /// one transaction over `store`. A missing argument is a bug, thrown.
    static func run(_ core: RouterCore, _ d: RouteDecl, _ ctx: ArkDB.Ctx, _ autos: Args, _ args0: Args,
                    _ store: MemoryStore, writable: Swift.Bool) throws -> (Native, Repr?) {
        for f in d.input.fields where args0[f.name] == nil { throw Fault.bug("MissingArg \(f.name)") }
        let n = Native(store, ctx, autos, writable: writable)
        var result: Repr? = nil
        Ambient.with(.native(n)) {
            var args = args0
            for f in d.input.fields {
                guard var v = args[f.name] else { continue }
                if case .option = f.ty, v.isNull() { continue }
                for c in f.checks where !n.stopped {
                    switch c {
                    case .ir(let k):
                        let r = Checks.field(f.name, Field(f.ty, checks: [k]), v, exists: { t, key in n.store.getRow(t, [key]) != nil }, refine: { _, _ in true })
                        switch r {
                        case .ok(let v2): v = v2
                        case .failed(_, let msg): n.refuse(.refused(msg))
                        }
                    case .refine(let p, let why):
                        if n.value(p(.v(v))) != .bool(true) {
                            n.refuse(.refused(why ?? Checks.defaultMessage(f.name, .refine(.lit(.null), nil))))
                        }
                    }
                }
                args[f.name] = v
                if n.stopped { return }
            }
            let whole = d.input.arguments(args)
            for (p, why) in d.input.refines where !n.stopped {
                if n.value(p(whole)) != .bool(true) { n.refuse(.refused(why ?? Checks.invalid)) }
            }
            var provided: [Repr] = []
            for m in d.chain where !n.stopped {
                let r = m.run(Ctx(), core.makeDb(), m.input.arguments(args))
                n.flush()
                if m.kind == .provide, let r = r { provided.append(.v(n.value(r))) }
            }
            if n.stopped { return }
            result = d.run(Ctx(), core.makeDb(), whole, provided)
            n.flush()
        }
        return (n, result)
    }
}
