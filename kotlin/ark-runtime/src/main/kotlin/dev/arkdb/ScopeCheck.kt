// `docs/plan-guards.md` D2 What a person holds — the union of a module's
// scopes for one identity — as far as the verifier asks it
// (`rust/ark/src/scope.rs`): the scopes gathered by table, a filter folded
// under a context, the union's columns, and `check`, which holds what a
// client runs to what its person holds for every role set the module
// names. The client's half (`Holdings`: admits, project, the device
// schema, filtering facts) is round 3's.
package dev.arkdb

// The context a filter is folded under: everything a person is known by, or
// — for the verifier, which asks about every user at once — no user and no login.
internal class FoldCtx(val user: String?, val session: String?, val roles: Set<String>)

// A filter folded under a context: decided for every row, a predicate over
// the row's columns, or — where the context does not say — unknown.
private sealed class F {
    object True : F()

    object False : F()

    data class Rows(val p: Pred) : F()

    object Unknown : F()
}

/**
 * What one person holds of one table: the rows (`null`: every row) and the columns (`null`: all).
 */
public data class Held(val rows: List<Pred>?, val columns: List<String>?) {
    public val isWhole: Boolean
        get() = rows == null && columns == null
}

/**
 * `scope::Scopes`: every scope a module's procedures run, gathered by the table each hold names.
 */
public class Scopes(public val schema: Schema, public val holds: Map<String, List<Hold>>) {
    /** Whether the module has any scope at all. */
    public val isEmpty: Boolean
        get() = holds.isEmpty()

    /** The role names the scopes test, in order. */
    public fun roles(): java.util.SortedSet<String> {
        val out = sortedSetOf<String>(CodePointOrder)
        for (h in holds.values.flatten()) h.filter?.let { ScopeCheck.predRoles(it, out) }
        return out
    }

    public companion object {
        /** `Scopes::of`: the scopes some procedure lists in its `uses`, each once. */
        public fun of(m: Module): Scopes {
            val used = m.functions.filter { it.kind.isProcedure }.flatMap { it.uses }.toSet()
            val holds = java.util.TreeMap<String, MutableList<Hold>>(CodePointOrder)
            for (f in m.functions) {
                if (f.kind != FnKind.Scope || f.name !in used) continue
                for (h in f.holds) holds.getOrPut(h.table, ::ArrayList).add(h)
            }
            return Scopes(m.schema, holds)
        }
    }
}

public object ScopeCheck {
    // A context expression, folded: a value where the context decides it.
    internal fun value(e: Expr, ctx: FoldCtx): Value? =
        when (e) {
            is Expr.Lit -> e.v
            is Expr.CtxUser -> ctx.user?.let { Value.VText(it) }
            is Expr.CtxSession -> ctx.session?.let { Value.VText(it) }
            is Expr.HasRole -> Value.VBool(e.role in ctx.roles)
            is Expr.OpE ->
                when (e.op) {
                    Op.Not ->
                        e.args.singleOrNull()?.let { x ->
                            value(x, ctx)?.let { Value.VBool(it == Value.VBool(false)) }
                        }
                    Op.And -> {
                        val vs = e.args.map { value(it, ctx) }
                        when {
                            Value.VBool(false) in vs -> Value.VBool(false)
                            vs.all { it == Value.VBool(true) } -> Value.VBool(true)
                            else -> null
                        }
                    }
                    Op.Or -> {
                        val vs = e.args.map { value(it, ctx) }
                        when {
                            Value.VBool(true) in vs -> Value.VBool(true)
                            vs.all { it == Value.VBool(false) } -> Value.VBool(false)
                            else -> null
                        }
                    }
                    else -> null
                }
            is Expr.CmpE -> {
                val a = value(e.l, ctx)
                val b = a?.let { value(e.r, ctx) }
                if (a != null && b != null) Value.VBool(cmp(e.op, a, b)) else null
            }
            else -> null
        }

    private fun fold(p: Pred, ctx: FoldCtx): F {
        // A comparison of a column is over rows whatever the context.
        fun rhs(e: Expr): Expr = value(e, ctx)?.let { Expr.Lit(it) } ?: e
        return when (p) {
            is Pred.Cmp -> F.Rows(Pred.Cmp(p.column, p.op, rhs(p.e)))
            is Pred.In -> F.Rows(Pred.In(p.column, p.items.map(::rhs)))
            is Pred.Has -> F.Rows(Pred.Has(p.column, rhs(p.e)))
            is Pred.When ->
                when (value(p.e, ctx)) {
                    Value.VBool(true) -> F.True
                    null -> F.Unknown
                    else -> F.False
                }
            is Pred.All -> {
                val rows = ArrayList<Pred>()
                var unknown = false
                for (q in p.items) {
                    when (val f = fold(q, ctx)) {
                        is F.False -> return F.False
                        is F.True -> {}
                        is F.Rows -> rows.add(f.p)
                        is F.Unknown -> unknown = true
                    }
                }
                when {
                    unknown -> F.Unknown
                    rows.isEmpty() -> F.True
                    rows.size == 1 -> F.Rows(rows[0])
                    else -> F.Rows(Pred.All(rows))
                }
            }
            is Pred.AnyP -> {
                val rows = ArrayList<Pred>()
                var unknown = false
                for (q in p.items) {
                    when (val f = fold(q, ctx)) {
                        is F.True -> return F.True
                        is F.False -> {}
                        is F.Rows -> rows.add(f.p)
                        is F.Unknown -> unknown = true
                    }
                }
                // An unknown alternative beside known rows: the rows are a lower bound.
                when {
                    rows.isEmpty() && unknown -> F.Unknown
                    rows.isEmpty() -> F.False
                    rows.size == 1 -> F.Rows(rows[0])
                    else -> F.Rows(Pred.AnyP(rows))
                }
            }
            is Pred.Not ->
                when (val f = fold(p.e, ctx)) {
                    is F.True -> F.False
                    is F.False -> F.True
                    is F.Rows -> F.Rows(Pred.Not(f.p))
                    is F.Unknown -> F.Unknown
                }
            is Pred.Exists ->
                when (val f = fold(p.pred, ctx)) {
                    is F.False -> F.False
                    is F.True -> F.Rows(Pred.Exists(p.table, p.column, Pred.All(emptyList())))
                    is F.Rows -> F.Rows(Pred.Exists(p.table, p.column, f.p))
                    is F.Unknown -> F.Unknown
                }
        }
    }

    // `union`: the holds naming one table under a context — the rows of every
    // contributing hold, and their columns.
    internal fun union(t: Table, hs: List<Hold>, ctx: FoldCtx): Held {
        var rows: MutableList<Pred>? = ArrayList()
        val cols = HashSet<String>()
        var allCols = false
        var any = false
        for (h in hs) {
            when (val f = h.filter?.let { fold(it, ctx) } ?: F.True) {
                is F.False,
                is F.Unknown -> continue
                is F.True -> rows = null
                is F.Rows -> rows?.add(f.p)
            }
            any = true
            when (val c = h.columns) {
                is Projection.All -> allCols = true
                else -> t.columns.forEach { if (c.keeps(it.name)) cols.add(it.name) }
            }
        }
        // Nothing contributes: no row, so every column — there is none to show.
        if (!any || allCols) return Held(rows, null)
        cols.addAll(t.key)
        val columns = t.columns.map { it.name }.filter { it in cols }
        return Held(rows, if (columns.size < t.columns.size) columns else null)
    }

    internal fun predRoles(p: Pred, out: MutableSet<String>) {
        when (p) {
            is Pred.When -> exprRoles(p.e, out)
            is Pred.Cmp -> exprRoles(p.e, out)
            is Pred.Has -> exprRoles(p.e, out)
            is Pred.In -> p.items.forEach { exprRoles(it, out) }
            is Pred.All -> p.items.forEach { predRoles(it, out) }
            is Pred.AnyP -> p.items.forEach { predRoles(it, out) }
            is Pred.Not -> predRoles(p.e, out)
            is Pred.Exists -> predRoles(p.pred, out)
        }
    }

    private fun exprRoles(e: Expr, out: MutableSet<String>) {
        when (e) {
            is Expr.HasRole -> out.add(e.role)
            is Expr.OpE -> e.args.forEach { exprRoles(it, out) }
            is Expr.CmpE -> {
                exprRoles(e.l, out)
                exprRoles(e.r, out)
            }
            else -> {}
        }
    }

    // Role sets at most this many names make: past it, each role alone and none at all.
    private const val POWERSET_UP_TO = 10

    /**
     * `scope::check`: for every role set the module names, the columns each client-run part of each
     * procedure names must be held — or `Complaint.NotHeld` — unless a guard of the procedure
     * refuses on every path for that set.
     */
    internal fun check(
        m: Module,
        named: Map<String, Set<Pair<String, String>>>
    ): List<VerifyError> {
        val scopes = Scopes.of(m)
        if (scopes.isEmpty) return emptyList()
        val nameSet = scopes.roles()
        for (f in m.functions) if (f.kind == FnKind.Guard) stmtsRoles(f.body, nameSet)
        val names = nameSet.toList()
        val sets: List<Set<String>> =
            if (names.size <= POWERSET_UP_TO) {
                (0 until (1 shl names.size)).map { bits ->
                    names
                        .filterIndexed { i, _ -> bits and (1 shl i) != 0 }
                        .toSortedSet(CodePointOrder)
                }
            } else {
                listOf(sortedSetOf<String>(CodePointOrder)) +
                    names.map { sortedSetOf(CodePointOrder, it) }
            }
        val unions =
            sets.map { roles ->
                val ctx = FoldCtx(null, null, roles)
                m.schema.tables.associate { t ->
                    t.name to scopes.holds[t.name]?.let { union(t, it, ctx).columns?.toSet() }
                }
            }
        val out = ArrayList<VerifyError>()
        val byPair =
            compareBy<Pair<String, String>, String>(CodePointOrder) { it.first }
                .thenComparing({ it.second }, CodePointOrder)
        for (p in m.functions) {
            if (!p.kind.isProcedure) continue
            // What a client runs is the stripped procedure (D3).
            val c = Hash.closure(m, strip(p))
            val parts = (listOf(c.function) + c.helpers).filter { it.kind != FnKind.Scope }
            val guards =
                p.uses.mapNotNull { m.lookupFunction(it) }.filter { it.kind == FnKind.Guard }
            val wanted = java.util.TreeSet(byPair)
            for (f in parts) named[f.name]?.let { wanted.addAll(it) }
            val reported = HashSet<Pair<String, String>>()
            for ((roles, held) in sets.zip(unions)) {
                if (guards.any { refuses(it.body, roles) }) continue
                for (tc in wanted) {
                    val (t, col) = tc
                    val ok = !held.containsKey(t) || held[t]?.contains(col) ?: true
                    if (!ok && reported.add(tc))
                        out.add(VerifyError.In(p.name, Complaint.NotHeld(t, col, roles.toList())))
                }
            }
        }
        return out
    }

    private fun stmtsRoles(b: Block, out: MutableSet<String>) {
        for (s in b) {
            when (s) {
                is Stmt.If -> {
                    exprRoles(s.c, out)
                    stmtsRoles(s.then, out)
                    stmtsRoles(s.els, out)
                }
                is Stmt.Let -> exprRoles(s.e, out)
                is Stmt.Refuse -> exprRoles(s.e, out)
                else -> {}
            }
        }
    }

    // Whether a guard's body refuses on every path once its `has_role`s are folded for `roles`.
    private fun refuses(b: Block, roles: Set<String>): Boolean {
        val ctx = FoldCtx(null, null, roles)
        for (s in b) {
            when (s) {
                is Stmt.Refuse -> return true
                is Stmt.If ->
                    when (value(s.c, ctx)) {
                        Value.VBool(true) -> if (refuses(s.then, roles)) return true
                        Value.VBool(false) -> if (refuses(s.els, roles)) return true
                        null -> if (refuses(s.then, roles) && refuses(s.els, roles)) return true
                        else -> {}
                    }
                is Stmt.Return -> return false
                else -> {}
            }
        }
        return false
    }
}
