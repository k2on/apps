import Foundation

/// A verifier complaint: the function (or router) it is about, and what.
/// The complaint names the `Ark.Verify` constructor.
public struct VerifyError: Error, Equatable, CustomStringConvertible {
    public var subject: String
    public var complaint: String
    public init(_ subject: String, _ complaint: String) { self.subject = subject; self.complaint = complaint }
    public var description: String { return "\(subject): \(complaint)" }
}

/// Every complaint a module drew.
public struct VerifyErrors: Error, Equatable, CustomStringConvertible {
    public var errors: [VerifyError]
    public init(errors: [VerifyError]) { self.errors = errors }
    public var description: String { return errors.map { $0.description }.joined(separator: "; ") }
}

/// §9 The verifier's structural rules for spec version 3 (AUTHORING.md
/// §1.5), the schema's well-formedness, and `completeOrders`. A runtime executes verified modules; this
/// is what `Module.emit()` runs before it hands out bytes, and what a test
/// holds a hand-built module to. The expression type checker stays
/// `arkc`'s.
public enum Verify {
    /// Check the rules, and on success the module as it is hashed and
    /// sent: orders completed, every function normalised.
    public static func verify(_ m0: Module) -> Result<Module, VerifyErrors> {
        let m = completeOrders(m0)
        var errs: [VerifyError] = []
        func bad(_ s: String, _ c: String) { errs.append(VerifyError(s, c)) }
        if m.spec != specVersion { bad("module", "BadSpecVersion \(m.spec)") }
        for p in m.schema.problems() { bad("schema", "BadSchema " + p) }
        var names = Set<String>()
        for f in m.functions {
            if names.contains(f.name) { bad(f.name, "DuplicateFunction") }
            names.insert(f.name)
        }
        // Routers: names unique, uses are middleware.
        var rnames = Set<String>()
        for r in m.routers {
            if rnames.contains(r.name) { bad(r.name, "DuplicateRouter") }
            rnames.insert(r.name)
            for u in r.uses where !(m.lookupFunction(u)?.kind.isMiddleware ?? false) { bad(r.name, "NotMiddleware \(u)") }
        }
        for (i, f) in m.functions.enumerated() {
            let earlier = Set(m.functions[..<i].map { $0.name })
            switch f.kind {
            case .mutator, .query:
                guard let rn = f.router, let r = m.lookupRouter(rn) else { bad(f.name, "NoRouter"); continue }
                if f.kind == .query && !f.autos.isEmpty { bad(f.name, "AutosOnNonMutator") }
                if f.kind == .mutator && f.ret != nil { bad(f.name, "ReturnTypeOnMutator") }
                if f.kind == .query && f.ret == nil { bad(f.name, "NoReturnType") }
                if !isSubsequence(f.uses, of: r.uses) { bad(f.name, "UsesNotOnRouter") }
                for u in f.uses {
                    guard let mw = m.lookupFunction(u), mw.kind.isMiddleware else { continue }
                    if !earlier.contains(u) { bad(f.name, "MiddlewareNotYetDeclared \(u)") }
                    for want in mw.input {
                        guard let have = f.input.first(where: { $0.name == want.name }) else { bad(f.name, "MiddlewareInput \(u) \(want.name)"); continue }
                        if have.ty != want.ty { bad(f.name, "MiddlewareInput \(u) \(want.name)") }
                    }
                }
                let provides = Set(f.uses.filter { m.lookupFunction($0)?.kind == .provide })
                for p in providedIn(f) where !provides.contains(p) { bad(f.name, "NotProvided \(p)") }
            case .guard_, .provide:
                if f.router != nil { bad(f.name, "RouterOnNonProcedure") }
                if !f.uses.isEmpty { bad(f.name, "UsesOnNonProcedure") }
                if f.kind == .provide && f.ret == nil { bad(f.name, "NoReturnType") }
                if !providedIn(f).isEmpty { bad(f.name, "NotProvided") }
            case .helper:
                if f.router != nil { bad(f.name, "RouterOnNonProcedure") }
                if !f.uses.isEmpty { bad(f.name, "UsesOnNonProcedure") }
            }
            // CExists only on an id, of a table that exists.
            for nf in f.input {
                for c in nf.field.checks {
                    guard case .exists = c else { continue }
                    guard let tb = Checks.idTable(nf.ty) else { bad(f.name, "ExistsOnNonId \(nf.name)"); continue }
                    if m.schema.lookupTable(tb) == nil { bad(f.name, "UnknownTable \(tb)") }
                }
            }
            // `.on` names a declared unique index.
            for (t, on) in writesOn(f.body) where !on.isEmpty {
                guard let tbl = m.schema.lookupTable(t) else { bad(f.name, "UnknownTable \(t)"); continue }
                if !tbl.indexes.contains(where: { $0.unique && $0.columns == on }) { bad(f.name, "OnNotUnique \(t) \(on)") }
            }
        }
        if !errs.isEmpty { return .failure(VerifyErrors(errors: errs)) }
        return .success(Encode.normalizeModule(m))
    }

    static func isSubsequence(_ xs: [String], of ys: [String]) -> Bool {
        var i = 0
        for y in ys where i < xs.count && xs[i] == y { i += 1 }
        return i == xs.count
    }

    /// Every `EProvided` name a function's body mentions.
    static func providedIn(_ f: Function) -> [String] {
        var out: [String] = []
        func e(_ x: Expr) {
            switch x {
            case .provided(let n): out.append(n)
            case .field(let a, _), .some(let a): e(a)
            case .structOf(let fs): fs.values.forEach(e)
            case .list(let es), .op(_, let es), .call(_, let es), .std(_, let es), .get(_, let es), .exists(_, let es): es.forEach(e)
            case .match(let a, _, let b, let c), .ife(let a, let b, let c): e(a); e(b); e(c)
            case .cmp(_, let a, let b): e(a); e(b)
            case .map(let a, _, let b), .filter(let a, _, let b), .any(let a, _, let b), .all(let a, _, let b), .sortBy(let a, _, let b): e(a); e(b)
            case .fold(let a, let b, _, _, let c): e(a); e(b); e(c)
            case .select(let p): plan(p)
            default: break
            }
        }
        func plan(_ p: Plan) {
            if let f = p.filter { pred(f) }
            p.related.forEach { plan($0.plan) }
        }
        func pred(_ p: Pred) {
            switch p {
            case .pcmp(_, _, let x): e(x)
            case .pin(_, let xs): xs.forEach(e)
            case .pall(let ps), .pany(let ps): ps.forEach(pred)
            case .pnot(let q): pred(q)
            }
        }
        func s(_ x: Stmt) {
            switch x {
            case .sLet(_, let a), .sRefuse(let a), .sInsert(_, let a, _), .sUpsert(_, let a, _): e(a)
            case .sIf(let c, let a, let b): e(c); a.forEach(s); b.forEach(s)
            case .sFor(_, let xs, let b): e(xs); b.forEach(s)
            case .sUpdate(_, let ks, _, let a): ks.forEach(e); e(a)
            case .sDelete(_, let ks): ks.forEach(e)
            case .sReturn(let a): if let a = a { e(a) }
            }
        }
        f.body.forEach(s)
        return out
    }

    /// Every insert's and upsert's table and `on` columns.
    static func writesOn(_ b: Block) -> [(TableName, [FieldName])] {
        var out: [(TableName, [FieldName])] = []
        for st in b {
            switch st {
            case .sInsert(let t, _, let on), .sUpsert(let t, _, let on): out.append((t, on))
            case .sIf(_, let a, let c): out += writesOn(a) + writesOn(c)
            case .sFor(_, _, let body): out += writesOn(body)
            default: break
            }
        }
        return out
    }

    /// §9.4 Make every order total: append the table's key columns,
    /// ascending, after whatever the author ordered by, omitting any
    /// already present; in every plan of every function, related plans too.
    public static func completeOrders(_ m: Module) -> Module {
        var mm = m
        let sch = m.schema
        func plan(_ p: Plan) -> Plan {
            var q = p
            let keyCols = sch.lookupTable(p.table)?.key ?? []
            let present = p.order.map { $0.column }
            q.order = p.order + keyCols.filter { !present.contains($0) }.map { OrderBy($0, .asc) }
            q.related = p.related.map { r in var r2 = r; r2.plan = plan(r.plan); return r2 }
            return q
        }
        func ex(_ e: Expr) -> Expr {
            switch e {
            case .select(let p): return .select(plan(p))
            case .field(let a, let f): return .field(ex(a), f)
            case .structOf(let fs): return .structOf(fs.mapValues(ex))
            case .list(let es): return .list(es.map(ex))
            case .some(let a): return .some(ex(a))
            case .match(let a, let x, let b, let c): return .match(ex(a), x, ex(b), ex(c))
            case .ife(let a, let b, let c): return .ife(ex(a), ex(b), ex(c))
            case .op(let o, let es): return .op(o, es.map(ex))
            case .cmp(let o, let a, let b): return .cmp(o, ex(a), ex(b))
            case .call(let n, let es): return .call(n, es.map(ex))
            case .std(let f, let es): return .std(f, es.map(ex))
            case .map(let a, let x, let b): return .map(ex(a), x, ex(b))
            case .filter(let a, let x, let b): return .filter(ex(a), x, ex(b))
            case .any(let a, let x, let b): return .any(ex(a), x, ex(b))
            case .all(let a, let x, let b): return .all(ex(a), x, ex(b))
            case .sortBy(let a, let x, let b): return .sortBy(ex(a), x, ex(b))
            case .fold(let a, let z, let acc, let x, let b): return .fold(ex(a), ex(z), acc, x, ex(b))
            case .get(let t, let ks): return .get(t, ks.map(ex))
            case .exists(let t, let ks): return .exists(t, ks.map(ex))
            default: return e
            }
        }
        func st(_ s: Stmt) -> Stmt {
            switch s {
            case .sLet(let x, let e): return .sLet(x, ex(e))
            case .sIf(let c, let a, let b): return .sIf(ex(c), a.map(st), b.map(st))
            case .sFor(let x, let xs, let b): return .sFor(x, ex(xs), b.map(st))
            case .sInsert(let t, let e, let on): return .sInsert(t, ex(e), on)
            case .sUpsert(let t, let e, let on): return .sUpsert(t, ex(e), on)
            case .sUpdate(let t, let ks, let x, let e): return .sUpdate(t, ks.map(ex), x, ex(e))
            case .sDelete(let t, let ks): return .sDelete(t, ks.map(ex))
            case .sRefuse(let e): return .sRefuse(ex(e))
            case .sReturn(let e): return .sReturn(e.map(ex))
            }
        }
        mm.functions = m.functions.map { f in
            var g = f
            g.body = f.body.map(st)
            return g
        }
        return mm
    }
}
