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
package dev.arkdb

private typealias Ren = Map<Sym, Sym>

public object Normalize {
    /** `normalize`: a function with its symbols renumbered in binding order. */
    public fun function(f: Function): Function {
        var next: Sym = 0
        val input =
            f.input.map { (n, fd) ->
                val checks =
                    fd.checks.map { c ->
                        if (c is Check.Refine) {
                            val (e2, n2) = expr(emptyMap(), next, c.e)
                            next = n2
                            Check.Refine(e2, c.why)
                        } else c
                    }
                n to Field(fd.ty, checks)
            }
        val refine =
            f.refine.map { (e, why) ->
                val (e2, n2) = expr(emptyMap(), next, e)
                next = n2
                e2 to why
            }
        val plan =
            f.plan?.let {
                val (p2, n2) = plan(emptyMap(), next, it)
                next = n2
                p2
            }
        val (body, _) = block(emptyMap(), next, f.body)
        return f.copy(input = input, refine = refine, body = body, plan = plan)
    }

    /** `normalize_module`: every function normalised. */
    public fun module(m: Module): Module = m.copy(functions = m.functions.map(::function))

    private fun block(ren: Ren, next: Sym, blk: Block): Pair<Block, Ren> {
        var r = ren
        var n = next
        val out = ArrayList<Stmt>(blk.size)
        for (s in blk) {
            val (s2, r2, n2) = stmt(r, n, s)
            out.add(s2)
            r = r2
            n = n2
        }
        return out to r
    }

    private fun bind(ren: Ren, x: Sym, to: Sym): Ren = ren + (x to to)

    // The statement, the renaming extended by any binder it introduced, and the next free number.
    private fun stmt(ren: Ren, next: Sym, s: Stmt): Triple<Stmt, Ren, Sym> =
        when (s) {
            is Stmt.Let -> {
                val (e2, n2) = expr(ren, next, s.e)
                Triple(Stmt.Let(n2, e2), bind(ren, s.sym, n2), n2 + 1)
            }
            is Stmt.If -> {
                val (c2, n1) = expr(ren, next, s.c)
                val (a2, n2) = inner(ren, n1, s.then)
                val (b2, n3) = inner(ren, n2, s.els)
                Triple(Stmt.If(c2, a2, b2), ren, n3)
            }
            is Stmt.For -> {
                val (xs2, n1) = expr(ren, next, s.over)
                val (b2, n2) = inner(bind(ren, s.sym, n1), n1 + 1, s.body)
                Triple(Stmt.For(n1, xs2, b2), ren, n2)
            }
            is Stmt.Insert -> {
                val (e2, n1) = expr(ren, next, s.row)
                Triple(Stmt.Insert(s.table, e2, s.on), ren, n1)
            }
            is Stmt.Upsert -> {
                val (e2, n1) = expr(ren, next, s.row)
                Triple(Stmt.Upsert(s.table, e2, s.on), ren, n1)
            }
            // The key first, then the existing row's binder, then the new row.
            is Stmt.Update -> {
                val (ks2, n1) = many(ren, next, s.key)
                val (e2, n2) = expr(bind(ren, s.sym, n1), n1 + 1, s.row)
                Triple(Stmt.Update(s.table, ks2, n1, e2), ren, n2)
            }
            is Stmt.Delete -> {
                val (ks2, n1) = many(ren, next, s.key)
                Triple(Stmt.Delete(s.table, ks2), ren, n1)
            }
            is Stmt.Refuse -> {
                val (e2, n1) = expr(ren, next, s.e)
                Triple(Stmt.Refuse(e2), ren, n1)
            }
            is Stmt.Return -> {
                val e = s.e
                if (e == null) Triple(s, ren, next)
                else {
                    val (e2, n1) = expr(ren, next, e)
                    Triple(Stmt.Return(e2), ren, n1)
                }
            }
            // D3 A nested block, as a branch is.
            is Stmt.Private -> {
                val (b2, n1) = inner(ren, next, s.body)
                Triple(Stmt.Private(b2), ren, n1)
            }
        }

    // A nested block: its bindings do not escape, their numbers are consumed.
    private fun inner(ren: Ren, n: Sym, blk: Block): Pair<Block, Sym> {
        val (blk2, _) = block(ren, n, blk)
        var best = n
        for (s in blk2) for (b in stmtBinders(s)) best = maxOf(best, b + 1)
        return blk2 to best
    }

    private fun stmtBinders(s: Stmt): List<Sym> =
        when (s) {
            is Stmt.Let -> listOf(s.sym) + exprBinders(s.e)
            is Stmt.If -> exprBinders(s.c) + (s.then + s.els).flatMap(::stmtBinders)
            is Stmt.For -> listOf(s.sym) + exprBinders(s.over) + s.body.flatMap(::stmtBinders)
            is Stmt.Insert -> exprBinders(s.row)
            is Stmt.Upsert -> exprBinders(s.row)
            is Stmt.Refuse -> exprBinders(s.e)
            is Stmt.Update -> listOf(s.sym) + s.key.flatMap(::exprBinders) + exprBinders(s.row)
            is Stmt.Delete -> s.key.flatMap(::exprBinders)
            is Stmt.Return -> s.e?.let(::exprBinders) ?: emptyList()
            is Stmt.Private -> s.body.flatMap(::stmtBinders)
        }

    private fun exprBinders(e: Expr): List<Sym> =
        when (e) {
            is Expr.Match -> listOf(e.sym) + listOf(e.e, e.some, e.none).flatMap(::exprBinders)
            is Expr.MapE -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
            is Expr.FilterE -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
            is Expr.AnyE -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
            is Expr.AllE -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.body)
            is Expr.SortBy -> listOf(e.sym) + exprBinders(e.over) + exprBinders(e.key)
            is Expr.Fold ->
                listOf(e.acc, e.sym) + listOf(e.over, e.init, e.body).flatMap(::exprBinders)
            is Expr.Select -> planBinders(e.plan)
            else -> children(e).flatMap(::exprBinders)
        }

    private fun planBinders(p: Plan): List<Sym> {
        val v = ArrayList<Sym>()
        p.filter?.let { v.addAll(predBinders(it)) }
        p.row?.let { v.add(it) }
        p.members?.let { v.add(it) }
        for (l in p.lookups) {
            v.addAll(l.key.flatMap(::exprBinders))
            v.add(l.sym)
        }
        for (r in p.related) {
            v.addAll(r.on.flatMap { exprBinders(it.second) })
            v.addAll(planBinders(r.plan))
            v.add(r.sym)
        }
        p.having?.let { v.addAll(exprBinders(it)) }
        p.project?.let { v.addAll(exprBinders(it)) }
        for ((k, _) in p.order) if (k is Key.ExprK) v.addAll(exprBinders(k.e))
        return v
    }

    private fun predBinders(p: Pred): List<Sym> =
        when (p) {
            is Pred.Cmp -> exprBinders(p.e)
            is Pred.In -> p.items.flatMap(::exprBinders)
            is Pred.All -> p.items.flatMap(::predBinders)
            is Pred.AnyP -> p.items.flatMap(::predBinders)
            is Pred.Not -> predBinders(p.e)
            is Pred.Has -> exprBinders(p.e)
            is Pred.When -> exprBinders(p.e)
            is Pred.Exists -> predBinders(p.pred)
        }

    private fun many(ren: Ren, next: Sym, es: List<Expr>): Pair<List<Expr>, Sym> {
        var n = next
        val out =
            es.map {
                val (e2, n2) = expr(ren, n, it)
                n = n2
                e2
            }
        return out to n
    }

    private fun expr(ren: Ren, next: Sym, e: Expr): Pair<Expr, Sym> =
        when (e) {
            is Expr.Var -> Expr.Var(ren[e.sym] ?: e.sym) to next
            is Expr.FieldE -> expr(ren, next, e.e).let { (e2, n) -> Expr.FieldE(e2, e.name) to n }
            is Expr.StructE -> {
                var n = next
                val out = LinkedHashMap<String, Expr>()
                for ((k, v) in e.fields) {
                    val (v2, n2) = expr(ren, n, v)
                    out[k] = v2
                    n = n2
                }
                Expr.StructE(out) to n
            }
            is Expr.ListE -> many(ren, next, e.items).let { (es, n) -> Expr.ListE(es) to n }
            is Expr.SomeE -> expr(ren, next, e.e).let { (e2, n) -> Expr.SomeE(e2) to n }
            is Expr.Match -> {
                val (e2, n1) = expr(ren, next, e.e)
                val (a2, n2) = expr(bind(ren, e.sym, n1), n1 + 1, e.some)
                val (b2, n3) = expr(ren, n2, e.none)
                Expr.Match(e2, n1, a2, b2) to n3
            }
            is Expr.IfE -> {
                val (c2, n1) = expr(ren, next, e.c)
                val (a2, n2) = expr(ren, n1, e.then)
                val (b2, n3) = expr(ren, n2, e.els)
                Expr.IfE(c2, a2, b2) to n3
            }
            is Expr.OpE -> many(ren, next, e.args).let { (es, n) -> Expr.OpE(e.op, es) to n }
            is Expr.CmpE -> {
                val (a2, n1) = expr(ren, next, e.l)
                val (b2, n2) = expr(ren, n1, e.r)
                Expr.CmpE(e.op, a2, b2) to n2
            }
            is Expr.Call -> many(ren, next, e.args).let { (es, n) -> Expr.Call(e.fn, es) to n }
            is Expr.StdE -> many(ren, next, e.args).let { (es, n) -> Expr.StdE(e.fn, es) to n }
            is Expr.MapE ->
                binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.MapE(xs, x, b) }
            is Expr.FilterE ->
                binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.FilterE(xs, x, b) }
            is Expr.AnyE ->
                binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.AnyE(xs, x, b) }
            is Expr.AllE ->
                binder1(ren, next, e.over, e.sym, e.body) { xs, x, b -> Expr.AllE(xs, x, b) }
            is Expr.SortBy ->
                binder1(ren, next, e.over, e.sym, e.key) { xs, x, k -> Expr.SortBy(xs, x, k) }
            is Expr.Fold -> {
                val (xs2, n0) = expr(ren, next, e.over)
                val (z2, n1) = expr(ren, n0, e.init)
                val (b2, n2) = expr(bind(bind(ren, e.acc, n1), e.sym, n1 + 1), n1 + 2, e.body)
                Expr.Fold(xs2, z2, n1, n1 + 1, b2) to n2
            }
            is Expr.Select -> plan(ren, next, e.plan).let { (p2, n) -> Expr.Select(p2) to n }
            is Expr.Get -> many(ren, next, e.key).let { (ks, n) -> Expr.Get(e.table, ks) to n }
            is Expr.ExistsE ->
                many(ren, next, e.key).let { (ks, n) -> Expr.ExistsE(e.table, ks) to n }
            is Expr.Lit,
            is Expr.Arg,
            is Expr.AutoE,
            is Expr.CtxUser,
            is Expr.CtxSession,
            is Expr.HasRole,
            is Expr.Provided,
            is Expr.NoneE -> e to next
        }

    private fun binder1(
        ren: Ren,
        next: Sym,
        xs: Expr,
        x: Sym,
        b: Expr,
        mk: (Expr, Sym, Expr) -> Expr
    ): Pair<Expr, Sym> {
        val (xs2, n1) = expr(ren, next, xs)
        val (b2, n2) = expr(bind(ren, x, n1), n1 + 1, b)
        return mk(xs2, n1, b2) to n2
    }

    // A plan in `ren`, the scope around it.
    private fun plan(ren: Ren, next: Sym, p: Plan): Pair<Plan, Sym> {
        var n = next
        val filter =
            p.filter?.let {
                val (f2, n2) = pred(ren, n, it)
                n = n2
                f2
            }
        // The node's own scope: the scope around the plan and its binders.
        var node: Ren = ren
        fun open(x: Sym): Sym {
            node = bind(node, x, n)
            n += 1
            return n - 1
        }
        val row = p.row?.let(::open)
        val members = p.members?.let(::open)
        val lookups =
            p.lookups.map { l ->
                val (key, n1) = many(node, n, l.key)
                n = n1
                l.copy(key = key, sym = open(l.sym))
            }
        val related =
            p.related.map { r ->
                val on =
                    r.on.map { (c, e) ->
                        val (e2, n1) = expr(node, n, e)
                        n = n1
                        c to e2
                    }
                val (child, n1) = plan(ren, n, r.plan)
                n = n1
                Related(r.name, open(r.sym), on, child)
            }
        fun one(e: Expr?): Expr? =
            e?.let {
                val (e2, n1) = expr(node, n, it)
                n = n1
                e2
            }
        val having = one(p.having)
        val project = one(p.project)
        val order =
            p.order.map { (k, d) ->
                when (k) {
                    is Key.Column -> k to d
                    is Key.ExprK -> Key.ExprK(one(k.e)!!) to d
                }
            }
        return Plan(
            p.source, filter, row, members, lookups, related, having, project, order, p.limit) to n
    }

    private fun pred(ren: Ren, next: Sym, p: Pred): Pair<Pred, Sym> =
        when (p) {
            is Pred.Cmp -> expr(ren, next, p.e).let { (e2, n) -> Pred.Cmp(p.column, p.op, e2) to n }
            is Pred.In -> many(ren, next, p.items).let { (es, n) -> Pred.In(p.column, es) to n }
            is Pred.All -> preds(ren, next, p.items).let { (ps, n) -> Pred.All(ps) to n }
            is Pred.AnyP -> preds(ren, next, p.items).let { (ps, n) -> Pred.AnyP(ps) to n }
            is Pred.Not -> pred(ren, next, p.e).let { (q, n) -> Pred.Not(q) to n }
            is Pred.Has -> expr(ren, next, p.e).let { (e2, n) -> Pred.Has(p.column, e2) to n }
            is Pred.When -> expr(ren, next, p.e).let { (e2, n) -> Pred.When(e2) to n }
            is Pred.Exists ->
                pred(ren, next, p.pred).let { (q, n) -> Pred.Exists(p.table, p.column, q) to n }
        }

    private fun preds(ren: Ren, next: Sym, ps: List<Pred>): Pair<List<Pred>, Sym> {
        var n = next
        val out =
            ps.map {
                val (q, n2) = pred(ren, n, it)
                n = n2
                q
            }
        return out to n
    }
}
