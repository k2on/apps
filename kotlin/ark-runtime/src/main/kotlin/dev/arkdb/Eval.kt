// §6 Evaluation: what a domain means (`rust/ark/src/eval.rs`). An
// interpreter of the IR over any `Store`. A read — a query's plan, a
// mutator's `select` — means what `View.pull` answers, and is asked of it
// through `View.read`: that is the one evaluator of plans, and it
// evaluates the expressions inside one through `Scope`, which is this
// file's evaluator with the plan's binders in scope.
//
// A procedure runs as the spec has it: its input checked field by field,
// then each middleware in its `uses` order, then the body — one
// transaction, and a refusal anywhere leaves the store untouched. A
// `Refusal` is a verdict every replica reaches; an `EvalError` is a bug.
// Evaluation order is part of the meaning: `and`/`or` short-circuit left
// to right, `if` and `match` evaluate only the taken arm, list elements
// and call arguments left to right, a struct's fields in field-name order.
package dev.arkdb

import java.math.BigInteger

/**
 * Who authored an entry (`eval::Ctx`): the user the authority verified, the login, the roles frozen
 * in the entry, and whether this run is the authority's — which alone runs private blocks (D3).
 */
public data class Ctx(
    val user: String,
    val session: String,
    val roles: Set<String> = emptySet(),
    val authority: Boolean = false,
) {
    /** Whether this is `Ctx.nobody`. */
    public val isNobody: Boolean
        get() = user.isEmpty() && session.isEmpty()

    /** The same author, holding these roles. */
    public fun withRoles(roles: Collection<String>): Ctx =
        copy(roles = roles.toSortedSet(CodePointOrder))

    /** The same author, run as the authority runs it. */
    public fun asAuthority(authority: Boolean): Ctx = copy(authority = authority)

    public companion object {
        /** §11.2a Who authors before anyone has signed in. */
        public val nobody: Ctx = Ctx("", "")
    }
}

/** A bug, never a verdict (`eval::EvalError`). */
public sealed class EvalError {
    public data class UnknownFunction(val name: String) : EvalError()

    public data class WrongKind(val name: String, val kind: FnKind) : EvalError()

    public data class MissingArg(val name: String) : EvalError()

    public data class MissingAuto(val name: String) : EvalError()

    public data class UnboundVar(val sym: Sym) : EvalError()

    public data class NoSuchField(val name: String) : EvalError()

    public data class TypeError(val what: String) : EvalError()

    public data class Arity(val what: String) : EvalError()

    public data class NoReturn(val name: String) : EvalError()

    public data class Impure(val what: String) : EvalError()

    public data class UnknownTable(val table: String) : EvalError()

    public data class CompositeParentKey(val table: String) : EvalError()

    public data class NotProvided(val name: String) : EvalError()
}

/** The two ways a computation stops short (`eval::EvalFault`), thrown. */
public sealed class EvalFault : Exception(null, null, false, false) {
    public class Verdict(public val refusal: Refusal) : EvalFault() {
        override val message: String
            get() = refusal.show()
    }

    public class Bug(public val error: EvalError) : EvalFault() {
        override val message: String
            get() = error.toString()
    }
}

/**
 * What a mutator run came to: its changes, applied, or the verdict that left the store as it was.
 */
public sealed class Ran {
    public data class Ok(val changes: List<Change>) : Ran()

    public data class Refused(val refusal: Refusal) : Ran()
}

/**
 * What the form validator says (`eval::Checked`): a message per field that fails, and the values.
 */
public data class Checked(val messages: List<Pair<String, String>>, val values: Map<String, Value>)

// §6.4 A local: one per binding, pushed over the one before.
internal class Frame(val sym: Sym, val value: Value, val up: Frame?)

// Everything an expression is evaluated in.
internal class Env(
    val schema: Schema,
    val helpers: List<Function>,
    val kind: FnKind,
    val ctx: Ctx,
    val args: Map<String, Value>,
    val autos: Map<String, Value>,
    val provided: Map<String, Value>,
    // A plan node's binders, under every frame.
    val node: List<Pair<Sym, Value>>,
    val locals: Frame?,
    // §13 In a plan's node (and a helper one calls), a sum's fold is judged on its result alone.
    val wide: Boolean,
) {
    fun local(x: Sym): Value? {
        var at = locals
        while (at != null) {
            if (at.sym == x) return at.value
            at = at.up
        }
        for (i in node.indices.reversed()) if (node[i].first == x) return node[i].second
        return null
    }

    // This scope with one more local. Binding one leaves the node's sum rule, as the reference's
    // does.
    fun under(sym: Sym, v: Value): Env =
        Env(schema, helpers, kind, ctx, args, autos, provided, node, Frame(sym, v, locals), false)

    fun withFrame(f: Frame): Env =
        Env(schema, helpers, kind, ctx, args, autos, provided, node, f, false)

    fun copy(
        kind: FnKind = this.kind,
        args: Map<String, Value> = this.args,
        node: List<Pair<Sym, Value>> = this.node,
        locals: Frame? = this.locals,
        wide: Boolean = this.wide,
        autos: Map<String, Value> = this.autos,
        provided: Map<String, Value> = this.provided,
    ): Env = Env(schema, helpers, kind, ctx, args, autos, provided, node, locals, wide)
}

// The store as it stands, the changes made so far, and the private blocks the
// authority's run reached, each with the locals it was written among.
private class St(val store: Store) {
    val changes = ArrayList<Change>()
    val deferred = ArrayList<Pair<Block, List<Pair<Sym, Value>>>>()
}

// A block that returned, with what.
private class Returned(val v: Value?)

// The store an expression that may not read runs against: empty, never written.
private class NoStore(override val schema: Schema) : Store {
    override fun get(table: String, key: List<Value>): Row? = null

    override fun scan(table: String): List<Row> = emptyList()

    override fun applyChange(change: Change) {}
}

/**
 * §1.3 What the expressions of a plan are evaluated in (`eval::Scope`): the function's helpers,
 * context, arguments and provided values, and the locals bound around the read. A filter is
 * evaluated here; everything a node computes in a `NodeScope` of it.
 */
public class Scope internal constructor(internal val outer: Env) {
    public val schema: Schema
        get() = outer.schema

    /** An expression constant for the read: a filter's right-hand side. */
    public fun eval(e: Expr): Value = Eval.pure(outer, e)

    /** A node's scope: nothing bound yet, no read, and a sum judged on its result. */
    public fun node(): NodeScope =
        NodeScope(outer.copy(kind = FnKind.Helper, node = emptyList(), locals = null, wide = true))

    public companion object {
        /** A scope with no locals: a query's, or a read outside any procedure. */
        public fun of(
            schema: Schema,
            helpers: List<Function>,
            ctx: Ctx,
            args: Map<String, Value>,
            provided: Map<String, Value>
        ): Scope =
            Scope(
                Env(
                    schema,
                    helpers,
                    FnKind.Helper,
                    ctx,
                    args,
                    emptyMap(),
                    provided,
                    emptyList(),
                    null,
                    false))
    }
}

/** One node's scope: its binders, bound as the plan evaluates them (`eval::NodeScope`). */
public class NodeScope internal constructor(private val env: Env) {
    private val bound = ArrayList<Pair<Sym, Value>>()

    public fun bind(x: Sym, v: Value) {
        bound.add(x to v)
    }

    public fun eval(e: Expr): Value = Eval.pure(env.copy(node = bound.toList()), e)
}

public object Eval {
    private fun bug(e: EvalError): Nothing = throw EvalFault.Bug(e)

    private fun verdict(r: Refusal): Nothing = throw EvalFault.Verdict(r)

    /** §6.1 `apply`: a mutator by name, through its current closure. */
    public fun apply(
        m: Module,
        name: String,
        ctx: Ctx,
        autos: Map<String, Value>,
        args: Map<String, Value>,
        store: Store
    ): Ran {
        val f = m.lookupFunction(name) ?: bug(EvalError.UnknownFunction(name))
        return applyClosure(m.schema, Hash.closure(m, f), ctx, autos, args, store)
    }

    /**
     * §6.1 `apply_closure`: how an entry replays. The changes, already applied to the store — or
     * the verdict, the store unchanged; a bug is thrown as `EvalFault.Bug`.
     */
    public fun applyClosure(
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        autos: Map<String, Value>,
        args: Map<String, Value>,
        store: Store
    ): Ran {
        val f = c.function
        if (f.kind != FnKind.Mutator) bug(EvalError.WrongKind(f.name, f.kind))
        for ((a, _) in f.autos) if (!autos.containsKey(a)) bug(EvalError.MissingAuto(a))
        val st = St(Overlay(store))
        try {
            procedure(st, sch, c, ctx, autos, args)
        } catch (v: EvalFault.Verdict) {
            return Ran.Refused(v.refusal)
        }
        store.applyChanges(st.changes)
        return Ran.Ok(st.changes)
    }

    /**
     * §6.2 `query_closure`: a query's value, or a fault (a verdict is shown and recorded nowhere).
     */
    public fun queryClosure(
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        args: Map<String, Value>,
        store: Store
    ): Value {
        val f = c.function
        if (f.kind != FnKind.Query) bug(EvalError.WrongKind(f.name, f.kind))
        return procedure(St(Overlay(store)), sch, c, ctx, emptyMap(), args)
            ?: bug(EvalError.NoReturn(f.name))
    }

    /** §6.2 `query_as`: a query by name, as someone. */
    public fun queryAs(
        m: Module,
        name: String,
        ctx: Ctx,
        args: Map<String, Value>,
        store: Store
    ): Value {
        val f = m.lookupFunction(name) ?: bug(EvalError.UnknownFunction(name))
        return queryClosure(m.schema, Hash.closure(m, f), ctx, args, store)
    }

    /**
     * §1.7 `middleware`: the input checked, then each middleware in `uses` order, over `store`: the
     * checked input and what each provide returned. Nothing is written.
     */
    public fun middleware(
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        args: Map<String, Value>,
        store: Store
    ): Pair<Map<String, Value>, Map<String, Value>> =
        preamble(St(Overlay(store)), sch, c, ctx, args)

    // A procedure, whole: preamble, then the body (or the plan), then — at the
    // authority only — the private blocks the body reached.
    private fun procedure(
        st: St,
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        autos: Map<String, Value>,
        args0: Map<String, Value>
    ): Value? {
        val f = c.function
        val (args, provided) = preamble(st, sch, c, ctx, args0)
        val env = Env(sch, c.helpers, f.kind, ctx, args, autos, provided, emptyList(), null, false)
        val plan = f.plan
        if (f.kind == FnKind.Query && plan != null)
            return Value.VList(View.read(sch, plan, Scope(env), st.store))
        val out = block(st, env, f.body)?.v
        val deferred = ArrayList(st.deferred)
        st.deferred.clear()
        for ((b, locals) in deferred) block(st, env.copy(node = locals), b)
        return out
    }

    // The input checked, then the middleware.
    private fun preamble(
        st: St,
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        args0: Map<String, Value>
    ): Pair<Map<String, Value>, Map<String, Value>> {
        val f = c.function
        val args = checkedInput(st, sch, c, ctx, args0)
        val provided = LinkedHashMap<String, Value>()
        for (u in f.uses) {
            val mw = c.helpers.firstOrNull { it.name == u } ?: bug(EvalError.UnknownFunction(u))
            if (!mw.kind.isMiddleware) bug(EvalError.WrongKind(u, mw.kind))
            // D2 A scope runs nothing.
            if (mw.kind == FnKind.Scope) continue
            val env =
                Env(
                    sch,
                    c.helpers,
                    mw.kind,
                    ctx,
                    args,
                    emptyMap(),
                    provided,
                    emptyList(),
                    null,
                    false)
            val r = block(st, env, mw.body)
            when {
                mw.kind == FnKind.Guard && (r == null || r.v == null) -> {}
                mw.kind == FnKind.Provide && r != null && r.v != null -> provided[u] = r.v
                else -> bug(EvalError.NoReturn(u))
            }
        }
        return args to provided
    }

    // §1.3 The input, decoded and checked.
    private fun checkedInput(
        st: St,
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        args0: Map<String, Value>
    ): Map<String, Value> {
        val f = c.function
        val args = LinkedHashMap(args0)
        for ((n, _) in f.input) if (!args.containsKey(n)) bug(EvalError.MissingArg(n))
        for ((n, fd) in f.input) {
            val v = args[n]!!
            val now = HashMap(args)
            val (v2, msg) =
                checkField(n, fd, v, { t, k -> st.store.exists(t, listOf(k)) }) { e, x ->
                    val local = HashMap(now)
                    local[n] = x
                    val env =
                        Env(
                            sch,
                            c.helpers,
                            FnKind.Helper,
                            ctx,
                            local,
                            emptyMap(),
                            emptyMap(),
                            emptyList(),
                            null,
                            false)
                    pureBool(St(Overlay(MemoryStore.empty(sch))), env, e)
                }
            if (msg != null) verdict(Refusal.Refused(msg))
            args[n] = v2
        }
        for ((e, why) in f.refine) {
            val env =
                Env(
                    sch,
                    c.helpers,
                    FnKind.Helper,
                    ctx,
                    args,
                    emptyMap(),
                    emptyMap(),
                    emptyList(),
                    null,
                    false)
            if (!pureBool(st, env, e)) verdict(Refusal.Refused(why ?: "invalid"))
        }
        return args
    }

    // A check's expression: pure, a Bool.
    private fun pureBool(st: St, env: Env, e: Expr): Boolean =
        when (val v = eval(st, env, e)) {
            is Value.VBool -> v.value
            else -> bug(EvalError.TypeError("a check is a Bool, not ${v.show()}"))
        }

    /** §1.3 `default_message`: the message a check refuses with when it names none. */
    public fun defaultMessage(field: String, check: Check, ty: Ty): String =
        when (check) {
            is Check.Trim -> "$field: invalid"
            is Check.MinLen -> "$field: at least ${check.n} characters"
            is Check.MaxLen -> "$field: at most ${check.n} characters"
            is Check.Range ->
                when {
                    check.lo != null && check.hi != null ->
                        "$field: between ${check.lo} and ${check.hi}"
                    check.lo != null -> "$field: at least ${check.lo}"
                    check.hi != null -> "$field: at most ${check.hi}"
                    else -> "$field: invalid"
                }
            is Check.NonEmpty -> "$field: at least one"
            is Check.Exists -> "$field: no such ${idTable(ty) ?: "row"}"
            is Check.Refine -> "$field: invalid"
        }

    /** `id_table`: the table an id field names, through an option. */
    public fun idTable(ty: Ty): String? =
        when (ty) {
            is Ty.TId -> ty.table
            is Ty.TOption -> idTable(ty.of)
            else -> null
        }

    /**
     * §1.3 `check_field`: one field's checks, in order — the value as normalised so far (a `trim`
     * rewrites it for every later check and the body) and the message of the first check that
     * fails. A field of an option type is checked only when it is `Some`.
     */
    public fun checkField(
        name: String,
        field: Field,
        value: Value,
        exists: (String, Value) -> Boolean,
        refine: (Expr, Value) -> Boolean,
    ): Pair<Value, String?> {
        if (value is Value.VNull && field.ty is Ty.TOption) return value to null
        fun bad(what: String): Nothing = bug(EvalError.TypeError("$name: $what"))
        var v = value
        for (c in field.checks) {
            val ok =
                when (c) {
                    is Check.Trim -> {
                        val t = v as? Value.VText ?: bad("trim of a value that is not text")
                        v = Value.VText(Std.trim(t.value))
                        true
                    }
                    is Check.MinLen,
                    is Check.MaxLen -> {
                        val t =
                            (v as? Value.VText ?: bad("a length check on a value that is not text"))
                                .value
                        val len = t.codePointCount(0, t.length).toLong()
                        if (c is Check.MinLen) len >= c.n else len <= (c as Check.MaxLen).n
                    }
                    is Check.Range -> {
                        val x =
                            (v as? Value.VInt ?: bad("a range on a value that is not an int")).value
                        (c.lo == null || x >= c.lo) && (c.hi == null || x <= c.hi)
                    }
                    is Check.NonEmpty ->
                        (v as? Value.VList ?: bad("non_empty on a value that is not a list"))
                            .items
                            .isNotEmpty()
                    is Check.Exists ->
                        exists(idTable(field.ty) ?: bad("exists on a value that is not an id"), v)
                    is Check.Refine -> refine(c.e, v)
                }
            if (!ok) return v to (c.given ?: defaultMessage(name, c, field.ty))
        }
        return v to null
    }

    /**
     * §1.3 `check`, the form validator: each field present through its checks, `trim` normalising;
     * the refinements over the whole input only when every field is present and passed. Nothing is
     * refused and nothing is written.
     */
    public fun check(
        sch: Schema,
        c: Closure,
        ctx: Ctx,
        partial: Map<String, Value>,
        store: Store
    ): Checked {
        val f = c.function
        val messages = ArrayList<Pair<String, String>>()
        val values = LinkedHashMap<String, Value>()
        var all = true
        for ((n, fd) in f.input) {
            val v = partial[n]
            if (v == null) {
                all = false
                continue
            }
            val refine = { e: Expr, x: Value ->
                val local = HashMap<String, Value>(values)
                for ((k, pv) in partial) if (!values.containsKey(k)) local[k] = pv
                local[n] = x
                val env =
                    Env(
                        sch,
                        c.helpers,
                        FnKind.Helper,
                        ctx,
                        local,
                        emptyMap(),
                        emptyMap(),
                        emptyList(),
                        null,
                        false)
                pureBool(St(Overlay(MemoryStore.empty(sch))), env, e)
            }
            try {
                val (v2, msg) = checkField(n, fd, v, { t, k -> store.exists(t, listOf(k)) }, refine)
                if (msg != null) {
                    all = false
                    messages.add(n to msg)
                }
                values[n] = v2
            } catch (r: EvalFault.Verdict) {
                all = false
                messages.add(n to refusalText(r.refusal))
            }
        }
        if (all) {
            for ((e, why) in f.refine) {
                val env =
                    Env(
                        sch,
                        c.helpers,
                        FnKind.Helper,
                        ctx,
                        HashMap(values),
                        emptyMap(),
                        emptyMap(),
                        emptyList(),
                        null,
                        false)
                try {
                    if (!pureBool(St(Overlay(MemoryStore.empty(sch))), env, e))
                        messages.add("" to (why ?: "invalid"))
                } catch (r: EvalFault.Verdict) {
                    messages.add("" to refusalText(r.refusal))
                }
            }
        }
        return Checked(messages, values)
    }

    /** `refusal_text`: the message of a `refuse`, otherwise the refusal shown. */
    public fun refusalText(r: Refusal): String = if (r is Refusal.Refused) r.why else r.show()

    /**
     * §6.6 `select_plan`: a plan whose right-hand sides are already values, pulled over a store.
     */
    public fun selectPlan(sch: Schema, plan: Plan, store: Store): List<Value> =
        View.read(sch, plan, Scope.of(sch, emptyList(), Ctx.nobody, emptyMap(), emptyMap()), store)

    // An expression that reads nothing, over a store that holds nothing.
    internal fun pure(env: Env, e: Expr): Value = eval(St(NoStore(env.schema)), env, e)

    // §6.3 Statements -------------------------------------------------------

    // A `let` binds for the rest of its block and no further.
    private fun block(st: St, env0: Env, blk: Block): Returned? {
        var env = env0
        for (s in blk) {
            if (s is Stmt.Let) {
                env = env.under(s.sym, eval(st, env, s.e))
                continue
            }
            exec(st, env, s)?.let {
                return it
            }
        }
        return null
    }

    private fun exec(st: St, env: Env, s: Stmt): Returned? {
        when (s) {
            is Stmt.Let -> throw IllegalStateException("a block binds its lets")
            is Stmt.If -> return block(st, env, if (bool(eval(st, env, s.c))) s.then else s.els)
            is Stmt.For -> {
                for (v in list(eval(st, env, s.over))) block(st, env.under(s.sym, v), s.body)?.let {
                    return it
                }
            }
            is Stmt.Insert -> {
                mutating(env)
                val row = Rules.rowFor(st.store, s.table, strct(eval(st, env, s.row)))
                wrote(st, Rules.insert(st.store, s.table, row, s.on))
            }
            is Stmt.Upsert -> {
                mutating(env)
                val row = Rules.rowFor(st.store, s.table, strct(eval(st, env, s.row)))
                wrote(st, Rules.upsert(st.store, s.table, row, s.on))
            }
            is Stmt.Update -> {
                mutating(env)
                val key = evalMany(st, env, s.key)
                val old = st.store.get(s.table, key)
                if (old != null) {
                    val row =
                        Rules.rowFor(
                            st.store, s.table, strct(eval(st, env.under(s.sym, old), s.row)))
                    wrote(st, Rules.update(st.store, s.table, key, row))
                }
            }
            is Stmt.Delete -> {
                mutating(env)
                wrote(st, st.store.delete(s.table, evalMany(st, env, s.key)))
            }
            is Stmt.Refuse -> {
                if (env.kind == FnKind.Helper) bug(EvalError.Impure("refuse inside a helper"))
                verdict(Refusal.Refused(text(eval(st, env, s.e))))
            }
            is Stmt.Return -> return Returned(s.e?.let { eval(st, env, it) })
            // D3 Deferred, at the authority; nothing anywhere else.
            is Stmt.Private -> if (env.ctx.authority) st.deferred.add(s.body to captured(env))
        }
        return null
    }

    // The locals in scope, outermost first: what a private block runs over.
    private fun captured(env: Env): List<Pair<Sym, Value>> {
        val frames = ArrayList<Pair<Sym, Value>>()
        var at = env.locals
        while (at != null) {
            frames.add(at.sym to at.value)
            at = at.up
        }
        return env.node + frames.reversed()
    }

    private fun wrote(st: St, w: Written) {
        when (w) {
            is Written.Refused -> verdict(w.refusal)
            is Written.Ok -> w.change?.let { st.changes.add(it) }
        }
    }

    private fun mutating(env: Env) {
        if (env.kind != FnKind.Mutator) bug(EvalError.Impure("write outside a mutator"))
    }

    private fun reading(env: Env) {
        if (env.kind == FnKind.Helper) bug(EvalError.Impure("read inside a helper"))
    }

    // §6.4 Expressions --------------------------------------------------------

    private fun eval(st: St, env: Env, e: Expr): Value =
        when (e) {
            is Expr.Lit -> e.v
            is Expr.Arg -> env.args[e.name] ?: bug(EvalError.MissingArg(e.name))
            is Expr.AutoE -> env.autos[e.name] ?: bug(EvalError.MissingAuto(e.name))
            is Expr.Var -> env.local(e.sym) ?: bug(EvalError.UnboundVar(e.sym))
            is Expr.Provided -> env.provided[e.fn] ?: bug(EvalError.NotProvided(e.fn))
            is Expr.FieldE ->
                when (val v = eval(st, env, e.e)) {
                    is Value.VStruct -> v.fields[e.name] ?: bug(EvalError.NoSuchField(e.name))
                    else -> typeError("Struct", v)
                }
            is Expr.Get -> {
                reading(env)
                st.store.get(e.table, evalMany(st, env, e.key)) ?: Value.VNull
            }
            is Expr.CtxUser -> Value.VText(env.ctx.user)
            is Expr.CtxSession -> Value.VText(env.ctx.session)
            is Expr.HasRole -> Value.VBool(e.role in env.ctx.roles)
            is Expr.StructE -> {
                val m = LinkedHashMap<String, Value>()
                for ((k, x) in e.fields) m[k] = eval(st, env, x)
                Value.VStruct(m)
            }
            is Expr.ListE -> Value.VList(evalMany(st, env, e.items))
            is Expr.SomeE -> eval(st, env, e.e)
            is Expr.NoneE -> Value.VNull
            is Expr.Match -> {
                val v = eval(st, env, e.e)
                if (v is Value.VNull) eval(st, env, e.none)
                else eval(st, env.under(e.sym, v), e.some)
            }
            is Expr.IfE -> eval(st, env, if (bool(eval(st, env, e.c))) e.then else e.els)
            is Expr.OpE -> op(st, env, e)
            is Expr.CmpE -> {
                val x = eval(st, env, e.l)
                val y = eval(st, env, e.r)
                Value.VBool(cmp(e.op, x, y))
            }
            is Expr.Call -> {
                val vals = e.args.map { eval(st, env, it) }
                val f =
                    env.helpers.firstOrNull { it.name == e.fn }
                        ?: bug(EvalError.UnknownFunction(e.fn))
                if (f.kind != FnKind.Helper) bug(EvalError.WrongKind(e.fn, f.kind))
                call(st, env, f, vals)
            }
            is Expr.StdE -> {
                val vals = e.args.map { eval(st, env, it) }
                try {
                    Std.apply(e.fn, vals)
                } catch (x: StdError.Fault) {
                    verdict(Refusal.Refused(x.why))
                } catch (x: StdError.Arity) {
                    bug(EvalError.Arity("${x.fn.name}/${x.n}"))
                } catch (x: StdError.TypeMismatch) {
                    bug(EvalError.TypeError(x.fn.name))
                }
            }
            is Expr.MapE ->
                Value.VList(
                    list(eval(st, env, e.over)).map { eval(st, env.under(e.sym, it), e.body) })
            is Expr.FilterE -> {
                val vs = list(eval(st, env, e.over))
                val keep = vs.map { bool(eval(st, env.under(e.sym, it), e.body)) }
                Value.VList(vs.filterIndexed { i, _ -> keep[i] })
            }
            // Every element is evaluated; the result is the disjunction (conjunction).
            is Expr.AnyE -> {
                var acc = false
                for (v in list(eval(st, env, e.over))) acc =
                    bool(eval(st, env.under(e.sym, v), e.body)) or acc
                Value.VBool(acc)
            }
            is Expr.AllE -> {
                var acc = true
                for (v in list(eval(st, env, e.over))) acc =
                    bool(eval(st, env.under(e.sym, v), e.body)) and acc
                Value.VBool(acc)
            }
            // Stable, under the total order of the key.
            is Expr.SortBy -> {
                val vs = list(eval(st, env, e.over))
                val keys = vs.map { eval(st, env.under(e.sym, it), e.key) }
                Value.VList(
                    vs.indices.sortedWith { a, b -> compareValue(keys[a], keys[b]) }.map { vs[it] })
            }
            is Expr.Fold -> fold(st, env, e)
            is Expr.Select -> {
                reading(env)
                Value.VList(View.read(env.schema, e.plan, Scope(env), st.store))
            }
            is Expr.ExistsE -> {
                reading(env)
                Value.VBool(st.store.exists(e.table, evalMany(st, env, e.key)))
            }
        }

    private fun op(st: St, env: Env, e: Expr.OpE): Value {
        val es = e.args
        return when {
            e.op == Op.And && es.size == 2 ->
                if (bool(eval(st, env, es[0]))) eval(st, env, es[1]) else Value.VBool(false)
            e.op == Op.Or && es.size == 2 ->
                if (bool(eval(st, env, es[0]))) Value.VBool(true) else eval(st, env, es[1])
            e.op == Op.Not && es.size == 1 -> Value.VBool(!bool(eval(st, env, es[0])))
            e.op == Op.Neg && es.size == 1 -> {
                val n = int(eval(st, env, es[0]))
                if (n == Long.MIN_VALUE) verdict(Refusal.Refused("integer overflow"))
                Value.VInt(-n)
            }
            e.op in ARITH && es.size == 2 -> {
                val x = int(eval(st, env, es[0]))
                val y = int(eval(st, env, es[1]))
                val r = arith(e.op, x, y)
                r.first?.let { Value.VInt(it) } ?: verdict(Refusal.Refused(r.second!!))
            }
            else -> bug(EvalError.Arity(e.op.name))
        }
    }

    private val ARITH = setOf(Op.Add, Op.Sub, Op.Mul, Op.Div, Op.Mod)

    // §13 In a plan's node a sum is arithmetic over the integers: init and every
    // term added wide, the total checked once (`docs/plan-db.md` D4).
    private fun fold(st: St, env: Env, e: Expr.Fold): Value {
        val vs = list(eval(st, env, e.over))
        var a = eval(st, env, e.init)
        val f = if (env.wide) sumStep(e.body, e.acc) else null
        if (f != null && a is Value.VInt) {
            var total = BigInteger.valueOf(a.value)
            for (v in vs) total =
                total.add(BigInteger.valueOf(int(eval(st, env.under(e.sym, v), f))))
            if (total.bitLength() > 63) verdict(Refusal.Refused("integer overflow"))
            return Value.VInt(total.toLong())
        }
        for (v in vs) a =
            eval(st, env.withFrame(Frame(e.sym, v, Frame(e.acc, a, env.locals))), e.body)
        return a
    }

    /**
     * §13 `sum_step`: the term of a fold whose step is `acc + f` or `f + acc`, `f` not reading the
     * accumulator — the shape whose result alone is judged in a plan's node.
     */
    public fun sumStep(body: Expr, acc: Sym): Expr? {
        if (body !is Expr.OpE || body.op != Op.Add || body.args.size != 2) return null
        val (l, r) = body.args
        if (l is Expr.Var && l.sym == acc && !mentions(r, acc)) return r
        if (r is Expr.Var && r.sym == acc && !mentions(l, acc)) return l
        return null
    }

    /** Whether `x` occurs in `e` (a plan inside a `Select` is not looked into). */
    public fun mentions(e: Expr, x: Sym): Boolean =
        if (e is Expr.Var) e.sym == x else children(e).any { mentions(it, x) }

    private fun evalMany(st: St, env: Env, es: List<Expr>): List<Value> =
        es.map { eval(st, env, it) }

    // Call a helper: its arguments bound as a fresh environment; helpers never
    // see the caller's locals, autos, arguments or provided values.
    private fun call(st: St, env: Env, f: Function, vals: List<Value>): Value {
        if (vals.size != f.input.size) bug(EvalError.Arity(f.name))
        val args = LinkedHashMap<String, Value>()
        for ((p, v) in f.input.zip(vals)) args[p.first] = v
        val env2 =
            Env(
                env.schema,
                env.helpers,
                FnKind.Helper,
                env.ctx,
                args,
                emptyMap(),
                emptyMap(),
                emptyList(),
                null,
                env.wide)
        return block(st, env2, f.body)?.v ?: bug(EvalError.NoReturn(f.name))
    }

    /**
     * §6.5 `arith`: checked arithmetic — the result, or the fault's text. Division truncates toward
     * zero and the remainder takes the dividend's sign; `MIN / -1` and `MIN % -1` overflow.
     */
    public fun arith(op: Op, x: Long, y: Long): Pair<Long?, String?> {
        val overflow = null to "integer overflow"
        return try {
            when (op) {
                Op.Add -> Math.addExact(x, y) to null
                Op.Sub -> Math.subtractExact(x, y) to null
                Op.Mul -> Math.multiplyExact(x, y) to null
                Op.Div,
                Op.Mod ->
                    when {
                        y == 0L -> null to "division by zero"
                        x == Long.MIN_VALUE && y == -1L -> overflow
                        op == Op.Div -> x / y to null
                        else -> x % y to null
                    }
                else -> null to "not an arithmetic operator"
            }
        } catch (_: ArithmeticException) {
            overflow
        }
    }

    // Coercions: a bug when the verifier's type does not hold.

    private fun typeError(want: String, v: Value): Nothing =
        bug(EvalError.TypeError("expected $want, got ${v.show().take(60)}"))

    private fun bool(v: Value): Boolean = (v as? Value.VBool)?.value ?: typeError("Bool", v)

    private fun int(v: Value): Long = (v as? Value.VInt)?.value ?: typeError("Int", v)

    private fun text(v: Value): String = (v as? Value.VText)?.value ?: typeError("Text", v)

    private fun list(v: Value): List<Value> = (v as? Value.VList)?.items ?: typeError("List", v)

    private fun strct(v: Value): Row = v as? Value.VStruct ?: typeError("Struct", v)
}
