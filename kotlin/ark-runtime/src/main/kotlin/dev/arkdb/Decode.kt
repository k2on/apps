// §7.2 A module from a value: the inverse of Ark.Encode, strict about shape.
package dev.arkdb

public object Decode {
    public class DecodeError(public val path: List<String>, public val what: String) :
        Exception("${path.joinToString("/")}: $what")

    private fun bad(here: List<String>, what: String): Nothing = throw DecodeError(here, what)

    /** The whole module. */
    public fun fromValue(v: Value): Module {
        val fs = tagged(listOf("module"), "module", v)
        val spec = int(listOf("module", "spec"), field(fs, "spec"))
        val sch = schemaFromValue(field(fs, "schema"))
        val fns = list(listOf("module", "functions"), field(fs, "functions")) { functionFromValue(it) }
        val live = list(listOf("module", "live"), field(fs, "live")) { x ->
            val ffs = tagged(listOf("frame"), "frame", x)
            text(listOf("frame", "name"), field(ffs, "name")) to tyFromValue(field(ffs, "ty"))
        }
        return Module(Math.toIntExact(spec), sch, fns, live)
    }

    /** A closure as an authority stores or sends one: `{ t: "closure", fn, helpers }`. */
    public fun closureFromValue(v: Value): Closure {
        val fs = tagged(listOf("closure"), "closure", v)
        val fn = functionFromValue(field(fs, "fn"))
        val hs = list(listOf("closure", "helpers"), field(fs, "helpers")) { functionFromValue(it) }
        return Closure(fn, hs)
    }

    public fun schemaFromValue(v: Value): Schema = Schema(
        list(listOf("schema"), v) { x ->
            val fs = tagged(listOf("scope"), "scope", x)
            val n = text(listOf("scope", "name"), field(fs, "name"))
            Scope(n, list(listOf("scope", n), field(fs, "tables")) { table(it) })
        },
    )

    private fun table(x: Value): Table {
        val fs = tagged(listOf("table"), "table", x)
        val n = text(listOf("table", "name"), field(fs, "name"))
        val cs = list(listOf("table", n, "columns"), field(fs, "columns")) { c ->
            val cfs = tagged(listOf("column"), "column", c)
            val cn = text(listOf("column", "name"), field(cfs, "name"))
            Column(cn, tyFromValue(field(cfs, "ty")), bool(listOf("column", cn, "nullable"), field(cfs, "nullable")))
        }
        val k = list(listOf("table", n, "key"), field(fs, "key")) { text(listOf("table", n, "key"), it) }
        val ixs = list(listOf("table", n, "indexes"), field(fs, "indexes")) { i ->
            val ifs = tagged(listOf("index"), "index", i)
            Index(
                list(listOf("index", "columns"), field(ifs, "columns")) { text(listOf("index", "columns"), it) },
                bool(listOf("index", "unique"), field(ifs, "unique")),
            )
        }
        val rs = list(listOf("table", n, "refs"), field(fs, "refs")) { r ->
            val rfs = tagged(listOf("ref"), "ref", r)
            Ref(text(listOf("ref", "column"), field(rfs, "column")), text(listOf("ref", "table"), field(rfs, "table")))
        }
        return Table(n, cs, k, ixs, rs)
    }

    public fun tyFromValue(v: Value): Ty {
        val (t, fs) = taggedAny(listOf("ty"), v)
        return when (t) {
            "bool" -> Ty.TBool
            "int" -> Ty.TInt
            "text" -> Ty.TText
            "bytes" -> Ty.TBytes
            "id" -> Ty.TId(text(listOf("ty", "id"), field(fs, "table")))
            "enum" -> Ty.TEnum(list(listOf("ty", "enum"), field(fs, "variants")) { text(listOf("ty", "enum"), it) })
            "option" -> Ty.TOption(tyFromValue(field(fs, "of")))
            "list" -> Ty.TList(tyFromValue(field(fs, "of")))
            "struct" -> Ty.TStruct(structMap(listOf("ty", "struct"), field(fs, "fields")).mapValues { tyFromValue(it.value) })
            else -> bad(listOf("ty"), "unknown type tag $t")
        }
    }

    public fun functionFromValue(v: Value): Function {
        val fs = tagged(listOf("fn"), "fn", v)
        val n = text(listOf("fn", "name"), field(fs, "name"))
        val here = listOf("fn", n)
        val k = when (val ks = text(here + "kind", field(fs, "kind"))) {
            "mutator" -> FnKind.Mutator
            "query" -> FnKind.Query
            "helper" -> FnKind.Helper
            else -> bad(here, "unknown kind $ks")
        }
        val sc = optional(field(fs, "scope")) { text(here + "scope", it) }
        val autos = list(here + "autos", field(fs, "autos")) { x ->
            val (t, afs) = taggedAny(listOf("auto"), x)
            val an = text(listOf("auto", "name"), field(afs, "name"))
            when (t) {
                "new_id" -> an to Auto.NewId(text(listOf("auto", an), field(afs, "table")))
                "now" -> an to Auto.Now
                else -> bad(listOf("auto", an), "unknown auto $t")
            }
        }
        val args = list(here + "args", field(fs, "args")) { x ->
            val afs = tagged(listOf("arg"), "arg", x)
            text(listOf("arg", "name"), field(afs, "name")) to tyFromValue(field(afs, "ty"))
        }
        val ret = optional(field(fs, "ret")) { tyFromValue(it) }
        val body = list(here + "body", field(fs, "body")) { stmt(here, it) }
        return Function(n, k, sc, autos, args, ret, body, emptyMap())
    }

    private fun stmt(here: List<String>, v: Value): Stmt {
        val (t, fs) = taggedAny(here, v)
        val p = here + t
        return when (t) {
            "let" -> Stmt.Let(sym(p, field(fs, "sym")), expr(p, field(fs, "e")))
            "if" -> Stmt.If(
                expr(p, field(fs, "c")),
                list(p, field(fs, "then")) { stmt(p, it) },
                list(p, field(fs, "else")) { stmt(p, it) },
            )
            "for" -> Stmt.For(sym(p, field(fs, "sym")), expr(p, field(fs, "in")), list(p, field(fs, "body")) { stmt(p, it) })
            "put" -> Stmt.Put(text(p, field(fs, "table")), expr(p, field(fs, "row")))
            "delete" -> Stmt.Delete(text(p, field(fs, "table")), list(p, field(fs, "key")) { expr(p, it) })
            "refuse" -> Stmt.Refuse(expr(p, field(fs, "e")))
            "return" -> Stmt.Return(optional(field(fs, "e")) { expr(p, it) })
            else -> bad(here, "unknown statement $t")
        }
    }

    private fun expr(here: List<String>, v: Value): Expr {
        val (t, fs) = taggedAny(here, v)
        val p = here + t
        fun e(k: String): Expr = expr(p, field(fs, k))
        fun s(k: String): Sym = sym(p, field(fs, k))
        fun es(k: String): List<Expr> = list(p, field(fs, k)) { expr(p, it) }
        return when (t) {
            "lit" -> Expr.Lit(field(fs, "v"))
            "arg" -> Expr.Arg(text(p, field(fs, "name")))
            "auto" -> Expr.Auto(text(p, field(fs, "name")))
            "var" -> Expr.Var(s("sym"))
            "ctx_user" -> Expr.CtxUser
            "ctx_session" -> Expr.CtxSession
            "field" -> Expr.Field(e("e"), text(p, field(fs, "name")))
            "struct" -> Expr.Struct(structMap(p, field(fs, "fields")).mapValues { expr(p, it.value) })
            "list" -> Expr.ListE(es("items"))
            "some" -> Expr.Some(e("e"))
            "none" -> Expr.None(tyFromValue(field(fs, "ty")))
            "match" -> Expr.Match(e("e"), s("sym"), e("some"), e("none"))
            "ife" -> Expr.If(e("c"), e("then"), e("else"))
            "op" -> Expr.Op(op(p, text(p, field(fs, "op"))), es("args"))
            "cmp" -> Expr.Cmp(cmpOp(p, text(p, field(fs, "op"))), e("l"), e("r"))
            "call" -> Expr.Call(text(p, field(fs, "fn")), es("args"))
            "std" -> Expr.Std(stdFn(p, text(p, field(fs, "fn"))), es("args"))
            "map" -> Expr.Map(e("in"), s("sym"), e("body"))
            "filter" -> Expr.Filter(e("in"), s("sym"), e("body"))
            "any" -> Expr.Any(e("in"), s("sym"), e("body"))
            "all" -> Expr.All(e("in"), s("sym"), e("body"))
            "sort_by" -> Expr.SortBy(e("in"), s("sym"), e("key"))
            "fold" -> Expr.Fold(e("in"), e("init"), s("acc"), s("sym"), e("body"))
            "select" -> Expr.Select(plan(p, field(fs, "plan")))
            "get" -> Expr.Get(text(p, field(fs, "table")), es("key"))
            "exists" -> Expr.Exists(text(p, field(fs, "table")), es("key"))
            else -> bad(here, "unknown expression $t")
        }
    }

    private fun plan(here: List<String>, v: Value): IR.Plan {
        val fs = tagged(here, "plan", v)
        val tbl = text(here, field(fs, "table"))
        val f = optional(field(fs, "filter")) { pred(here + tbl, it) }
        val o = list(here, field(fs, "order")) { x ->
            val bfs = tagged(here, "by", x)
            val c = text(here, field(bfs, "column"))
            val d = when (val ds = text(here, field(bfs, "dir"))) {
                "asc" -> Dir.Asc
                "desc" -> Dir.Desc
                else -> bad(here, "unknown direction $ds")
            }
            c to d
        }
        val l = optional(field(fs, "limit")) { Math.toIntExact(int(here, it)) }
        val rs = list(here, field(fs, "related")) { x ->
            val rfs = tagged(here, "related", x)
            val n = text(here, field(rfs, "name"))
            IR.Related(
                n,
                Relation(text(here, field(rfs, "parent")), text(here, field(rfs, "child")), text(here, field(rfs, "column"))),
                plan(here + n, field(rfs, "plan")),
            )
        }
        return IR.Plan(tbl, f, o, l, rs)
    }

    private fun pred(here: List<String>, v: Value): IR.Pred {
        val (t, fs) = taggedAny(here, v)
        return when (t) {
            "pcmp" -> IR.Pred.Cmp(
                text(here, field(fs, "column")),
                cmpOp(here, text(here, field(fs, "op"))),
                expr(here, field(fs, "e")),
            )
            "pin" -> IR.Pred.In(text(here, field(fs, "column")), list(here, field(fs, "items")) { expr(here, it) })
            "pall" -> IR.Pred.All(list(here, field(fs, "items")) { pred(here, it) })
            "pany" -> IR.Pred.Any(list(here, field(fs, "items")) { pred(here, it) })
            "pnot" -> IR.Pred.Not(pred(here, field(fs, "e")))
            else -> bad(here, "unknown predicate $t")
        }
    }

    private fun op(here: List<String>, s: String): Op = Op.ofWire(s) ?: bad(here, "unknown operator $s")

    private fun cmpOp(here: List<String>, s: String): CmpOp = CmpOp.ofWire(s) ?: bad(here, "unknown comparison $s")

    private fun stdFn(here: List<String>, s: String): StdFn = StdFn.ofName(s) ?: bad(here, "unknown standard function $s")

    // Primitives --------------------------------------------------------------

    private fun tagged(here: List<String>, want: String, v: Value): Map<String, Value> {
        val (t, fs) = taggedAny(here, v)
        if (t != want) bad(here, "expected $want, found $t")
        return fs
    }

    private fun taggedAny(here: List<String>, v: Value): Pair<String, Map<String, Value>> {
        val fs = (v as? Value.VStruct)?.fields ?: bad(here, "expected a struct")
        val t = (fs["t"] as? Value.VText)?.value ?: bad(here, "a node needs a text tag \"t\"")
        return t to fs
    }

    private fun field(fs: Map<String, Value>, k: String): Value = fs[k] ?: bad(listOf(k), "missing field")

    private fun <A> optional(v: Value, f: (Value) -> A): A? = if (v is Value.VNull) null else f(v)

    private fun <A> list(here: List<String>, v: Value, f: (Value) -> A): List<A> =
        (v as? Value.VList)?.items?.map(f) ?: bad(here, "expected a list")

    private fun structMap(here: List<String>, v: Value): Map<String, Value> =
        (v as? Value.VStruct)?.fields ?: bad(here, "expected a struct")

    private fun text(here: List<String>, v: Value): String = (v as? Value.VText)?.value ?: bad(here, "expected text")

    private fun int(here: List<String>, v: Value): Long = (v as? Value.VInt)?.value ?: bad(here, "expected an int")

    private fun bool(here: List<String>, v: Value): Boolean = (v as? Value.VBool)?.value ?: bad(here, "expected a bool")

    private fun sym(here: List<String>, v: Value): Sym = Math.toIntExact(int(here, v))
}
