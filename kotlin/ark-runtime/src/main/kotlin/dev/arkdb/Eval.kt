// §6 Evaluation (Ark.Eval): what a native procedure must mean, as an
// interpreter of IR closures. Evaluation order is part of the meaning.
package dev.arkdb

/** Who authored an entry: the user the authority verified, and the login it was authored under. */
public data class Ctx(val user: String, val session: String)

public object Eval {
    /** The answer to applying a mutator: the store after it with its changes, or the verdict. */
    public sealed class Applied {
        public data class Ok(val store: MemoryStore, val changes: List<Change>) : Applied()
        public data class Refused(val refusal: Refusal) : Applied()
    }

    /** The answer to a query: its value, or the refusal a check or a middleware gave. */
    public sealed class Answer {
        public data class Ok(val value: Value) : Answer()
        public data class Refused(val refusal: Refusal) : Answer()

        /** The value, or the refusal thrown as `Fault.Refuse`. */
        public fun orThrow(): Value = when (this) {
            is Ok -> value
            is Refused -> throw Fault.Refuse(refusal)
        }
    }

    /**
     * §1.3 (AUTHORING.md) What the form validator says about a partial input:
     * each failing field's first message, in input order (`""` names a
     * whole-input refinement), and every present field's normalised value.
     */
    public data class Checked(val messages: List<Pair<String, String>>, val values: Args) {
        public val ok: Boolean get() = messages.isEmpty()

        public fun messageFor(field: String): String? = messages.firstOrNull { it.first == field }?.second
    }

    // Why a block stopped: a return. Faults are thrown as `Fault`.
    private class Returned(val value: Value?) : RuntimeException(null, null, false, false)

    private class Env(
        val schema: Schema,
        val helpers: List<Function>,
        val kind: FnKind,
        val ctx: Ctx,
        val args: Args,
        val autos: Args,
        val locals: Map<Sym, Value>,
        val provided: Map<String, Value> = emptyMap(),
    ) {
        fun bind(x: Sym, v: Value): Env = Env(schema, helpers, kind, ctx, args, autos, locals + (x to v), provided)

        fun withArgs(a: Args): Env = Env(schema, helpers, kind, ctx, a, autos, locals, provided)
    }

    private fun bug(text: String): Nothing = throw Fault.Bug(text)

    private fun verdict(r: Refusal): Nothing = throw Fault.Refuse(r)

    /**
     * §6.1 Apply a mutator to a store. On a verdict the store is unchanged;
     * the whole entry rolls back. Bugs are thrown as `Fault.Bug`.
     */
    public fun apply(m: Module, name: String, ctx: Ctx, autos: Args, args: Args, st: MemoryStore): Applied =
        applyClosure(m.schema, Hash.closure(m, function(m, name)), ctx, autos, args, st)

    /**
     * A mutator's closure: its input checked and normalised (§1.3), each
     * middleware in its `uses` order (§1.2), then the body — all against one
     * fork of the store, so a refusal anywhere leaves `st` untouched.
     */
    public fun applyClosure(sch: Schema, c: Closure, ctx: Ctx, autos: Args, args: Args, st: MemoryStore): Applied {
        val fn = c.fn
        if (fn.kind != FnKind.Mutator) bug("WrongKind ${HsShow.text(fn.name)} ${fn.kind}")
        for ((a, _) in fn.autos) if (a !in autos) bug("MissingAuto ${HsShow.text(a)}")
        for ((a, _) in fn.input) if (a !in args) bug("MissingArg ${HsShow.text(a)}")
        val work = st.fork()
        try {
            val env = prelude(Env(sch, c.helpers, FnKind.Mutator, ctx, args, autos, emptyMap()), fn, work)
            block(env, fn.body, work)
        } catch (r: Returned) {
            // a mutator's return ends it
        } catch (f: Fault.Refuse) {
            return Applied.Refused(f.refusal)
        }
        return Applied.Ok(work, work.changes.toList())
    }

    /** §6.2 Run a query; never changes the store. A check, a middleware or a fault such as an overflow may refuse it. */
    public fun query(m: Module, name: String, ctx: Ctx, args: Args, st: MemoryStore): Answer =
        queryClosure(m.schema, Hash.closure(m, function(m, name)), ctx, args, st)

    public fun queryClosure(sch: Schema, c: Closure, ctx: Ctx, args: Args, st: MemoryStore): Answer {
        val fn = c.fn
        if (fn.kind != FnKind.Query) bug("WrongKind ${HsShow.text(fn.name)} ${fn.kind}")
        for ((a, _) in fn.input) if (a !in args) bug("MissingArg ${HsShow.text(a)}")
        val work = st.fork()
        try {
            val env = prelude(Env(sch, c.helpers, FnKind.Query, ctx, args, emptyMap(), emptyMap()), fn, work)
            block(env, fn.body, work)
        } catch (r: Returned) {
            return Answer.Ok(r.value ?: bug("NoReturn ${HsShow.text(fn.name)}"))
        } catch (f: Fault.Refuse) {
            return Answer.Refused(f.refusal)
        }
        bug("NoReturn ${HsShow.text(fn.name)}")
    }

    /** Run a helper on its arguments, with no store at all. */
    public fun evalHelper(m: Module, name: String, vals: List<Value>): Value {
        val fn = function(m, name)
        val c = Hash.closure(m, fn)
        val env = Env(m.schema, c.helpers, FnKind.Helper, Ctx("", ""), emptyMap(), emptyMap(), emptyMap())
        return call(env, fn, vals, MemoryStore(m.schema))
    }

    private fun function(m: Module, name: String): Function =
        m.lookupFunction(name) ?: bug("UnknownFunction ${HsShow.text(name)}")

    // §1.3 Input checks, and §1.2 middleware ----------------------------------

    /** The default message of a failing check (`Ark.Eval.defaultMessage`); `table` is the id's table, for `exists`. */
    public fun defaultMessage(field: String, c: Check, table: String? = null): String = when (c) {
        is Check.Trim -> "$field: invalid"
        is Check.MinLen -> "$field: at least ${c.n} characters"
        is Check.MaxLen -> "$field: at most ${c.n} characters"
        is Check.Range -> when {
            c.lo != null && c.hi != null -> "$field: between ${c.lo} and ${c.hi}"
            c.lo != null -> "$field: at least ${c.lo}"
            c.hi != null -> "$field: at most ${c.hi}"
            else -> "$field: invalid"
        }
        is Check.NonEmpty -> "$field: at least one"
        is Check.Exists -> "$field: no such ${table ?: "row"}"
        is Check.Refine -> "$field: invalid"
    }

    /** The default message of a failing whole-input refinement. */
    public const val DEFAULT_REFINE_MESSAGE: String = "invalid"

    /** The table an id field names, through an option. */
    public fun idTable(t: Ty): String? = when (t) {
        is Ty.TId -> t.table
        is Ty.TOption -> idTable(t.of)
        else -> null
    }

    // One field's checks against its value: the value after any trims, or the
    // message of the first check that failed. `None` passes untouched.
    private fun checkField(env: Env, name: String, f: Field, v0: Value, st: Store): Pair<Value, String?> {
        var v = v0
        if (v is Value.VNull) return v to null
        for (c in f.checks) {
            val ok = when (c) {
                is Check.Trim -> {
                    v = Std.trim(v)
                    true
                }
                is Check.MinLen -> textLen(v) >= c.n
                is Check.MaxLen -> textLen(v) <= c.n
                is Check.Range -> {
                    val n = int(v)
                    (c.lo == null || n >= c.lo) && (c.hi == null || n <= c.hi)
                }
                is Check.NonEmpty -> list(v).isNotEmpty()
                is Check.Exists -> {
                    val t = idTable(f.ty) ?: bug("TypeError ${HsShow.text("exists on a field that is not an id")}")
                    bool(st.exists(t, listOf(v)))
                }
                is Check.Refine -> bool(eval(env.withArgs(env.args + (name to v)), c.e, st))
            }
            if (!ok) return v to (c.why ?: defaultMessage(name, c, idTable(f.ty)))
        }
        return v to null
    }

    private fun textLen(v: Value): Long = int(Std.textLen(v))

    // Checks, then middleware: the environment the body runs in, with the
    // input normalised and every provided value bound.
    private fun prelude(env0: Env, fn: Function, st: Store): Env {
        val args = LinkedHashMap(env0.args)
        for ((name, f) in fn.input) {
            val (v, why) = checkField(env0.withArgs(args), name, f, args.getValue(name), st)
            if (why != null) verdict(Refusal.Refused(why))
            args[name] = v
        }
        var env = env0.withArgs(args)
        for ((e, why) in fn.refine) {
            if (!bool(eval(env, e, st))) verdict(Refusal.Refused(why ?: DEFAULT_REFINE_MESSAGE))
        }
        val provided = LinkedHashMap<String, Value>()
        for (u in fn.uses) {
            val mw = env.helpers.firstOrNull { it.name == u } ?: bug("UnknownFunction ${HsShow.text(u)}")
            if (!mw.kind.isMiddleware) bug("WrongKind ${HsShow.text(u)} ${mw.kind}")
            val menv = Env(env.schema, env.helpers, mw.kind, env.ctx, args, emptyMap(), emptyMap(), provided.toMap())
            try {
                block(menv, mw.body, st)
                if (mw.kind == FnKind.Provide) bug("NoReturn ${HsShow.text(mw.name)}")
            } catch (r: Returned) {
                if (mw.kind == FnKind.Provide) provided[u] = r.value ?: bug("NoReturn ${HsShow.text(mw.name)}")
            }
        }
        env = Env(env.schema, env.helpers, env.kind, env.ctx, args, env.autos, env.locals, provided)
        return env
    }

    /**
     * §1.3 The form validator: every present field's checks, `trim` first
     * where it is, and the whole-input refinements only when every field is
     * present and passed. Nothing is written; `ctx` is only for a refinement
     * that reads it.
     */
    public fun check(sch: Schema, c: Closure, partial: Args, st: Store, ctx: Ctx = Ctx("", "")): Checked {
        val fn = c.fn
        val work: Store = (st as? MemoryStore)?.fork() ?: st
        val env = Env(sch, c.helpers, FnKind.Query, ctx, partial, emptyMap(), emptyMap())
        val values = LinkedHashMap<String, Value>()
        val messages = ArrayList<Pair<String, String>>()
        for ((name, f) in fn.input) {
            val v0 = partial[name] ?: continue
            try {
                val (v, why) = checkField(env.withArgs(partial + values), name, f, v0, work)
                values[name] = v
                if (why != null) messages.add(name to why)
            } catch (r: Fault.Refuse) {
                messages.add(name to r.refusal.text)
            }
        }
        if (messages.isEmpty() && fn.input.all { it.first in values }) {
            for ((e, why) in fn.refine) {
                val ok = try {
                    bool(eval(env.withArgs(values), e, work))
                } catch (r: Fault.Refuse) {
                    false
                }
                if (!ok) messages.add("" to (why ?: DEFAULT_REFINE_MESSAGE))
            }
        }
        return Checked(messages, values)
    }

    // §6.3 Statements -------------------------------------------------------

    private fun block(env0: Env, stmts: List<Stmt>, st: Store): Env {
        var env = env0
        for (s in stmts) env = exec(env, s, st)
        return env
    }

    private fun exec(env: Env, s: Stmt, st: Store): Env {
        when (s) {
            is Stmt.Let -> return env.bind(s.sym, eval(env, s.e, st))
            is Stmt.If -> {
                val b = bool(eval(env, s.c, st))
                block(env, if (b) s.then else s.els, st)
                return env
            }
            is Stmt.For -> {
                val vs = list(eval(env, s.over, st))
                for (v in vs) block(env.bind(s.sym, v), s.body, st)
                return env
            }
            is Stmt.Insert -> {
                mutating(env)
                Writes.insert(st, s.table, struct(eval(env, s.row, st)), s.on)
                return env
            }
            is Stmt.Upsert -> {
                mutating(env)
                Writes.upsert(st, s.table, struct(eval(env, s.row, st)), s.on)
                return env
            }
            is Stmt.Update -> {
                mutating(env)
                val key = s.key.map { eval(env, it, st) }
                Writes.update(st, s.table, key) { old -> struct(eval(env.bind(s.sym, old), s.row, st)) }
                return env
            }
            is Stmt.Delete -> {
                mutating(env)
                val key = s.key.map { eval(env, it, st) }
                st.delete(s.table, key)
                return env
            }
            is Stmt.Refuse -> {
                if (env.kind != FnKind.Mutator && !env.kind.isMiddleware) {
                    bug("Impure ${HsShow.text("refuse outside a mutator")}")
                }
                val t = text(eval(env, s.e, st))
                verdict(Refusal.Refused(t))
            }
            is Stmt.Return -> throw Returned(s.e?.let { eval(env, it, st) })
        }
    }

    private fun mutating(env: Env) {
        if (env.kind != FnKind.Mutator) bug("Impure ${HsShow.text("write outside a mutator")}")
    }

    private fun reading(env: Env) {
        if (env.kind == FnKind.Helper) bug("Impure ${HsShow.text("read inside a helper")}")
    }

    // §6.4 Expressions ------------------------------------------------------

    private fun eval(env: Env, e: Expr, st: Store): Value = when (e) {
        is Expr.Lit -> e.v
        is Expr.Arg -> env.args[e.name] ?: bug("MissingArg ${HsShow.text(e.name)}")
        is Expr.Auto -> env.autos[e.name] ?: bug("MissingAuto ${HsShow.text(e.name)}")
        is Expr.Var -> env.locals[e.sym] ?: bug("UnboundVar ${e.sym}")
        is Expr.CtxUser -> Value.VText(env.ctx.user)
        is Expr.CtxSession -> Value.VText(env.ctx.session)
        is Expr.Field -> {
            val m = struct(eval(env, e.e, st))
            m.fields[e.name] ?: bug("NoSuchField ${HsShow.text(e.name)}")
        }
        // Fields are evaluated in field-name order, which is the map's order.
        is Expr.Struct -> {
            val out = LinkedHashMap<String, Value>()
            for ((k, x) in e.fields) out[k] = eval(env, x, st)
            Value.VStruct(out)
        }
        is Expr.ListE -> Value.VList(e.items.map { eval(env, it, st) })
        // An option is flat: `Some v` is `v` and `None` is `VNull`.
        is Expr.Some -> eval(env, e.e, st)
        is Expr.None -> Value.VNull
        is Expr.Match -> {
            val v = eval(env, e.e, st)
            if (v.isNull()) eval(env, e.none, st) else eval(env.bind(e.sym, v), e.some, st)
        }
        is Expr.If -> {
            val t = bool(eval(env, e.c, st))
            eval(env, if (t) e.then else e.els, st)
        }
        is Expr.Op -> op(env, e, st)
        is Expr.Cmp -> {
            val x = eval(env, e.l, st)
            val y = eval(env, e.r, st)
            Value.VBool(e.op.holds(compareValue(x, y)))
        }
        is Expr.Call -> {
            val vals = e.args.map { eval(env, it, st) }
            val fn = env.helpers.firstOrNull { it.name == e.fn } ?: bug("UnknownFunction ${HsShow.text(e.fn)}")
            if (fn.kind != FnKind.Helper) bug("WrongKind ${HsShow.text(e.fn)} ${fn.kind}")
            call(env, fn, vals, st)
        }
        is Expr.Std -> {
            val vals = e.args.map { eval(env, it, st) }
            Std.call(e.fn, vals)
        }
        is Expr.Map -> {
            val vs = list(eval(env, e.over, st))
            Value.VList(vs.map { eval(env.bind(e.sym, it), e.body, st) })
        }
        is Expr.Filter -> {
            val vs = list(eval(env, e.over, st))
            Value.VList(vs.filter { bool(eval(env.bind(e.sym, it), e.body, st)) })
        }
        is Expr.Any -> {
            val vs = list(eval(env, e.over, st))
            // Every element is evaluated (`or <$> mapM`), so every fault is seen.
            var r = false
            for (v in vs) if (bool(eval(env.bind(e.sym, v), e.body, st))) r = true
            Value.VBool(r)
        }
        is Expr.All -> {
            val vs = list(eval(env, e.over, st))
            var r = true
            for (v in vs) if (!bool(eval(env.bind(e.sym, v), e.body, st))) r = false
            Value.VBool(r)
        }
        // Stable, under `compareValue` of the key.
        is Expr.SortBy -> {
            val vs = list(eval(env, e.over, st))
            val keyed = vs.map { it to eval(env.bind(e.sym, it), e.key, st) }
            Value.VList(keyed.sortedWith { p, q -> compareValue(p.second, q.second) }.map { it.first })
        }
        is Expr.Fold -> {
            val vs = list(eval(env, e.over, st))
            var acc = eval(env, e.init, st)
            for (v in vs) acc = eval(env.bind(e.acc, acc).bind(e.sym, v), e.body, st)
            acc
        }
        is Expr.Select -> {
            reading(env)
            Value.VList(select(env, e.plan, st))
        }
        is Expr.Get -> {
            reading(env)
            val key = e.key.map { eval(env, it, st) }
            st.get(e.table, key)
        }
        is Expr.Exists -> {
            reading(env)
            val key = e.key.map { eval(env, it, st) }
            st.exists(e.table, key)
        }
        is Expr.Provided -> env.provided[e.fn] ?: bug("NotProvided ${HsShow.text(e.fn)}")
    }

    private fun op(env: Env, e: Expr.Op, st: Store): Value {
        val es = e.args
        return when {
            e.op == Op.And && es.size == 2 -> {
                val x = bool(eval(env, es[0], st))
                if (x) eval(env, es[1], st) else Value.VBool(false)
            }
            e.op == Op.Or && es.size == 2 -> {
                val x = bool(eval(env, es[0], st))
                if (x) Value.VBool(true) else eval(env, es[1], st)
            }
            e.op == Op.Not && es.size == 1 -> Value.VBool(!bool(eval(env, es[0], st)))
            e.op == Op.Neg && es.size == 1 -> {
                val n = int(eval(env, es[0], st))
                if (n == Long.MIN_VALUE) verdict(Refusal.Refused("integer overflow"))
                Value.VInt(-n)
            }
            es.size == 2 && e.op in setOf(Op.Add, Op.Sub, Op.Mul, Op.Div, Op.Mod) -> {
                val x = Value.VInt(int(eval(env, es[0], st)))
                val y = Value.VInt(int(eval(env, es[1], st)))
                when (e.op) {
                    Op.Add -> Ops.add(x, y)
                    Op.Sub -> Ops.sub(x, y)
                    Op.Mul -> Ops.mul(x, y)
                    Op.Div -> Ops.div(x, y)
                    else -> Ops.mod(x, y)
                }
            }
            else -> bug("Arity ${HsShow.text(e.op.name)}")
        }
    }

    // Call a helper: a fresh environment with its arguments and nothing of
    // its caller's; what it returned.
    private fun call(env: Env, fn: Function, vals: List<Value>, st: Store): Value {
        if (vals.size != fn.input.size) bug("Arity ${HsShow.text(fn.name)}")
        val env2 = Env(env.schema, env.helpers, FnKind.Helper, env.ctx, fn.input.map { it.first }.zip(vals).toMap(), emptyMap(), emptyMap())
        try {
            block(env2, fn.body, st)
        } catch (r: Returned) {
            return r.value ?: bug("NoReturn ${HsShow.text(fn.name)}")
        }
        bug("NoReturn ${HsShow.text(fn.name)}")
    }

    // §6.6 Select ------------------------------------------------------------

    // Scan, filter, sort stably, take, attach. A child plan runs once per
    // parent with the join column pinned, so its right-hand sides are
    // evaluated per parent and a child limit is per parent.
    private fun select(env: Env, p: IR.Plan, st: Store): List<Value> {
        val tbl = env.schema.lookupTable(p.table) ?: bug("UnknownTable ${HsShow.text(p.table)}")
        val keep = p.filter?.let { predicate(env, it, st) }
        val admitted = st.scan(p.table).filter { keep == null || keep.admits(it) }
        val ordered = admitted.sortedWith(Select.orderBy(p.order))
        val taken = if (p.limit != null) ordered.take(p.limit) else ordered
        return taken.map { attach(env, tbl, p.related, it, st) }
    }

    private fun attach(env: Env, tbl: Table, rels: List<IR.Related>, row: Row, st: Store): Value {
        val key = tbl.keyOf(row)
        val pk = when {
            key.size == 1 -> key[0]
            rels.isEmpty() -> Value.VNull
            else -> bug("CompositeParentKey ${HsShow.text(tbl.name)}")
        }
        if (rels.isEmpty()) return row
        val fields = LinkedHashMap<String, Value>(row.fields)
        for (r in rels) {
            val pin = IR.Pred.Cmp(r.relation.column, CmpOp.Eq, Expr.Lit(pk))
            val f = r.plan.filter
            val child = r.plan.copy(filter = if (f == null) pin else IR.Pred.All(listOf(pin, f)))
            fields[r.name] = Value.VList(select(env, child, st))
        }
        return Value.VStruct(fields)
    }

    // The right-hand sides of a filter are evaluated once, before the scan.
    private fun predicate(env: Env, p: IR.Pred, st: Store): Pred = when (p) {
        is IR.Pred.Cmp -> Pred.Cmp(p.column, p.op, eval(env, p.e, st))
        is IR.Pred.In -> Pred.In(p.column, p.items.map { eval(env, it, st) })
        is IR.Pred.All -> Pred.All(p.items.map { predicate(env, it, st) })
        is IR.Pred.Any -> Pred.Any(p.items.map { predicate(env, it, st) })
        is IR.Pred.Not -> Pred.Not(predicate(env, p.e, st))
    }

    /** §13.1 A plan with every right-hand side evaluated, for a view to maintain. */
    public fun evalPlan(sch: Schema, c: Closure, args: Args, p: IR.Plan, st: MemoryStore): Plan {
        val env = Env(sch, c.helpers, FnKind.Query, Ctx("", ""), args, emptyMap(), emptyMap())
        return evalPlanIn(env, p, st)
    }

    private fun evalPlanIn(env: Env, p: IR.Plan, st: Store): Plan = Plan(
        p.table,
        p.filter?.let { predicate(env, it, st) },
        p.order,
        p.limit,
        p.related.map { r -> Related(r.name, r.relation, evalPlanIn(env, r.plan, st)) },
    )

    // Coercions: a bug when the verifier's type does not hold --------------

    private fun bool(v: Value): Boolean = (v as? Value.VBool)?.value
        ?: bug("TypeError ${HsShow.text("expected Bool, got " + v.show().take(60))}")

    private fun int(v: Value): Long = (v as? Value.VInt)?.value
        ?: bug("TypeError ${HsShow.text("expected Int, got " + v.show().take(60))}")

    private fun text(v: Value): String = (v as? Value.VText)?.value
        ?: bug("TypeError ${HsShow.text("expected Text, got " + v.show().take(60))}")

    private fun list(v: Value): List<Value> = (v as? Value.VList)?.items
        ?: bug("TypeError ${HsShow.text("expected List, got " + v.show().take(60))}")

    private fun struct(v: Value): Value.VStruct = (v as? Value.VStruct)
        ?: bug("TypeError ${HsShow.text("expected Struct, got " + v.show().take(60))}")
}
