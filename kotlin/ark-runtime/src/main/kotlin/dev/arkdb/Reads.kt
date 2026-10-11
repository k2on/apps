// §1.7 What a function reads outside a plan, by static inspection
// (`ir/reads.rs`). A maintained query runs its middleware at hydrate and again
// when a table the middleware read moves: every table a `Select`, `Get` or
// `Exists` in the body names (a `Select`'s related and looked-up tables too),
// every table a write names, and every table an input field's `exists` check
// reads. A query's own plan is left out — the view maintains its reads. A
// scope reads the tables it holds and, for the `exists` form, the table it
// reaches through.
package dev.arkdb

public object Reads {
    /** `ir::reads`: the tables `f` reads outside its plan. */
    public fun reads(f: Function): java.util.TreeSet<String> {
        val out = java.util.TreeSet<String>()
        for ((_, field) in f.input) {
            for (c in field.checks) {
                when (c) {
                    is Check.Exists -> Eval.idTable(field.ty)?.let { out.add(it) }
                    is Check.Refine -> expr(c.e, out)
                    else -> {}
                }
            }
        }
        for ((e, _) in f.refine) expr(e, out)
        block(f.body, out)
        for (h in f.holds) {
            out.add(h.table)
            h.filter?.let { pred(it, out) }
        }
        return out
    }

    private fun block(b: Block, out: MutableSet<String>) {
        for (s in b) {
            when (s) {
                is Stmt.Let -> expr(s.e, out)
                is Stmt.Refuse -> expr(s.e, out)
                is Stmt.Return -> s.e?.let { expr(it, out) }
                is Stmt.If -> {
                    expr(s.c, out)
                    block(s.then, out)
                    block(s.els, out)
                }
                is Stmt.For -> {
                    expr(s.over, out)
                    block(s.body, out)
                }
                is Stmt.Insert -> {
                    out.add(s.table)
                    expr(s.row, out)
                }
                is Stmt.Upsert -> {
                    out.add(s.table)
                    expr(s.row, out)
                }
                is Stmt.Update -> {
                    out.add(s.table)
                    s.key.forEach { expr(it, out) }
                    expr(s.row, out)
                }
                is Stmt.Delete -> {
                    out.add(s.table)
                    s.key.forEach { expr(it, out) }
                }
                is Stmt.Private -> block(s.body, out)
            }
        }
    }

    private fun plan(p: Plan, out: MutableSet<String>) {
        out.add(p.table)
        p.filter?.let { pred(it, out) }
        for (l in p.lookups) {
            out.add(l.table)
            l.key.forEach { expr(it, out) }
        }
        for (r in p.related) {
            r.on.forEach { (_, e) -> expr(e, out) }
            plan(r.plan, out)
        }
        p.having?.let { expr(it, out) }
        p.project?.let { expr(it, out) }
        for ((k, _) in p.order) if (k is Key.ExprK) expr(k.e, out)
    }

    private fun pred(p: Pred, out: MutableSet<String>) {
        when (p) {
            is Pred.Cmp -> expr(p.e, out)
            is Pred.In -> p.items.forEach { expr(it, out) }
            is Pred.All -> p.items.forEach { pred(it, out) }
            is Pred.AnyP -> p.items.forEach { pred(it, out) }
            is Pred.Not -> pred(p.e, out)
            is Pred.Has -> expr(p.e, out)
            is Pred.When -> expr(p.e, out)
            is Pred.Exists -> {
                out.add(p.table)
                pred(p.pred, out)
            }
        }
    }

    private fun expr(e: Expr, out: MutableSet<String>) {
        when (e) {
            is Expr.Lit,
            is Expr.Arg,
            is Expr.AutoE,
            is Expr.Var,
            is Expr.CtxUser,
            is Expr.CtxSession,
            is Expr.HasRole,
            is Expr.Provided,
            is Expr.NoneE -> {}
            is Expr.FieldE -> expr(e.e, out)
            is Expr.SomeE -> expr(e.e, out)
            is Expr.StructE -> e.fields.values.forEach { expr(it, out) }
            is Expr.ListE -> e.items.forEach { expr(it, out) }
            is Expr.OpE -> e.args.forEach { expr(it, out) }
            is Expr.Call -> e.args.forEach { expr(it, out) }
            is Expr.StdE -> e.args.forEach { expr(it, out) }
            is Expr.Match -> {
                expr(e.e, out)
                expr(e.some, out)
                expr(e.none, out)
            }
            is Expr.IfE -> {
                expr(e.c, out)
                expr(e.then, out)
                expr(e.els, out)
            }
            is Expr.CmpE -> {
                expr(e.l, out)
                expr(e.r, out)
            }
            is Expr.MapE -> {
                expr(e.over, out)
                expr(e.body, out)
            }
            is Expr.FilterE -> {
                expr(e.over, out)
                expr(e.body, out)
            }
            is Expr.AnyE -> {
                expr(e.over, out)
                expr(e.body, out)
            }
            is Expr.AllE -> {
                expr(e.over, out)
                expr(e.body, out)
            }
            is Expr.SortBy -> {
                expr(e.over, out)
                expr(e.key, out)
            }
            is Expr.Fold -> {
                expr(e.over, out)
                expr(e.init, out)
                expr(e.body, out)
            }
            is Expr.Select -> plan(e.plan, out)
            is Expr.Get -> {
                out.add(e.table)
                e.key.forEach { expr(it, out) }
            }
            is Expr.ExistsE -> {
                out.add(e.table)
                e.key.forEach { expr(it, out) }
            }
        }
    }
}
