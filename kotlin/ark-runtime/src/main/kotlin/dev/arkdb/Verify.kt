// §9 Verification at spec version 4 (`rust/ark/src/verify.rs`): what a
// module must satisfy before anything runs it or hashes it. `verify`
// returns the form a function's hash is taken of — orders completed,
// symbols renumbered — so that the hash is of the program as it will run.
// A module may arrive from anywhere, so every rule is checked, by the name
// the reference gives it.
package dev.arkdb

/** Why a module does not verify (`verify::VerifyError`). */
public sealed class VerifyError {
    public data class BadSpecVersion(val spec: Long) : VerifyError()

    public data class BadSchema(val error: SchemaError) : VerifyError()

    public data class DuplicateFunction(val name: String) : VerifyError()

    public data class DuplicateRouter(val name: String) : VerifyError()

    /** Router, then the complaint: a name in `uses` that is no middleware. */
    public data class InRouter(val router: String, val notMiddleware: String) : VerifyError()

    /** Function, then the complaint. */
    public data class In(val function: String, val complaint: Complaint) : VerifyError()
}

/** What is wrong inside one function (`verify::Complaint`), by the reference's names. */
public sealed class Complaint {
    override fun toString(): String = javaClass.simpleName

    public object AutosOnNonMutator : Complaint()

    public object ReturnTypeOnMutator : Complaint()

    public object NoReturnType : Complaint()

    public data class DuplicateName(val name: String) : Complaint()

    public data class UnknownAutoTable(val table: String) : Complaint()

    public object NestedOption : Complaint()

    public data class TypeMismatch(val site: String, val want: Ty, val got: Ty) : Complaint()

    public data class NotAStruct(val field: String) : Complaint()

    public data class NoSuchField(val field: String) : Complaint()

    public data class UnknownArg(val name: String) : Complaint()

    public data class UnknownAuto(val name: String) : Complaint()

    public data class UnboundSymbol(val sym: Sym) : Complaint()

    public object ReadNotBound : Complaint()

    public object ReadInHelper : Complaint()

    public object WriteOutsideMutator : Complaint()

    public object RefuseInHelper : Complaint()

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

    public data class UnknownRouter(val router: String?) : Complaint()

    public object RouterOnNonProcedure : Complaint()

    public object UsesOnNonProcedure : Complaint()

    public data class UsesNotOnRouter(val uses: List<String>) : Complaint()

    public data class NotMiddleware(val name: String) : Complaint()

    public data class MiddlewareNotYetDeclared(val name: String) : Complaint()

    public data class MiddlewareInput(val middleware: String, val field: String) : Complaint()

    public object ChecksOnMiddleware : Complaint()

    public data class CheckOnWrongType(val field: String, val check: String) : Complaint()

    public data class OnNotUnique(val table: String, val columns: List<String>) : Complaint()

    public data class NotProvided(val name: String) : Complaint()

    public object RefineOnNonProcedure : Complaint()

    public object QueryWithoutPlan : Complaint()

    public object QueryWithBody : Complaint()

    public object PlanOutsideQuery : Complaint()

    public data class PlanFeatureOutsideQuery(val what: String) : Complaint()

    public object ReadInPlan : Complaint()

    public object AutoInPlan : Complaint()

    public object NoRowBinder : Complaint()

    public object MembersWithoutGroup : Complaint()

    public object EmptyRole : Complaint()

    public object ScopeLeafInPlan : Complaint()

    public data class NotAScope(val what: String) : Complaint()

    public object ScopeHoldsNothing : Complaint()

    public object HoldsOutsideScope : Complaint()

    public data class ScopeReadsBeyondCtx(val what: String) : Complaint()

    public data class ExistsNotAReference(val table: String, val column: String) : Complaint()

    public object NestedExists : Complaint()

    public data class ProjectionDropsKey(val table: String, val column: String) : Complaint()

    public data class NotHeld(val table: String, val column: String, val roles: List<String>) :
        Complaint()

    public object PrivateOutsideMutator : Complaint()

    public object NestedPrivate : Complaint()

    public object ReturnInPrivate : Complaint()

    public object PrivateUndeclared : Complaint()
}

/** A module that did not verify, and every reason. */
public class VerifyFailed(public val errors: List<VerifyError>) :
    Exception(errors.joinToString("; "))

private class Complain(val c: Complaint) : Exception(null, null, false, false)

private fun complain(c: Complaint): Nothing = throw Complain(c)

/** Comparison under the one total order (`view::cmp`). */
public fun cmp(op: CmpOp, a: Value, b: Value): Boolean {
    val o = compareValue(a, b)
    return when (op) {
        CmpOp.Eq -> o == 0
        CmpOp.Ne -> o != 0
        CmpOp.Lt -> o < 0
        CmpOp.Le -> o <= 0
        CmpOp.Gt -> o > 0
        CmpOp.Ge -> o >= 0
    }
}

// D2 The columns a function names, as the verifier meets them: by table
// where the table is known, and otherwise as a field read of a struct,
// resolved after to the table whose row the struct is.
internal class Named {
    val columns: MutableSet<Pair<String, String>> = LinkedHashSet()
    val fields: MutableList<Pair<Map<String, Ty>, String>> = ArrayList()

    fun columns(sch: Schema): Set<Pair<String, String>> {
        val out = LinkedHashSet(columns)
        for ((fs, f) in fields) {
            for (t in sch.tables) {
                val row = t.column(f) != null && t.columns.all { fs[it.name] == it.columnTy }
                if (row) out.add(t.name to f)
            }
        }
        return out
    }
}

public object Verify {
    /** `verify`: the module as it is to be hashed and run, or every reason it is not. */
    public fun verify(m0: Module): Module {
        val m = completeOrders(m0)
        if (m.spec != SPEC_VERSION) throw VerifyFailed(listOf(VerifyError.BadSpecVersion(m.spec)))
        val schemaErrors = checkSchema(m.schema)
        if (schemaErrors.isNotEmpty())
            throw VerifyFailed(schemaErrors.map { VerifyError.BadSchema(it) })
        val dups = duplicates(m.functions.map { it.name })
        if (dups.isNotEmpty()) throw VerifyFailed(dups.map { VerifyError.DuplicateFunction(it) })
        val rdups = duplicates(m.routers.map { it.name })
        if (rdups.isNotEmpty()) throw VerifyFailed(rdups.map { VerifyError.DuplicateRouter(it) })
        val errs = ArrayList<VerifyError>()
        for (r in m.routers) {
            for (u in r.uses) {
                if (m.lookupFunction(u)?.kind?.isMiddleware != true)
                    errs.add(VerifyError.InRouter(r.name, u))
            }
        }
        val named = LinkedHashMap<String, Set<Pair<String, String>>>()
        for ((i, f) in m.functions.withIndex()) {
            val seen = Named()
            try {
                functionNaming(m, i, f, seen)
            } catch (c: Complain) {
                errs.add(VerifyError.In(f.name, c.c))
            }
            named[f.name] = seen.columns(m.schema)
        }
        // D2 What a client runs names only what its person holds.
        if (errs.isEmpty()) errs.addAll(ScopeCheck.check(m, named))
        if (errs.isNotEmpty()) throw VerifyFailed(errs)
        return m.copy(functions = m.functions.map(Normalize::function))
    }

    private fun duplicates(names: List<String>): List<String> {
        val seen = HashSet<String>()
        val out = ArrayList<String>()
        for (n in names) if (!seen.add(n) && n !in out) out.add(n)
        return out
    }

    private fun isSubsequence(xs: List<String>, ys: List<String>): Boolean {
        val it = ys.iterator()
        return xs.all { x ->
            var found = false
            while (it.hasNext()) {
                if (it.next() == x) {
                    found = true
                    break
                }
            }
            found
        }
    }

    // The context a function is typed in.
    internal data class G(
        val m: Module,
        val named: Named,
        val index: Int,
        val f: Function,
        val kind: FnKind,
        val args: Map<String, Ty>,
        val provided: Map<String, Ty>,
        val locals: Map<Sym, Ty>,
        val private: Boolean,
    ) {
        val schema: Schema
            get() = m.schema

        fun names(t: String, c: String) {
            named.columns.add(t to c)
        }

        fun bind(x: Sym, t: Ty): G = copy(locals = locals + (x to t))
    }

    private fun functionNaming(m: Module, i: Int, f: Function, named: Named) {
        val sch = m.schema
        when (f.kind) {
            FnKind.Mutator,
            FnKind.Query -> {
                val rn = f.router ?: complain(Complaint.UnknownRouter(null))
                val r = m.lookupRouter(rn) ?: complain(Complaint.UnknownRouter(rn))
                if (!isSubsequence(f.uses, r.uses)) complain(Complaint.UsesNotOnRouter(f.uses))
                for (u in f.uses) {
                    val j = m.functions.indexOfFirst { it.name == u }
                    val g = if (j >= 0) m.functions[j] else null
                    if (g == null || !g.kind.isMiddleware) complain(Complaint.NotMiddleware(u))
                    if (j >= i) complain(Complaint.MiddlewareNotYetDeclared(u))
                    for ((n, fd) in g.input) {
                        val here = f.input.firstOrNull { it.first == n }?.second?.ty
                        if (here != fd.ty) complain(Complaint.MiddlewareInput(u, n))
                    }
                }
                if (f.kind == FnKind.Mutator && f.ret != null)
                    complain(Complaint.ReturnTypeOnMutator)
                if (f.kind == FnKind.Query && f.ret == null) complain(Complaint.NoReturnType)
                if (f.kind == FnKind.Query && f.autos.isNotEmpty())
                    complain(Complaint.AutosOnNonMutator)
                if (f.kind == FnKind.Query) {
                    if (f.plan == null) complain(Complaint.QueryWithoutPlan)
                    if (f.body.isNotEmpty()) complain(Complaint.QueryWithBody)
                }
            }
            FnKind.Guard,
            FnKind.Provide -> {
                if (f.router != null) complain(Complaint.RouterOnNonProcedure)
                if (f.uses.isNotEmpty()) complain(Complaint.UsesOnNonProcedure)
                if (f.autos.isNotEmpty()) complain(Complaint.AutosOnNonMutator)
                if (f.input.any { it.second.checks.isNotEmpty() } || f.refine.isNotEmpty())
                    complain(Complaint.ChecksOnMiddleware)
                if (f.kind == FnKind.Guard && f.ret != null) complain(Complaint.ReturnTypeOnMutator)
                if (f.kind == FnKind.Provide && f.ret == null) complain(Complaint.NoReturnType)
            }
            FnKind.Scope -> return scopeOk(m, i, f)
            FnKind.Helper -> {
                if (f.router != null) complain(Complaint.RouterOnNonProcedure)
                if (f.uses.isNotEmpty()) complain(Complaint.UsesOnNonProcedure)
                if (f.autos.isNotEmpty()) complain(Complaint.AutosOnNonMutator)
                if (f.ret == null) complain(Complaint.NoReturnType)
                if (f.input.any { it.second.checks.isNotEmpty() } || f.refine.isNotEmpty())
                    complain(Complaint.RefineOnNonProcedure)
            }
        }
        if (f.holds.isNotEmpty()) complain(Complaint.HoldsOutsideScope)
        // D3 Only a mutator has a server half, and one that has one says so.
        if (f.private && f.kind != FnKind.Mutator) complain(Complaint.PrivateOutsideMutator)
        if (!f.private && hasPrivate(f.body)) complain(Complaint.PrivateUndeclared)
        duplicates(f.input.map { it.first } + f.autos.map { it.first }).firstOrNull()?.let {
            complain(Complaint.DuplicateName(it))
        }
        for ((_, fd) in f.input) noNestedOption(fd.ty)
        for ((_, a) in f.autos) if (a is Auto.NewId && sch.lookupTable(a.table) == null)
            complain(Complaint.UnknownAutoTable(a.table))
        f.ret?.let(::noNestedOption)
        val args = f.input.associate { (n, fd) -> n to fd.ty }
        val provided = LinkedHashMap<String, Ty>()
        for (u in f.uses) {
            val g = m.lookupFunction(u) ?: continue
            if (g.kind == FnKind.Provide) g.ret?.let { provided[g.name] = it }
        }
        val g = G(m, named, i, f, f.kind, args, provided, emptyMap(), false)
        // The input's checks, each over its own field as it stands.
        for ((n, fd) in f.input) {
            val inner = (fd.ty as? Ty.TOption)?.of ?: fd.ty
            for (c in fd.checks) {
                val fits =
                    when (c) {
                        is Check.Trim,
                        is Check.MinLen,
                        is Check.MaxLen -> inner == Ty.TText
                        is Check.Range -> inner == Ty.TInt
                        is Check.NonEmpty -> inner is Ty.TList
                        is Check.Exists ->
                            if (inner is Ty.TId) {
                                if (sch.lookupTable(inner.table) == null)
                                    complain(Complaint.UnknownTable(inner.table))
                                true
                            } else false
                        is Check.Refine -> {
                            val gc =
                                g.copy(
                                    args = args + (n to inner),
                                    kind = FnKind.Helper,
                                    provided = emptyMap())
                            expect(gc, "refine", Ty.TBool, c.e)
                            true
                        }
                    }
                if (!fits) complain(Complaint.CheckOnWrongType(n, c.toString()))
            }
        }
        for ((e, _) in f.refine) expect(
            g.copy(kind = FnKind.Helper, provided = emptyMap()), "refine", Ty.TBool, e)
        val p = f.plan
        if (p != null) {
            if (f.kind != FnKind.Query) complain(Complaint.PlanOutsideQuery)
            val got = Ty.TList(queryPlan(g, p))
            val ret = f.ret
            if (ret == null || !same(sch, ret, got))
                complain(Complaint.TypeMismatch("query result", ret ?: Ty.TBool, got))
            return
        }
        block(g, f.body)
        val mustReturn =
            f.kind == FnKind.Query || f.kind == FnKind.Helper || f.kind == FnKind.Provide
        if (mustReturn && !returns(f.body)) complain(Complaint.MayNotReturn)
    }

    private fun noNestedOption(t: Ty) {
        when (t) {
            is Ty.TOption ->
                if (t.of is Ty.TOption) complain(Complaint.NestedOption) else noNestedOption(t.of)
            is Ty.TList -> noNestedOption(t.of)
            is Ty.TStruct -> t.fields.values.forEach(::noNestedOption)
            else -> {}
        }
    }

    // A block definitely returns when its last statement does, or is an `if` both of whose branches
    // do.
    private fun returns(b: Block): Boolean =
        when (val s = b.lastOrNull()) {
            is Stmt.Return,
            is Stmt.Refuse -> true
            is Stmt.If -> returns(s.then) && returns(s.els)
            else -> false
        }

    private fun block(g: G, b: Block) {
        var g2 = g
        for (s in b) g2 = stmt(g2, s)
    }

    private fun stmt(g: G, s: Stmt): G {
        fun readOk() {
            if (g.kind == FnKind.Helper) complain(Complaint.ReadInHelper)
        }
        fun mutating() {
            if (g.kind != FnKind.Mutator) complain(Complaint.WriteOutsideMutator)
        }
        when (s) {
            is Stmt.Let -> {
                val e = s.e
                val t =
                    when (e) {
                        is Expr.Select -> {
                            readOk()
                            planTy(g, e.plan)
                        }
                        is Expr.Get -> {
                            readOk()
                            keyed(g, e.table, e.key)
                            Ty.TOption(table(g, e.table).rowTy)
                        }
                        is Expr.ExistsE -> {
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
            }
            is Stmt.For -> {
                val t = elemOf("for", infer(g, null, s.over))
                block(g.bind(s.sym, t), s.body)
            }
            is Stmt.Insert -> insertLike(g, s.table, s.row, s.on, ::mutating)
            is Stmt.Upsert -> insertLike(g, s.table, s.row, s.on, ::mutating)
            is Stmt.Update -> {
                mutating()
                keyed(g, s.table, s.key)
                written(g, s.table, s.row)
                val tbl = table(g, s.table)
                rowFits(g.bind(s.sym, tbl.rowTy), tbl, s.row)
            }
            is Stmt.Delete -> {
                mutating()
                keyed(g, s.table, s.key)
            }
            is Stmt.Refuse -> {
                if (g.kind == FnKind.Helper) complain(Complaint.RefuseInHelper)
                expect(g, "refuse", Ty.TText, s.e)
            }
            is Stmt.Return -> {
                if (g.private) complain(Complaint.ReturnInPrivate)
                val want = g.f.ret
                val e = s.e
                when {
                    want == null && e == null -> {}
                    want == null -> complain(Complaint.ReturnTypeOnMutator)
                    e == null -> complain(Complaint.NoReturnType)
                    else -> expect(g, "return", want, e)
                }
            }
            // D3 Typed as the body around it is; what it names goes nowhere.
            is Stmt.Private -> {
                if (g.kind != FnKind.Mutator) complain(Complaint.PrivateOutsideMutator)
                if (g.private) complain(Complaint.NestedPrivate)
                block(g.copy(named = Named(), private = true), s.body)
            }
        }
        return g
    }

    private fun insertLike(g: G, t: String, e: Expr, on: List<String>, mutating: () -> Unit) {
        mutating()
        val tbl = table(g, t)
        written(g, t, e)
        on.forEach { g.names(t, it) }
        rowFits(g, tbl, e)
        if (on.isNotEmpty()) {
            val want = on.toSet()
            val unique =
                tbl.indexes.any {
                    it.unique && it.columns.toSet() == want && it.columns.size == on.size
                }
            if (!unique) complain(Complaint.OnNotUnique(t, on))
        }
    }

    // The columns a write names: every field of the struct it writes, written out (D2).
    private fun written(g: G, t: String, e: Expr) {
        if (e is Expr.StructE) e.fields.keys.forEach { g.names(t, it) }
    }

    // A written row may leave nullable columns out, so it is checked field by field.
    private fun rowFits(g: G, tbl: Table, e: Expr) {
        val row = tbl.rowTy as Ty.TStruct
        val got = infer(g, row, e)
        val site = "write ${tbl.name}"
        if (got !is Ty.TStruct) complain(Complaint.TypeMismatch(site, row, got))
        for ((k, ty) in got.fields) {
            val w = row.fields[k] ?: complain(Complaint.UnknownColumn(tbl.name, k))
            if (w != ty) complain(Complaint.TypeMismatch("$site.$k", w, ty))
        }
        for (c in tbl.columns) if (!c.nullable && !got.fields.containsKey(c.name))
            complain(Complaint.TypeMismatch(site, row, got))
    }

    private fun table(g: G, t: String): Table =
        g.schema.lookupTable(t) ?: complain(Complaint.UnknownTable(t))

    private fun keyed(g: G, t: String, ks: List<Expr>) {
        val want = table(g, t).keyTy
        if (want.size != ks.size) complain(Complaint.KeyArity(t, want.size, ks.size))
        for ((w, k) in want.zip(ks)) expect(g, "key of $t", w, k)
    }

    /**
     * §9 `same`: two types the same, where a table's row is that table's row whichever schema it
     * was written against — structs differing only in nullable columns of one table, carried whole
     * by the wider, are the same type (`docs/plan-db.md` D1).
     */
    internal fun same(sch: Schema, a: Ty, b: Ty): Boolean {
        if (a == b) return true
        return when {
            a is Ty.TOption && b is Ty.TOption -> same(sch, a.of, b.of)
            a is Ty.TList && b is Ty.TList -> same(sch, a.of, b.of)
            a is Ty.TStruct && b is Ty.TStruct -> {
                val (wide, narrow) =
                    if (a.fields.size >= b.fields.size) a.fields to b.fields
                    else b.fields to a.fields
                if (!narrow.all { (k, t) -> wide[k]?.let { same(sch, it, t) } == true })
                    return false
                val extra = wide.keys.filter { !narrow.containsKey(it) }
                if (extra.isEmpty()) return true
                sch.tables.any { tbl ->
                    tbl.columns.all { wide[it.name] == it.columnTy } &&
                        extra.all { tbl.column(it)?.nullable == true }
                }
            }
            else -> false
        }
    }

    private fun expect(g: G, site: String, want: Ty, e: Expr) {
        val got = infer(g, want, e)
        if (!same(g.schema, got, want)) complain(Complaint.TypeMismatch(site, want, got))
    }

    private fun elemOf(site: String, t: Ty): Ty =
        (t as? Ty.TList)?.of ?: complain(Complaint.TypeMismatch(site, Ty.TList(t), t))

    // §9.1 Typing of expressions; the expected type is used only where an
    // expression cannot be typed on its own.
    private fun infer(g: G, want: Ty?, e: Expr): Ty =
        when (e) {
            is Expr.Lit -> lit(want, e.v)
            is Expr.Arg -> g.args[e.name] ?: complain(Complaint.UnknownArg(e.name))
            is Expr.AutoE -> {
                val a = g.f.autos.firstOrNull { it.first == e.name }?.second
                when {
                    a is Auto.NewId && g.kind == FnKind.Mutator -> Ty.TId(a.table)
                    a is Auto.Now && g.kind == FnKind.Mutator -> Ty.TInt
                    else -> complain(Complaint.UnknownAuto(e.name))
                }
            }
            is Expr.Var -> g.locals[e.sym] ?: complain(Complaint.UnboundSymbol(e.sym))
            is Expr.CtxUser,
            is Expr.CtxSession -> Ty.TText
            is Expr.HasRole -> if (e.role.isEmpty()) complain(Complaint.EmptyRole) else Ty.TBool
            is Expr.Provided -> g.provided[e.fn] ?: complain(Complaint.NotProvided(e.fn))
            is Expr.FieldE -> {
                val t = infer(g, null, e.e)
                if (t !is Ty.TStruct) complain(Complaint.NotAStruct(e.name))
                val ft = t.fields[e.name] ?: complain(Complaint.NoSuchField(e.name))
                g.named.fields.add(t.fields to e.name)
                ft
            }
            is Expr.StructE -> {
                val wf = (want as? Ty.TStruct)?.fields
                Ty.TStruct(e.fields.mapValues { (k, x) -> infer(g, wf?.get(k), x) })
            }
            is Expr.ListE -> {
                val we = (want as? Ty.TList)?.of
                val ts = e.items.map { infer(g, we, it) }
                when {
                    ts.isEmpty() && we != null -> Ty.TList(we)
                    ts.isEmpty() -> complain(Complaint.NeedsAnnotation("empty list"))
                    else -> {
                        val t = ts[0]
                        for (t2 in ts.drop(1)) if (!same(g.schema, t2, t))
                            complain(Complaint.TypeMismatch("list element", t, t2))
                        Ty.TList(t)
                    }
                }
            }
            is Expr.SomeE -> {
                val t = infer(g, (want as? Ty.TOption)?.of, e.e)
                if (t is Ty.TOption) complain(Complaint.NestedOption)
                Ty.TOption(t)
            }
            is Expr.NoneE -> {
                val o = Ty.TOption(e.ty)
                noNestedOption(o)
                o
            }
            is Expr.Match -> {
                val o = infer(g, null, e.e)
                if (o !is Ty.TOption) complain(Complaint.TypeMismatch("match", Ty.TOption(o), o))
                val ta = infer(g.bind(e.sym, o.of), want, e.some)
                val tb = infer(g, ta, e.none)
                if (!same(g.schema, ta, tb)) complain(Complaint.TypeMismatch("match arms", ta, tb))
                ta
            }
            is Expr.IfE -> {
                expect(g, "if", Ty.TBool, e.c)
                val ta = infer(g, want, e.then)
                val tb = infer(g, ta, e.els)
                if (!same(g.schema, ta, tb)) complain(Complaint.TypeMismatch("if arms", ta, tb))
                ta
            }
            is Expr.OpE -> {
                fun all(t: Ty): Ty {
                    for (x in e.args) expect(
                        g, if (t == Ty.TBool) "boolean operator" else "arithmetic", t, x)
                    return t
                }
                val n = e.args.size
                when (e.op) {
                    Op.And,
                    Op.Or -> if (n == 2) all(Ty.TBool) else complain(Complaint.BadOp(e.op.name))
                    Op.Not -> if (n == 1) all(Ty.TBool) else complain(Complaint.BadOp(e.op.name))
                    Op.Neg -> if (n == 1) all(Ty.TInt) else complain(Complaint.BadOp(e.op.name))
                    else -> if (n == 2) all(Ty.TInt) else complain(Complaint.BadOp(e.op.name))
                }
            }
            is Expr.CmpE -> {
                val ta = infer(g, null, e.l)
                expect(g, "comparison", ta, e.r)
                Ty.TBool
            }
            is Expr.Call -> {
                val j = g.m.functions.indexOfFirst { it.name == e.fn }
                if (j < 0) complain(Complaint.UnknownHelper(e.fn))
                val f = g.m.functions[j]
                if (f.kind != FnKind.Helper) complain(Complaint.NotAHelper(e.fn))
                if (j >= g.index) complain(Complaint.HelperNotYetDeclared(e.fn))
                if (e.args.size != f.input.size)
                    complain(Complaint.Arity(e.fn, f.input.size, e.args.size))
                for ((p, x) in f.input.zip(e.args)) expect(g, "argument of ${e.fn}", p.second.ty, x)
                f.ret ?: complain(Complaint.NotAHelper(e.fn))
            }
            is Expr.StdE -> stdTy(e.fn, e.args.map { infer(g, null, it) }, want)
            is Expr.MapE -> {
                val t = elemOf("map", infer(g, null, e.over))
                Ty.TList(infer(g.bind(e.sym, t), (want as? Ty.TList)?.of, e.body))
            }
            is Expr.FilterE -> {
                val t = elemOf("filter", infer(g, null, e.over))
                expect(g.bind(e.sym, t), "filter body", Ty.TBool, e.body)
                Ty.TList(t)
            }
            is Expr.AnyE -> {
                val t = elemOf("any/all", infer(g, null, e.over))
                expect(g.bind(e.sym, t), "any/all body", Ty.TBool, e.body)
                Ty.TBool
            }
            is Expr.AllE -> {
                val t = elemOf("any/all", infer(g, null, e.over))
                expect(g.bind(e.sym, t), "any/all body", Ty.TBool, e.body)
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
                expect(g.bind(e.sym, t).bind(e.acc, a), "fold body", a, e.body)
                a
            }
            is Expr.Select,
            is Expr.Get,
            is Expr.ExistsE -> complain(Complaint.ReadNotBound)
        }

    private fun lit(want: Ty?, v: Value): Ty =
        when (v) {
            is Value.VNull -> complain(Complaint.NeedsAnnotation("null literal; use None"))
            is Value.VBool -> Ty.TBool
            is Value.VInt -> Ty.TInt
            is Value.VText -> if (want is Ty.TEnum) want else Ty.TText
            is Value.VBytes -> Ty.TBytes
            is Value.VId ->
                if (want is Ty.TId) want else complain(Complaint.NeedsAnnotation("id literal"))
            is Value.VList ->
                complain(Complaint.NeedsAnnotation("list literal; use a list expression"))
            is Value.VStruct ->
                complain(Complaint.NeedsAnnotation("struct literal; use a struct expression"))
        }

    // §9.2 The type of a read in a body: the plan must be v3-shaped (§1.4).
    private fun planTy(g: G, p: Plan): Ty {
        v3Shaped(g, p)
        val t = table(g, p.table)
        p.filter?.let { predOk(g, t, it) }
        for ((k, _) in p.order) {
            if (k is Key.Column) {
                g.names(t.name, k.column)
                if (t.column(k.column) == null) complain(Complaint.UnknownColumn(t.name, k.column))
            }
        }
        val fields = LinkedHashMap((t.rowTy as Ty.TStruct).fields)
        for (r in p.related) fields[r.name] = planTy(g, r.plan)
        return Ty.TList(Ty.TStruct(fields))
    }

    // §1.4 What a read outside a query may be, or the first thing it has that it may not.
    private fun v3Shaped(g: G, p: Plan) {
        fun feature(what: String): Nothing = complain(Complaint.PlanFeatureOutsideQuery(what))
        if (p.source is Source.Group) feature("a group source")
        if (p.members != null) complain(Complaint.MembersWithoutGroup)
        if (p.lookups.isNotEmpty()) feature("a lookup")
        if (p.having != null) feature("a having")
        if (p.project != null) feature("a projection")
        if (p.order.any { it.first is Key.ExprK }) feature("an expression order key")
        if (p.related.isNotEmpty() && p.row == null) complain(Complaint.NoRowBinder)
        for (r in p.related) {
            // The one related form a body may read: `child.col == row.key`.
            val on = r.on.singleOrNull()
            val e = on?.second
            val ok =
                on != null &&
                    p.row != null &&
                    e is Expr.FieldE &&
                    e.e == Expr.Var(p.row) &&
                    g.schema.childrenOf(p.table).any {
                        it.child == r.plan.table && it.column == on.first
                    } &&
                    g.schema.lookupTable(p.table)?.key == listOf(e.name)
            if (!ok) feature("a related plan that is not a reference")
        }
    }

    // §1.10 A query's plan: every node typed under its own binders.
    private fun queryPlan(g: G, p: Plan): Ty {
        planReadsNothing(p)
        return nodeTy(g, p)
    }

    private fun nodeTy(g: G, p: Plan): Ty {
        val t = table(g, p.table)
        p.filter?.let { predOk(g, t, it) }
        val rowTy =
            when (val s = p.source) {
                is Source.Table -> t.rowTy
                is Source.Group -> {
                    val fs = LinkedHashMap<String, Ty>()
                    for (c in s.by) {
                        g.names(t.name, c)
                        val col = t.column(c) ?: complain(Complaint.UnknownColumn(t.name, c))
                        fs[c] = col.columnTy
                    }
                    Ty.TStruct(fs)
                }
            }
        if (!p.isBare && p.row == null) complain(Complaint.NoRowBinder)
        if (p.members != null && p.source !is Source.Group) complain(Complaint.MembersWithoutGroup)
        var node = g.copy(locals = emptyMap())
        p.row?.let { node = node.bind(it, rowTy) }
        p.members?.let { node = node.bind(it, Ty.TList(t.rowTy)) }
        for (l in p.lookups) {
            val lt = table(g, l.table)
            val want = lt.keyTy
            if (want.size != l.key.size)
                complain(Complaint.KeyArity(l.table, want.size, l.key.size))
            for ((w, k) in want.zip(l.key)) {
                val got = infer(node, w, k)
                if (got != w && got != Ty.TOption(w))
                    complain(Complaint.TypeMismatch("key of ${l.table}", w, got))
            }
            node = node.bind(l.sym, Ty.TOption(lt.rowTy))
        }
        val fields = LinkedHashMap((rowTy as Ty.TStruct).fields)
        for (r in p.related) {
            val ct = table(g, r.plan.table)
            for ((c, e) in r.on) {
                g.names(ct.name, c)
                val col = ct.column(c) ?: complain(Complaint.UnknownColumn(ct.name, c))
                val want = col.columnTy
                val got = infer(node, want, e)
                if (got != want && stripOption(got) != stripOption(want))
                    complain(Complaint.TypeMismatch("on ${ct.name}.$c", want, got))
            }
            val child = Ty.TList(nodeTy(g, r.plan))
            fields[r.name] = child
            node = node.bind(r.sym, child)
        }
        p.having?.let { expect(node, "having", Ty.TBool, it) }
        for ((k, _) in p.order) {
            when (k) {
                is Key.Column -> {
                    g.names(t.name, k.column)
                    if (!rowTy.fields.containsKey(k.column))
                        complain(Complaint.UnknownColumn(t.name, k.column))
                }
                is Key.ExprK -> infer(node, null, k.e)
            }
        }
        return p.project?.let { infer(node, null, it) } ?: Ty.TStruct(fields)
    }

    private fun stripOption(t: Ty): Ty = (t as? Ty.TOption)?.of ?: t

    // §1.3 No expression in a plan reads or draws an auto, at any depth.
    private fun planReadsNothing(p: Plan) {
        val es = ArrayList<Expr>()
        p.filter?.let { predExprs(it, es) }
        p.lookups.forEach { es.addAll(it.key) }
        p.related.forEach { r -> r.on.forEach { es.add(it.second) } }
        p.having?.let(es::add)
        p.project?.let(es::add)
        for ((k, _) in p.order) if (k is Key.ExprK) es.add(k.e)
        for (e in es) forbidden(e)?.let(::complain)
        p.related.forEach { planReadsNothing(it.plan) }
    }

    private fun predExprs(p: Pred, out: MutableList<Expr>) {
        when (p) {
            is Pred.Cmp -> out.add(p.e)
            is Pred.In -> out.addAll(p.items)
            is Pred.All -> p.items.forEach { predExprs(it, out) }
            is Pred.AnyP -> p.items.forEach { predExprs(it, out) }
            is Pred.Not -> predExprs(p.e, out)
            is Pred.Exists -> predExprs(p.pred, out)
            is Pred.Has -> out.add(p.e)
            is Pred.When -> out.add(p.e)
        }
    }

    // The first read or auto inside an expression.
    private fun forbidden(e: Expr): Complaint? =
        when (e) {
            is Expr.Select,
            is Expr.Get,
            is Expr.ExistsE -> Complaint.ReadInPlan
            is Expr.AutoE -> Complaint.AutoInPlan
            else -> children(e).firstNotNullOfOrNull(::forbidden)
        }

    private fun predOk(g: G, t: Table, p: Pred) {
        fun col(c: String): Column {
            g.names(t.name, c)
            return t.column(c) ?: complain(Complaint.UnknownColumn(t.name, c))
        }
        when (p) {
            is Pred.Cmp -> expect(g, "filter on ${p.column}", col(p.column).columnTy, p.e)
            is Pred.In -> {
                val ty = col(p.column).columnTy
                p.items.forEach { expect(g, "filter on ${p.column}", ty, it) }
            }
            is Pred.All -> p.items.forEach { predOk(g, t, it) }
            is Pred.AnyP -> p.items.forEach { predOk(g, t, it) }
            is Pred.Not -> predOk(g, t, p.e)
            // D4 A substring of a text column, the needle text.
            is Pred.Has -> {
                val ty = col(p.column).columnTy
                if (ty != Ty.TText && ty != Ty.TOption(Ty.TText))
                    complain(Complaint.TypeMismatch("has on ${p.column}", Ty.TText, ty))
                expect(g, "has on ${p.column}", Ty.TText, p.e)
            }
            is Pred.When,
            is Pred.Exists -> complain(Complaint.ScopeLeafInPlan)
        }
    }

    // D2 A scope: middleware with nothing but holds, at least one.
    private fun scopeOk(m: Module, i: Int, f: Function) {
        fun not(what: String): Nothing = complain(Complaint.NotAScope(what))
        if (f.router != null) complain(Complaint.RouterOnNonProcedure)
        if (f.uses.isNotEmpty()) complain(Complaint.UsesOnNonProcedure)
        if (f.autos.isNotEmpty()) complain(Complaint.AutosOnNonMutator)
        if (f.input.isNotEmpty() || f.refine.isNotEmpty()) not("input")
        if (f.ret != null) not("a return type")
        if (f.body.isNotEmpty()) not("a body")
        if (f.plan != null) not("a plan")
        if (f.holds.isEmpty()) complain(Complaint.ScopeHoldsNothing)
        val g = G(m, Named(), i, f, FnKind.Helper, emptyMap(), emptyMap(), emptyMap(), false)
        for (h in f.holds) {
            val t = table(g, h.table)
            h.filter?.let { scopePredOk(g, t, it, false) }
            val named =
                when (val c = h.columns) {
                    is Projection.All -> emptyList()
                    is Projection.Pick -> c.columns
                    is Projection.Exclude -> c.columns
                }
            val seen = HashSet<String>()
            for (c in named) {
                if (t.column(c) == null) complain(Complaint.UnknownColumn(t.name, c))
                if (!seen.add(c)) complain(Complaint.ProjectionDropsKey(t.name, c))
            }
            t.key
                .firstOrNull { !h.columns.keeps(it) }
                ?.let { complain(Complaint.ProjectionDropsKey(t.name, it)) }
        }
    }

    // A scope's predicate: right-hand sides of the context and literals alone,
    // plus `When` and one `Exists` through a reference.
    private fun scopePredOk(g: G, t: Table, p: Pred, nested: Boolean) {
        fun col(c: String): Column = t.column(c) ?: complain(Complaint.UnknownColumn(t.name, c))
        when (p) {
            is Pred.Cmp -> {
                ctxOnly(p.e)
                expect(g, "scope on ${t.name}.${p.column}", col(p.column).columnTy, p.e)
            }
            is Pred.In -> {
                val ty = col(p.column).columnTy
                p.items.forEach {
                    ctxOnly(it)
                    expect(g, "scope on ${t.name}.${p.column}", ty, it)
                }
            }
            is Pred.Has -> {
                ctxOnly(p.e)
                predOk(g, t, p)
            }
            is Pred.When -> {
                ctxOnly(p.e)
                expect(g, "scope condition", Ty.TBool, p.e)
            }
            is Pred.All -> p.items.forEach { scopePredOk(g, t, it, nested) }
            is Pred.AnyP -> p.items.forEach { scopePredOk(g, t, it, nested) }
            is Pred.Not -> scopePredOk(g, t, p.e, nested)
            is Pred.Exists -> {
                if (nested) complain(Complaint.NestedExists)
                val child = table(g, p.table)
                val refers =
                    child.refs.any { it.column == p.column && it.table == t.name } &&
                        t.key.size == 1
                if (!refers) complain(Complaint.ExistsNotAReference(p.table, p.column))
                scopePredOk(g, child, p.pred, true)
            }
        }
    }

    // An expression a scope may hold: the context, literals, and the operators and comparisons over
    // them.
    private fun ctxOnly(e: Expr) {
        fun beyond(what: String): Nothing = complain(Complaint.ScopeReadsBeyondCtx(what))
        when (e) {
            is Expr.Lit,
            is Expr.CtxUser,
            is Expr.CtxSession,
            is Expr.HasRole -> {}
            is Expr.OpE -> e.args.forEach(::ctxOnly)
            is Expr.CmpE -> {
                ctxOnly(e.l)
                ctxOnly(e.r)
            }
            is Expr.Arg -> beyond("an argument")
            is Expr.AutoE -> beyond("an auto")
            is Expr.Var -> beyond("a local")
            is Expr.Provided -> beyond("a provided value")
            is Expr.Select,
            is Expr.Get,
            is Expr.ExistsE -> beyond("a read")
            is Expr.Call -> beyond("a call")
            else -> beyond("an expression of more than the context")
        }
    }

    // §9.3 Signatures of the standard library.
    private fun stdTy(f: StdFn, ts: List<Ty>, want: Ty?): Ty {
        fun option(t: Ty): Ty =
            if (t is Ty.TOption) complain(Complaint.NestedOption) else Ty.TOption(t)
        fun misuse(): Nothing = complain(Complaint.StdMisuse("${f.name} applied to $ts"))
        val a = ts.getOrNull(0)
        val b = ts.getOrNull(1)
        fun arity(n: Int) {
            if (ts.size != n) misuse()
        }
        return when (f) {
            StdFn.Trim,
            StdFn.Lower -> {
                arity(1)
                if (a == Ty.TText) Ty.TText else misuse()
            }
            StdFn.IsEmpty,
            StdFn.IsAlnum -> {
                arity(1)
                if (a == Ty.TText) Ty.TBool else misuse()
            }
            StdFn.Concat -> {
                arity(1)
                if (a is Ty.TList && a.of == Ty.TText) Ty.TText else misuse()
            }
            StdFn.Chars -> {
                arity(1)
                if (a == Ty.TText) Ty.TList(Ty.TText) else misuse()
            }
            StdFn.TextLen -> {
                arity(1)
                if (a == Ty.TText) Ty.TInt else misuse()
            }
            StdFn.StartsWith -> {
                arity(2)
                if (a == Ty.TText && b == Ty.TText) Ty.TBool else misuse()
            }
            StdFn.SplitOnce -> {
                arity(2)
                if (a == Ty.TText && b == Ty.TText)
                    Ty.TOption(Ty.TStruct(mapOf("before" to Ty.TText, "after" to Ty.TText)))
                else misuse()
            }
            StdFn.TextOfInt -> {
                arity(1)
                if (a == Ty.TInt) Ty.TText else misuse()
            }
            StdFn.Hex -> {
                arity(1)
                if (a == Ty.TBytes) Ty.TText else misuse()
            }
            StdFn.Min,
            StdFn.Max -> {
                arity(2)
                if (a == Ty.TInt && b == Ty.TInt) Ty.TInt else misuse()
            }
            StdFn.Clamp -> {
                arity(3)
                if (ts.all { it == Ty.TInt }) Ty.TInt else misuse()
            }
            StdFn.Abs -> {
                arity(1)
                if (a == Ty.TInt) Ty.TInt else misuse()
            }
            StdFn.Fnv1a64 -> {
                arity(1)
                if (a == Ty.TText) Ty.TInt else misuse()
            }
            StdFn.Sha256 -> {
                arity(1)
                if (a == Ty.TBytes) Ty.TBytes else misuse()
            }
            StdFn.IdOfText -> {
                arity(1)
                if (a != Ty.TText) misuse()
                if (want is Ty.TOption && want.of is Ty.TId) want
                else complain(Complaint.NeedsAnnotation("id_of_text needs its table from context"))
            }
            StdFn.TextOfId -> {
                arity(1)
                if (a is Ty.TId) Ty.TText else misuse()
            }
            StdFn.NilId -> {
                arity(0)
                if (want is Ty.TId) want
                else complain(Complaint.NeedsAnnotation("nil id needs its table from context"))
            }
            StdFn.Utf8 -> {
                arity(1)
                if (a == Ty.TText) Ty.TBytes else misuse()
            }
            StdFn.First,
            StdFn.Last -> {
                arity(1)
                if (a is Ty.TList) option(a.of) else misuse()
            }
            StdFn.Len -> {
                arity(1)
                if (a is Ty.TList) Ty.TInt else misuse()
            }
            StdFn.Contains -> {
                arity(2)
                if (a is Ty.TList && a.of == b) Ty.TBool else misuse()
            }
            StdFn.Reverse -> {
                arity(1)
                if (a is Ty.TList) a else misuse()
            }
            StdFn.IsSome -> {
                arity(1)
                if (a is Ty.TOption) Ty.TBool else misuse()
            }
            StdFn.UnwrapOr -> {
                arity(2)
                if (a is Ty.TOption && b != null && a.of == b) b else misuse()
            }
            StdFn.Unwrap -> {
                arity(1)
                if (a is Ty.TOption) a.of else misuse()
            }
        }
    }

    /**
     * §9.4 `complete_orders`: every order made total — the table's key columns ascending (a group's
     * `by`), after whatever the author ordered by, omitting any already there — in every plan of
     * every function, related plans included.
     */
    public fun completeOrders(m: Module): Module {
        val sch = m.schema
        return m.copy(
            functions =
                m.functions.map { f ->
                    f.copy(
                        body = f.body.map { completeStmt(sch, it) },
                        plan = f.plan?.let { completeOrder(sch, it) })
                })
    }

    private fun completeStmt(sch: Schema, s: Stmt): Stmt =
        when (s) {
            is Stmt.Let -> Stmt.Let(s.sym, completeExpr(sch, s.e))
            is Stmt.Insert -> s.copy(row = completeExpr(sch, s.row))
            is Stmt.Upsert -> s.copy(row = completeExpr(sch, s.row))
            is Stmt.Refuse -> Stmt.Refuse(completeExpr(sch, s.e))
            is Stmt.Return -> Stmt.Return(s.e?.let { completeExpr(sch, it) })
            is Stmt.Update ->
                s.copy(key = s.key.map { completeExpr(sch, it) }, row = completeExpr(sch, s.row))
            is Stmt.If ->
                Stmt.If(
                    completeExpr(sch, s.c),
                    s.then.map { completeStmt(sch, it) },
                    s.els.map { completeStmt(sch, it) })
            is Stmt.For ->
                Stmt.For(s.sym, completeExpr(sch, s.over), s.body.map { completeStmt(sch, it) })
            is Stmt.Delete -> s.copy(key = s.key.map { completeExpr(sch, it) })
            is Stmt.Private -> Stmt.Private(s.body.map { completeStmt(sch, it) })
        }

    private fun completeExpr(sch: Schema, e: Expr): Expr =
        if (e is Expr.Select) Expr.Select(completeOrder(sch, e.plan))
        else mapChildren(e) { completeExpr(sch, it) }

    /** `eval::complete_order`: a plan's order made total, in every related plan too. */
    public fun completeOrder(sch: Schema, p: Plan): Plan {
        val keys =
            when (val s = p.source) {
                is Source.Table -> sch.lookupTable(s.table)?.key ?: emptyList()
                is Source.Group -> s.by
            }
        val order = ArrayList(p.order)
        for (k in keys) if (order.none { it.first == Key.Column(k) })
            order.add(Key.Column(k) to Dir.Asc)
        return p.copy(
            order = order, related = p.related.map { it.copy(plan = completeOrder(sch, it.plan)) })
    }
}

/**
 * An expression with `f` applied to each of its direct children (`children`), the rest as it was.
 */
public fun mapChildren(e: Expr, f: (Expr) -> Expr): Expr =
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
        is Expr.Select -> e
        is Expr.FieldE -> Expr.FieldE(f(e.e), e.name)
        is Expr.SomeE -> Expr.SomeE(f(e.e))
        is Expr.StructE -> Expr.StructE(e.fields.mapValues { f(it.value) })
        is Expr.ListE -> Expr.ListE(e.items.map(f))
        is Expr.OpE -> Expr.OpE(e.op, e.args.map(f))
        is Expr.Call -> Expr.Call(e.fn, e.args.map(f))
        is Expr.StdE -> Expr.StdE(e.fn, e.args.map(f))
        is Expr.Get -> Expr.Get(e.table, e.key.map(f))
        is Expr.ExistsE -> Expr.ExistsE(e.table, e.key.map(f))
        is Expr.Match -> Expr.Match(f(e.e), e.sym, f(e.some), f(e.none))
        is Expr.IfE -> Expr.IfE(f(e.c), f(e.then), f(e.els))
        is Expr.CmpE -> Expr.CmpE(e.op, f(e.l), f(e.r))
        is Expr.MapE -> Expr.MapE(f(e.over), e.sym, f(e.body))
        is Expr.FilterE -> Expr.FilterE(f(e.over), e.sym, f(e.body))
        is Expr.AnyE -> Expr.AnyE(f(e.over), e.sym, f(e.body))
        is Expr.AllE -> Expr.AllE(f(e.over), e.sym, f(e.body))
        is Expr.SortBy -> Expr.SortBy(f(e.over), e.sym, f(e.key))
        is Expr.Fold -> Expr.Fold(f(e.over), f(e.init), e.acc, e.sym, f(e.body))
    }
