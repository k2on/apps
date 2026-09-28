import Foundation

/// §7 The module as a value — every node a struct with a `"t"` tag — and
/// alpha-normalisation, exactly as `Ark.Encode`.
public enum Encode {
    static func node(_ t: String, _ fs: [(String, Value)]) -> Value {
        var m: [String: Value] = ["t": .text(t)]
        for (k, v) in fs { m[k] = v }
        return .record(m)
    }

    static func txt(_ s: String) -> Value { return .text(s) }
    static func int(_ n: Int) -> Value { return .int(Int64(n)) }

    /// The whole module. Functions are carried without dependency hashes,
    /// because the module carries the helpers themselves.
    public static func toValue(_ m: Module) -> Value {
        return node("module", [
            ("spec", int(m.spec)),
            ("schema", schemaValue(m.schema)),
            ("functions", .list(m.functions.map { functionValue([:], $0) })),
            ("live", .list(m.live.map { node("frame", [("name", txt($0.name)), ("ty", tyValue($0.ty))]) })),
            ("routers", .list(m.routers.map(routerValue))),
        ])
    }

    public static func routerValue(_ r: Router) -> Value {
        return node("router", [("name", txt(r.name)), ("uses", .list(r.uses.map(txt)))])
    }

    static func optText(_ s: String?) -> Value { return s.map(txt) ?? .null }

    /// §1.3 One input field: its name, type and checks.
    public static func fieldValue(_ f: NamedField) -> Value {
        return node("field", [("name", txt(f.name)), ("ty", tyValue(f.ty)), ("checks", .list(f.field.checks.map(checkValue)))])
    }

    public static func checkValue(_ c: Check) -> Value {
        switch c {
        case .trim: return node("trim", [])
        case .minLen(let n, let why): return node("min_len", [("n", int(n)), ("why", optText(why))])
        case .maxLen(let n, let why): return node("max_len", [("n", int(n)), ("why", optText(why))])
        case .range(let lo, let hi, let why): return node("range", [("lo", lo.map(int) ?? .null), ("hi", hi.map(int) ?? .null), ("why", optText(why))])
        case .nonEmpty(let why): return node("non_empty", [("why", optText(why))])
        case .exists(let why): return node("exists", [("why", optText(why))])
        case .refine(let e, let why): return node("refine", [("e", expr(e)), ("why", optText(why))])
        }
    }

    public static func schemaValue(_ sch: Schema) -> Value {
        func col(_ c: Column) -> Value {
            return node("column", [("name", txt(c.name)), ("ty", tyValue(c.ty)), ("nullable", .bool(c.nullable))])
        }
        func ix(_ i: Index) -> Value {
            return node("index", [("columns", .list(i.columns.map(txt))), ("unique", .bool(i.unique))])
        }
        func ref(_ r: Ref) -> Value {
            return node("ref", [("column", txt(r.column)), ("table", txt(r.table))])
        }
        func table(_ t: Table) -> Value {
            return node("table", [
                ("name", txt(t.name)),
                ("columns", .list(t.columns.map(col))),
                ("key", .list(t.key.map(txt))),
                ("indexes", .list(t.indexes.map(ix))),
                ("refs", .list(t.refs.map(ref))),
            ])
        }
        return .list(sch.tables.map(table))
    }

    public static func tyValue(_ t: Ty) -> Value {
        switch t {
        case .bool: return node("bool", [])
        case .int: return node("int", [])
        case .text: return node("text", [])
        case .bytes: return node("bytes", [])
        case .id(let tb): return node("id", [("table", txt(tb))])
        case .enumOf(let vs): return node("enum", [("variants", .list(vs.map(txt)))])
        case .option(let x): return node("option", [("of", tyValue(x))])
        case .list(let x): return node("list", [("of", tyValue(x))])
        case .structOf(let fs): return node("struct", [("fields", .record(fs.mapValues(tyValue)))])
        }
    }

    /// One function, normalised, with the hashes of the helpers it calls.
    /// Names are not carried.
    public static func functionValue(_ deps: [String: Value], _ fn0: Function) -> Value {
        let fn = normalize(fn0)
        let kind = fn.kind.wireName
        func auto(_ a: NamedAuto) -> Value {
            switch a.auto {
            case .newId(let t): return node("new_id", [("name", txt(a.name)), ("table", txt(t))])
            case .now: return node("now", [("name", txt(a.name))])
            }
        }
        return node("fn", [
            ("name", txt(fn.name)),
            ("deps", .record(deps)),
            ("kind", txt(kind)),
            ("router", fn.router.map(txt) ?? .null),
            ("uses", .list(fn.uses.map(txt))),
            ("autos", .list(fn.autos.map(auto))),
            ("input", .list(fn.input.map(fieldValue))),
            ("refine", .list(fn.refine.map { node("refine", [("e", expr($0.expr)), ("why", optText($0.why))]) })),
            ("ret", fn.ret.map(tyValue) ?? .null),
            ("body", .list(fn.body.map(stmt))),
        ])
    }

    static func stmt(_ s: Stmt) -> Value {
        switch s {
        case .sLet(let x, let e): return node("let", [("sym", int(x)), ("e", expr(e))])
        case .sIf(let c, let a, let b): return node("if", [("c", expr(c)), ("then", .list(a.map(stmt))), ("else", .list(b.map(stmt)))])
        case .sFor(let x, let xs, let b): return node("for", [("sym", int(x)), ("in", expr(xs)), ("body", .list(b.map(stmt)))])
        case .sInsert(let t, let e, let on): return node("insert", [("table", txt(t)), ("row", expr(e)), ("on", .list(on.map(txt)))])
        case .sUpsert(let t, let e, let on): return node("upsert", [("table", txt(t)), ("row", expr(e)), ("on", .list(on.map(txt)))])
        case .sUpdate(let t, let ks, let x, let e): return node("update", [("table", txt(t)), ("key", .list(ks.map(expr))), ("sym", int(x)), ("row", expr(e))])
        case .sDelete(let t, let ks): return node("delete", [("table", txt(t)), ("key", .list(ks.map(expr)))])
        case .sRefuse(let e): return node("refuse", [("e", expr(e))])
        case .sReturn(let me): return node("return", [("e", me.map(expr) ?? .null)])
        }
    }

    static func expr(_ e: Expr) -> Value {
        switch e {
        case .lit(let v): return node("lit", [("v", v)])
        case .arg(let a): return node("arg", [("name", txt(a))])
        case .auto(let a): return node("auto", [("name", txt(a))])
        case .variable(let x): return node("var", [("sym", int(x))])
        case .ctxUser: return node("ctx_user", [])
        case .ctxSession: return node("ctx_session", [])
        case .provided(let f): return node("provided", [("fn", txt(f))])
        case .field(let x, let f): return node("field", [("e", expr(x)), ("name", txt(f))])
        case .structOf(let fs): return node("struct", [("fields", .record(fs.mapValues(expr)))])
        case .list(let es): return node("list", [("items", .list(es.map(expr)))])
        case .some(let x): return node("some", [("e", expr(x))])
        case .none(let t): return node("none", [("ty", tyValue(t))])
        case .match(let x, let s, let a, let b): return node("match", [("e", expr(x)), ("sym", int(s)), ("some", expr(a)), ("none", expr(b))])
        case .ife(let c, let a, let b): return node("ife", [("c", expr(c)), ("then", expr(a)), ("else", expr(b))])
        case .op(let op, let es): return node("op", [("op", txt(op.rawValue)), ("args", .list(es.map(expr)))])
        case .cmp(let op, let a, let b): return node("cmp", [("op", txt(op.rawValue)), ("l", expr(a)), ("r", expr(b))])
        case .call(let n, let es): return node("call", [("fn", txt(n)), ("args", .list(es.map(expr)))])
        case .std(let f, let es): return node("std", [("fn", txt(f.rawValue)), ("args", .list(es.map(expr)))])
        case .map(let xs, let x, let b): return node("map", [("in", expr(xs)), ("sym", int(x)), ("body", expr(b))])
        case .filter(let xs, let x, let b): return node("filter", [("in", expr(xs)), ("sym", int(x)), ("body", expr(b))])
        case .any(let xs, let x, let b): return node("any", [("in", expr(xs)), ("sym", int(x)), ("body", expr(b))])
        case .all(let xs, let x, let b): return node("all", [("in", expr(xs)), ("sym", int(x)), ("body", expr(b))])
        case .sortBy(let xs, let x, let k): return node("sort_by", [("in", expr(xs)), ("sym", int(x)), ("key", expr(k))])
        case .fold(let xs, let z, let acc, let x, let b):
            return node("fold", [("in", expr(xs)), ("init", expr(z)), ("acc", int(acc)), ("sym", int(x)), ("body", expr(b))])
        case .select(let p): return node("select", [("plan", plan(p))])
        case .get(let t, let ks): return node("get", [("table", txt(t)), ("key", .list(ks.map(expr)))])
        case .exists(let t, let ks): return node("exists", [("table", txt(t)), ("key", .list(ks.map(expr)))])
        }
    }

    static func plan(_ p: Plan) -> Value {
        return node("plan", [
            ("table", txt(p.table)),
            ("filter", p.filter.map(predV) ?? .null),
            ("order", .list(p.order.map { node("by", [("column", txt($0.column)), ("dir", txt($0.dir == .asc ? "asc" : "desc"))]) })),
            ("limit", p.limit.map(int) ?? .null),
            ("related", .list(p.related.map { r in
                node("related", [
                    ("name", txt(r.name)),
                    ("parent", txt(r.relation.parent)),
                    ("child", txt(r.relation.child)),
                    ("column", txt(r.relation.column)),
                    ("plan", plan(r.plan)),
                ])
            })),
        ])
    }

    static func predV(_ p: Pred) -> Value {
        switch p {
        case .pcmp(let c, let op, let e): return node("pcmp", [("column", txt(c)), ("op", txt(op.rawValue)), ("e", expr(e))])
        case .pin(let c, let es): return node("pin", [("column", txt(c)), ("items", .list(es.map(expr)))])
        case .pall(let ps): return node("pall", [("items", .list(ps.map(predV)))])
        case .pany(let ps): return node("pany", [("items", .list(ps.map(predV)))])
        case .pnot(let q): return node("pnot", [("e", predV(q))])
        }
    }

    // MARK: §7.1 Alpha-normalisation

    /// Symbols renumbered 0, 1, 2… in the order their binders are met,
    /// walking the body top to bottom, left to right.
    ///
    /// One numbering covers the function: every check's expression in
    /// input order, then every refinement, then the body (AUTHORING.md,
    /// Appendix A). A check's expression mentions no local, so each starts
    /// from an empty renaming.
    public static func normalize(_ fn: Function) -> Function {
        var f = fn
        var next = 0
        f.input = fn.input.map { nf in
            var g = nf
            g.field.checks = nf.field.checks.map { c in
                guard case .refine(let e, let why) = c else { return c }
                let (e2, n2) = renumberExpr([:], next, e)
                next = n2
                return .refine(e2, why)
            }
            return g
        }
        f.refine = fn.refine.map { r in
            let (e2, n2) = renumberExpr([:], next, r.expr)
            next = n2
            return Refine(e2, r.why)
        }
        let (body, _) = renumberBlock([:], next, fn.body)
        f.body = body
        // Names follow their binders: the same walk over both forms pairs
        // every old symbol with its new number.
        var names: [Sym: String] = [:]
        if !fn.names.isEmpty {
            for (old, new) in zip(allBinders(fn), allBinders(f)) {
                if let n = fn.names[old] { names[new] = n }
            }
        }
        f.names = names
        return f
    }

    /// Every binder of a function, in one fixed walk: checks, refinements, body.
    static func allBinders(_ fn: Function) -> [Sym] {
        var out: [Sym] = []
        for nf in fn.input { for c in nf.field.checks { if case .refine(let e, _) = c { out += exprBinders(e) } } }
        for r in fn.refine { out += exprBinders(r.expr) }
        return out + fn.body.flatMap(binders)
    }

    public static func normalizeModule(_ m: Module) -> Module {
        var mm = m
        mm.functions = m.functions.map(normalize)
        return mm
    }

    typealias Ren = [Sym: Sym]

    static func renumberBlock(_ ren: Ren, _ next: Int, _ block: Block) -> (Block, Ren) {
        var r = ren
        var n = next
        var out: Block = []
        for s in block {
            let (s2, r2, n2) = renumberStmt(r, n, s)
            out.append(s2)
            r = r2
            n = n2
        }
        return (out, r)
    }

    /// A nested block's bindings do not escape it, but their numbers are
    /// still consumed.
    static func inner(_ r: Ren, _ n: Int, _ blk: Block) -> (Block, Int) {
        let (b2, _) = renumberBlock(r, n, blk)
        return (b2, countBinders(b2, n))
    }

    static func renumberStmt(_ ren: Ren, _ next: Int, _ s: Stmt) -> (Stmt, Ren, Int) {
        switch s {
        case .sLet(let x, let e):
            let (e2, n2) = renumberExpr(ren, next, e)
            var r2 = ren
            r2[x] = n2
            return (.sLet(n2, e2), r2, n2 + 1)
        case .sIf(let c, let a, let b):
            let (c2, n1) = renumberExpr(ren, next, c)
            let (a2, n2) = inner(ren, n1, a)
            let (b2, n3) = inner(ren, n2, b)
            return (.sIf(c2, a2, b2), ren, n3)
        case .sFor(let x, let xs, let b):
            let (xs2, n1) = renumberExpr(ren, next, xs)
            var r2 = ren
            r2[x] = n1
            let (b2, n2) = inner(r2, n1 + 1, b)
            return (.sFor(n1, xs2, b2), ren, n2)
        case .sInsert(let t, let e, let on):
            let (e2, n1) = renumberExpr(ren, next, e)
            return (.sInsert(t, e2, on), ren, n1)
        case .sUpsert(let t, let e, let on):
            let (e2, n1) = renumberExpr(ren, next, e)
            return (.sUpsert(t, e2, on), ren, n1)
        case .sUpdate(let t, let ks, let x, let e):
            // The key, then the binder for the existing row, then the new row over it.
            let (ks2, n1) = renumberMany(ren, next, ks)
            var r2 = ren
            r2[x] = n1
            let (e2, n2) = renumberExpr(r2, n1 + 1, e)
            return (.sUpdate(t, ks2, n1, e2), ren, n2)
        case .sDelete(let t, let ks):
            let (ks2, n1) = renumberMany(ren, next, ks)
            return (.sDelete(t, ks2), ren, n1)
        case .sRefuse(let e):
            let (e2, n1) = renumberExpr(ren, next, e)
            return (.sRefuse(e2), ren, n1)
        case .sReturn(let me):
            guard let e = me else { return (.sReturn(nil), ren, next) }
            let (e2, n1) = renumberExpr(ren, next, e)
            return (.sReturn(e2), ren, n1)
        }
    }

    /// One past the largest binder in a block, or the given floor.
    static func countBinders(_ blk: Block, _ n: Int) -> Int {
        var m = n
        for b in blk.flatMap(binders) { m = Swift.max(m, b + 1) }
        return m
    }

    static func binders(_ s: Stmt) -> [Sym] {
        switch s {
        case .sLet(let x, let e): return [x] + exprBinders(e)
        case .sIf(let c, let a, let b): return exprBinders(c) + a.flatMap(binders) + b.flatMap(binders)
        case .sFor(let x, let xs, let b): return [x] + exprBinders(xs) + b.flatMap(binders)
        case .sInsert(_, let e, _), .sUpsert(_, let e, _): return exprBinders(e)
        case .sUpdate(_, let ks, let x, let e): return [x] + ks.flatMap(exprBinders) + exprBinders(e)
        case .sDelete(_, let ks): return ks.flatMap(exprBinders)
        case .sRefuse(let e): return exprBinders(e)
        case .sReturn(let me): return me.map(exprBinders) ?? []
        }
    }

    static func exprBinders(_ e: Expr) -> [Sym] {
        switch e {
        case .match(let x, let s, let a, let b): return [s] + [x, a, b].flatMap(exprBinders)
        case .map(let xs, let x, let b), .filter(let xs, let x, let b), .any(let xs, let x, let b), .all(let xs, let x, let b), .sortBy(let xs, let x, let b):
            return [x] + [xs, b].flatMap(exprBinders)
        case .fold(let xs, let z, let acc, let x, let b): return [acc, x] + [xs, z, b].flatMap(exprBinders)
        case .field(let x, _): return exprBinders(x)
        case .structOf(let fs): return sortedFieldNamesOf(fs).flatMap { exprBinders(fs[$0]!) }
        case .list(let es): return es.flatMap(exprBinders)
        case .some(let x): return exprBinders(x)
        case .ife(let c, let a, let b): return [c, a, b].flatMap(exprBinders)
        case .op(_, let es): return es.flatMap(exprBinders)
        case .cmp(_, let a, let b): return exprBinders(a) + exprBinders(b)
        case .call(_, let es): return es.flatMap(exprBinders)
        case .std(_, let es): return es.flatMap(exprBinders)
        case .select(let p): return planBinders(p)
        case .get(_, let ks): return ks.flatMap(exprBinders)
        case .exists(_, let ks): return ks.flatMap(exprBinders)
        default: return []
        }
    }

    static func planBinders(_ p: Plan) -> [Sym] {
        return (p.filter.map(predBinders) ?? []) + p.related.flatMap { planBinders($0.plan) }
    }

    static func predBinders(_ p: Pred) -> [Sym] {
        switch p {
        case .pcmp(_, _, let e): return exprBinders(e)
        case .pin(_, let es): return es.flatMap(exprBinders)
        case .pall(let ps), .pany(let ps): return ps.flatMap(predBinders)
        case .pnot(let q): return predBinders(q)
        }
    }

    static func renumberMany(_ ren: Ren, _ next: Int, _ es: [Expr]) -> ([Expr], Int) {
        var n = next
        var out: [Expr] = []
        for e in es {
            let (e2, n2) = renumberExpr(ren, n, e)
            out.append(e2)
            n = n2
        }
        return (out, n)
    }

    /// Field names in code point order: the order a struct's fields are
    /// walked in, which is the map's order in the spec.
    static func sortedFieldNamesOf<T>(_ m: [String: T]) -> [String] {
        return m.keys.sorted { compareText($0, $1) < 0 }
    }

    static func renumberExpr(_ ren: Ren, _ next: Int, _ e: Expr) -> (Expr, Int) {
        func binder1(_ xs: Expr, _ x: Sym, _ b: Expr, _ mk: (Expr, Sym, Expr) -> Expr) -> (Expr, Int) {
            let (xs2, n1) = renumberExpr(ren, next, xs)
            var r2 = ren
            r2[x] = n1
            let (b2, n2) = renumberExpr(r2, n1 + 1, b)
            return (mk(xs2, n1, b2), n2)
        }
        switch e {
        case .variable(let x): return (.variable(ren[x] ?? x), next)
        case .field(let x, let f):
            let (x2, n) = renumberExpr(ren, next, x)
            return (.field(x2, f), n)
        case .structOf(let fs):
            let keys = sortedFieldNamesOf(fs)
            let (es, n) = renumberMany(ren, next, keys.map { fs[$0]! })
            var m: [String: Expr] = [:]
            for (k, v) in zip(keys, es) { m[k] = v }
            return (.structOf(m), n)
        case .list(let es):
            let (es2, n) = renumberMany(ren, next, es)
            return (.list(es2), n)
        case .some(let x):
            let (x2, n) = renumberExpr(ren, next, x)
            return (.some(x2), n)
        case .match(let x, let s, let a, let b):
            let (x2, n1) = renumberExpr(ren, next, x)
            var r2 = ren
            r2[s] = n1
            let (a2, n2) = renumberExpr(r2, n1 + 1, a)
            let (b2, n3) = renumberExpr(ren, n2, b)
            return (.match(x2, n1, a2, b2), n3)
        case .ife(let c, let a, let b):
            let (c2, n1) = renumberExpr(ren, next, c)
            let (a2, n2) = renumberExpr(ren, n1, a)
            let (b2, n3) = renumberExpr(ren, n2, b)
            return (.ife(c2, a2, b2), n3)
        case .op(let op, let es):
            let (es2, n) = renumberMany(ren, next, es)
            return (.op(op, es2), n)
        case .cmp(let op, let a, let b):
            let (a2, n1) = renumberExpr(ren, next, a)
            let (b2, n2) = renumberExpr(ren, n1, b)
            return (.cmp(op, a2, b2), n2)
        case .call(let f, let es):
            let (es2, n) = renumberMany(ren, next, es)
            return (.call(f, es2), n)
        case .std(let f, let es):
            let (es2, n) = renumberMany(ren, next, es)
            return (.std(f, es2), n)
        case .map(let xs, let x, let b): return binder1(xs, x, b) { .map($0, $1, $2) }
        case .filter(let xs, let x, let b): return binder1(xs, x, b) { .filter($0, $1, $2) }
        case .any(let xs, let x, let b): return binder1(xs, x, b) { .any($0, $1, $2) }
        case .all(let xs, let x, let b): return binder1(xs, x, b) { .all($0, $1, $2) }
        case .sortBy(let xs, let x, let k): return binder1(xs, x, k) { .sortBy($0, $1, $2) }
        case .fold(let xs, let z, let acc, let x, let b):
            let (xs2, n0) = renumberExpr(ren, next, xs)
            let (z2, n1) = renumberExpr(ren, n0, z)
            var r2 = ren
            r2[acc] = n1
            r2[x] = n1 + 1
            let (b2, n2) = renumberExpr(r2, n1 + 2, b)
            return (.fold(xs2, z2, n1, n1 + 1, b2), n2)
        case .select(let p):
            let (p2, n) = renumberPlan(ren, next, p)
            return (.select(p2), n)
        case .get(let t, let ks):
            let (ks2, n) = renumberMany(ren, next, ks)
            return (.get(t, ks2), n)
        case .exists(let t, let ks):
            let (ks2, n) = renumberMany(ren, next, ks)
            return (.exists(t, ks2), n)
        default: return (e, next)
        }
    }

    static func renumberPlan(_ ren: Ren, _ next: Int, _ p: Plan) -> (Plan, Int) {
        var q = p
        var n = next
        if let f = p.filter {
            let (f2, n1) = renumberPred(ren, next, f)
            q.filter = f2
            n = n1
        }
        var rels: [Related] = []
        for r in p.related {
            let (rp, n2) = renumberPlan(ren, n, r.plan)
            var r2 = r
            r2.plan = rp
            rels.append(r2)
            n = n2
        }
        q.related = rels
        return (q, n)
    }

    static func renumberPred(_ ren: Ren, _ next: Int, _ p: Pred) -> (Pred, Int) {
        func many(_ ps: [Pred]) -> ([Pred], Int) {
            var n = next
            var out: [Pred] = []
            for q in ps {
                let (q2, n2) = renumberPred(ren, n, q)
                out.append(q2)
                n = n2
            }
            return (out, n)
        }
        switch p {
        case .pcmp(let c, let op, let e):
            let (e2, n) = renumberExpr(ren, next, e)
            return (.pcmp(c, op, e2), n)
        case .pin(let c, let es):
            let (es2, n) = renumberMany(ren, next, es)
            return (.pin(c, es2), n)
        case .pall(let ps):
            let (ps2, n) = many(ps)
            return (.pall(ps2), n)
        case .pany(let ps):
            let (ps2, n) = many(ps)
            return (.pany(ps2), n)
        case .pnot(let q):
            let (q2, n) = renumberPred(ren, next, q)
            return (.pnot(q2), n)
        }
    }

    // MARK: calls

    /// The names of the helpers a function calls directly — in its checks,
    /// its refinements and its body — sorted, unique.
    public static func calls(_ fn: Function) -> [String] {
        var seen = Set<String>()
        for nf in fn.input { for c in nf.field.checks { if case .refine(let e, _) = c { for n in exprCalls(e) { seen.insert(n) } } } }
        for r in fn.refine { for n in exprCalls(r.expr) { seen.insert(n) } }
        for s in fn.body { for n in stmtCalls(s) { seen.insert(n) } }
        return seen.sorted { compareText($0, $1) < 0 }
    }

    static func stmtCalls(_ s: Stmt) -> [String] {
        switch s {
        case .sLet(_, let e): return exprCalls(e)
        case .sIf(let c, let a, let b): return exprCalls(c) + a.flatMap(stmtCalls) + b.flatMap(stmtCalls)
        case .sFor(_, let xs, let b): return exprCalls(xs) + b.flatMap(stmtCalls)
        case .sInsert(_, let e, _), .sUpsert(_, let e, _): return exprCalls(e)
        case .sUpdate(_, let ks, _, let e): return ks.flatMap(exprCalls) + exprCalls(e)
        case .sDelete(_, let ks): return ks.flatMap(exprCalls)
        case .sRefuse(let e): return exprCalls(e)
        case .sReturn(let me): return me.map(exprCalls) ?? []
        }
    }

    static func exprCalls(_ e: Expr) -> [String] {
        switch e {
        case .call(let n, let es): return [n] + es.flatMap(exprCalls)
        case .field(let x, _): return exprCalls(x)
        case .structOf(let fs): return fs.values.flatMap(exprCalls)
        case .list(let es): return es.flatMap(exprCalls)
        case .some(let x): return exprCalls(x)
        case .match(let x, _, let a, let b): return [x, a, b].flatMap(exprCalls)
        case .ife(let c, let a, let b): return [c, a, b].flatMap(exprCalls)
        case .op(_, let es): return es.flatMap(exprCalls)
        case .cmp(_, let a, let b): return exprCalls(a) + exprCalls(b)
        case .std(_, let es): return es.flatMap(exprCalls)
        case .map(let xs, _, let b), .filter(let xs, _, let b), .any(let xs, _, let b), .all(let xs, _, let b), .sortBy(let xs, _, let b):
            return exprCalls(xs) + exprCalls(b)
        case .fold(let xs, let z, _, _, let b): return [xs, z, b].flatMap(exprCalls)
        case .select(let p): return planCalls(p)
        case .get(_, let ks): return ks.flatMap(exprCalls)
        case .exists(_, let ks): return ks.flatMap(exprCalls)
        default: return []
        }
    }

    static func planCalls(_ p: Plan) -> [String] {
        return (p.filter.map(predCalls) ?? []) + p.related.flatMap { planCalls($0.plan) }
    }

    static func predCalls(_ p: Pred) -> [String] {
        switch p {
        case .pcmp(_, _, let e): return exprCalls(e)
        case .pin(_, let es): return es.flatMap(exprCalls)
        case .pall(let ps), .pany(let ps): return ps.flatMap(predCalls)
        case .pnot(let q): return predCalls(q)
        }
    }
}
