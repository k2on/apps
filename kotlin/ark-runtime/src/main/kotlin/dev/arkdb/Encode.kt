// §7 The module as a value (`rust/ark/src/ir/encode.rs`): every node a
// struct with a `"t"` tag, so that `Canon.encode` is the only encoder there
// is and a module's bytes are canonical by the same rule as a row's.
// Symbol names are not carried; the hash never sees them.
package dev.arkdb

public object Encode {
    private fun node(t: String, vararg fields: Pair<String, Value>): Value =
        node(t, fields.toList())

    private fun node(t: String, fields: List<Pair<String, Value>>): Value {
        val m = LinkedHashMap<String, Value>()
        m["t"] = Value.VText(t)
        for ((k, v) in fields) m[k] = v
        return Value.VStruct(m)
    }

    private fun txt(s: String): Value = Value.VText(s)

    private fun int(n: Long): Value = Value.VInt(n)

    private fun <T> list(xs: List<T>, f: (T) -> Value): Value = Value.VList(xs.map(f))

    private fun texts(xs: List<String>): Value = Value.VList(xs.map { Value.VText(it) })

    /**
     * `module_value`: functions carried without their dependency hashes; the type section and the
     * live states only when a module declares one, so a module without live state is the bytes it
     * was.
     */
    public fun module(m: Module): Value {
        val fields =
            mutableListOf(
                "spec" to int(m.spec),
                "schema" to schema(m.schema),
                "functions" to list(m.functions) { function(emptyMap(), it) },
                "routers" to list(m.routers, ::router),
                "live" to list(m.live) { (n, t) -> node("frame", "name" to txt(n), "ty" to ty(t)) },
            )
        if (m.types.isNotEmpty()) fields.add("types" to list(m.types) { (n, t) -> typeDecl(n, t) })
        if (m.states.isNotEmpty()) fields.add("states" to list(m.states) { state(emptyMap(), it) })
        return node("module", fields)
    }

    /** `type_decl_value`: one declaration of the type section, `{"t":"type","name","ty"}`. */
    public fun typeDecl(name: String, t: Ty): Value =
        node("type", "name" to txt(name), "ty" to ty(t))

    /**
     * `state_value`: a live state, every transition normalised, with the hashes of the helpers it
     * reaches directly as `deps`; the hooks only where present. A module writes it with none.
     */
    public fun state(deps: Map<String, Value>, s: State): Value {
        val fields =
            mutableListOf(
                "name" to txt(s.name),
                "deps" to Value.VStruct(deps),
                "ty" to ty(s.ty),
                "init" to expr(s.init),
                "scope" to expr(s.scope),
                "transitions" to list(s.transitions, ::transition),
            )
        s.onJoin?.let { fields.add("on_join" to transition(it)) }
        s.onLeave?.let { fields.add("on_leave" to transition(it)) }
        s.onRestore?.let { fields.add("on_restore" to transition(it)) }
        return node("state", fields)
    }

    /** `transition_value`: one transition, normalised. */
    public fun transition(t0: Transition): Value {
        val t = Normalize.transition(t0)
        return node(
            "transition",
            "name" to txt(t.name),
            "input" to list(t.input) { (n, fd) -> field(n, fd) },
            "guards" to
                list(t.guards) { (e, why) -> node("guard", "e" to expr(e), "why" to txt(why)) },
            "body" to list(t.body, ::stmt),
        )
    }

    /** `transition_calls`: the helpers a transition calls directly, sorted. */
    public fun transitionCalls(t: Transition): List<String> {
        val acc = sortedSetOf<String>(CodePointOrder)
        for ((_, fd) in t.input) for (c in fd.checks) if (c is Check.Refine) exprCalls(c.e, acc)
        for ((e, _) in t.guards) exprCalls(e, acc)
        for (s in t.body) stmtCalls(s, acc)
        return acc.toList()
    }

    /** `state_calls`: the helpers a live state reaches directly, sorted. */
    public fun stateCalls(s: State): List<String> {
        val acc = sortedSetOf<String>(CodePointOrder)
        exprCalls(s.init, acc)
        exprCalls(s.scope, acc)
        for (t in s.allTransitions()) acc.addAll(transitionCalls(t))
        return acc.toList()
    }

    /** `router_value`. */
    public fun router(r: Router): Value =
        node("router", "name" to txt(r.name), "uses" to texts(r.uses))

    /** `schema_value`. */
    public fun schema(sch: Schema): Value = list(sch.tables, ::table)

    private fun table(t: Table): Value =
        node(
            "table",
            "name" to txt(t.name),
            "columns" to
                list(t.columns) { c ->
                    node(
                        "column",
                        "name" to txt(c.name),
                        "ty" to ty(c.ty),
                        "nullable" to Value.VBool(c.nullable))
                },
            "key" to texts(t.key),
            // D4 A text index is an index of kind `text`, after the others.
            "indexes" to
                Value.VList(
                    t.indexes.map {
                        node(
                            "index",
                            "columns" to texts(it.columns),
                            "unique" to Value.VBool(it.unique))
                    } +
                        t.text.map {
                            node(
                                "index",
                                "columns" to texts(listOf(it)),
                                "unique" to Value.VBool(false),
                                "kind" to txt("text"))
                        }),
            "refs" to
                list(t.refs) { node("ref", "column" to txt(it.column), "table" to txt(it.table)) },
        )

    /** `ty_value`. */
    public fun ty(t: Ty): Value =
        when (t) {
            is Ty.TBool -> node("bool")
            is Ty.TInt -> node("int")
            is Ty.TText -> node("text")
            is Ty.TBytes -> node("bytes")
            is Ty.TId -> node("id", "table" to txt(t.table))
            is Ty.TEnum -> node("enum", "variants" to texts(t.variants))
            is Ty.TOption -> node("option", "of" to ty(t.of))
            is Ty.TList -> node("list", "of" to ty(t.of))
            is Ty.TStruct ->
                node("struct", "fields" to Value.VStruct(t.fields.mapValues { ty(it.value) }))
            is Ty.TNamed -> node("named", "name" to txt(t.name))
            is Ty.TNonEmpty -> node("non_empty_list", "of" to ty(t.of))
            is Ty.TPerDevice -> node("per_device", "of" to ty(t.of))
            is Ty.TEphemeral -> node("ephemeral", "of" to ty(t.of))
            is Ty.TSum ->
                node(
                    "sum",
                    "variants" to
                        Value.VStruct(
                            t.variants.mapValues { (_, fs) ->
                                Value.VStruct(fs.mapValues { ty(it.value) })
                            }))
        }

    /**
     * `function_value`: one function, normalised, with the hashes of what it reaches directly. A
     * query's `plan`, a scope's `holds` and `private: true` are written only where they are, so no
     * function of an earlier spec moves.
     */
    public fun function(deps: Map<String, Value>, fn0: Function): Value {
        val f = Normalize.function(fn0)
        val fields =
            mutableListOf(
                "name" to txt(f.name),
                "deps" to Value.VStruct(deps),
                "kind" to txt(f.kind.wire),
                "router" to (f.router?.let(::txt) ?: Value.VNull),
                "uses" to texts(f.uses),
                "autos" to
                    list(f.autos) { (n, a) ->
                        when (a) {
                            is Auto.NewId ->
                                node("new_id", "name" to txt(n), "table" to txt(a.table))
                            is Auto.Now -> node("now", "name" to txt(n))
                        }
                    },
                "input" to list(f.input) { (n, fd) -> field(n, fd) },
                "refine" to
                    list(f.refine) { (e, why) ->
                        node("refine", "e" to expr(e), "why" to why(why))
                    },
                "ret" to (f.ret?.let(::ty) ?: Value.VNull),
                "body" to list(f.body, ::stmt),
            )
        f.plan?.let { fields.add("plan" to plan(it)) }
        if (f.kind == FnKind.Scope) fields.add("holds" to list(f.holds, ::hold))
        if (f.private) fields.add("private" to Value.VBool(true))
        return node("fn", fields)
    }

    /** `hold_value`: `pick` or `exclude` only where the projection is not every column. */
    public fun hold(h: Hold): Value {
        val fields =
            mutableListOf(
                "table" to txt(h.table), "filter" to (h.filter?.let(::pred) ?: Value.VNull))
        when (val c = h.columns) {
            is Projection.All -> {}
            is Projection.Pick -> fields.add("pick" to texts(c.columns))
            is Projection.Exclude -> fields.add("exclude" to texts(c.columns))
        }
        return node("hold", fields)
    }

    private fun why(w: String?): Value = w?.let(::txt) ?: Value.VNull

    private fun optInt(n: Long?): Value = n?.let(::int) ?: Value.VNull

    /** `field_value`: one input field. */
    public fun field(name: String, f: Field): Value =
        node("field", "name" to txt(name), "ty" to ty(f.ty), "checks" to list(f.checks, ::check))

    /** `check_value`. */
    public fun check(c: Check): Value =
        when (c) {
            is Check.Trim -> node("trim")
            is Check.MinLen -> node("min_len", "n" to int(c.n), "why" to why(c.why))
            is Check.MaxLen -> node("max_len", "n" to int(c.n), "why" to why(c.why))
            is Check.Range ->
                node("range", "lo" to optInt(c.lo), "hi" to optInt(c.hi), "why" to why(c.why))
            is Check.NonEmpty -> node("non_empty", "why" to why(c.why))
            is Check.Exists -> node("exists", "why" to why(c.why))
            is Check.Refine -> node("refine", "e" to expr(c.e), "why" to why(c.why))
        }

    /** `closure_value`: `{t: "closure", fn, helpers}`. */
    public fun closure(c: Closure): Value =
        node(
            "closure",
            "fn" to function(emptyMap(), c.function),
            "helpers" to list(c.helpers) { function(emptyMap(), it) })

    private fun stmt(s: Stmt): Value =
        when (s) {
            is Stmt.Let -> node("let", "sym" to int(s.sym), "e" to expr(s.e))
            is Stmt.If ->
                node(
                    "if",
                    "c" to expr(s.c),
                    "then" to list(s.then, ::stmt),
                    "else" to list(s.els, ::stmt))
            is Stmt.For ->
                node(
                    "for",
                    "sym" to int(s.sym),
                    "in" to expr(s.over),
                    "body" to list(s.body, ::stmt))
            is Stmt.Insert ->
                node("insert", "table" to txt(s.table), "row" to expr(s.row), "on" to texts(s.on))
            is Stmt.Upsert ->
                node("upsert", "table" to txt(s.table), "row" to expr(s.row), "on" to texts(s.on))
            is Stmt.Update ->
                node(
                    "update",
                    "table" to txt(s.table),
                    "key" to list(s.key, ::expr),
                    "sym" to int(s.sym),
                    "row" to expr(s.row))
            is Stmt.Delete -> node("delete", "table" to txt(s.table), "key" to list(s.key, ::expr))
            is Stmt.Refuse -> node("refuse", "e" to expr(s.e))
            is Stmt.Return -> node("return", "e" to (s.e?.let(::expr) ?: Value.VNull))
            is Stmt.Private -> node("private", "body" to list(s.body, ::stmt))
        }

    /** An expression as a value. */
    public fun expr(e: Expr): Value =
        when (e) {
            is Expr.Lit -> node("lit", "v" to e.v)
            is Expr.Arg -> node("arg", "name" to txt(e.name))
            is Expr.AutoE -> node("auto", "name" to txt(e.name))
            is Expr.Var -> node("var", "sym" to int(e.sym))
            is Expr.CtxUser -> node("ctx_user")
            is Expr.CtxSession -> node("ctx_session")
            is Expr.HasRole -> node("has_role", "role" to txt(e.role))
            is Expr.Provided -> node("provided", "fn" to txt(e.fn))
            is Expr.FieldE -> node("field", "e" to expr(e.e), "name" to txt(e.name))
            is Expr.StructE ->
                node("struct", "fields" to Value.VStruct(e.fields.mapValues { expr(it.value) }))
            is Expr.ListE -> node("list", "items" to list(e.items, ::expr))
            is Expr.SomeE -> node("some", "e" to expr(e.e))
            is Expr.NoneE -> node("none", "ty" to ty(e.ty))
            is Expr.Match ->
                node(
                    "match",
                    "e" to expr(e.e),
                    "sym" to int(e.sym),
                    "some" to expr(e.some),
                    "none" to expr(e.none))
            is Expr.IfE ->
                node("ife", "c" to expr(e.c), "then" to expr(e.then), "else" to expr(e.els))
            is Expr.OpE -> node("op", "op" to txt(e.op.wire), "args" to list(e.args, ::expr))
            is Expr.CmpE -> node("cmp", "op" to txt(e.op.wire), "l" to expr(e.l), "r" to expr(e.r))
            is Expr.Call -> node("call", "fn" to txt(e.fn), "args" to list(e.args, ::expr))
            is Expr.StdE -> node("std", "fn" to txt(e.fn.name), "args" to list(e.args, ::expr))
            is Expr.MapE ->
                node("map", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
            is Expr.FilterE ->
                node("filter", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
            is Expr.AnyE ->
                node("any", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
            is Expr.AllE ->
                node("all", "in" to expr(e.over), "sym" to int(e.sym), "body" to expr(e.body))
            is Expr.SortBy ->
                node("sort_by", "in" to expr(e.over), "sym" to int(e.sym), "key" to expr(e.key))
            is Expr.Fold ->
                node(
                    "fold",
                    "in" to expr(e.over),
                    "init" to expr(e.init),
                    "acc" to int(e.acc),
                    "sym" to int(e.sym),
                    "body" to expr(e.body))
            is Expr.Select -> node("select", "plan" to plan(e.plan))
            is Expr.Get -> node("get", "table" to txt(e.table), "key" to list(e.key, ::expr))
            is Expr.ExistsE -> node("exists", "table" to txt(e.table), "key" to list(e.key, ::expr))
            is Expr.Variant ->
                node(
                    "variant",
                    "ty" to ty(e.ty),
                    "name" to txt(e.name),
                    "fields" to Value.VStruct(e.fields.mapValues { expr(it.value) }))
            is Expr.Case ->
                node(
                    "case",
                    "e" to expr(e.e),
                    "arms" to
                        list(e.arms) { a ->
                            node(
                                "arm", "name" to txt(a.name), "sym" to int(a.sym), "e" to expr(a.e))
                        })
            is Expr.LiveState -> node("live_state")
            is Expr.CtxNow -> node("ctx_now")
            is Expr.CtxDevice -> node("ctx_device")
            is Expr.Slot -> node("slot", "e" to expr(e.e), "key" to expr(e.key))
            is Expr.SetSlot ->
                node("set_slot", "e" to expr(e.e), "key" to expr(e.key), "v" to expr(e.v))
            is Expr.Slots -> node("slots", "e" to expr(e.e))
        }

    private fun dir(d: Dir): Value = txt(if (d == Dir.Asc) "asc" else "desc")

    /**
     * `plan_value` (§1.8): v3's keys — `table`, `filter`, `order`, `limit`, `related` — and the v4
     * ones only when present, so a v3-shaped plan encodes as it did.
     */
    public fun plan(p: Plan): Value {
        val fields =
            mutableListOf(
                "table" to txt(p.table),
                "filter" to (p.filter?.let(::pred) ?: Value.VNull),
                "order" to
                    list(p.order) { (k, d) ->
                        when (k) {
                            is Key.Column -> node("by", "column" to txt(k.column), "dir" to dir(d))
                            is Key.ExprK -> node("by", "expr" to expr(k.e), "dir" to dir(d))
                        }
                    },
                "limit" to optInt(p.limit),
                "related" to list(p.related, ::related),
            )
        val s = p.source
        if (s is Source.Group) fields.add("group" to texts(s.by))
        p.row?.let { fields.add("row" to int(it)) }
        p.members?.let { fields.add("members" to int(it)) }
        if (p.lookups.isNotEmpty()) fields.add("lookups" to list(p.lookups, ::lookup))
        p.having?.let { fields.add("having" to expr(it)) }
        p.project?.let { fields.add("project" to expr(it)) }
        return node("plan", fields)
    }

    private fun lookup(l: Lookup): Value =
        node(
            "lookup",
            "name" to txt(l.name),
            "sym" to int(l.sym),
            "table" to txt(l.table),
            "key" to list(l.key, ::expr))

    private fun related(r: Related): Value =
        node(
            "related",
            "name" to txt(r.name),
            "sym" to int(r.sym),
            "on" to list(r.on) { (c, e) -> Value.VList(listOf(txt(c), expr(e))) },
            "plan" to plan(r.plan))

    /** A predicate as a value. */
    public fun pred(p: Pred): Value =
        when (p) {
            is Pred.Cmp ->
                node("pcmp", "column" to txt(p.column), "op" to txt(p.op.wire), "e" to expr(p.e))
            is Pred.In -> node("pin", "column" to txt(p.column), "items" to list(p.items, ::expr))
            is Pred.All -> node("pall", "items" to list(p.items, ::pred))
            is Pred.AnyP -> node("pany", "items" to list(p.items, ::pred))
            is Pred.Not -> node("pnot", "e" to pred(p.e))
            is Pred.Has -> node("phas", "column" to txt(p.column), "e" to expr(p.e))
            is Pred.When -> node("pwhen", "e" to expr(p.e))
            is Pred.Exists ->
                node(
                    "pexists",
                    "table" to txt(p.table),
                    "column" to txt(p.column),
                    "pred" to pred(p.pred))
        }

    /**
     * `calls`: the helpers a function calls directly — in its checks, its refinements, its body and
     * its plan — sorted, without repeats. Middleware is not a call; see `reaches`.
     */
    public fun calls(f: Function): List<String> {
        val acc = sortedSetOf<String>(CodePointOrder)
        for ((_, fd) in f.input) for (c in fd.checks) if (c is Check.Refine) exprCalls(c.e, acc)
        for ((e, _) in f.refine) exprCalls(e, acc)
        for (s in f.body) stmtCalls(s, acc)
        f.plan?.let { planCalls(it, acc) }
        return acc.toList()
    }

    /**
     * `reaches`: the helpers a function calls and the middleware it uses, sorted, without repeats.
     */
    public fun reaches(f: Function): List<String> {
        val acc = sortedSetOf<String>(CodePointOrder)
        acc.addAll(calls(f))
        acc.addAll(f.uses)
        return acc.toList()
    }

    private fun stmtCalls(s: Stmt, acc: MutableSet<String>) {
        when (s) {
            is Stmt.Let -> exprCalls(s.e, acc)
            is Stmt.Insert -> exprCalls(s.row, acc)
            is Stmt.Upsert -> exprCalls(s.row, acc)
            is Stmt.Refuse -> exprCalls(s.e, acc)
            is Stmt.Update -> {
                s.key.forEach { exprCalls(it, acc) }
                exprCalls(s.row, acc)
            }
            is Stmt.If -> {
                exprCalls(s.c, acc)
                (s.then + s.els).forEach { stmtCalls(it, acc) }
            }
            is Stmt.For -> {
                exprCalls(s.over, acc)
                s.body.forEach { stmtCalls(it, acc) }
            }
            is Stmt.Delete -> s.key.forEach { exprCalls(it, acc) }
            is Stmt.Return -> s.e?.let { exprCalls(it, acc) }
            // D3 The server's closure carries what a private block calls.
            is Stmt.Private -> s.body.forEach { stmtCalls(it, acc) }
        }
    }

    private fun exprCalls(e: Expr, acc: MutableSet<String>) {
        if (e is Expr.Call) acc.add(e.fn)
        if (e is Expr.Select) {
            planCalls(e.plan, acc)
            return
        }
        for (c in children(e)) exprCalls(c, acc)
    }

    private fun planCalls(p: Plan, acc: MutableSet<String>) {
        p.filter?.let { predCalls(it, acc) }
        p.lookups.flatMap { it.key }.forEach { exprCalls(it, acc) }
        for (r in p.related) {
            r.on.forEach { exprCalls(it.second, acc) }
            planCalls(r.plan, acc)
        }
        p.having?.let { exprCalls(it, acc) }
        p.project?.let { exprCalls(it, acc) }
        for ((k, _) in p.order) if (k is Key.ExprK) exprCalls(k.e, acc)
    }

    private fun predCalls(p: Pred, acc: MutableSet<String>) {
        when (p) {
            is Pred.Cmp -> exprCalls(p.e, acc)
            is Pred.In -> p.items.forEach { exprCalls(it, acc) }
            is Pred.All -> p.items.forEach { predCalls(it, acc) }
            is Pred.AnyP -> p.items.forEach { predCalls(it, acc) }
            is Pred.Not -> predCalls(p.e, acc)
            is Pred.Has -> exprCalls(p.e, acc)
            is Pred.When -> exprCalls(p.e, acc)
            is Pred.Exists -> predCalls(p.pred, acc)
        }
    }
}

/**
 * The expressions directly inside one, binders' bodies included; a plan inside a `Select` is not
 * looked into (`view::children`).
 */
public fun children(e: Expr): List<Expr> =
    when (e) {
        is Expr.Lit,
        is Expr.Arg,
        is Expr.AutoE,
        is Expr.Var,
        is Expr.CtxUser,
        is Expr.CtxSession,
        is Expr.HasRole,
        is Expr.Provided,
        is Expr.NoneE,
        is Expr.Select,
        is Expr.LiveState,
        is Expr.CtxNow,
        is Expr.CtxDevice -> emptyList()
        is Expr.Variant -> e.fields.values.toList()
        is Expr.Case -> listOf(e.e) + e.arms.map { it.e }
        is Expr.Slot -> listOf(e.e, e.key)
        is Expr.SetSlot -> listOf(e.e, e.key, e.v)
        is Expr.Slots -> listOf(e.e)
        is Expr.FieldE -> listOf(e.e)
        is Expr.SomeE -> listOf(e.e)
        is Expr.StructE -> e.fields.values.toList()
        is Expr.ListE -> e.items
        is Expr.OpE -> e.args
        is Expr.Call -> e.args
        is Expr.StdE -> e.args
        is Expr.Get -> e.key
        is Expr.ExistsE -> e.key
        is Expr.Match -> listOf(e.e, e.some, e.none)
        is Expr.IfE -> listOf(e.c, e.then, e.els)
        is Expr.CmpE -> listOf(e.l, e.r)
        is Expr.MapE -> listOf(e.over, e.body)
        is Expr.FilterE -> listOf(e.over, e.body)
        is Expr.AnyE -> listOf(e.over, e.body)
        is Expr.AllE -> listOf(e.over, e.body)
        is Expr.SortBy -> listOf(e.over, e.key)
        is Expr.Fold -> listOf(e.over, e.init, e.body)
    }
