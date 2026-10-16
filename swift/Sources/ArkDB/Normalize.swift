// §7.1 Alpha-normalisation (`rust/ark/src/ir/normalize.rs`): symbols are
// renumbered 0, 1, 2… in the order their binders are met walking the
// function — its checks, its refinements, a query's plan, the body — top
// to bottom, left to right, binders before the scopes they open: the
// evaluation order. Free symbols are left as they are. A nested block's
// bindings do not escape it, but their numbers are still consumed.
//
// A plan is walked in its evaluation order: the filter (in the scope
// around the plan), then `row`, `members`, each lookup's key and then its
// symbol, each related plan's `on`, its child plan (in the scope around the
// parent, since scope is flat per node) and then its symbol, then
// `having`, `project` and the expression order keys.

private typealias Ren = [Sym: Sym]

public enum Normalize {
    /// `normalize`: a function with its symbols renumbered in binding order.
    public static func function(_ f: Function) -> Function {
        var next: Sym = 0
        let input = f.input.map { i -> Named<Field> in
            let checks = i.value.checks.map { c -> Check in
                if case .refine(let e, let why) = c {
                    let (e2, n2) = expr([:], next, e)
                    next = n2
                    return .refine(e2, why)
                }
                return c
            }
            return Named(i.name, Field(ty: i.value.ty, checks: checks))
        }
        let refine = f.refine.map { r -> Refinement in
            let (e2, n2) = expr([:], next, r.e)
            next = n2
            return Refinement(e: e2, why: r.why)
        }
        var plan: Plan? = nil
        if let p = f.plan {
            let (p2, n2) = self.plan([:], next, p)
            next = n2
            plan = p2
        }
        let (body, _) = block([:], next, f.body)
        var g = f
        g.input = input
        g.refine = refine
        g.body = body
        g.plan = plan
        return g
    }

    /// `normalize_module`: every function and every live state normalised.
    public static func module(_ m: Module) -> Module {
        var n = m
        n.functions = m.functions.map(function)
        n.states = m.states.map(state)
        return n
    }

    /// `normalize_transition`: a transition numbered as a function would be —
    /// its checks, its guards (as a function's refinements are), then its body.
    public static func transition(_ t: Transition) -> Transition {
        let f = function(transitionFunction(t))
        return Transition(name: t.name, input: f.input, guards: f.refine.map { Guard(e: $0.e, why: $0.why ?? "") }, body: f.body)
    }

    /// `normalize_state`: its init, its scope and each transition normalised, each on its own.
    public static func state(_ s: LiveStateDecl) -> LiveStateDecl {
        var n = s
        n.initial = expr([:], 0, s.initial).0
        n.scope = expr([:], 0, s.scope).0
        n.transitions = s.transitions.map(transition)
        n.onJoin = s.onJoin.map(transition)
        n.onLeave = s.onLeave.map(transition)
        n.onRestore = s.onRestore.map(transition)
        return n
    }

    /// `transition_function`: a transition as the function the verifier and
    /// the numbering read it as — its input, its guards as refinements, its body.
    public static func transitionFunction(_ t: Transition) -> Function {
        Function(name: t.name, kind: .provide, router: nil, uses: [], autos: [], input: t.input, refine: t.guards.map { Refinement(e: $0.e, why: $0.why) }, ret: nil, body: t.body, plan: nil, holds: [], isPrivate: false)
    }

    private static func block(_ ren: Ren, _ next: Sym, _ blk: Block) -> (Block, Ren) {
        var r = ren
        var n = next
        var out = [Stmt]()
        out.reserveCapacity(blk.count)
        for s in blk {
            let (s2, r2, n2) = stmt(r, n, s)
            out.append(s2)
            r = r2
            n = n2
        }
        return (out, r)
    }

    private static func bind(_ ren: Ren, _ x: Sym, _ to: Sym) -> Ren {
        var r = ren
        r[x] = to
        return r
    }

    // The statement, the renaming extended by any binder it introduced, and the next free number.
    private static func stmt(_ ren: Ren, _ next: Sym, _ s: Stmt) -> (Stmt, Ren, Sym) {
        switch s {
        case .letS(let x, let e):
            let (e2, n2) = expr(ren, next, e)
            return (.letS(n2, e2), bind(ren, x, n2), n2 + 1)
        case .ifS(let c, let a, let b):
            let (c2, n1) = expr(ren, next, c)
            let (a2, n2) = inner(ren, n1, a)
            let (b2, n3) = inner(ren, n2, b)
            return (.ifS(c2, a2, b2), ren, n3)
        case .forS(let x, let o, let body):
            let (xs2, n1) = expr(ren, next, o)
            let (b2, n2) = inner(bind(ren, x, n1), n1 + 1, body)
            return (.forS(n1, xs2, b2), ren, n2)
        case .insert(let t, let row, let on):
            let (e2, n1) = expr(ren, next, row)
            return (.insert(t, e2, on), ren, n1)
        case .upsert(let t, let row, let on):
            let (e2, n1) = expr(ren, next, row)
            return (.upsert(t, e2, on), ren, n1)
        // The key first, then the existing row's binder, then the new row.
        case .update(let t, let key, let x, let row):
            let (ks2, n1) = many(ren, next, key)
            let (e2, n2) = expr(bind(ren, x, n1), n1 + 1, row)
            return (.update(t, ks2, n1, e2), ren, n2)
        case .delete(let t, let key):
            let (ks2, n1) = many(ren, next, key)
            return (.delete(t, ks2), ren, n1)
        case .refuse(let e):
            let (e2, n1) = expr(ren, next, e)
            return (.refuse(e2), ren, n1)
        case .ret(let e):
            guard let e = e else { return (s, ren, next) }
            let (e2, n1) = expr(ren, next, e)
            return (.ret(e2), ren, n1)
        // D3 A nested block, as a branch is.
        case .privateS(let body):
            let (b2, n1) = inner(ren, next, body)
            return (.privateS(b2), ren, n1)
        }
    }

    // A nested block: its bindings do not escape, their numbers are consumed.
    private static func inner(_ ren: Ren, _ n: Sym, _ blk: Block) -> (Block, Sym) {
        let (blk2, _) = block(ren, n, blk)
        var best = n
        for s in blk2 { for b in stmtBinders(s) { best = Swift.max(best, b + 1) } }
        return (blk2, best)
    }

    private static func stmtBinders(_ s: Stmt) -> [Sym] {
        switch s {
        case .letS(let x, let e): return [x] + exprBinders(e)
        case .ifS(let c, let a, let b): return exprBinders(c) + (a + b).flatMap(stmtBinders)
        case .forS(let x, let o, let body): return [x] + exprBinders(o) + body.flatMap(stmtBinders)
        case .insert(_, let e, _), .upsert(_, let e, _), .refuse(let e): return exprBinders(e)
        case .update(_, let k, let x, let r): return [x] + k.flatMap(exprBinders) + exprBinders(r)
        case .delete(_, let k): return k.flatMap(exprBinders)
        case .ret(let e): return e.map(exprBinders) ?? []
        case .privateS(let body): return body.flatMap(stmtBinders)
        }
    }

    private static func exprBinders(_ e: Expr) -> [Sym] {
        switch e {
        case .match(let x, let s, let a, let n): return [s] + [x, a, n].flatMap(exprBinders)
        case .map(let o, let s, let b), .filter(let o, let s, let b), .any(let o, let s, let b), .all(let o, let s, let b), .sortBy(let o, let s, let b):
            return [s] + exprBinders(o) + exprBinders(b)
        case .fold(let o, let z, let a, let s, let b): return [a, s] + [o, z, b].flatMap(exprBinders)
        case .select(let p): return planBinders(p)
        case .caseOf(let x, let arms): return exprBinders(x) + arms.flatMap { [$0.sym] + exprBinders($0.e) }
        default: return children(e).flatMap(exprBinders)
        }
    }

    private static func planBinders(_ p: Plan) -> [Sym] {
        var v = [Sym]()
        if let f = p.filter { v += predBinders(f) }
        if let r = p.row { v.append(r) }
        if let m = p.members { v.append(m) }
        for l in p.lookups {
            v += l.key.flatMap(exprBinders)
            v.append(l.sym)
        }
        for r in p.related {
            v += r.on.flatMap { exprBinders($0.e) }
            v += planBinders(r.plan)
            v.append(r.sym)
        }
        if let h = p.having { v += exprBinders(h) }
        if let pr = p.project { v += exprBinders(pr) }
        for o in p.order { if case .expr(let e) = o.key { v += exprBinders(e) } }
        return v
    }

    private static func predBinders(_ p: Pred) -> [Sym] {
        switch p {
        case .cmp(_, _, let e), .has(_, let e), .when(let e): return exprBinders(e)
        case .inP(_, let xs): return xs.flatMap(exprBinders)
        case .all(let ps), .any(let ps): return ps.flatMap(predBinders)
        case .not(let q), .exists(_, _, let q): return predBinders(q)
        }
    }

    private static func many(_ ren: Ren, _ next: Sym, _ es: [Expr]) -> ([Expr], Sym) {
        var n = next
        let out = es.map { e -> Expr in
            let (e2, n2) = expr(ren, n, e)
            n = n2
            return e2
        }
        return (out, n)
    }

    private static func expr(_ ren: Ren, _ next: Sym, _ e: Expr) -> (Expr, Sym) {
        func binder1(_ xs: Expr, _ x: Sym, _ b: Expr, _ mk: (Expr, Sym, Expr) -> Expr) -> (Expr, Sym) {
            let (xs2, n1) = expr(ren, next, xs)
            let (b2, n2) = expr(bind(ren, x, n1), n1 + 1, b)
            return (mk(xs2, n1, b2), n2)
        }
        switch e {
        case .variable(let x): return (.variable(ren[x] ?? x), next)
        case .field(let x, let name):
            let (e2, n) = expr(ren, next, x)
            return (.field(e2, name), n)
        case .structE(let fs):
            var n = next
            var out = [Expr]()
            for x in fs.exprs {
                let (v2, n2) = expr(ren, n, x)
                out.append(v2)
                n = n2
            }
            return (.structE(ExprFields(sorted: fs.names, out)), n)
        case .listE(let xs):
            let (es, n) = many(ren, next, xs)
            return (.listE(es), n)
        case .some(let x):
            let (e2, n) = expr(ren, next, x)
            return (.some(e2), n)
        case .match(let x, let s, let a, let b):
            let (e2, n1) = expr(ren, next, x)
            let (a2, n2) = expr(bind(ren, s, n1), n1 + 1, a)
            let (b2, n3) = expr(ren, n2, b)
            return (.match(e2, n1, a2, b2), n3)
        case .ife(let c, let a, let b):
            let (c2, n1) = expr(ren, next, c)
            let (a2, n2) = expr(ren, n1, a)
            let (b2, n3) = expr(ren, n2, b)
            return (.ife(c2, a2, b2), n3)
        case .op(let o, let xs):
            let (es, n) = many(ren, next, xs)
            return (.op(o, es), n)
        case .cmp(let o, let l, let r):
            let (a2, n1) = expr(ren, next, l)
            let (b2, n2) = expr(ren, n1, r)
            return (.cmp(o, a2, b2), n2)
        case .call(let f, let xs):
            let (es, n) = many(ren, next, xs)
            return (.call(f, es), n)
        case .std(let f, let xs):
            let (es, n) = many(ren, next, xs)
            return (.std(f, es), n)
        case .map(let o, let x, let b): return binder1(o, x, b) { .map($0, $1, $2) }
        case .filter(let o, let x, let b): return binder1(o, x, b) { .filter($0, $1, $2) }
        case .any(let o, let x, let b): return binder1(o, x, b) { .any($0, $1, $2) }
        case .all(let o, let x, let b): return binder1(o, x, b) { .all($0, $1, $2) }
        case .sortBy(let o, let x, let k): return binder1(o, x, k) { .sortBy($0, $1, $2) }
        case .fold(let o, let z, let a, let x, let b):
            let (xs2, n0) = expr(ren, next, o)
            let (z2, n1) = expr(ren, n0, z)
            let (b2, n2) = expr(bind(bind(ren, a, n1), x, n1 + 1), n1 + 2, b)
            return (.fold(xs2, z2, n1, n1 + 1, b2), n2)
        case .select(let p):
            let (p2, n) = plan(ren, next, p)
            return (.select(p2), n)
        case .get(let t, let k):
            let (ks, n) = many(ren, next, k)
            return (.get(t, ks), n)
        case .exists(let t, let k):
            let (ks, n) = many(ren, next, k)
            return (.exists(t, ks), n)
        // A sum built: its fields in name order, as a struct's.
        case .variant(let t, let name, let fs):
            var n = next
            var out = [Expr]()
            for x in fs.exprs {
                let (v2, n2) = expr(ren, n, x)
                out.append(v2)
                n = n2
            }
            return (.variant(t, name, ExprFields(sorted: fs.names, out)), n)
        // The scrutinee, then each arm in order: its binder, then its body.
        case .caseOf(let x, let arms):
            let (e2, k0) = expr(ren, next, x)
            var k = k0
            var out = [Arm]()
            for a in arms {
                let (b2, k2) = expr(bind(ren, a.sym, k), k + 1, a.e)
                out.append(Arm(name: a.name, sym: k, e: b2))
                k = k2
            }
            return (.caseOf(e2, out), k)
        case .slot(let a, let b):
            let (a2, n1) = expr(ren, next, a)
            let (b2, n2) = expr(ren, n1, b)
            return (.slot(a2, b2), n2)
        case .setSlot(let a, let b, let c):
            let (a2, n1) = expr(ren, next, a)
            let (b2, n2) = expr(ren, n1, b)
            let (c2, n3) = expr(ren, n2, c)
            return (.setSlot(a2, b2, c2), n3)
        case .slots(let a):
            let (a2, n1) = expr(ren, next, a)
            return (.slots(a2), n1)
        case .liveState, .ctxNow, .ctxDevice,
             .lit, .arg, .auto, .ctxUser, .ctxSession, .hasRole, .provided, .none:
            return (e, next)
        }
    }

    // A plan in `ren`, the scope around it.
    private static func plan(_ ren: Ren, _ next: Sym, _ p: Plan) -> (Plan, Sym) {
        var n = next
        var filter: Pred? = nil
        if let f = p.filter {
            let (f2, n2) = pred(ren, n, f)
            n = n2
            filter = f2
        }
        // The node's own scope: the scope around the plan and its binders.
        var node: Ren = ren
        func open(_ x: Sym) -> Sym {
            node = bind(node, x, n)
            n += 1
            return n - 1
        }
        let row = p.row.map(open)
        let members = p.members.map(open)
        let lookups = p.lookups.map { l -> Lookup in
            let (key, n1) = many(node, n, l.key)
            n = n1
            var l2 = l
            l2.key = key
            l2.sym = open(l.sym)
            return l2
        }
        let related = p.related.map { r -> Related in
            let on = r.on.map { o -> On in
                let (e2, n1) = expr(node, n, o.e)
                n = n1
                return On(column: o.column, e: e2)
            }
            let (child, n1) = plan(ren, n, r.plan)
            n = n1
            return Related(name: r.name, sym: open(r.sym), on: on, plan: child)
        }
        func one(_ e: Expr?) -> Expr? {
            guard let e = e else { return nil }
            let (e2, n1) = expr(node, n, e)
            n = n1
            return e2
        }
        let having = one(p.having)
        let project = one(p.project)
        let order = p.order.map { o -> OrderKey in
            switch o.key {
            case .column: return o
            case .expr(let e): return OrderKey(.expr(one(e)!), o.dir)
            }
        }
        return (Plan(source: p.source, filter: filter, row: row, members: members, lookups: lookups, related: related, having: having, project: project, order: order, limit: p.limit), n)
    }

    private static func pred(_ ren: Ren, _ next: Sym, _ p: Pred) -> (Pred, Sym) {
        switch p {
        case .cmp(let c, let o, let e):
            let (e2, n) = expr(ren, next, e)
            return (.cmp(c, o, e2), n)
        case .inP(let c, let xs):
            let (es, n) = many(ren, next, xs)
            return (.inP(c, es), n)
        case .all(let ps):
            let (qs, n) = preds(ren, next, ps)
            return (.all(qs), n)
        case .any(let ps):
            let (qs, n) = preds(ren, next, ps)
            return (.any(qs), n)
        case .not(let q):
            let (q2, n) = pred(ren, next, q)
            return (.not(q2), n)
        case .has(let c, let e):
            let (e2, n) = expr(ren, next, e)
            return (.has(c, e2), n)
        case .when(let e):
            let (e2, n) = expr(ren, next, e)
            return (.when(e2), n)
        case .exists(let t, let c, let q):
            let (q2, n) = pred(ren, next, q)
            return (.exists(t, c, q2), n)
        }
    }

    private static func preds(_ ren: Ren, _ next: Sym, _ ps: [Pred]) -> ([Pred], Sym) {
        var n = next
        let out = ps.map { q -> Pred in
            let (q2, n2) = pred(ren, n, q)
            n = n2
            return q2
        }
        return (out, n)
    }
}
