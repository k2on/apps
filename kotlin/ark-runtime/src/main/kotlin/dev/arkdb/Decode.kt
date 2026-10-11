// §7.2 A module from a value: the inverse of `Encode` (`rust/ark/src/ir/decode.rs`).
// Strict about shape — an unknown tag, a missing field or a field of the
// wrong type is a `ModuleError` naming where — and lenient about nothing.
package dev.arkdb

/** Where in the module the shape was wrong, and how (`ir::DecodeError`). */
public class ModuleError(public val path: List<String>, public val what: String) :
    Exception(path.joinToString("/") + ": " + what)

private typealias Fields = Map<String, Value>

public object Decode {
    private fun err(here: List<String>, what: String): Nothing = throw ModuleError(here, what)

    /** `module_from_value`: the whole module. */
    public fun module(v: Value): Module {
        val fs = tagged(listOf("module"), "module", v)
        val spec = int(listOf("module", "spec"), field(fs, "spec"))
        val schema = schema(field(fs, "schema"))
        val functions = list(listOf("module", "functions"), field(fs, "functions"), ::function)
        val routers = list(listOf("module", "routers"), field(fs, "routers"), ::router)
        val live =
            list(listOf("module", "live"), field(fs, "live")) { x ->
                val f = tagged(listOf("frame"), "frame", x)
                text(listOf("frame", "name"), field(f, "name")) to ty(field(f, "ty"))
            }
        return Module(spec, schema, functions, routers, live)
    }

    /** `router_from_value`. */
    public fun router(v: Value): Router {
        val fs = tagged(listOf("router"), "router", v)
        val name = text(listOf("router", "name"), field(fs, "name"))
        val here = listOf("router", name, "uses")
        return Router(name, list(here, field(fs, "uses")) { text(here, it) })
    }

    /** `closure_from_value`: `{t: "closure", fn, helpers}`. */
    public fun closure(v: Value): Closure {
        val fs = tagged(listOf("closure"), "closure", v)
        return Closure(
            function(field(fs, "fn")),
            list(listOf("closure", "helpers"), field(fs, "helpers"), ::function))
    }

    /** `schema_from_value`. */
    public fun schema(v: Value): Schema = Schema(list(listOf("schema"), v, ::table))

    private fun table(x: Value): Table {
        val fs = tagged(listOf("table"), "table", x)
        val name = text(listOf("table", "name"), field(fs, "name"))
        fun here(k: String) = listOf("table", name, k)
        val columns = list(here("columns"), field(fs, "columns"), ::column)
        val key = list(here("key"), field(fs, "key")) { text(here("key"), it) }
        val all = list(here("indexes"), field(fs, "indexes"), ::index)
        val refs = list(here("refs"), field(fs, "refs"), ::reference)
        val indexes = ArrayList<Index>()
        val texts = ArrayList<String>()
        for ((ix, isText) in all) {
            when {
                !isText -> indexes.add(ix)
                ix.columns.size == 1 -> texts.add(ix.columns[0])
                else -> err(here("indexes"), "a text index is on one column")
            }
        }
        return Table(name, columns, key, indexes, refs, texts)
    }

    private fun column(x: Value): Column {
        val fs = tagged(listOf("column"), "column", x)
        val name = text(listOf("column", "name"), field(fs, "name"))
        return Column(
            name,
            ty(field(fs, "ty")),
            bool(listOf("column", name, "nullable"), field(fs, "nullable")))
    }

    // An index, and whether it is of kind `text` (D4): `kind` is written only for one.
    private fun index(x: Value): Pair<Index, Boolean> {
        val fs = tagged(listOf("index"), "index", x)
        val columns =
            list(listOf("index", "columns"), field(fs, "columns")) {
                text(listOf("index", "columns"), it)
            }
        val unique = bool(listOf("index", "unique"), field(fs, "unique"))
        val k = fs["kind"]
        val isText =
            when {
                k == null -> false
                text(listOf("index", "kind"), k) == "text" && !unique -> true
                else -> err(listOf("index", "kind"), "unknown index kind ${k.show()}")
            }
        return Index(columns, unique) to isText
    }

    private fun reference(x: Value): Ref {
        val fs = tagged(listOf("ref"), "ref", x)
        return Ref(
            text(listOf("ref", "column"), field(fs, "column")),
            text(listOf("ref", "table"), field(fs, "table")))
    }

    /** `ty_from_value`. */
    public fun ty(v: Value): Ty {
        val (t, fs) = taggedAny(listOf("ty"), v)
        return when (t) {
            "bool" -> Ty.TBool
            "int" -> Ty.TInt
            "text" -> Ty.TText
            "bytes" -> Ty.TBytes
            "id" -> Ty.TId(text(listOf("ty", "id"), field(fs, "table")))
            "enum" ->
                Ty.TEnum(
                    list(listOf("ty", "enum"), field(fs, "variants")) {
                        text(listOf("ty", "enum"), it)
                    })
            "option" -> Ty.TOption(ty(field(fs, "of")))
            "list" -> Ty.TList(ty(field(fs, "of")))
            "struct" ->
                Ty.TStruct(
                    structMap(listOf("ty", "struct"), field(fs, "fields")).mapValues {
                        ty(it.value)
                    })
            else -> err(listOf("ty"), "unknown type tag $t")
        }
    }

    /** `function_from_value`. */
    public fun function(v: Value): Function {
        val fs = tagged(listOf("fn"), "fn", v)
        val name = text(listOf("fn", "name"), field(fs, "name"))
        val here = listOf("fn", name)
        fun p(k: String) = here + k
        val kindText = text(p("kind"), field(fs, "kind"))
        val kind = FnKind.parse(kindText) ?: err(here, "unknown kind $kindText")
        val router = optional(field(fs, "router")) { text(p("router"), it) }
        val uses = list(p("uses"), field(fs, "uses")) { text(p("uses"), it) }
        val autos = list(p("autos"), field(fs, "autos"), ::auto)
        val input = list(p("input"), field(fs, "input")) { inputField(p("input"), it) }
        val refine =
            list(p("refine"), field(fs, "refine")) { x ->
                val rs = tagged(p("refine"), "refine", x)
                expr(p("refine"), field(rs, "e")) to why(p("refine"), field(rs, "why"))
            }
        val ret = optional(field(fs, "ret"), ::ty)
        val body = list(p("body"), field(fs, "body")) { stmt(here, it) }
        val plan = fs["plan"]?.let { plan(p("plan"), it) }
        // D2 Written for a scope and nothing else.
        val h = fs["holds"]
        val holds =
            when {
                kind == FnKind.Scope && h != null -> list(p("holds"), h) { hold(p("holds"), it) }
                kind == FnKind.Scope -> err(p("holds"), "missing field")
                h == null -> emptyList()
                else -> err(p("holds"), "only a scope holds")
            }
        // D3 Present only when true.
        val pv = fs["private"]
        val private =
            when {
                pv == null -> false
                pv == Value.VBool(true) -> true
                else -> err(p("private"), "present only as true")
            }
        return Function(
            name, kind, router, uses, autos, input, refine, ret, body, plan, holds, private)
    }

    /** `hold_from_value`. */
    public fun hold(v: Value): Hold = hold(listOf("hold"), v)

    private fun hold(at: List<String>, v: Value): Hold {
        val fs = tagged(at, "hold", v)
        val table = text(at, field(fs, "table"))
        val here = at + table
        val filter = optional(field(fs, "filter")) { pred(here, it) }
        fun names(x: Value) = list(here, x) { text(here, it) }
        val pick = fs["pick"]
        val exclude = fs["exclude"]
        val columns =
            when {
                pick == null && exclude == null -> Projection.All
                exclude == null -> Projection.Pick(names(pick!!))
                pick == null -> Projection.Exclude(names(exclude))
                else -> err(here, "a hold picks or excludes, not both")
            }
        return Hold(table, filter, columns)
    }

    private fun why(here: List<String>, v: Value): String? = optional(v) { text(here, it) }

    private fun inputField(here: List<String>, x: Value): Pair<String, Field> {
        val fs = tagged(here, "field", x)
        val n = text(here, field(fs, "name"))
        val p = here + n
        val t = ty(field(fs, "ty"))
        return n to Field(t, list(p, field(fs, "checks")) { check(p, it) })
    }

    private fun check(here: List<String>, v: Value): Check {
        val (t, fs) = taggedAny(here, v)
        val p = here + t
        fun w() = why(p, field(fs, "why"))
        fun optInt(k: String) = optional(field(fs, k)) { int(p, it) }
        return when (t) {
            "trim" -> Check.Trim
            "min_len" -> Check.MinLen(int(p, field(fs, "n")), w())
            "max_len" -> Check.MaxLen(int(p, field(fs, "n")), w())
            "range" -> Check.Range(optInt("lo"), optInt("hi"), w())
            "non_empty" -> Check.NonEmpty(w())
            "exists" -> Check.Exists(w())
            "refine" -> Check.Refine(expr(p, field(fs, "e")), w())
            else -> err(here, "unknown check $t")
        }
    }

    private fun auto(x: Value): Pair<String, Auto> {
        val (t, fs) = taggedAny(listOf("auto"), x)
        val n = text(listOf("auto", "name"), field(fs, "name"))
        return when (t) {
            "new_id" -> n to Auto.NewId(text(listOf("auto", n), field(fs, "table")))
            "now" -> n to Auto.Now
            else -> err(listOf("auto", n), "unknown auto $t")
        }
    }

    private fun stmt(here: List<String>, v: Value): Stmt {
        val (t, fs) = taggedAny(here, v)
        val p = here + t
        fun e(k: String) = expr(p, field(fs, k))
        fun b(k: String) = list(p, field(fs, k)) { stmt(p, it) }
        fun names(k: String) = list(p, field(fs, k)) { text(p, it) }
        fun es(k: String) = list(p, field(fs, k)) { expr(p, it) }
        return when (t) {
            "let" -> Stmt.Let(sym(p, field(fs, "sym")), e("e"))
            "if" -> Stmt.If(e("c"), b("then"), b("else"))
            "for" -> Stmt.For(sym(p, field(fs, "sym")), e("in"), b("body"))
            "insert" -> Stmt.Insert(text(p, field(fs, "table")), e("row"), names("on"))
            "upsert" -> Stmt.Upsert(text(p, field(fs, "table")), e("row"), names("on"))
            "update" ->
                Stmt.Update(
                    text(p, field(fs, "table")), es("key"), sym(p, field(fs, "sym")), e("row"))
            "delete" -> Stmt.Delete(text(p, field(fs, "table")), es("key"))
            "refuse" -> Stmt.Refuse(e("e"))
            "return" -> Stmt.Return(optional(field(fs, "e")) { expr(p, it) })
            "private" -> Stmt.Private(b("body"))
            else -> err(here, "unknown statement $t")
        }
    }

    private fun expr(here: List<String>, v: Value): Expr {
        val (t, fs) = taggedAny(here, v)
        val p = here + t
        fun e(k: String) = expr(p, field(fs, k))
        fun s(k: String) = sym(p, field(fs, k))
        fun es(k: String) = list(p, field(fs, k)) { expr(p, it) }
        fun tx(k: String) = text(p, field(fs, k))
        return when (t) {
            "lit" -> Expr.Lit(field(fs, "v"))
            "arg" -> Expr.Arg(tx("name"))
            "auto" -> Expr.AutoE(tx("name"))
            "var" -> Expr.Var(s("sym"))
            "ctx_user" -> Expr.CtxUser
            "ctx_session" -> Expr.CtxSession
            "has_role" -> Expr.HasRole(tx("role"))
            "provided" -> Expr.Provided(tx("fn"))
            "field" -> Expr.FieldE(e("e"), tx("name"))
            "struct" ->
                Expr.StructE(structMap(p, field(fs, "fields")).mapValues { expr(p, it.value) })
            "list" -> Expr.ListE(es("items"))
            "some" -> Expr.SomeE(e("e"))
            "none" -> Expr.NoneE(ty(field(fs, "ty")))
            "match" -> Expr.Match(e("e"), s("sym"), e("some"), e("none"))
            "ife" -> Expr.IfE(e("c"), e("then"), e("else"))
            "op" -> {
                val name = tx("op")
                Expr.OpE(Op.parse(name) ?: err(p, "unknown operator $name"), es("args"))
            }
            "cmp" -> Expr.CmpE(cmpOp(p, field(fs, "op")), e("l"), e("r"))
            "call" -> Expr.Call(tx("fn"), es("args"))
            "std" -> {
                val name = tx("fn")
                Expr.StdE(
                    StdFn.parse(name) ?: err(p, "unknown standard function $name"), es("args"))
            }
            "map" -> Expr.MapE(e("in"), s("sym"), e("body"))
            "filter" -> Expr.FilterE(e("in"), s("sym"), e("body"))
            "any" -> Expr.AnyE(e("in"), s("sym"), e("body"))
            "all" -> Expr.AllE(e("in"), s("sym"), e("body"))
            "sort_by" -> Expr.SortBy(e("in"), s("sym"), e("key"))
            "fold" -> Expr.Fold(e("in"), e("init"), s("acc"), s("sym"), e("body"))
            "select" -> Expr.Select(plan(p, field(fs, "plan")))
            "get" -> Expr.Get(tx("table"), es("key"))
            "exists" -> Expr.ExistsE(tx("table"), es("key"))
            else -> err(here, "unknown expression $t")
        }
    }

    /** `plan_from_value`: a plan read from its value alone, outside any module. */
    public fun plan(v: Value): Plan = plan(emptyList(), v)

    // §1.8 The v3 keys always, the v4 ones read when present.
    private fun plan(at: List<String>, v: Value): Plan {
        val fs = tagged(at, "plan", v)
        val table = text(at, field(fs, "table"))
        val here = at + table
        val filter = optional(field(fs, "filter")) { pred(here, it) }
        val order =
            list(here, field(fs, "order")) { x ->
                val bs = tagged(here, "by", x)
                val c = bs["column"]
                val ex = bs["expr"]
                val key =
                    when {
                        c != null && ex == null -> Key.Column(text(here, c))
                        c == null && ex != null -> Key.ExprK(expr(here, ex))
                        else -> err(here, "an order key is a column or an expression")
                    }
                val d =
                    when (val dt = text(here, field(bs, "dir"))) {
                        "asc" -> Dir.Asc
                        "desc" -> Dir.Desc
                        else -> err(here, "unknown direction $dt")
                    }
                key to d
            }
        val limit = optional(field(fs, "limit")) { int(here, it) }
        val related =
            list(here, field(fs, "related")) { x ->
                val rs = tagged(here, "related", x)
                val name = text(here, field(rs, "name"))
                val sub = here + name
                val sym = sym(sub, field(rs, "sym"))
                val on =
                    list(sub, field(rs, "on")) { pair ->
                        val xs = (pair as? Value.VList)?.items
                        if (xs == null || xs.size != 2) err(sub, "an on pair is [column, expr]")
                        text(sub, xs[0]) to expr(sub, xs[1])
                    }
                Related(name, sym, on, plan(sub, field(rs, "plan")))
            }
        val g = fs["group"]
        val source =
            if (g == null) Source.Table(table)
            else Source.Group(table, list(here, g) { text(here, it) })
        val row = fs["row"]?.let { sym(here, it) }
        val members = fs["members"]?.let { sym(here, it) }
        val ls = fs["lookups"]
        val lookups =
            if (ls == null) emptyList()
            else
                list(here, ls) { x ->
                    val l = tagged(here, "lookup", x)
                    Lookup(
                        text(here, field(l, "name")),
                        sym(here, field(l, "sym")),
                        text(here, field(l, "table")),
                        list(here, field(l, "key")) { expr(here, it) },
                    )
                }
        val having = fs["having"]?.let { expr(here, it) }
        val project = fs["project"]?.let { expr(here, it) }
        return Plan(source, filter, row, members, lookups, related, having, project, order, limit)
    }

    private fun pred(here: List<String>, v: Value): Pred {
        val (t, fs) = taggedAny(here, v)
        return when (t) {
            "pcmp" ->
                Pred.Cmp(
                    text(here, field(fs, "column")),
                    cmpOp(here, field(fs, "op")),
                    expr(here, field(fs, "e")))
            "pin" ->
                Pred.In(
                    text(here, field(fs, "column")),
                    list(here, field(fs, "items")) { expr(here, it) })
            "pall" -> Pred.All(list(here, field(fs, "items")) { pred(here, it) })
            "pany" -> Pred.AnyP(list(here, field(fs, "items")) { pred(here, it) })
            "pnot" -> Pred.Not(pred(here, field(fs, "e")))
            "phas" -> Pred.Has(text(here, field(fs, "column")), expr(here, field(fs, "e")))
            "pwhen" -> Pred.When(expr(here, field(fs, "e")))
            "pexists" ->
                Pred.Exists(
                    text(here, field(fs, "table")),
                    text(here, field(fs, "column")),
                    pred(here, field(fs, "pred")))
            else -> err(here, "unknown predicate $t")
        }
    }

    private fun cmpOp(here: List<String>, v: Value): CmpOp {
        val name = text(here, v)
        return CmpOp.parse(name) ?: err(here, "unknown comparison $name")
    }

    // Primitives --------------------------------------------------------

    private fun tagged(here: List<String>, want: String, v: Value): Fields {
        val (t, fs) = taggedAny(here, v)
        if (t != want) err(here, "expected $want, found $t")
        return fs
    }

    private fun taggedAny(here: List<String>, v: Value): Pair<String, Fields> {
        if (v !is Value.VStruct) err(here, "expected a struct")
        val t = v.fields["t"] as? Value.VText ?: err(here, "a node needs a text tag \"t\"")
        return t.value to v.fields
    }

    private fun field(fs: Fields, k: String): Value = fs[k] ?: err(listOf(k), "missing field")

    private fun <T> optional(v: Value, f: (Value) -> T): T? = if (v is Value.VNull) null else f(v)

    private fun <T> list(here: List<String>, v: Value, f: (Value) -> T): List<T> {
        if (v !is Value.VList) err(here, "expected a list")
        return v.items.map(f)
    }

    private fun structMap(here: List<String>, v: Value): Map<String, Value> {
        if (v !is Value.VStruct) err(here, "expected a struct")
        return v.fields
    }

    private fun text(here: List<String>, v: Value): String =
        (v as? Value.VText)?.value ?: err(here, "expected text")

    private fun int(here: List<String>, v: Value): Long =
        (v as? Value.VInt)?.value ?: err(here, "expected an int")

    private fun bool(here: List<String>, v: Value): Boolean =
        (v as? Value.VBool)?.value ?: err(here, "expected a bool")

    private fun sym(here: List<String>, v: Value): Sym = int(here, v)
}
