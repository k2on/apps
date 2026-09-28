// §9 Verification (Ark.Verify): what a module must satisfy before anything
// runs it, generates from it or hashes it.
package dev.arkdb

public object Verify {
    public sealed class VerifyError {
        public data class BadSpecVersion(val spec: Int) : VerifyError()
        public data class BadSchema(val error: SchemaError) : VerifyError()
        public data class DuplicateFunction(val name: String) : VerifyError()
        public data class DuplicateRouter(val name: String) : VerifyError()
        /** A router whose uses are not all middleware. */
        public data class BadRouter(val router: String, val what: String) : VerifyError()
        public data class In(val function: String, val complaint: Complaint) : VerifyError()
    }

    public sealed class Complaint {
        public object AutosOnNonMutator : Complaint()
        public object ReturnTypeOnMutator : Complaint()
        public object NoReturnType : Complaint()
        public data class DuplicateName(val name: String) : Complaint()
        public data class UnknownAutoTable(val table: String) : Complaint()
        public object NestedOption : Complaint()
        public data class TypeMismatch(val where: String, val expected: Ty, val actual: Ty) : Complaint()
        public data class NotAStruct(val field: String) : Complaint()
        public data class NoSuchField(val field: String) : Complaint()
        public data class UnknownArg(val name: String) : Complaint()
        public data class UnknownAuto(val name: String) : Complaint()
        public data class UnboundSymbol(val sym: Sym) : Complaint()
        public object ReadNotBound : Complaint()
        public object ReadInHelper : Complaint()
        public object WriteOutsideMutator : Complaint()
        public object RefuseOutsideMutator : Complaint()
        public data class UnknownTable(val table: String) : Complaint()
        public data class UnknownColumn(val table: String, val column: String) : Complaint()
        public data class UnknownHelper(val name: String) : Complaint()
        public data class HelperNotYetDeclared(val name: String) : Complaint()
        public data class NotAHelper(val name: String) : Complaint()
        public data class Arity(val name: String, val want: Int, val got: Int) : Complaint()
        public data class BadOp(val op: String) : Complaint()
        public object MayNotReturn : Complaint()
        public data class NeedsAnnotation(val what: String) : Complaint()
        public data class BadRelation(val parent: String, val child: String) : Complaint()
        public data class KeyArity(val table: String, val want: Int, val got: Int) : Complaint()
        public data class StdMisuse(val what: String) : Complaint()
        public object ProcedureWithoutRouter : Complaint()
        public data class UnknownRouter(val router: String) : Complaint()
        public data class RouterOnNonProcedure(val router: String) : Complaint()
        public data class UsesNotOnRouter(val uses: List<String>) : Complaint()
        public data class UsesOnNonProcedure(val uses: List<String>) : Complaint()
        public data class NotMiddleware(val name: String) : Complaint()
        public data class MiddlewareInput(val middleware: String, val field: String) : Complaint()
        public data class OnNotUnique(val table: String, val columns: List<String>) : Complaint()
        public data class NotProvided(val name: String) : Complaint()
        public data class BadCheck(val field: String, val what: String) : Complaint()
    }

    public class VerifyFailed(public val errors: List<VerifyError>) : Exception(errors.joinToString("; "))

    private class Complained(val complaints: List<Complaint>) : RuntimeException(null, null, false, false)

    private fun err(c: Complaint): Nothing = throw Complained(listOf(c))

    /**
     * Verify a module. On success, the module as it is to be hashed and
     * generated from: orders completed and every function normalised.
     * Throws `VerifyFailed`.
     */
    public fun verify(m0: Module): Module {
        val m = completeOrders(m0)
        if (m.spec != SPEC_VERSION) throw VerifyFailed(listOf(VerifyError.BadSpecVersion(m.spec)))
        val se = checkSchema(m.schema)
        if (se.isNotEmpty()) throw VerifyFailed(se.map { VerifyError.BadSchema(it) })
        val names = m.functions.map { it.name }
        val dups = names.groupingBy { it }.eachCount().filter { it.value > 1 }.keys
        if (dups.isNotEmpty()) throw VerifyFailed(names.filter { it in dups }.distinct().map { VerifyError.DuplicateFunction(it) })
        val rnames = m.routers.map { it.name }
        val rdups = rnames.groupingBy { it }.eachCount().filter { it.value > 1 }.keys
        if (rdups.isNotEmpty()) throw VerifyFailed(rnames.filter { it in rdups }.distinct().map { VerifyError.DuplicateRouter(it) })
        val rerrors = m.routers.flatMap { r ->
            val out = ArrayList<VerifyError>()
            for (u in r.uses) {
                val f = m.lookupFunction(u)
                if (f == null || !f.kind.isMiddleware) out.add(VerifyError.BadRouter(r.name, "$u is not middleware"))
            }
            out
        }
        if (rerrors.isNotEmpty()) throw VerifyFailed(rerrors)
        val errors = m.functions.withIndex().flatMap { (i, fn) -> verifyFunction(m, i, fn) }
        if (errors.isNotEmpty()) throw VerifyFailed(errors)
        return m.copy(functions = m.functions.map { Encode.normalize(it) })
    }

    /** Verify the i-th function of a module; its index decides which helpers it may call. */
    public fun verifyFunction(m: Module, i: Int, fn: Function): List<VerifyError> = try {
        checkFunction(m, i, fn)
        emptyList()
    } catch (c: Complained) {
        c.complaints.map { VerifyError.In(fn.name, it) }
    }

    private fun checkFunction(m: Module, i: Int, fn: Function) {
        val sch = m.schema
        when (fn.kind) {
            FnKind.Mutator, FnKind.Query -> {
                val rn = fn.router ?: err(Complaint.ProcedureWithoutRouter)
                val r = m.lookupRouter(rn) ?: err(Complaint.UnknownRouter(rn))
                if (!subsequence(fn.uses, r.uses)) err(Complaint.UsesNotOnRouter(fn.uses))
                if (fn.kind == FnKind.Mutator && fn.ret != null) err(Complaint.ReturnTypeOnMutator)
                if (fn.kind == FnKind.Query && fn.ret == null) err(Complaint.NoReturnType)
                if (fn.kind == FnKind.Query && fn.autos.isNotEmpty()) err(Complaint.AutosOnNonMutator)
                // Every middleware it runs reads fields this input has, at their types.
                for (u in fn.uses) {
                    val mw = m.lookupFunction(u) ?: err(Complaint.NotMiddleware(u))
                    if (!mw.kind.isMiddleware) err(Complaint.NotMiddleware(u))
                    for ((n, f) in mw.input) {
                        val mine = fn.input.firstOrNull { it.first == n }?.second
                        if (mine == null || mine.ty != f.ty) err(Complaint.MiddlewareInput(u, n))
                    }
                }
            }
            FnKind.Guard, FnKind.Provide -> {
                fn.router?.let { err(Complaint.RouterOnNonProcedure(it)) }
                if (fn.uses.isNotEmpty()) err(Complaint.UsesOnNonProcedure(fn.uses))
                if (fn.autos.isNotEmpty()) err(Complaint.AutosOnNonMutator)
                if (fn.kind == FnKind.Guard && fn.ret != null) err(Complaint.ReturnTypeOnMutator)
                if (fn.kind == FnKind.Provide && fn.ret == null) err(Complaint.NoReturnType)
            }
            FnKind.Helper -> {
                fn.router?.let { err(Complaint.RouterOnNonProcedure(it)) }
                if (fn.uses.isNotEmpty()) err(Complaint.UsesOnNonProcedure(fn.uses))
                if (fn.autos.isNotEmpty()) err(Complaint.AutosOnNonMutator)
                if (fn.ret == null) err(Complaint.NoReturnType)
            }
        }
        val argNames = fn.args.map { it.first } + fn.autos.map { it.first }
        val dupArgs = argNames.groupingBy { it }.eachCount().filter { it.value > 1 }.keys
        if (dupArgs.isNotEmpty()) throw Complained(argNames.filter { it in dupArgs }.map { Complaint.DuplicateName(it) })
        for ((_, t) in fn.args) noNestedOption(t)
        for ((_, a) in fn.autos) if (a is Auto.NewId && sch.lookupTable(a.table) == null) err(Complaint.UnknownAutoTable(a.table))
        fn.ret?.let { noNestedOption(it) }
        val g = G(m, i, fn, emptyMap())
        for ((n, f) in fn.input) checks(g, n, f)
        for ((e, _) in fn.refine) expect(g, "refine", Ty.TBool, e)
        block(g, fn.body)
        if (fn.kind != FnKind.Mutator && fn.kind != FnKind.Guard && !returns(fn.body)) err(Complaint.MayNotReturn)
    }

    private fun subsequence(xs: List<String>, ys: List<String>): Boolean {
        var j = 0
        for (x in xs) {
            while (j < ys.size && ys[j] != x) j++
            if (j == ys.size) return false
            j++
        }
        return true
    }

    // §1.3 A field's checks must fit its type (through an option).
    private fun checks(g: G, name: String, f: Field) {
        val t = (f.ty as? Ty.TOption)?.of ?: f.ty
        for (c in f.checks) {
            val ok = when (c) {
                is Check.Trim, is Check.MinLen, is Check.MaxLen -> t == Ty.TText
                is Check.Range -> t == Ty.TInt
                is Check.NonEmpty -> t is Ty.TList
                is Check.Exists -> {
                    val tbl = (t as? Ty.TId)?.table
                    if (tbl != null) table(g, tbl)
                    tbl != null
                }
                is Check.Refine -> {
                    val g2 = G(g.mod, g.index, g.fn.copy(input = g.fn.input.map { (n, fl) -> if (n == name) n to Field(t, emptyList()) else n to fl }), g.locals)
                    expect(g2, "refine of $name", Ty.TBool, c.e)
                    true
                }
            }
            if (!ok) err(Complaint.BadCheck(name, c.toString()))
        }
    }

    private class G(val mod: Module, val index: Int, val fn: Function, val locals: Map<Sym, Ty>) {
        val schema: Schema get() = mod.schema
        val kind: FnKind get() = fn.kind
        fun bind(x: Sym, t: Ty): G = G(mod, index, fn, locals + (x to t))
    }

    private fun noNestedOption(t: Ty) {
        when (t) {
            is Ty.TOption -> {
                if (t.of is Ty.TOption) err(Complaint.NestedOption)
                noNestedOption(t.of)
            }
            is Ty.TList -> noNestedOption(t.of)
            is Ty.TStruct -> t.fields.values.forEach { noNestedOption(it) }
            else -> Unit
        }
    }

    // A block definitely returns when its last statement does, or is an if both of whose branches do.
    private fun returns(stmts: List<Stmt>): Boolean {
        if (stmts.isEmpty()) return false
        return when (val last = stmts.last()) {
            is Stmt.Return -> true
            is Stmt.Refuse -> true
            is Stmt.If -> returns(last.then) && returns(last.els)
            else -> false
        }
    }

    private fun block(g0: G, stmts: List<Stmt>): G {
        var g = g0
        for (s in stmts) g = stmt(g, s)
        return g
    }

    private fun stmt(g: G, s: Stmt): G {
        fun mutating() {
            if (g.kind != FnKind.Mutator) err(Complaint.WriteOutsideMutator)
        }
        fun readOk() {
            if (g.kind == FnKind.Helper) err(Complaint.ReadInHelper)
        }
        when (s) {
            is Stmt.Let -> {
                val e = s.e
                val t = when (e) {
                    is Expr.Select -> {
                        readOk()
                        planTy(g, e.plan)
                    }
                    is Expr.Get -> {
                        readOk()
                        keyed(g, e.table, e.key)
                        Ty.TOption(table(g, e.table).rowTy)
                    }
                    is Expr.Exists -> {
                        readOk()
                        keyed(g, e.table, e.key)
                        Ty.TBool
                    }
                    else -> infer(g, null, e)
                }
                return g.bind(s.sym, t)
            }
            is Stmt.If -> {
                expect(g, "if condition", Ty.TBool, s.c)
                block(g, s.then)
                block(g, s.els)
                return g
            }
            is Stmt.For -> {
                val t = elemOf("for", infer(g, null, s.over))
                block(g.bind(s.sym, t), s.body)
                return g
            }
            is Stmt.Insert -> {
                write(g, s.table, s.row, s.on)
                return g
            }
            is Stmt.Upsert -> {
                write(g, s.table, s.row, s.on)
                return g
            }
            is Stmt.Update -> {
                mutating()
                keyed(g, s.table, s.key)
                val t = table(g, s.table)
                rowOf(g.bind(s.sym, t.rowTy), s.table, s.row)
                return g
            }
            is Stmt.Delete -> {
                mutating()
                keyed(g, s.table, s.key)
                return g
            }
            is Stmt.Refuse -> {
                if (g.kind != FnKind.Mutator && !g.kind.isMiddleware) err(Complaint.RefuseOutsideMutator)
                expect(g, "refuse", Ty.TText, s.e)
                return g
            }
            is Stmt.Return -> {
                val want = g.fn.ret
                val e = s.e
                when {
                    want == null && e == null -> Unit
                    want == null -> err(Complaint.ReturnTypeOnMutator)
                    e == null -> err(Complaint.NoReturnType)
                    else -> expect(g, "return", want, e)
                }
                return g
            }
        }
    }

    // A written row: every field it has a column of the right type, every
    // non-nullable column there; `on` empty or a declared unique index.
    private fun write(g: G, tbl: String, row: Expr, on: List<String>) {
        if (g.kind != FnKind.Mutator) err(Complaint.WriteOutsideMutator)
        val t = table(g, tbl)
        if (on.isNotEmpty() && t.indexes.none { it.unique && it.columns == on } && on != t.key) err(Complaint.OnNotUnique(tbl, on))
        rowOf(g, tbl, row)
    }

    private fun rowOf(g: G, tbl: String, row: Expr) {
        val t = table(g, tbl)
        val want = t.rowTy
        val got = infer(g, want, row)
        if (got is Ty.TStruct) {
            for ((k, ty) in got.fields) {
                val w = want.fields[k] ?: err(Complaint.UnknownColumn(tbl, k))
                if (w != ty) err(Complaint.TypeMismatch("write $tbl.$k", w, ty))
            }
            for (c in t.columns) {
                if (!c.nullable && !got.fields.containsKey(c.name)) err(Complaint.TypeMismatch("write $tbl", want, got))
            }
        } else {
            err(Complaint.TypeMismatch("write $tbl", want, got))
        }
    }

    private fun table(g: G, tbl: String): Table = g.schema.lookupTable(tbl) ?: err(Complaint.UnknownTable(tbl))

    private fun keyed(g: G, tbl: String, ks: List<Expr>) {
        val t = table(g, tbl)
        val want = t.keyTy
        if (want.size != ks.size) err(Complaint.KeyArity(tbl, want.size, ks.size))
        for ((w, k) in want.zip(ks)) expect(g, "key of $tbl", w, k)
    }

    private fun expect(g: G, site: String, want: Ty, e: Expr) {
        val got = infer(g, want, e)
        if (got != want) err(Complaint.TypeMismatch(site, want, got))
    }

    private fun elemOf(site: String, t: Ty): Ty = if (t is Ty.TList) t.of else err(Complaint.TypeMismatch(site, Ty.TList(t), t))

    // §9.1 Typing of expressions.
    private fun infer(g: G, want: Ty?, e: Expr): Ty = when (e) {
        is Expr.Lit -> lit(want, e.v)
        is Expr.Arg -> g.fn.args.firstOrNull { it.first == e.name }?.second ?: err(Complaint.UnknownArg(e.name))
        is Expr.Auto -> when (val a = g.fn.autos.firstOrNull { it.first == e.name }?.second) {
            is Auto.NewId -> Ty.TId(a.table)
            is Auto.Now -> Ty.TInt
            null -> err(Complaint.UnknownAuto(e.name))
        }
        is Expr.Var -> g.locals[e.sym] ?: err(Complaint.UnboundSymbol(e.sym))
        is Expr.CtxUser -> Ty.TText
        is Expr.CtxSession -> Ty.TText
        is Expr.Field -> when (val t = infer(g, null, e.e)) {
            is Ty.TStruct -> t.fields[e.name] ?: err(Complaint.NoSuchField(e.name))
            else -> err(Complaint.NotAStruct(e.name))
        }
        is Expr.Struct -> {
            val wantF = (want as? Ty.TStruct)?.fields ?: emptyMap()
            Ty.TStruct(e.fields.mapValues { (k, x) -> infer(g, wantF[k], x) })
        }
        is Expr.ListE -> {
            val wantE = (want as? Ty.TList)?.of
            val ts = e.items.map { infer(g, wantE, it) }
            when {
                ts.isEmpty() && wantE != null -> Ty.TList(wantE)
                ts.isEmpty() -> err(Complaint.NeedsAnnotation("empty list"))
                else -> {
                    val t = ts[0]
                    for (t2 in ts.drop(1)) if (t2 != t) err(Complaint.TypeMismatch("list element", t, t2))
                    Ty.TList(t)
                }
            }
        }
        is Expr.Some -> {
            val t = infer(g, (want as? Ty.TOption)?.of, e.e)
            if (t is Ty.TOption) err(Complaint.NestedOption)
            Ty.TOption(t)
        }
        is Expr.None -> {
            noNestedOption(Ty.TOption(e.ty))
            Ty.TOption(e.ty)
        }
        is Expr.Match -> {
            val te = infer(g, null, e.e)
            val inner = if (te is Ty.TOption) te.of else err(Complaint.TypeMismatch("match", Ty.TOption(te), te))
            val ta = infer(g.bind(e.sym, inner), want, e.some)
            val tb = infer(g, ta, e.none)
            if (ta != tb) err(Complaint.TypeMismatch("match arms", ta, tb))
            ta
        }
        is Expr.If -> {
            expect(g, "if", Ty.TBool, e.c)
            val ta = infer(g, want, e.then)
            val tb = infer(g, ta, e.els)
            if (ta != tb) err(Complaint.TypeMismatch("if arms", ta, tb))
            ta
        }
        is Expr.Op -> {
            val es = e.args
            when {
                e.op == Op.And && es.size == 2 -> bools(g, es)
                e.op == Op.Or && es.size == 2 -> bools(g, es)
                e.op == Op.Not && es.size == 1 -> bools(g, es)
                e.op == Op.Neg && es.size == 1 -> ints(g, es)
                es.size == 2 && e.op in setOf(Op.Add, Op.Sub, Op.Mul, Op.Div, Op.Mod) -> ints(g, es)
                else -> err(Complaint.BadOp(e.op.name))
            }
        }
        is Expr.Cmp -> {
            val ta = infer(g, null, e.l)
            expect(g, "comparison", ta, e.r)
            Ty.TBool
        }
        is Expr.Call -> {
            val fns = g.mod.functions
            val j = fns.indexOfFirst { it.name == e.fn }
            if (j < 0) err(Complaint.UnknownHelper(e.fn))
            val f = fns[j]
            if (f.kind != FnKind.Helper) err(Complaint.NotAHelper(e.fn))
            if (j >= g.index) err(Complaint.HelperNotYetDeclared(e.fn))
            if (e.args.size != f.args.size) err(Complaint.Arity(e.fn, f.args.size, e.args.size))
            for ((a, x) in f.args.zip(e.args)) expect(g, "argument of ${e.fn}", a.second, x)
            f.ret ?: err(Complaint.NotAHelper(e.fn))
        }
        is Expr.Std -> stdTy(e.fn, e.args.map { infer(g, null, it) }, want)
        is Expr.Map -> {
            val t = elemOf("map", infer(g, null, e.over))
            Ty.TList(infer(g.bind(e.sym, t), (want as? Ty.TList)?.of, e.body))
        }
        is Expr.Filter -> {
            val t = elemOf("filter", infer(g, null, e.over))
            expect(g.bind(e.sym, t), "filter body", Ty.TBool, e.body)
            Ty.TList(t)
        }
        is Expr.Any -> {
            val t = elemOf("any", infer(g, null, e.over))
            expect(g.bind(e.sym, t), "any body", Ty.TBool, e.body)
            Ty.TBool
        }
        is Expr.All -> {
            val t = elemOf("all", infer(g, null, e.over))
            expect(g.bind(e.sym, t), "all body", Ty.TBool, e.body)
            Ty.TBool
        }
        is Expr.SortBy -> {
            val t = elemOf("sort_by", infer(g, null, e.over))
            infer(g.bind(e.sym, t), null, e.key)
            Ty.TList(t)
        }
        is Expr.Fold -> {
            val t = elemOf("fold", infer(g, null, e.over))
            val a = infer(g, want, e.init)
            expect(g.bind(e.acc, a).bind(e.sym, t), "fold body", a, e.body)
            a
        }
        is Expr.Select -> err(Complaint.ReadNotBound)
        is Expr.Get -> err(Complaint.ReadNotBound)
        is Expr.Exists -> err(Complaint.ReadNotBound)
        is Expr.Provided -> {
            if (e.fn !in g.fn.uses) err(Complaint.NotProvided(e.fn))
            val p = g.mod.lookupFunction(e.fn)
            if (p == null || p.kind != FnKind.Provide) err(Complaint.NotProvided(e.fn))
            p.ret ?: err(Complaint.NotProvided(e.fn))
        }
    }

    private fun bools(g: G, es: List<Expr>): Ty {
        for (x in es) expect(g, "boolean operator", Ty.TBool, x)
        return Ty.TBool
    }

    private fun ints(g: G, es: List<Expr>): Ty {
        for (x in es) expect(g, "arithmetic", Ty.TInt, x)
        return Ty.TInt
    }

    private fun lit(want: Ty?, v: Value): Ty = when (v) {
        is Value.VNull -> err(Complaint.NeedsAnnotation("null literal; use None"))
        is Value.VBool -> Ty.TBool
        is Value.VInt -> Ty.TInt
        is Value.VText -> if (want is Ty.TEnum) want else Ty.TText
        is Value.VBytes -> Ty.TBytes
        is Value.VId -> if (want is Ty.TId) want else err(Complaint.NeedsAnnotation("id literal"))
        is Value.VList -> err(Complaint.NeedsAnnotation("list literal; use a list expression"))
        is Value.VStruct -> err(Complaint.NeedsAnnotation("struct literal; use a struct expression"))
    }

    // §9.2 The type of a plan's rows.
    private fun planTy(g: G, p: IR.Plan): Ty {
        val t = table(g, p.table)
        fun col(c: String): Column = t.column(c) ?: err(Complaint.UnknownColumn(t.name, c))
        fun predOk(q: IR.Pred) {
            when (q) {
                is IR.Pred.Cmp -> expect(g, "filter on ${q.column}", col(q.column).columnTy, q.e)
                is IR.Pred.In -> {
                    val cl = col(q.column)
                    for (x in q.items) expect(g, "filter on ${q.column}", cl.columnTy, x)
                }
                is IR.Pred.All -> q.items.forEach { predOk(it) }
                is IR.Pred.Any -> q.items.forEach { predOk(it) }
                is IR.Pred.Not -> predOk(q.e)
            }
        }
        p.filter?.let { predOk(it) }
        for ((c, _) in p.order) col(c)
        val rels = p.related.map { r ->
            val rel = r.relation
            if (rel.parent != p.table || rel !in g.schema.childrenOf(p.table)) err(Complaint.BadRelation(p.table, rel.child))
            if (r.plan.table != rel.child) err(Complaint.BadRelation(p.table, r.plan.table))
            r.name to planTy(g, r.plan)
        }
        val base = t.rowTy.fields
        return Ty.TList(Ty.TStruct(base + rels.toMap()))
    }

    // §9.3 Signatures of the standard library.
    private fun stdTy(f: StdFn, ts: List<Ty>, want: Ty?): Ty {
        fun misuse(): Nothing = err(Complaint.StdMisuse("$f applied to $ts"))
        fun option(t: Ty): Ty = if (t is Ty.TOption) err(Complaint.NestedOption) else Ty.TOption(t)
        val t0 = ts.getOrNull(0)
        val t1 = ts.getOrNull(1)
        val t2 = ts.getOrNull(2)
        return when (f) {
            StdFn.Trim -> if (ts == listOf(Ty.TText)) Ty.TText else misuse()
            StdFn.IsEmpty -> if (ts == listOf(Ty.TText)) Ty.TBool else misuse()
            StdFn.Concat -> if (ts == listOf(Ty.TList(Ty.TText))) Ty.TText else misuse()
            StdFn.Lower -> if (ts == listOf(Ty.TText)) Ty.TText else misuse()
            StdFn.IsAlnum -> if (ts == listOf(Ty.TText)) Ty.TBool else misuse()
            StdFn.Chars -> if (ts == listOf(Ty.TText)) Ty.TList(Ty.TText) else misuse()
            StdFn.TextLen -> if (ts == listOf(Ty.TText)) Ty.TInt else misuse()
            StdFn.StartsWith -> if (ts == listOf(Ty.TText, Ty.TText)) Ty.TBool else misuse()
            StdFn.SplitOnce -> if (ts == listOf(Ty.TText, Ty.TText)) {
                Ty.TOption(Ty.TStruct(mapOf("before" to Ty.TText, "after" to Ty.TText)))
            } else {
                misuse()
            }
            StdFn.TextOfInt -> if (ts == listOf(Ty.TInt)) Ty.TText else misuse()
            StdFn.Hex -> if (ts == listOf(Ty.TBytes)) Ty.TText else misuse()
            StdFn.Min -> if (ts == listOf(Ty.TInt, Ty.TInt)) Ty.TInt else misuse()
            StdFn.Max -> if (ts == listOf(Ty.TInt, Ty.TInt)) Ty.TInt else misuse()
            StdFn.Clamp -> if (ts == listOf(Ty.TInt, Ty.TInt, Ty.TInt)) Ty.TInt else misuse()
            StdFn.Abs -> if (ts == listOf(Ty.TInt)) Ty.TInt else misuse()
            StdFn.Fnv1a64 -> if (ts == listOf(Ty.TText)) Ty.TInt else misuse()
            StdFn.Sha256 -> if (ts == listOf(Ty.TBytes)) Ty.TBytes else misuse()
            StdFn.IdOfText -> if (ts == listOf(Ty.TText)) {
                if (want is Ty.TOption && want.of is Ty.TId) want else err(Complaint.NeedsAnnotation("id_of_text needs its table from context"))
            } else {
                misuse()
            }
            StdFn.TextOfId -> if (ts.size == 1 && t0 is Ty.TId) Ty.TText else misuse()
            StdFn.NilId -> if (ts.isEmpty()) {
                if (want is Ty.TId) want else err(Complaint.NeedsAnnotation("nil id needs its table from context"))
            } else {
                misuse()
            }
            StdFn.Utf8 -> if (ts == listOf(Ty.TText)) Ty.TBytes else misuse()
            StdFn.First -> if (ts.size == 1 && t0 is Ty.TList) option(t0.of) else misuse()
            StdFn.Last -> if (ts.size == 1 && t0 is Ty.TList) option(t0.of) else misuse()
            StdFn.Len -> if (ts.size == 1 && t0 is Ty.TList) Ty.TInt else misuse()
            StdFn.Contains -> if (ts.size == 2 && t0 is Ty.TList && t0.of == t1) Ty.TBool else misuse()
            StdFn.Reverse -> if (ts.size == 1 && t0 is Ty.TList) Ty.TList(t0.of) else misuse()
            StdFn.IsSome -> if (ts.size == 1 && t0 is Ty.TOption) Ty.TBool else misuse()
            StdFn.UnwrapOr -> if (ts.size == 2 && t0 is Ty.TOption && t0.of == t1) t0.of else misuse()
            StdFn.Unwrap -> if (ts.size == 1 && t0 is Ty.TOption) t0.of else misuse()
        }.also { if (t2 != null && f != StdFn.Clamp) misuse() }
    }

    // §9.4 Make every order total: append the table's key columns ascending.
    public fun completeOrders(m: Module): Module {
        val sch = m.schema
        fun plan(p: IR.Plan): IR.Plan {
            val keyCols = sch.lookupTable(p.table)?.key ?: emptyList()
            val present = p.order.map { it.first }
            val extra = keyCols.filter { it !in present }.map { it to Dir.Asc }
            return p.copy(order = p.order + extra, related = p.related.map { r -> r.copy(plan = plan(r.plan)) })
        }
        fun ex(e: Expr): Expr = when (e) {
            is Expr.Select -> Expr.Select(plan(e.plan))
            is Expr.Field -> Expr.Field(ex(e.e), e.name)
            is Expr.Struct -> Expr.Struct(e.fields.mapValues { ex(it.value) })
            is Expr.ListE -> Expr.ListE(e.items.map { ex(it) })
            is Expr.Some -> Expr.Some(ex(e.e))
            is Expr.Match -> Expr.Match(ex(e.e), e.sym, ex(e.some), ex(e.none))
            is Expr.If -> Expr.If(ex(e.c), ex(e.then), ex(e.els))
            is Expr.Op -> Expr.Op(e.op, e.args.map { ex(it) })
            is Expr.Cmp -> Expr.Cmp(e.op, ex(e.l), ex(e.r))
            is Expr.Call -> Expr.Call(e.fn, e.args.map { ex(it) })
            is Expr.Std -> Expr.Std(e.fn, e.args.map { ex(it) })
            is Expr.Map -> Expr.Map(ex(e.over), e.sym, ex(e.body))
            is Expr.Filter -> Expr.Filter(ex(e.over), e.sym, ex(e.body))
            is Expr.Any -> Expr.Any(ex(e.over), e.sym, ex(e.body))
            is Expr.All -> Expr.All(ex(e.over), e.sym, ex(e.body))
            is Expr.SortBy -> Expr.SortBy(ex(e.over), e.sym, ex(e.key))
            is Expr.Fold -> Expr.Fold(ex(e.over), ex(e.init), e.acc, e.sym, ex(e.body))
            is Expr.Get -> Expr.Get(e.table, e.key.map { ex(it) })
            is Expr.Exists -> Expr.Exists(e.table, e.key.map { ex(it) })
            else -> e
        }
        fun stmt(s: Stmt): Stmt = when (s) {
            is Stmt.Let -> Stmt.Let(s.sym, ex(s.e))
            is Stmt.If -> Stmt.If(ex(s.c), s.then.map { stmt(it) }, s.els.map { stmt(it) })
            is Stmt.For -> Stmt.For(s.sym, ex(s.over), s.body.map { stmt(it) })
            is Stmt.Insert -> Stmt.Insert(s.table, ex(s.row), s.on)
            is Stmt.Upsert -> Stmt.Upsert(s.table, ex(s.row), s.on)
            is Stmt.Update -> Stmt.Update(s.table, s.key.map { ex(it) }, s.sym, ex(s.row))
            is Stmt.Delete -> Stmt.Delete(s.table, s.key.map { ex(it) })
            is Stmt.Refuse -> Stmt.Refuse(ex(s.e))
            is Stmt.Return -> Stmt.Return(s.e?.let { ex(it) })
        }
        return m.copy(
            functions = m.functions.map { f ->
                f.copy(
                    body = f.body.map { stmt(it) },
                    input = f.input.map { (n, fl) -> n to Field(fl.ty, fl.checks.map { c -> if (c is Check.Refine) Check.Refine(ex(c.e), c.why) else c }) },
                    refine = f.refine.map { (e, why) -> ex(e) to why },
                )
            },
        )
    }
}
