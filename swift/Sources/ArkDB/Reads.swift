// §1.7 What a function reads outside a plan, by static inspection
// (`rust/ark/src/ir/reads.rs`). A maintained query runs its middleware at
// hydrate and again when a table the middleware read moves: every table a
// `Select`, `Get` or `Exists` in the body names (a `Select`'s related and
// looked-up tables too), every table a write names, and every table an input
// field's `exists` check reads. A query's own plan is left out — the view
// maintains its reads. A scope reads the tables it holds and, for the
// `exists` form, the table it reaches through.

public enum Reads {
    /// `ir::reads`: the tables `f` reads outside its plan, in code point order.
    public static func reads(_ f: Function) -> [String] {
        var out = Set<String>()
        for field in f.input {
            for c in field.value.checks {
                switch c {
                case .exists:
                    if let t = Eval.idTable(field.value.ty) { out.insert(t) }
                case .refine(let e, _): expr(e, &out)
                default: break
                }
            }
        }
        for r in f.refine { expr(r.e, &out) }
        block(f.body, &out)
        for h in f.holds {
            out.insert(h.table)
            if let p = h.filter { pred(p, &out) }
        }
        return out.sorted { compareText($0, $1) < 0 }
    }

    private static func block(_ b: Block, _ out: inout Set<String>) {
        for s in b {
            switch s {
            case .letS(_, let e), .refuse(let e), .ret(let e?): expr(e, &out)
            case .ret(nil): break
            case .ifS(let c, let t, let f):
                expr(c, &out)
                block(t, &out)
                block(f, &out)
            case .forS(_, let e, let body):
                expr(e, &out)
                block(body, &out)
            case .insert(let t, let e, _), .upsert(let t, let e, _):
                out.insert(t)
                expr(e, &out)
            case .update(let t, let k, _, let e):
                out.insert(t)
                for x in k { expr(x, &out) }
                expr(e, &out)
            case .delete(let t, let k):
                out.insert(t)
                for x in k { expr(x, &out) }
            // `docs/plan-guards.md` D3 What a private block reads, which only
            // the server's module carries.
            case .privateS(let body): block(body, &out)
            }
        }
    }

    private static func plan(_ p: Plan, _ out: inout Set<String>) {
        out.insert(p.table)
        if let f = p.filter { pred(f, &out) }
        for l in p.lookups {
            out.insert(l.table)
            for e in l.key { expr(e, &out) }
        }
        for r in p.related {
            for o in r.on { expr(o.e, &out) }
            plan(r.plan, &out)
        }
        if let h = p.having { expr(h, &out) }
        if let pr = p.project { expr(pr, &out) }
        for o in p.order {
            if case .expr(let e) = o.key { expr(e, &out) }
        }
    }

    private static func pred(_ p: Pred, _ out: inout Set<String>) {
        switch p {
        case .cmp(_, _, let e), .has(_, let e), .when(let e): expr(e, &out)
        case .inP(_, let es): for e in es { expr(e, &out) }
        case .all(let ps), .any(let ps): for q in ps { pred(q, &out) }
        case .not(let q): pred(q, &out)
        case .exists(let t, _, let q):
            out.insert(t)
            pred(q, &out)
        }
    }

    private static func expr(_ e: Expr, _ out: inout Set<String>) {
        switch e {
        case .select(let p): plan(p, &out)
        case .get(let t, _), .exists(let t, _): out.insert(t)
        default: break
        }
        for c in children(e) { expr(c, &out) }
    }
}
