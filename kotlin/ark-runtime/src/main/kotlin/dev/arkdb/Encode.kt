// §7 The module as a value, and the normal form a hash is taken of (Ark.Encode).
package dev.arkdb

public object Encode {
    private fun node(t: String, vararg fs: Pair<String, Value>): Value =
        Value.VStruct(mapOf("t" to Value.VText(t), *fs))

    private fun txt(s: String): Value = Value.VText(s)

    private fun int(n: Int): Value = Value.VInt(n.toLong())

    private fun <A> list(xs: List<A>, f: (A) -> Value): Value = Value.VList(xs.map(f))

    /** The whole module. Functions are carried without their dependency hashes here. */
    public fun toValue(m: Module): Value = node(
        "module",
        "spec" to int(m.spec),
        "schema" to schemaValue(m.schema),
        "functions" to list(m.functions) { functionValue(emptyMap(), it) },
        "live" to list(m.live) { (n, t) -> node("frame", "name" to txt(n), "ty" to tyValue(t)) },
        "routers" to list(m.routers) { r ->
            node("router", "name" to txt(r.name), "scope" to txt(r.scope), "uses" to list(r.uses) { txt(it) })
        },
    )

    public fun schemaValue(sch: Schema): Value = list(sch.scopes) { s ->
        node("scope", "name" to txt(s.name), "tables" to list(s.tables) { t ->
            node(
                "table",
                "name" to txt(t.name),
                "columns" to list(t.columns) { c ->
                    node("column", "name" to txt(c.name), "ty" to tyValue(c.ty), "nullable" to Value.VBool(c.nullable))
                },
                "key" to list(t.key) { txt(it) },
                "indexes" to list(t.indexes) { i ->
                    node("index", "columns" to list(i.columns) { txt(it) }, "unique" to Value.VBool(i.unique))
                },
                "refs" to list(t.refs) { r -> node("ref", "column" to txt(r.column), "table" to txt(r.table)) },
            )
        })
    }

    public fun tyValue(t: Ty): Value = when (t) {
        is Ty.TBool -> node("bool")
        is Ty.TInt -> node("int")
        is Ty.TText -> node("text")
        is Ty.TBytes -> node("bytes")
        is Ty.TId -> node("id", "table" to txt(t.table))
        is Ty.TEnum -> node("enum", "variants" to list(t.variants) { txt(it) })
        is Ty.TOption -> node("option", "of" to tyValue(t.of))
        is Ty.TList -> node("list", "of" to tyValue(t.of))
        is Ty.TStruct -> node("struct", "fields" to Value.VStruct(t.fields.mapValues { tyValue(it.value) }))
    }

    /**
     * One function, as normalised, with the hashes of the helpers it calls
     * (supplied by the caller). Names are not carried.
     */
    public fun functionValue(deps: Map<String, Value>, fn0: Function): Value {
        val fn = normalize(fn0)
        return node(
            "fn",
            "name" to txt(fn.name),
            "deps" to Value.VStruct(deps),
            "kind" to txt(kindName(fn.kind)),
            "scope" to (fn.scope?.let { txt(it) } ?: Value.VNull),
            "router" to (fn.router?.let { txt(it) } ?: Value.VNull),
            "uses" to list(fn.uses) { txt(it) },
            "autos" to list(fn.autos) { (n, a) ->
                when (a) {
                    is Auto.NewId -> node("new_id", "name" to txt(n), "table" to txt(a.table))
                    is Auto.Now -> node("now", "name" to txt(n))
                }
            },
            "input" to list(fn.input) { (n, f) -> fieldValue(n, f) },
            "refine" to list(fn.refine) { (e, why) -> node("refine", "e" to expr(e), "why" to opt(why)) },
            "ret" to (fn.ret?.let { tyValue(it) } ?: Value.VNull),
            "body" to list(fn.body) { stmt(it) },
        )
    }

    public fun kindName(k: FnKind): String = when (k) {
        FnKind.Mutator -> "mutator"
        FnKind.Query -> "query"
        FnKind.Helper -> "helper"
        FnKind.Guard -> "guard"
        FnKind.Provide -> "provide"
    }

    private fun opt(s: String?): Value = s?.let { txt(it) } ?: Value.VNull

    private fun optInt(n: Long?): Value = n?.let { Value.VInt(it) } ?: Value.VNull

    public fun fieldValue(n: String, f: Field): Value =
        node("field", "name" to txt(n), "ty" to tyValue(f.ty), "checks" to list(f.checks) { check(it) })

    public fun check(c: Check): Value = when (c) {
        is Check.Trim -> node("trim")
        is Check.MinLen -> node("min_len", "n" to int(c.n), "why" to opt(c.why))
        is Check.MaxLen -> node("max_len", "n" to int(c.n), "why" to opt(c.why))
        is Check.Range -> node("range", "lo" to optInt(c.lo), "hi" to optInt(c.hi), "why" to opt(c.why))
        is Check.NonEmpty -> node("non_empty", "why" to opt(c.why))
        is Check.Exists -> node("exists", "why" to opt(c.why))
        is Check.Refine -> node("refine", "e" to expr(c.e), "why" to opt(c.why))
    }

    private fun stmt(s: Stmt): Value = when (s) {
        is Stmt.Let -> node("let", "sym" to int(s.sym), "e" to expr(s.e))
        is Stmt.If -> node("if", "c" to expr(s.c), "then" to list(s.then) { stmt(it) }, "else" to list(s.els) { stmt(it) })
        is Stmt.For -> node("for", "sym" to int(s.sym), "in" to expr(s.over), "body" to list(s.body) { stmt(it) })
        is Stmt.Insert -> node("insert", "table" to txt(s.table), "row" to expr(s.row), "on" to list(s.on) { txt(it) })
        is Stmt.Upsert -> node("upsert", "table" to txt(s.table), "row" to expr(s.row), "on" to list(s.on) { txt(it) })
        is Stmt.Update -> node(
            "update", "table" to txt(s.table), "key" to list(s.key) { expr(it) }, "sym" to int(s.sym), "row" to expr(s.row),
        )
        is Stmt.Delete -> node("delete", "table" to txt(s.table), "key" to list(s.key) { expr(it) })
        is Stmt.Refuse -> node("refuse", "e" to expr(s.e))
        is Stmt.Return -> node("return", "e" to (s.e?.let { expr(it) } ?: Value.VNull))
    }

    private fun expr(e: Expr): Value = when (e) {
        is Expr.Lit -> node("lit", "v" to e.v)
        is Expr.Arg -> node("arg", "name" to txt(e.name))
        is Expr.Auto -> node("auto", "name" to txt(e.name))
        is Expr.Var -> node("var", "sym" to int(e.sym))
        is Expr.CtxUser -> node("ctx_user")
        is Expr.CtxSession -> node("ctx_session")
        is Expr.Field -> node("field", "e" to expr(e.e), "name" to txt(e.name))
        is Expr.Struct -> node("struct", "fields" to Value.VStruct(e.fields.mapValues { expr(it.value) }))
        is Expr.ListE -> node("list", "items" to list(e.items) { expr(it) })
        is Expr.Some -> node("some", "e" to expr(e.e))
        is Expr.None -> node("none", "ty" to tyValue(e.ty))
        is Expr.Match -> node("match", "e" to expr(e.e), "sym" to int(e.sym), "some" to expr(e.some), "none" to expr(e.none))
        is Expr.If -> node("ife", "c" to expr(e.c), "then" to expr(e.then), "else" to expr(e.els))
        is Expr.Op -> node("op", "op" to txt(e.op.wire), "args" to list(e.args) { expr(it) })
        is Expr.Cmp -> node("cmp", "op" to txt(e.op.wire), "l" to expr(e.l), "r" to expr(e.r))
        is Expr.Call -> node("call", "fn" to txt(e.fn), "args" to list(e.args) { expr(it) })
        is Expr.Std -> node("std", "fn" to txt(e.fn.name), "args" to list(e.args) { expr(it) })
        is Expr.Map -> node("map", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
        is Expr.Filter -> node("filter", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
        is Expr.Any -> node("any", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
        is Expr.All -> node("all", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
        is Expr.SortBy -> node("sort_by", "in" to expr(e.over), "sym" to int(e.sym), "key" to expr(e.key))
        is Expr.Fold -> node(
            "fold", "in" to expr(e.over), "init" to expr(e.init), "acc" to int(e.acc), "sym" to int(e.sym), "body" to expr(e.body),
        )
        is Expr.Select -> node("select", "plan" to plan(e.plan))
        is Expr.Get -> node("get", "table" to txt(e.table), "key" to list(e.key) { expr(it) })
        is Expr.Exists -> node("exists", "table" to txt(e.table), "key" to list(e.key) { expr(it) })
        is Expr.Provided -> node("provided", "fn" to txt(e.fn))
    }

    private fun plan(p: IR.Plan): Value = node(
        "plan",
        "table" to txt(p.table),
        "filter" to (p.filter?.let { pred(it) } ?: Value.VNull),
        "order" to list(p.order) { (c, d) -> node("by", "column" to txt(c), "dir" to txt(if (d == Dir.Asc) "asc" else "desc")) },
        "limit" to (p.limit?.let { int(it) } ?: Value.VNull),
        "related" to list(p.related) { r ->
            node(
                "related",
                "name" to txt(r.name),
                "parent" to txt(r.relation.parent),
                "child" to txt(r.relation.child),
                "column" to txt(r.relation.column),
                "plan" to plan(r.plan),
            )
        },
    )

    private fun pred(p: IR.Pred): Value = when (p) {
        is IR.Pred.Cmp -> node("pcmp", "column" to txt(p.column), "op" to txt(p.op.wire), "e" to expr(p.e))
        is IR.Pred.In -> node("pin", "column" to txt(p.column), "items" to list(p.items) { expr(it) })
        is IR.Pred.All -> node("pall", "items" to list(p.items) { pred(it) })
        is IR.Pred.Any -> node("pany", "items" to list(p.items) { pred(it) })
        is IR.Pred.Not -> node("pnot", "e" to pred(p.e))
    }

    // §7.1 Alpha-normalisation --------------------------------------------

    /**
     * Symbols renumbered 0, 1, 2… in the order their binders are met walking
     * the body top to bottom, left to right, binders before the scopes they
     * open. Free symbols are left as they are.
     */
    public fun normalize(fn: Function): Function {
        val ren = HashMap<Sym, Sym>()
        var next = 0
        // One numbering covers the function: the input's checks, then the
        // refinements, then the body (AUTHORING.md, Appendix A).
        val input = fn.input.map { (n, f) ->
            val checks = f.checks.map { c ->
                if (c is Check.Refine) {
                    val (e2, n2) = renumberExpr(HashMap(ren), next, c.e)
                    next = n2
                    Check.Refine(e2, c.why)
                } else {
                    c
                }
            }
            n to Field(f.ty, checks)
        }
        val refine = fn.refine.map { (e, why) ->
            val (e2, n2) = renumberExpr(HashMap(ren), next, e)
            next = n2
            e2 to why
        }
        val (body, _) = renumberBlock(ren, next, fn.body)
        val names = HashMap<Sym, String>()
        for ((old, new) in ren) fn.names[old]?.let { names[new] = it }
        return fn.copy(input = input, refine = refine, body = body, names = names)
    }

    public fun normalizeModule(m: Module): Module = m.copy(functions = m.functions.map { normalize(it) })

    private class Ren(val map: Map<Sym, Sym>)

    // The renaming is threaded as an immutable map: a nested block's bindings
    // do not escape it, but their numbers are still consumed.
    private fun renumberBlock(ren: HashMap<Sym, Sym>, next0: Int, blk: List<Stmt>): Pair<List<Stmt>, Int> {
        val out = ArrayList<Stmt>(blk.size)
        var next = next0
        for (s in blk) {
            val (s2, n2) = renumberStmt(ren, next, s)
            out.add(s2)
            next = n2
        }
        return out to next
    }

    private fun renumberStmt(ren: HashMap<Sym, Sym>, next: Int, s: Stmt): Pair<Stmt, Int> = when (s) {
        is Stmt.Let -> {
            val (e2, n1) = renumberExpr(ren, next, s.e)
            ren[s.sym] = n1
            Stmt.Let(n1, e2) to n1 + 1
        }
        is Stmt.If -> {
            val (c2, n1) = renumberExpr(ren, next, s.c)
            val (a2, n2) = inner(ren, n1, s.then)
            val (b2, n3) = inner(ren, n2, s.els)
            Stmt.If(c2, a2, b2) to n3
        }
        is Stmt.For -> {
            val (xs2, n1) = renumberExpr(ren, next, s.over)
            val ren2 = HashMap(ren)
            ren2[s.sym] = n1
            val (b2, n2) = inner(ren2, n1 + 1, s.body)
            Stmt.For(n1, xs2, b2) to n2
        }
        is Stmt.Insert -> {
            val (e2, n1) = renumberExpr(ren, next, s.row)
            Stmt.Insert(s.table, e2, s.on) to n1
        }
        is Stmt.Upsert -> {
            val (e2, n1) = renumberExpr(ren, next, s.row)
            Stmt.Upsert(s.table, e2, s.on) to n1
        }
        // The key, then the binder for the existing row, then the new row over it.
        is Stmt.Update -> {
            val (ks2, n1) = renumberMany(ren, next, s.key)
            val ren2 = HashMap(ren)
            ren2[s.sym] = n1
            val (e2, n2) = renumberExpr(ren2, n1 + 1, s.row)
            Stmt.Update(s.table, ks2, n1, e2) to n2
        }
        is Stmt.Delete -> {
            val (ks2, n1) = renumberMany(ren, next, s.key)
            Stmt.Delete(s.table, ks2) to n1
        }
        is Stmt.Refuse -> {
            val (e2, n1) = renumberExpr(ren, next, s.e)
            Stmt.Refuse(e2) to n1
        }
        is Stmt.Return -> if (s.e == null) {
            s to next
        } else {
            val (e2, n1) = renumberExpr(ren, next, s.e)
            Stmt.Return(e2) to n1
        }
    }

    // A nested block: its bindings do not escape, so the renaming is copied,
    // and the next free number is one past the largest binder in it.
    private fun inner(ren: HashMap<Sym, Sym>, n: Int, blk: List<Stmt>): Pair<List<Stmt>, Int> {
        val (blk2, _) = renumberBlock(HashMap(ren), n, blk)
        return blk2 to countBinders(blk2, n)
    }

    private fun countBinders(blk: List<Stmt>, n: Int): Int {
        var m = n
        for (b in blk.flatMap { binders(it) }) if (b + 1 > m) m = b + 1
        return m
    }

    private fun binders(s: Stmt): List<Sym> = when (s) {
        is Stmt.Let -> listOf(s.sym) + exprBinders(s.e)
        is Stmt.If -> exprBinders(s.c) + s.then.flatMap { binders(it) } + s.els.flatMap { binders(it) }
        is Stmt.For -> listOf(s.sym) + exprBinders(s.over) + s.body.flatMap { binders(it) }
        is Stmt.Insert -> exprBinders(s.row)
        is Stmt.Upsert -> exprBinders(s.row)
        is Stmt.Update -> listOf(s.sym) + s.key.flatMap { exprBinders(it) } + exprBinders(s.row)
        is Stmt.Delete -> s.key.flatMap { exprBinders(it) }
        is Stmt.Refuse -> exprBinders(s.e)
        is Stmt.Return -> s.e?.let { exprBinders(it) } ?: emptyList()
    }

    private fun exprBinders(e: Expr): List<Sym> = when (e) {
        is Expr.Match -> listOf(e.sym) + exprBinders(e.e) + exprBinders(e.some) + exprBinders(e.none)
        is Expr.Map -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
        is Expr.Filter -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
        is Expr.Any -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
        is Expr.All -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
        is Expr.SortBy -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.key)
        is Expr.Fold -> listOf(e.acc, e.sym) + exprBinders(e.over) + exprBinders(e.init) + exprBinders(e.body)
        is Expr.Field -> exprBinders(e.e)
        is Expr.Struct -> e.fields.values.flatMap { exprBinders(it) }
        is Expr.ListE -> e.items.flatMap { exprBinders(it) }
        is Expr.Some -> exprBinders(e.e)
        is Expr.If -> exprBinders(e.c) + exprBinders(e.then) + exprBinders(e.els)
        is Expr.Op -> e.args.flatMap { exprBinders(it) }
        is Expr.Cmp -> exprBinders(e.l) + exprBinders(e.r)
        is Expr.Call -> e.args.flatMap { exprBinders(it) }
        is Expr.Std -> e.args.flatMap { exprBinders(it) }
        is Expr.Select -> planBinders(e.plan)
        is Expr.Get -> e.key.flatMap { exprBinders(it) }
        is Expr.Exists -> e.key.flatMap { exprBinders(it) }
        else -> emptyList()
    }

    private fun planBinders(p: IR.Plan): List<Sym> =
        (p.filter?.let { predBinders(it) } ?: emptyList()) + p.related.flatMap { planBinders(it.plan) }

    private fun predBinders(p: IR.Pred): List<Sym> = when (p) {
        is IR.Pred.Cmp -> exprBinders(p.e)
        is IR.Pred.In -> p.items.flatMap { exprBinders(it) }
        is IR.Pred.All -> p.items.flatMap { predBinders(it) }
        is IR.Pred.Any -> p.items.flatMap { predBinders(it) }
        is IR.Pred.Not -> predBinders(p.e)
    }

    private fun renumberMany(ren: HashMap<Sym, Sym>, next0: Int, es: List<Expr>): Pair<List<Expr>, Int> {
        val out = ArrayList<Expr>(es.size)
        var next = next0
        for (e in es) {
            val (e2, n) = renumberExpr(ren, next, e)
            out.add(e2)
            next = n
        }
        return out to next
    }

    private fun renumberExpr(ren: HashMap<Sym, Sym>, next: Int, e: Expr): Pair<Expr, Int> = when (e) {
        is Expr.Var -> Expr.Var(ren[e.sym] ?: e.sym) to next
        is Expr.Field -> {
            val (e2, n) = renumberExpr(ren, next, e.e)
            Expr.Field(e2, e.name) to n
        }
        is Expr.Struct -> {
            val keys = e.fields.keys.toList()
            val (es, n) = renumberMany(ren, next, e.fields.values.toList())
            Expr.Struct(keys.zip(es).toMap()) to n
        }
        is Expr.ListE -> {
            val (es, n) = renumberMany(ren, next, e.items)
            Expr.ListE(es) to n
        }
        is Expr.Some -> {
            val (e2, n) = renumberExpr(ren, next, e.e)
            Expr.Some(e2) to n
        }
        is Expr.Match -> {
            val (e2, n1) = renumberExpr(ren, next, e.e)
            val ren2 = HashMap(ren)
            ren2[e.sym] = n1
            val (a2, n2) = renumberExpr(ren2, n1 + 1, e.some)
            val (b2, n3) = renumberExpr(ren, n2, e.none)
            Expr.Match(e2, n1, a2, b2) to n3
        }
        is Expr.If -> {
            val (c2, n1) = renumberExpr(ren, next, e.c)
            val (a2, n2) = renumberExpr(ren, n1, e.then)
            val (b2, n3) = renumberExpr(ren, n2, e.els)
            Expr.If(c2, a2, b2) to n3
        }
        is Expr.Op -> {
            val (es, n) = renumberMany(ren, next, e.args)
            Expr.Op(e.op, es) to n
        }
        is Expr.Cmp -> {
            val (a2, n1) = renumberExpr(ren, next, e.l)
            val (b2, n2) = renumberExpr(ren, n1, e.r)
            Expr.Cmp(e.op, a2, b2) to n2
        }
        is Expr.Call -> {
            val (es, n) = renumberMany(ren, next, e.args)
            Expr.Call(e.fn, es) to n
        }
        is Expr.Std -> {
            val (es, n) = renumberMany(ren, next, e.args)
            Expr.Std(e.fn, es) to n
        }
        is Expr.Map -> binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.Map(xs, x, b) }
        is Expr.Filter -> binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.Filter(xs, x, b) }
        is Expr.Any -> binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.Any(xs, x, b) }
        is Expr.All -> binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.All(xs, x, b) }
        is Expr.SortBy -> binder1(ren, next, e.over, e.sym, e.key) { xs, x, b -> Expr.SortBy(xs, x, b) }
        is Expr.Fold -> {
            val (xs2, n0) = renumberExpr(ren, next, e.over)
            val (z2, n1) = renumberExpr(ren, n0, e.init)
            val ren2 = HashMap(ren)
            ren2[e.acc] = n1
            ren2[e.sym] = n1 + 1
            val (b2, n2) = renumberExpr(ren2, n1 + 2, e.body)
            Expr.Fold(xs2, z2, n1, n1 + 1, b2) to n2
        }
        is Expr.Select -> {
            val (p2, n) = renumberPlan(ren, next, e.plan)
            Expr.Select(p2) to n
        }
        is Expr.Get -> {
            val (ks, n) = renumberMany(ren, next, e.key)
            Expr.Get(e.table, ks) to n
        }
        is Expr.Exists -> {
            val (ks, n) = renumberMany(ren, next, e.key)
            Expr.Exists(e.table, ks) to n
        }
        else -> e to next
    }

    private fun binder1(
        ren: HashMap<Sym, Sym>,
        next: Int,
        xs: Expr,
        x: Sym,
        b: Expr,
        mk: (Expr, Sym, Expr) -> Expr,
    ): Pair<Expr, Int> {
        val (xs2, n1) = renumberExpr(ren, next, xs)
        val ren2 = HashMap(ren)
        ren2[x] = n1
        val (b2, n2) = renumberExpr(ren2, n1 + 1, b)
        return mk(xs2, n1, b2) to n2
    }

    private fun renumberPlan(ren: HashMap<Sym, Sym>, next: Int, p: IR.Plan): Pair<IR.Plan, Int> {
        var n = next
        val f2 = p.filter?.let {
            val (f, n1) = renumberPred(ren, n, it)
            n = n1
            f
        }
        val rels = ArrayList<IR.Related>()
        for (r in p.related) {
            val (rp, n2) = renumberPlan(ren, n, r.plan)
            rels.add(r.copy(plan = rp))
            n = n2
        }
        return p.copy(filter = f2, related = rels) to n
    }

    private fun renumberPred(ren: HashMap<Sym, Sym>, next: Int, p: IR.Pred): Pair<IR.Pred, Int> = when (p) {
        is IR.Pred.Cmp -> {
            val (e2, n) = renumberExpr(ren, next, p.e)
            IR.Pred.Cmp(p.column, p.op, e2) to n
        }
        is IR.Pred.In -> {
            val (es, n) = renumberMany(ren, next, p.items)
            IR.Pred.In(p.column, es) to n
        }
        is IR.Pred.All -> {
            val (ps, n) = renumberPreds(ren, next, p.items)
            IR.Pred.All(ps) to n
        }
        is IR.Pred.Any -> {
            val (ps, n) = renumberPreds(ren, next, p.items)
            IR.Pred.Any(ps) to n
        }
        is IR.Pred.Not -> {
            val (q, n) = renumberPred(ren, next, p.e)
            IR.Pred.Not(q) to n
        }
    }

    private fun renumberPreds(ren: HashMap<Sym, Sym>, next0: Int, ps: List<IR.Pred>): Pair<List<IR.Pred>, Int> {
        val out = ArrayList<IR.Pred>(ps.size)
        var next = next0
        for (q in ps) {
            val (q2, n) = renumberPred(ren, next, q)
            out.add(q2)
            next = n
        }
        return out to next
    }

    // Calls -----------------------------------------------------------------

    /** The names of the helpers a function calls directly (checks and refinements included), each once, in code point order. */
    public fun calls(fn: Function): List<String> {
        val inChecks = fn.input.flatMap { (_, f) -> f.checks.flatMap { c -> if (c is Check.Refine) exprCalls(c.e) else emptyList() } }
        val inRefine = fn.refine.flatMap { exprCalls(it.first) }
        return (inChecks + inRefine + fn.body.flatMap { stmtCalls(it) }).toSortedSet(CodePointOrder).toList()
    }

    /**
     * What a function's hash depends on by name: the helpers it calls and,
     * for a procedure, the middleware it runs (AUTHORING.md §1.2), each once,
     * in code point order.
     */
    public fun deps(fn: Function): List<String> = (calls(fn) + fn.uses).toSortedSet(CodePointOrder).toList()

    private fun stmtCalls(s: Stmt): List<String> = when (s) {
        is Stmt.Let -> exprCalls(s.e)
        is Stmt.If -> exprCalls(s.c) + s.then.flatMap { stmtCalls(it) } + s.els.flatMap { stmtCalls(it) }
        is Stmt.For -> exprCalls(s.over) + s.body.flatMap { stmtCalls(it) }
        is Stmt.Insert -> exprCalls(s.row)
        is Stmt.Upsert -> exprCalls(s.row)
        is Stmt.Update -> s.key.flatMap { exprCalls(it) } + exprCalls(s.row)
        is Stmt.Delete -> s.key.flatMap { exprCalls(it) }
        is Stmt.Refuse -> exprCalls(s.e)
        is Stmt.Return -> s.e?.let { exprCalls(it) } ?: emptyList()
    }

    private fun exprCalls(e: Expr): List<String> = when (e) {
        is Expr.Call -> listOf(e.fn) + e.args.flatMap { exprCalls(it) }
        is Expr.Field -> exprCalls(e.e)
        is Expr.Struct -> e.fields.values.flatMap { exprCalls(it) }
        is Expr.ListE -> e.items.flatMap { exprCalls(it) }
        is Expr.Some -> exprCalls(e.e)
        is Expr.Match -> exprCalls(e.e) + exprCalls(e.some) + exprCalls(e.none)
        is Expr.If -> exprCalls(e.c) + exprCalls(e.then) + exprCalls(e.els)
        is Expr.Op -> e.args.flatMap { exprCalls(it) }
        is Expr.Cmp -> exprCalls(e.l) + exprCalls(e.r)
        is Expr.Std -> e.args.flatMap { exprCalls(it) }
        is Expr.Map -> exprCalls(e.over) + exprCalls(e.body)
        is Expr.Filter -> exprCalls(e.over) + exprCalls(e.body)
        is Expr.Any -> exprCalls(e.over) + exprCalls(e.body)
        is Expr.All -> exprCalls(e.over) + exprCalls(e.body)
        is Expr.SortBy -> exprCalls(e.over) + exprCalls(e.key)
        is Expr.Fold -> exprCalls(e.over) + exprCalls(e.init) + exprCalls(e.body)
        is Expr.Select -> planCalls(e.plan)
        is Expr.Get -> e.key.flatMap { exprCalls(it) }
        is Expr.Exists -> e.key.flatMap { exprCalls(it) }
        else -> emptyList()
    }

    private fun planCalls(p: IR.Plan): List<String> =
        (p.filter?.let { predCalls(it) } ?: emptyList()) + p.related.flatMap { planCalls(it.plan) }

    private fun predCalls(p: IR.Pred): List<String> = when (p) {
        is IR.Pred.Cmp -> exprCalls(p.e)
        is IR.Pred.In -> p.items.flatMap { exprCalls(it) }
        is IR.Pred.All -> p.items.flatMap { predCalls(it) }
        is IR.Pred.Any -> p.items.flatMap { predCalls(it) }
        is IR.Pred.Not -> predCalls(p.e)
    }
}
