// §7 The module as a value (`rust/ark/src/ir/encode.rs`): every node a
// struct with a `"t"` tag, so that `Canon.encode` is the only encoder there
// is and a module's bytes are canonical by the same rule as a row's.
// Symbol names are not carried; the hash never sees them.

public enum Encode {
    private static func node(_ t: String, _ fields: [(String, Value)] = []) -> Value {
        .record(Record([("t", .text(t))] + fields))
    }

    private static func txt(_ s: String) -> Value { .text(s) }
    private static func int(_ n: Int64) -> Value { .int(n) }
    private static func texts(_ xs: [String]) -> Value { .list(xs.map { .text($0) }) }

    /// `module_value`: functions carried without their dependency hashes.
    public static func module(_ m: Module) -> Value {
        let live: [Value] = m.live.map { node("frame", [("name", txt($0.name)), ("ty", ty($0.ty))]) }
        return node("module", [
            ("spec", int(m.spec)),
            ("schema", schema(m.schema)),
            ("functions", .list(m.functions.map { function([], $0) })),
            ("routers", .list(m.routers.map(router))),
            ("live", .list(live)),
        ])
    }

    /// `router_value`.
    public static func router(_ r: Router) -> Value { node("router", [("name", txt(r.name)), ("uses", texts(r.uses))]) }

    /// `schema_value`.
    public static func schema(_ sch: Schema) -> Value { .list(sch.tables.map(table)) }

    private static func table(_ t: Table) -> Value {
        node("table", [
            ("name", txt(t.name)),
            ("columns", .list(t.columns.map { c in node("column", [("name", txt(c.name)), ("ty", ty(c.ty)), ("nullable", .bool(c.nullable))]) })),
            ("key", texts(t.key)),
            // D4 A text index is an index of kind `text`, after the others.
            ("indexes", .list(t.indexes.map { node("index", [("columns", texts($0.columns)), ("unique", .bool($0.unique))]) }
                + t.text.map { node("index", [("columns", texts([$0])), ("unique", .bool(false)), ("kind", txt("text"))]) })),
            ("refs", .list(t.refs.map { node("ref", [("column", txt($0.column)), ("table", txt($0.table))]) })),
        ])
    }

    /// `ty_value`.
    public static func ty(_ t: Ty) -> Value {
        switch t {
        case .bool: return node("bool")
        case .int: return node("int")
        case .text: return node("text")
        case .bytes: return node("bytes")
        case .id(let tb): return node("id", [("table", txt(tb))])
        case .enumeration(let vs): return node("enum", [("variants", texts(vs))])
        case .option(let o): return node("option", [("of", ty(o))])
        case .list(let o): return node("list", [("of", ty(o))])
        case .structure(let fs): return node("struct", [("fields", .record(Record(sorted: fs.names, fs.types.map(ty))))])
        }
    }

    /// `function_value`: one function, normalised, with the hashes of what it
    /// reaches directly. A query's `plan`, a scope's `holds` and
    /// `private: true` are written only where they are, so no function of an
    /// earlier spec moves.
    public static func function(_ deps: [(String, Value)], _ fn0: Function) -> Value {
        let f = Normalize.function(fn0)
        var fields: [(String, Value)] = [
            ("name", txt(f.name)),
            ("deps", .record(Record(deps))),
            ("kind", txt(f.kind.rawValue)),
            ("router", f.router.map(txt) ?? .null),
            ("uses", texts(f.uses)),
            ("autos", .list(f.autos.map { a -> Value in
                switch a.value {
                case .newId(let t): return node("new_id", [("name", txt(a.name)), ("table", txt(t))])
                case .now: return node("now", [("name", txt(a.name))])
                }
            })),
            ("input", .list(f.input.map { field($0.name, $0.value) })),
            ("refine", .list(f.refine.map { node("refine", [("e", expr($0.e)), ("why", why($0.why))]) })),
            ("ret", f.ret.map(ty) ?? .null),
            ("body", .list(f.body.map(stmt))),
        ]
        if let p = f.plan { fields.append(("plan", plan(p))) }
        if f.kind == .scope { fields.append(("holds", .list(f.holds.map(hold)))) }
        if f.isPrivate { fields.append(("private", .bool(true))) }
        return node("fn", fields)
    }

    /// `hold_value`: `pick` or `exclude` only where the projection is not every column.
    public static func hold(_ h: Hold) -> Value {
        var fields: [(String, Value)] = [("table", txt(h.table)), ("filter", h.filter.map(pred) ?? .null)]
        switch h.columns {
        case .all: break
        case .pick(let cs): fields.append(("pick", texts(cs)))
        case .exclude(let cs): fields.append(("exclude", texts(cs)))
        }
        return node("hold", fields)
    }

    private static func why(_ w: String?) -> Value { w.map(txt) ?? .null }
    private static func optInt(_ n: Int64?) -> Value { n.map(int) ?? .null }

    /// `field_value`: one input field.
    public static func field(_ name: String, _ f: Field) -> Value {
        node("field", [("name", txt(name)), ("ty", ty(f.ty)), ("checks", .list(f.checks.map(check)))])
    }

    /// `check_value`.
    public static func check(_ c: Check) -> Value {
        switch c {
        case .trim: return node("trim")
        case .minLen(let n, let w): return node("min_len", [("n", int(n)), ("why", why(w))])
        case .maxLen(let n, let w): return node("max_len", [("n", int(n)), ("why", why(w))])
        case .range(let lo, let hi, let w): return node("range", [("lo", optInt(lo)), ("hi", optInt(hi)), ("why", why(w))])
        case .nonEmpty(let w): return node("non_empty", [("why", why(w))])
        case .exists(let w): return node("exists", [("why", why(w))])
        case .refine(let e, let w): return node("refine", [("e", expr(e)), ("why", why(w))])
        }
    }

    /// `closure_value`: `{t: "closure", fn, helpers}`.
    public static func closure(_ c: Closure) -> Value {
        node("closure", [("fn", function([], c.function)), ("helpers", .list(c.helpers.map { function([], $0) }))])
    }

    private static func stmt(_ s: Stmt) -> Value {
        switch s {
        case .letS(let x, let e): return node("let", [("sym", int(x)), ("e", expr(e))])
        case .ifS(let c, let a, let b): return node("if", [("c", expr(c)), ("then", .list(a.map(stmt))), ("else", .list(b.map(stmt)))])
        case .forS(let x, let o, let b): return node("for", [("sym", int(x)), ("in", expr(o)), ("body", .list(b.map(stmt)))])
        case .insert(let t, let r, let on): return node("insert", [("table", txt(t)), ("row", expr(r)), ("on", texts(on))])
        case .upsert(let t, let r, let on): return node("upsert", [("table", txt(t)), ("row", expr(r)), ("on", texts(on))])
        case .update(let t, let k, let x, let r): return node("update", [("table", txt(t)), ("key", .list(k.map(expr))), ("sym", int(x)), ("row", expr(r))])
        case .delete(let t, let k): return node("delete", [("table", txt(t)), ("key", .list(k.map(expr)))])
        case .refuse(let e): return node("refuse", [("e", expr(e))])
        case .ret(let e): return node("return", [("e", e.map(expr) ?? .null)])
        case .privateS(let b): return node("private", [("body", .list(b.map(stmt)))])
        }
    }

    /// An expression as a value.
    public static func expr(_ e: Expr) -> Value {
        func b(_ t: String, _ o: Expr, _ x: Sym, _ k: String, _ body: Expr) -> Value {
            node(t, [("in", expr(o)), ("sym", int(x)), (k, expr(body))])
        }
        switch e {
        case .lit(let v): return node("lit", [("v", v)])
        case .arg(let n): return node("arg", [("name", txt(n))])
        case .auto(let n): return node("auto", [("name", txt(n))])
        case .variable(let x): return node("var", [("sym", int(x))])
        case .ctxUser: return node("ctx_user")
        case .ctxSession: return node("ctx_session")
        case .hasRole(let r): return node("has_role", [("role", txt(r))])
        case .provided(let f): return node("provided", [("fn", txt(f))])
        case .field(let x, let n): return node("field", [("e", expr(x)), ("name", txt(n))])
        case .structE(let fs): return node("struct", [("fields", .record(Record(sorted: fs.names, fs.exprs.map(expr))))])
        case .listE(let xs): return node("list", [("items", .list(xs.map(expr)))])
        case .some(let x): return node("some", [("e", expr(x))])
        case .none(let t): return node("none", [("ty", ty(t))])
        case .match(let x, let s, let a, let n): return node("match", [("e", expr(x)), ("sym", int(s)), ("some", expr(a)), ("none", expr(n))])
        case .ife(let c, let a, let n): return node("ife", [("c", expr(c)), ("then", expr(a)), ("else", expr(n))])
        case .op(let o, let xs): return node("op", [("op", txt(o.rawValue)), ("args", .list(xs.map(expr)))])
        case .cmp(let o, let l, let r): return node("cmp", [("op", txt(o.rawValue)), ("l", expr(l)), ("r", expr(r))])
        case .call(let f, let xs): return node("call", [("fn", txt(f)), ("args", .list(xs.map(expr)))])
        case .std(let f, let xs): return node("std", [("fn", txt(f.rawValue)), ("args", .list(xs.map(expr)))])
        case .map(let o, let x, let body): return b("map", o, x, "body", body)
        case .filter(let o, let x, let body): return b("filter", o, x, "body", body)
        case .any(let o, let x, let body): return b("any", o, x, "body", body)
        case .all(let o, let x, let body): return b("all", o, x, "body", body)
        case .sortBy(let o, let x, let k): return b("sort_by", o, x, "key", k)
        case .fold(let o, let z, let a, let x, let body):
            return node("fold", [("in", expr(o)), ("init", expr(z)), ("acc", int(a)), ("sym", int(x)), ("body", expr(body))])
        case .select(let p): return node("select", [("plan", plan(p))])
        case .get(let t, let k): return node("get", [("table", txt(t)), ("key", .list(k.map(expr)))])
        case .exists(let t, let k): return node("exists", [("table", txt(t)), ("key", .list(k.map(expr)))])
        }
    }

    private static func dir(_ d: Dir) -> Value { txt(d == .asc ? "asc" : "desc") }

    /// `plan_value` (§1.8): v3's keys — `table`, `filter`, `order`, `limit`,
    /// `related` — and the v4 ones only when present, so a v3-shaped plan
    /// encodes as it did.
    public static func plan(_ p: Plan) -> Value {
        var fields: [(String, Value)] = [
            ("table", txt(p.table)),
            ("filter", p.filter.map(pred) ?? .null),
            ("order", .list(p.order.map { o -> Value in
                switch o.key {
                case .column(let c): return node("by", [("column", txt(c)), ("dir", dir(o.dir))])
                case .expr(let e): return node("by", [("expr", expr(e)), ("dir", dir(o.dir))])
                }
            })),
            ("limit", optInt(p.limit)),
            ("related", .list(p.related.map(related))),
        ]
        if case .group(_, let by) = p.source { fields.append(("group", texts(by))) }
        if let r = p.row { fields.append(("row", int(r))) }
        if let m = p.members { fields.append(("members", int(m))) }
        if !p.lookups.isEmpty { fields.append(("lookups", .list(p.lookups.map(lookup)))) }
        if let h = p.having { fields.append(("having", expr(h))) }
        if let pr = p.project { fields.append(("project", expr(pr))) }
        return node("plan", fields)
    }

    private static func lookup(_ l: Lookup) -> Value {
        node("lookup", [("name", txt(l.name)), ("sym", int(l.sym)), ("table", txt(l.table)), ("key", .list(l.key.map(expr)))])
    }

    private static func related(_ r: Related) -> Value {
        node("related", [("name", txt(r.name)), ("sym", int(r.sym)), ("on", .list(r.on.map { .list([txt($0.column), expr($0.e)]) })), ("plan", plan(r.plan))])
    }

    /// A predicate as a value.
    public static func pred(_ p: Pred) -> Value {
        switch p {
        case .cmp(let c, let o, let e): return node("pcmp", [("column", txt(c)), ("op", txt(o.rawValue)), ("e", expr(e))])
        case .inP(let c, let xs): return node("pin", [("column", txt(c)), ("items", .list(xs.map(expr)))])
        case .all(let ps): return node("pall", [("items", .list(ps.map(pred)))])
        case .any(let ps): return node("pany", [("items", .list(ps.map(pred)))])
        case .not(let q): return node("pnot", [("e", pred(q))])
        case .has(let c, let e): return node("phas", [("column", txt(c)), ("e", expr(e))])
        case .when(let e): return node("pwhen", [("e", expr(e))])
        case .exists(let t, let c, let q): return node("pexists", [("table", txt(t)), ("column", txt(c)), ("pred", pred(q))])
        }
    }

    /// `calls`: the helpers a function calls directly — in its checks, its
    /// refinements, its body and its plan — sorted, without repeats.
    /// Middleware is not a call; see `reaches`.
    public static func calls(_ f: Function) -> [String] {
        var acc = SortedSet<CodePoints>()
        for i in f.input {
            for c in i.value.checks {
                if case .refine(let e, _) = c { exprCalls(e, &acc) }
            }
        }
        for r in f.refine { exprCalls(r.e, &acc) }
        for s in f.body { stmtCalls(s, &acc) }
        if let p = f.plan { planCalls(p, &acc) }
        return acc.items.map { $0.s }
    }

    /// `reaches`: the helpers a function calls and the middleware it uses, sorted, without repeats.
    public static func reaches(_ f: Function) -> [String] {
        var acc = SortedSet<CodePoints>(calls(f).map(CodePoints.init))
        for u in f.uses { acc.insert(CodePoints(u)) }
        return acc.items.map { $0.s }
    }

    private static func stmtCalls(_ s: Stmt, _ acc: inout SortedSet<CodePoints>) {
        switch s {
        case .letS(_, let e), .insert(_, let e, _), .upsert(_, let e, _), .refuse(let e): exprCalls(e, &acc)
        case .update(_, let k, _, let r):
            for e in k { exprCalls(e, &acc) }
            exprCalls(r, &acc)
        case .ifS(let c, let a, let b):
            exprCalls(c, &acc)
            for x in a + b { stmtCalls(x, &acc) }
        case .forS(_, let o, let body):
            exprCalls(o, &acc)
            for x in body { stmtCalls(x, &acc) }
        case .delete(_, let k): for e in k { exprCalls(e, &acc) }
        case .ret(let e): if let e = e { exprCalls(e, &acc) }
        // D3 The server's closure carries what a private block calls.
        case .privateS(let body): for x in body { stmtCalls(x, &acc) }
        }
    }

    private static func exprCalls(_ e: Expr, _ acc: inout SortedSet<CodePoints>) {
        if case .call(let f, _) = e { acc.insert(CodePoints(f)) }
        if case .select(let p) = e {
            planCalls(p, &acc)
            return
        }
        for c in children(e) { exprCalls(c, &acc) }
    }

    private static func planCalls(_ p: Plan, _ acc: inout SortedSet<CodePoints>) {
        if let f = p.filter { predCalls(f, &acc) }
        for l in p.lookups { for e in l.key { exprCalls(e, &acc) } }
        for r in p.related {
            for o in r.on { exprCalls(o.e, &acc) }
            planCalls(r.plan, &acc)
        }
        if let h = p.having { exprCalls(h, &acc) }
        if let pr = p.project { exprCalls(pr, &acc) }
        for o in p.order { if case .expr(let e) = o.key { exprCalls(e, &acc) } }
    }

    private static func predCalls(_ p: Pred, _ acc: inout SortedSet<CodePoints>) {
        switch p {
        case .cmp(_, _, let e), .has(_, let e), .when(let e): exprCalls(e, &acc)
        case .inP(_, let xs): for e in xs { exprCalls(e, &acc) }
        case .all(let ps), .any(let ps): for q in ps { predCalls(q, &acc) }
        case .not(let q), .exists(_, _, let q): predCalls(q, &acc)
        }
    }
}

/// The expressions directly inside one, binders' bodies included; a plan
/// inside a `Select` is not looked into (`view::children`).
public func children(_ e: Expr) -> [Expr] {
    switch e {
    case .lit, .arg, .auto, .variable, .ctxUser, .ctxSession, .hasRole, .provided, .none, .select: return []
    case .field(let x, _), .some(let x): return [x]
    case .structE(let fs): return fs.exprs
    case .listE(let xs), .op(_, let xs), .call(_, let xs), .std(_, let xs), .get(_, let xs), .exists(_, let xs): return xs
    case .match(let x, _, let a, let n): return [x, a, n]
    case .ife(let c, let a, let n): return [c, a, n]
    case .cmp(_, let l, let r): return [l, r]
    case .map(let o, _, let b), .filter(let o, _, let b), .any(let o, _, let b), .all(let o, _, let b), .sortBy(let o, _, let b): return [o, b]
    case .fold(let o, let z, _, _, let b): return [o, z, b]
    }
}

/// An expression with `f` applied to each of its direct children (`children`), the rest as it was.
public func mapChildren(_ e: Expr, _ f: (Expr) throws -> Expr) rethrows -> Expr {
    switch e {
    case .lit, .arg, .auto, .variable, .ctxUser, .ctxSession, .hasRole, .provided, .none, .select: return e
    case .field(let x, let n): return .field(try f(x), n)
    case .some(let x): return .some(try f(x))
    case .structE(let fs): return .structE(try fs.map(f))
    case .listE(let xs): return .listE(try xs.map(f))
    case .op(let o, let xs): return .op(o, try xs.map(f))
    case .call(let n, let xs): return .call(n, try xs.map(f))
    case .std(let n, let xs): return .std(n, try xs.map(f))
    case .get(let t, let xs): return .get(t, try xs.map(f))
    case .exists(let t, let xs): return .exists(t, try xs.map(f))
    case .match(let x, let s, let a, let n): return .match(try f(x), s, try f(a), try f(n))
    case .ife(let c, let a, let n): return .ife(try f(c), try f(a), try f(n))
    case .cmp(let o, let l, let r): return .cmp(o, try f(l), try f(r))
    case .map(let o, let s, let b): return .map(try f(o), s, try f(b))
    case .filter(let o, let s, let b): return .filter(try f(o), s, try f(b))
    case .any(let o, let s, let b): return .any(try f(o), s, try f(b))
    case .all(let o, let s, let b): return .all(try f(o), s, try f(b))
    case .sortBy(let o, let s, let b): return .sortBy(try f(o), s, try f(b))
    case .fold(let o, let z, let a, let s, let b): return .fold(try f(o), try f(z), a, s, try f(b))
    }
}
