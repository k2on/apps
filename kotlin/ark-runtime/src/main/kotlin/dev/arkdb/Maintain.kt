// §1.5 A plan kept up to date (`rust/ark/src/view.rs`, "The view",
// "Maintenance" and R9 "The numbers, kept"): `hydrate` pulls it once and
// keeps the entries, indexed by key and by `(node, dependency)`; `pushAll`
// is told the changes of one settle — the store already after all of them
// — rebuilds the entries they touch, once each, and reports what it did to
// the answer as positions (`Patch`). The contract: after any sequence of
// changes the view is a fresh hydrate over the same store, and splicing
// the patches into the old answer gives the new one.
//
// R9 (`docs/plan-perf.md`, `docs/plan-db.md` D4): a related list, or a
// group's members, whose every use in its parent is an aggregate — `len`,
// a sum's `fold`, `min`/`max` of a column, `first`/`last` of a list ordered
// by one column — is kept as those numbers, moved by each change's terms,
// and never pulled whole: a count and a sum as integers of any size (a sum
// is judged on its total, §13), an extreme as the column's value, read
// again through an index when it leaves.
package dev.arkdb

import java.math.BigInteger
import java.util.TreeMap
import java.util.TreeSet

/**
 * What a view's expressions are evaluated in, owned (`view::Env`): the function's helpers, the
 * context, the checked arguments and the provided values.
 */
public data class ViewEnv(
    val helpers: List<Function>,
    val ctx: Ctx,
    val args: Map<String, Value>,
    val provided: Map<String, Value>,
) {
    /** The scope a plan is pulled in. */
    public fun scope(sch: Schema): Scope = Scope.of(sch, helpers, ctx, args, provided)
}

/**
 * §13.4 What `pushAll` did to a view's answer, as positions into the list as it stands
 * (`view::Patch`).
 */
public sealed class Patch {
    /** Puts the node before the element at `at` (`at == len` appends). */
    public data class Insert(val at: Int, val node: Value) : Patch()

    public data class Remove(val at: Int) : Patch()

    public data class Update(val at: Int, val node: Value) : Patch()
}

internal val depOrder: Comparator<Pair<NodeId, Value>> =
    compareBy<Pair<NodeId, Value>> { it.first }
        .thenComparator { a, b -> compareValue(a.second, b.second) }

private fun keySet(): TreeSet<List<Value>> = TreeSet(KeyOrder)

// R9 aggregates -----------------------------------------------------------------

/** R9 An aggregate a plan node's expressions take of one of its lists (`view::Agg`). */
public sealed class Agg {
    public object Count : Agg() {
        override fun toString(): String = "Count"
    }

    /** `init` plus the sum of `f` over the list's nodes, `x` each node. */
    public data class Sum(val x: Sym, val f: Expr, val init: Expr) : Agg()

    /** D4 The least value of a column over the list's rows, `Null` when it is empty. */
    public data class Min(val column: String) : Agg()

    /** D4 The greatest. */
    public data class Max(val column: String) : Agg()

    // The column and the direction an extreme is read in from an index.
    internal val extreme: Pair<String, Dir>?
        get() =
            when (this) {
                is Min -> column to Dir.Asc
                is Max -> column to Dir.Desc
                else -> null
            }
}

/**
 * What one aggregate a view keeps comes to (`view::Num`): an integer of any size, or a column's
 * value.
 */
public sealed class Num {
    public data class N(val n: BigInteger) : Num()

    public data class V(val v: Value) : Num()
}

// How one aggregate use reads its number, which is how it is rewritten.
private sealed class Use {
    // `len(list)` (no `init`), or a sum's fold (`init + number`).
    class Plain(val init: Expr?) : Use()

    // D4 `fold(list, init, |acc, x| min(acc, x.c))`, or `max`.
    class FoldU(val init: Expr, val g: StdFn) : Use()

    // D4 `match first(list) { y => some, None => none }` reading `y.col` alone.
    class Pick(val y: Sym, val col: String, val some: Expr, val none: Expr) : Use()
}

/**
 * R9 One plan node's expressions as a view evaluates them (`view::Face`): every aggregate use of a
 * kept list replaced by a number the view keeps, at a slot no plan binds (negative).
 */
public data class Face(
    val having: Expr? = null,
    val project: Expr? = null,
    val order: List<Expr?> = emptyList(),
    val members: List<Pair<Agg, Sym>>? = null,
    val ids: List<NodeId> = emptyList(),
    val kept: List<Boolean> = emptyList(),
)

/**
 * R9 A related plan whose list is kept as numbers: the aggregates its parent takes, and its face.
 */
public data class Kept(val aggs: List<Pair<Agg, Sym>>, val face: Face)

/** R9 What a view keeps as numbers, derived from the plan and the helpers (`view::Shape`). */
public data class Shape(val root: Face, val kept: Map<NodeId, Kept>)

/**
 * R9 What one entry keeps of a kept related plan for one `(node, dependency)` (`view::Held`): the
 * numbers, the child nodes themselves when the child plan has kept lists of its own, and who reads
 * them — `null` for the entry's own node, or a child node as `(node, dependency, key)`.
 */
public data class KeptNums(
    val nums: MutableList<Num>,
    val kids: TreeMap<List<Value>, Kid>,
    val parents: MutableSet<Triple<NodeId, Value, List<Value>>?>,
)

/** R9 One child node of a kept plan whose own lists are kept (`view::Kid`). */
public data class Kid(
    val row: Value,
    val admitted: Boolean,
    val node: Value,
    val subs: List<Value>
)

/** The numbers an entry keeps, by `(node, dependency)` (`view::HeldMap`). */
public class HeldMap : TreeMap<Pair<NodeId, Value>, KeptNums>(depOrder)

private typealias Ops = MutableList<Pair<Pair<NodeId, Value>, Boolean>>

private typealias Parent = Triple<NodeId, Value, List<Value>>?

// What recognising an aggregate needs beyond the expression: the helpers,
// and the one column each related list is ordered by (D4).
private class Rec(val helpers: List<Function>, val orders: Map<Sym, Pair<String, Dir>>)

internal object Shapes {
    /** R9 `shape`: the shape of a plan in a function whose helpers are `helpers`, over a schema. */
    fun shape(sch: Schema, plan: Plan, helpers: List<Function>): Shape {
        val next = longArrayOf(-1)
        val (root, kept) = face(sch, plan, 0, true, helpers, next) ?: (Face() to emptyMap())
        return Shape(root, kept)
    }

    // D4 The one column a list is ordered by, when it is.
    private fun oneColumnOrder(sch: Schema, c: Plan): Pair<String, Dir>? {
        val tbl = sch.lookupTable(c.table) ?: return null
        val first = c.order.firstOrNull() ?: return null
        val c0 = (first.first as? Key.Column)?.column ?: return null
        if (c.limit != null) return null
        val keys = tbl.key.filter { it != c0 }
        val rest = c.order.drop(1)
        val ok =
            rest.size <= keys.size &&
                rest.zip(keys).all { (kd, want) ->
                    kd.second == Dir.Asc && kd.first == Key.Column(want)
                }
        return if (ok) c0 to first.second else null
    }

    // D4 The columns a plan's filter holds equal however it is satisfied, read off the plan.
    private fun filterEqs(p: Pred?): List<String> {
        val out = ArrayList<String>()
        fun go(p: Pred) {
            when {
                p is Pred.Cmp && p.op == CmpOp.Eq -> out.add(p.column)
                p is Pred.All -> p.items.forEach(::go)
                else -> {}
            }
        }
        p?.let(::go)
        return out
    }

    // D4 Whether a store keeping the schema's indexes reads the rows `eq` holds
    // equal in `col`'s order through one of them (`Secondary::serves` for one column).
    private fun served(tbl: Table, eq: List<String>, col: String, dir: Dir): Boolean {
        val lists =
            (tbl.indexes.map { it.columns } + tbl.refs.map { listOf(it.column) }).filter {
                it.isNotEmpty() && it != tbl.key
            }
        fun held(c: String) = c in eq
        for (cols in lists) {
            val n = cols.takeWhile(::held).size
            val lead = cols.subList(0, n)
            val rest = cols.subList(n, cols.size)
            if (!eq.all { it in lead } || held(col)) continue
            if (rest.size == 1 && rest[0] == col) return true
            if (rest.isEmpty() && dir == Dir.Asc && tbl.key.firstOrNull { !held(it) } == col)
                return true
        }
        return false
    }

    // D4 Whether every extreme among `aggs` can be kept over `p`'s rows with `pinned` held equal.
    private fun extremesKept(
        sch: Schema,
        p: Plan,
        pinned: List<String>,
        aggs: List<Agg>,
        child: Boolean
    ): Boolean {
        if (aggs.none { it.extreme != null }) return true
        val tbl = sch.lookupTable(p.table) ?: return false
        if (child && (p.project != null || p.having != null || p.related.isNotEmpty())) return false
        val eq = pinned + filterEqs(p.filter)
        return aggs
            .mapNotNull { it.extreme }
            .all { (c, d) -> tbl.column(c)?.nullable == false && served(tbl, eq, c, d) }
    }

    // A plan's face, and the kept plans beneath it; `null` when a child plan
    // (`root` false) cannot be all numbers.
    private fun face(
        sch: Schema,
        p: Plan,
        base: NodeId,
        root: Boolean,
        helpers: List<Function>,
        next: LongArray
    ): Pair<Face, Map<NodeId, Kept>>? {
        if (!root &&
            (p.lookups.isNotEmpty() ||
                p.limit != null ||
                p.members != null ||
                p.source !is Source.Table))
            return null
        val ids = ArrayList<NodeId>()
        var id = base + p.lookups.size
        for (r in p.related) {
            ids.add(id)
            id += 1 + View.nodeCount(r.plan)
        }
        // Pass one: every use of every list.
        val cands = TreeSet<Sym>(p.related.map { it.sym })
        if (p.members != null && root) cands.add(p.members)
        val uses = TreeMap<Sym, MutableList<Agg>?>()
        for (s in cands) uses[s] = ArrayList()
        val orders = HashMap<Sym, Pair<String, Dir>>()
        for (r in p.related) oneColumnOrder(sch, r.plan)?.let { orders[r.sym] = it }
        val rec = Rec(helpers, orders)
        val exprs =
            listOfNotNull(p.having, p.project) + p.order.mapNotNull { (it.first as? Key.ExprK)?.e }
        for (e in exprs) scan(e, cands, rec, uses)
        // D4 An extreme no index serves leaves its list a list.
        for (r in p.related) {
            val aggs = uses[r.sym] ?: continue
            if (!extremesKept(sch, r.plan, r.on.map { it.first }, aggs, true)) uses[r.sym] = null
        }
        val src = p.source
        if (p.members != null && root && src is Source.Group) {
            val aggs = uses[p.members]
            if (aggs != null && !extremesKept(sch, p, src.by, aggs, false)) uses[p.members] = null
        }
        // The default node carries every related list whole.
        if (p.project == null) for (r in p.related) uses[r.sym] = null
        // Members read before the numbers are bound are a list.
        if (p.members != null) {
            val early =
                p.lookups.flatMap { it.key } + p.related.flatMap { r -> r.on.map { it.second } }
            if (early.any { Eval.mentions(it, p.members) }) uses[p.members] = null
        }
        val kept = TreeMap<NodeId, Kept>()
        val flags = ArrayList<Boolean>()
        val slots = TreeMap<Sym, List<Pair<Agg, Sym>>>()
        fun assign(aggs: List<Agg>): List<Pair<Agg, Sym>> =
            aggs.map { a ->
                val s = next[0]
                next[0] -= if (a is Agg.Count) 1 else 2
                a to s
            }
        for ((r, rid) in p.related.zip(ids)) {
            val child =
                if (uses[r.sym] != null) face(sch, r.plan, rid + 1, false, helpers, next) else null
            if (child != null) {
                val aggs = assign(uses[r.sym] ?: emptyList())
                slots[r.sym] = aggs
                kept.putAll(child.second)
                kept[rid] = Kept(aggs, child.first)
                flags.add(true)
            } else {
                if (!root) return null
                flags.add(false)
            }
        }
        val members =
            if (p.members != null && root) {
                uses[p.members]?.let { aggs ->
                    val a = assign(aggs)
                    slots[p.members] = a
                    a
                }
            } else null
        fun rw(e: Expr) = rewrite(e, slots, rec)
        return Face(
            p.having?.let(::rw),
            p.project?.let(::rw),
            p.order.map { (k, _) -> (k as? Key.ExprK)?.let { rw(it.e) } },
            members,
            ids,
            flags,
        ) to kept
    }

    // One use of a list that is an aggregate: the list's symbol, the aggregate, and how it reads
    // it.
    private fun aggregateUse(e: Expr, cands: Set<Sym>, rec: Rec): Triple<Sym, Agg, Use>? {
        fun listVar(es: List<Expr>): Sym? =
            (es.singleOrNull() as? Expr.Var)?.sym?.takeIf { it in cands }
        when (e) {
            is Expr.StdE ->
                if (e.fn == StdFn.Len)
                    listVar(e.args)?.let {
                        return Triple(it, Agg.Count, Use.Plain(null))
                    }
            is Expr.Fold -> {
                val s = (e.over as? Expr.Var)?.sym?.takeIf { it in cands } ?: return null
                extremeStep(e.body, e.acc, e.sym)?.let { (g, c) ->
                    return Triple(
                        s, if (g == StdFn.Min) Agg.Min(c) else Agg.Max(c), Use.FoldU(e.init, g))
                }
                val f = Eval.sumStep(e.body, e.acc) ?: return null
                if (closed(f, listOf(e.sym), false) && closed(e.init, emptyList(), false)) {
                    return Triple(s, Agg.Sum(e.sym, f, e.init), Use.Plain(e.init))
                }
            }
            // D4 `first`/`last` of a list ordered by one column, read for that column alone.
            is Expr.Match -> {
                val o = e.e
                if (o is Expr.StdE && (o.fn == StdFn.First || o.fn == StdFn.Last)) {
                    val s = listVar(o.args) ?: return null
                    val (c, d) = rec.orders[s] ?: return null
                    if (!onlyField(e.some, e.sym, c)) return null
                    val least = (d == Dir.Asc) == (o.fn == StdFn.First)
                    return Triple(
                        s,
                        if (least) Agg.Min(c) else Agg.Max(c),
                        Use.Pick(e.sym, c, e.some, e.none))
                }
            }
            is Expr.Call -> {
                val s = listVar(e.args) ?: return null
                val h =
                    rec.helpers.firstOrNull { it.name == e.fn && it.kind == FnKind.Helper }
                        ?: return null
                val (agg, init) = helperAgg(h) ?: return null
                return Triple(s, agg, Use.Plain(init))
            }
            else -> {}
        }
        return null
    }

    // D4 A fold's step that is `min(acc, x.c)` or `max(acc, x.c)`, either way round.
    private fun extremeStep(body: Expr, acc: Sym, x: Sym): Pair<StdFn, String>? {
        if (body !is Expr.StdE ||
            (body.fn != StdFn.Min && body.fn != StdFn.Max) ||
            body.args.size != 2)
            return null
        val (a, b) = body.args
        fun fieldOfX(e: Expr): String? = (e as? Expr.FieldE)?.takeIf { it.e == Expr.Var(x) }?.name
        if (a == Expr.Var(acc))
            fieldOfX(b)?.let {
                return body.fn to it
            }
        if (b == Expr.Var(acc))
            fieldOfX(a)?.let {
                return body.fn to it
            }
        return null
    }

    // D4 Whether every occurrence of `y` in `e` is `y.c`.
    private fun onlyField(e: Expr, y: Sym, c: String): Boolean =
        when {
            e is Expr.FieldE && e.e == Expr.Var(y) -> e.name == c
            e is Expr.Var -> e.sym != y
            else -> children(e).all { onlyField(it, y, c) }
        }

    // A helper whose whole body is an aggregate of its one parameter.
    private fun helperAgg(h: Function): Pair<Agg, Expr?>? {
        val p = h.input.singleOrNull()?.first ?: return null
        val b = ((h.body.singleOrNull() as? Stmt.Return)?.e) ?: return null
        fun isP(e: Expr) = e is Expr.Arg && e.name == p
        if (b is Expr.StdE && b.fn == StdFn.Len && b.args.size == 1 && isP(b.args[0]))
            return Agg.Count to null
        if (b is Expr.Fold && isP(b.over)) {
            val f = Eval.sumStep(b.body, b.acc) ?: return null
            if (closed(f, listOf(b.sym), true) && closed(b.init, emptyList(), true))
                return Agg.Sum(b.sym, f, b.init) to b.init
        }
        return null
    }

    // Pass one: note each use of a candidate list — an aggregate, or anything else, which makes it
    // a list.
    private fun scan(e: Expr, cands: Set<Sym>, rec: Rec, uses: MutableMap<Sym, MutableList<Agg>?>) {
        val u = aggregateUse(e, cands, rec)
        if (u != null) {
            val (s, agg, use) = u
            val aggs = uses[s]
            if (aggs != null && agg !in aggs) aggs.add(agg)
            when (use) {
                is Use.Plain -> use.init?.let { scan(it, cands, rec, uses) }
                is Use.FoldU -> scan(use.init, cands, rec, uses)
                is Use.Pick -> {
                    scan(use.some, cands, rec, uses)
                    scan(use.none, cands, rec, uses)
                }
            }
            return
        }
        if (e is Expr.Var && e.sym in cands) uses[e.sym] = null
        for (c in children(e)) scan(c, cands, rec, uses)
    }

    // Pass two: each aggregate use of a kept list replaced by its number.
    private fun rewrite(e: Expr, slots: Map<Sym, List<Pair<Agg, Sym>>>, rec: Rec): Expr {
        val kept = slots.keys
        return mapExpr(e) { x ->
            val (s, agg, u) = aggregateUse(x, kept, rec) ?: return@mapExpr null
            val slot = slots[s]?.firstOrNull { it.first == agg }?.second ?: return@mapExpr null
            when (u) {
                // The sum's `init` is in its number, which is two slots.
                is Use.Plain ->
                    if (u.init == null) Expr.Var(slot)
                    else Expr.OpE(Op.Add, listOf(Expr.Var(slot), Expr.Var(slot - 1)))
                is Use.FoldU -> {
                    val init = rewrite(u.init, slots, rec)
                    val y = slot - 1
                    Expr.Match(Expr.Var(slot), y, Expr.StdE(u.g, listOf(init, Expr.Var(y))), init)
                }
                is Use.Pick -> {
                    val some =
                        mapExpr(u.some) { z ->
                            if (z is Expr.FieldE && z.e == Expr.Var(u.y) && z.name == u.col)
                                Expr.Var(u.y)
                            else null
                        }
                    Expr.Match(
                        Expr.Var(slot), u.y, rewrite(some, slots, rec), rewrite(u.none, slots, rec))
                }
            }
        }
    }

    // Whether every symbol free in `e` is one of `allowed` and nothing is read.
    private fun closed(e: Expr, allowed: List<Sym>, strict: Boolean): Boolean {
        fun go(e: Expr, bound: List<Sym>): Boolean =
            when (e) {
                is Expr.Var -> e.sym in bound
                is Expr.Arg,
                is Expr.AutoE,
                is Expr.Provided -> !strict
                is Expr.Select,
                is Expr.Get,
                is Expr.ExistsE -> false
                is Expr.Match -> go(e.e, bound) && go(e.none, bound) && go(e.some, bound + e.sym)
                is Expr.MapE -> go(e.over, bound) && go(e.body, bound + e.sym)
                is Expr.FilterE -> go(e.over, bound) && go(e.body, bound + e.sym)
                is Expr.AnyE -> go(e.over, bound) && go(e.body, bound + e.sym)
                is Expr.AllE -> go(e.over, bound) && go(e.body, bound + e.sym)
                is Expr.SortBy -> go(e.over, bound) && go(e.key, bound + e.sym)
                is Expr.Fold ->
                    go(e.over, bound) && go(e.init, bound) && go(e.body, bound + e.acc + e.sym)
                else -> children(e).all { go(it, bound) }
            }
        return go(e, allowed)
    }

    // `e` with `f` applied top-down: where `f` answers, its answer replaces the subexpression.
    private fun mapExpr(e: Expr, f: (Expr) -> Expr?): Expr =
        f(e) ?: mapChildren(e) { mapExpr(it, f) }
}

// The kept numbers, against a plan, its shape, a scope and a store.
private class Cx(
    val sch: Schema,
    val plan: Plan,
    val shape: Shape,
    val scope: Scope,
    val st: Store
) {
    val rel: List<Related?> = View.nodes(plan).map { (it as? Node.RelatedN)?.r }
    val filters: Map<NodeId, Filter?> =
        shape.kept.keys.associateWith { id -> rootFilter(related(id).plan) }
    val rootFilter: Filter? = rootFilter(plan)

    fun rootFilter(p: Plan): Filter? = p.filter?.let { View.evalPred(it) { e -> scope.eval(e) } }

    fun related(id: NodeId): Related =
        rel[id] ?: throw IllegalStateException("a kept node is a related plan")

    fun kept(id: NodeId): Kept = shape.kept[id]!!
}

// A group's members as the entry is built with them: none, the list, or its numbers.
private sealed class Members {
    object None : Members()

    class ListM(val v: Value) : Members()

    class Nums(val ns: List<Num>) : Members()
}

private object R9 {
    fun intOf(v: Value): Long =
        (v as? Value.VInt)?.value
            ?: throw EvalFault.Bug(
                EvalError.TypeError("an aggregate's term is an Int, not ${v.show()}"))

    fun boolOf(v: Value): Boolean =
        (v as? Value.VBool)?.value
            ?: throw EvalFault.Bug(EvalError.TypeError("a having is a Bool, not ${v.show()}"))

    // What `aggs` come to over no node: nothing counted, a sum's `init`, no extreme.
    fun zero(cx: Cx, aggs: List<Pair<Agg, Sym>>): MutableList<Num> =
        aggs
            .map { (a, _) ->
                when (a) {
                    is Agg.Count -> Num.N(BigInteger.ZERO)
                    is Agg.Sum -> Num.N(BigInteger.valueOf(intOf(cx.scope.node().eval(a.init))))
                    else -> Num.V(Value.VNull)
                }
            }
            .toMutableList()

    fun wide(n: Num): BigInteger =
        (n as? Num.N)?.n ?: throw EvalFault.Bug(EvalError.TypeError("a count or a sum at $n"))

    // One admitted node's terms put in.
    fun putIn(aggs: List<Pair<Agg, Sym>>, nums: MutableList<Num>, terms: List<Num>) {
        for (i in aggs.indices) {
            val ex = aggs[i].first.extreme
            val n = nums[i]
            val t = terms[i]
            if (ex == null) {
                nums[i] = Num.N(wide(n).add(wide(t)))
            } else if (n is Num.V && t is Num.V) {
                val o = compareValue(t.v, n.v)
                val beyond = if (ex.second == Dir.Asc) o < 0 else o > 0
                if (n.v is Value.VNull || beyond) nums[i] = t
            } else {
                throw EvalFault.Bug(EvalError.TypeError("an extreme $n by $t"))
            }
        }
    }

    // One admitted node's terms taken out; whether an extreme was taken with it.
    fun takeOut(aggs: List<Pair<Agg, Sym>>, nums: MutableList<Num>, terms: List<Num>): Boolean {
        var stale = false
        for (i in aggs.indices) {
            val n = nums[i]
            if (aggs[i].first.extreme == null) {
                nums[i] = Num.N(wide(n).subtract(wide(terms[i])))
            } else if (n is Num.V) {
                stale = stale || n.v is Value.VNull || n == terms[i]
            } else {
                throw EvalFault.Bug(EvalError.TypeError("an extreme at $n"))
            }
        }
        return stale
    }

    // The numbers bound for a node to read, at their slots: a sum as two
    // clamped `Int`s whose checked sum is the total when it is one.
    fun bindNums(node: NodeScope, aggs: List<Pair<Agg, Sym>>, nums: List<Num>) {
        val lo = BigInteger.valueOf(Long.MIN_VALUE)
        val hi = BigInteger.valueOf(Long.MAX_VALUE)
        fun clamp(n: BigInteger): Long = n.max(lo).min(hi).toLong()
        for ((ag, n) in aggs.zip(nums)) {
            val (a, slot) = ag
            when {
                a is Agg.Sum && n is Num.N -> {
                    val first = clamp(n.n)
                    node.bind(slot, Value.VInt(first))
                    node.bind(slot - 1, Value.VInt(clamp(n.n.subtract(BigInteger.valueOf(first)))))
                }
                n is Num.V -> node.bind(slot, n.v)
                n is Num.N -> node.bind(slot, Value.VInt(clamp(n.n)))
            }
        }
    }

    fun column(node: Value, c: String): Value =
        (node as? Value.VStruct)?.fields?.get(c) ?: Value.VNull

    // The terms one admitted node adds to the aggregates.
    fun terms(cx: Cx, aggs: List<Pair<Agg, Sym>>, node: Value): List<Num> =
        aggs.map { (a, _) ->
            when (a) {
                is Agg.Count -> Num.N(BigInteger.ONE)
                is Agg.Sum -> {
                    val n = cx.scope.node()
                    n.bind(a.x, node)
                    Num.N(BigInteger.valueOf(intOf(n.eval(a.f))))
                }
                is Agg.Min -> Num.V(column(node, a.column))
                is Agg.Max -> Num.V(column(node, a.column))
            }
        }

    // A related plan's pins for one dependency.
    fun pinsOf(r: Related, on: Value): List<Pair<String, Value>> =
        if (on is Value.VList) r.on.map { it.first }.zip(on.items) else emptyList()

    // D4 Every extreme among `aggs` read again from the store, which is final.
    fun extremes(
        cx: Cx,
        plan: Plan,
        pins: List<Pair<String, Value>>,
        aggs: List<Pair<Agg, Sym>>,
        nums: MutableList<Num>
    ) {
        if (aggs.none { it.first.extreme != null }) return
        val (_, filter) = View.source(cx.sch, plan, pins, cx.scope)
        val eq = View.equalities(filter)
        val sp = View.spans(filter)
        val keep: Keep = { View.admits(filter, it) }
        for (i in aggs.indices) {
            val (c, d) = aggs[i].first.extreme ?: continue
            val ordered = cx.st.scanOrdered(plan.table, eq, sp, listOf(c to d), keep, 1)
            val row =
                if (ordered != null) ordered.firstOrNull()
                else
                    View.fetch(cx.st, plan.table, filter).reduceOrNull { a, b ->
                        val o = compareValue(b.fields[c] ?: Value.VNull, a.fields[c] ?: Value.VNull)
                        if ((d == Dir.Asc && o < 0) || (d == Dir.Desc && o > 0)) b else a
                    }
            nums[i] = Num.V(row?.fields?.get(c) ?: Value.VNull)
        }
    }

    // A child row of a kept plan with no related plans: its node, when its having admits it.
    fun rowNode(cx: Cx, id: NodeId, row: Value): Value? {
        val c = cx.related(id).plan
        val face = cx.kept(id).face
        val node = cx.scope.node()
        c.row?.let { node.bind(it, row) }
        face.having?.let { if (!boolOf(node.eval(it))) return null }
        return face.project?.let { node.eval(it) } ?: row
    }

    // The terms a child row adds to its parent's numbers: none when the filter or the having
    // refuses it.
    fun rowTerms(cx: Cx, id: NodeId, row: Row): List<Num>? {
        if (!View.admits(cx.filters[id], row)) return null
        val node = rowNode(cx, id, row) ?: return null
        return terms(cx, cx.kept(id).aggs, node)
    }

    // A child node of a kept plan whose lists are kept: its having and node over its numbers.
    fun kidNode(
        cx: Cx,
        id: NodeId,
        row: Value,
        subs: List<Value>,
        held: HeldMap
    ): Pair<Boolean, Value> {
        val c = cx.related(id).plan
        val face = cx.kept(id).face
        val node = cx.scope.node()
        c.row?.let { node.bind(it, row) }
        for ((jid, on) in face.ids.zip(subs)) bindNums(
            node, cx.kept(jid).aggs, held[jid to on]!!.nums)
        val admitted = face.having?.let { boolOf(node.eval(it)) } ?: true
        val value =
            when {
                !admitted -> Value.VNull
                face.project != null -> node.eval(face.project)
                else -> row
            }
        return admitted to value
    }

    fun kidTerms(cx: Cx, id: NodeId, kid: Kid): List<Num>? =
        if (!kid.admitted) null else terms(cx, cx.kept(id).aggs, kid.node)

    // A child row of a kept plan whose lists are kept, as a node.
    fun buildKid(
        cx: Cx,
        id: NodeId,
        on: Value,
        key: List<Value>,
        row: Value,
        held: HeldMap,
        ops: Ops
    ): Kid {
        val c = cx.related(id).plan
        val face = cx.kept(id).face
        val subs = ArrayList<Value>()
        for ((r, jid) in c.related.zip(face.ids)) {
            val node = cx.scope.node()
            c.row?.let { node.bind(it, row) }
            val sub = Value.VList(r.on.map { node.eval(it.second) })
            ensure(cx, jid, sub, Triple(id, on, key), held, ops)
            subs.add(sub)
        }
        val (admitted, node) = kidNode(cx, id, row, subs, held)
        return Kid(row, admitted, node, subs)
    }

    // The numbers of `(id, on)`, read by `parent`: kept if some reader holds
    // them already, pulled from the store otherwise.
    fun ensure(cx: Cx, id: NodeId, on: Value, parent: Parent, held: HeldMap, ops: Ops) {
        val k = id to on
        held[k]?.let {
            it.parents.add(parent)
            return
        }
        val r = cx.related(id)
        val c = r.plan
        val kept = cx.kept(id)
        val pins = pinsOf(r, on)
        val h = KeptNums(zero(cx, kept.aggs), TreeMap(KeyOrder), linkedSetOf(parent))
        if (c.related.isEmpty() && kept.aggs.all { it.first.extreme != null }) {
            // D4 Extremes alone: one indexed read each, not the list.
            extremes(cx, c, pins, kept.aggs, h.nums)
        } else {
            val (tbl, filter) = View.source(cx.sch, c, pins, cx.scope)
            for (row in View.fetch(cx.st, c.table, filter)) {
                if (c.related.isEmpty()) {
                    rowNode(cx, id, row)?.let { putIn(kept.aggs, h.nums, terms(cx, kept.aggs, it)) }
                } else {
                    val key = tbl.keyOf(row)
                    val kid = buildKid(cx, id, on, key, row, held, ops)
                    kidTerms(cx, id, kid)?.let { putIn(kept.aggs, h.nums, it) }
                    h.kids[key] = kid
                }
            }
        }
        ops.add(k to true)
        held[k] = h
    }

    // `parent` stops reading `k`; numbers nobody reads are let go, and what their children read
    // with them.
    fun release(cx: Cx, k: Pair<NodeId, Value>, parent: Parent, held: HeldMap, ops: Ops) {
        val h = held[k] ?: return
        h.parents.remove(parent)
        if (h.parents.isNotEmpty()) return
        held.remove(k)
        val ids = cx.kept(k.first).face.ids
        for ((key, kid) in h.kids) {
            val me = Triple(k.first, k.second, key)
            for ((jid, sub) in ids.zip(kid.subs)) release(cx, jid to sub, me, held, ops)
        }
        ops.add(k to false)
    }
}

// R9 A change to a table a kept plan reads, as one entry's numbers see it.
private class Hit(val id: NodeId, val on: Value, val old: Row?, val new: Row?)

// What a held number is waiting for in a sweep.
private class Dirt {
    var before: List<Num>? = null
    val reread = TreeSet(KeyOrder)
    val reeval = TreeSet(KeyOrder)
    var stale = false
}

// R9's group bookkeeping: what the changes did to one group's members (`view::Moves`).
private class Moves {
    val arrived = TreeSet(KeyOrder)
    val left = TreeSet(KeyOrder)

    fun has(base: Set<List<Value>>?, k: List<Value>): Boolean =
        k in arrived || (k !in left && base?.contains(k) == true)

    // `k` leaves; whether it was a member.
    fun leave(base: Set<List<Value>>?, k: List<Value>): Boolean {
        if (!has(base, k)) return false
        if (!arrived.remove(k)) left.add(k)
        return true
    }

    // `k` arrives; whether it was not a member.
    fun arrive(base: Set<List<Value>>?, k: List<Value>): Boolean {
        if (has(base, k)) return false
        if (!left.remove(k)) arrived.add(k)
        return true
    }

    fun size(base: Set<List<Value>>?): Int = (base?.size ?: 0) + arrived.size - left.size

    fun keys(base: Set<List<Value>>?): List<List<Value>> {
        val out = TreeSet(KeyOrder)
        base?.forEach { if (it !in left) out.add(it) }
        out.addAll(arrived)
        return out.toList()
    }
}

// R9, D4 One touched group's kept member numbers.
private class Track(val nums: MutableList<Num>?, var stale: Boolean)

// What the changes touch: the keys to rebuild, each with its hits; the group moves; the group sums.
private class Touched {
    val keys = TreeMap<List<Value>, MutableList<Hit>>(KeyOrder)
    val groups = TreeMap<List<Value>, Moves>(KeyOrder)
    val sums = TreeMap<List<Value>, Track>(KeyOrder)
}

/**
 * §1.5 A plan kept up to date (`view::View`): the plan, its environment, its shape, and one `Entry`
 * per candidate, with the two indexes that route a change.
 */
public class MaintainedView
internal constructor(
    public val plan: Plan,
    public val env: ViewEnv,
    public val shape: Shape,
    // The admitted entries in answer order, not cut to the limit, each as its place.
    internal val entries: MutableList<List<Value>>,
    internal val byKey: TreeMap<List<Value>, Entry>,
    internal val byDep: TreeMap<Pair<NodeId, Value>, TreeSet<List<Value>>>,
    internal val groups: TreeMap<List<Value>, TreeSet<List<Value>>>,
) {
    /** The answer as it stands: the admitted entries' nodes in order, cut to the limit. */
    public fun rows(): List<Value> = (0 until minOf(entries.size, limit())).map(::nodeAt)

    override fun equals(other: Any?): Boolean =
        other is MaintainedView &&
            plan == other.plan &&
            env == other.env &&
            shape == other.shape &&
            entries == other.entries &&
            byKey == other.byKey &&
            byDep == other.byDep &&
            groups == other.groups

    override fun hashCode(): Int = byKey.hashCode()

    private fun limit(): Int {
        val n = plan.limit ?: return Int.MAX_VALUE
        return if (n <= 0) 0 else if (n > Int.MAX_VALUE) Int.MAX_VALUE else n.toInt()
    }

    private fun keyAt(i: Int): List<Value> = entries[i].subList(plan.order.size, entries[i].size)

    private fun nodeAt(i: Int): Value = byKey[keyAt(i)]!!.node

    private fun positionIn(from: Int, to: Int, e: Entry): Int {
        var lo = from
        var hi = to
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (View.comparePlace(plan, entries[mid], e) < 0) lo = mid + 1 else hi = mid
        }
        return lo
    }

    private fun fitsAt(p: Int, e: Entry): Boolean {
        val before = p == 0 || View.comparePlace(plan, entries[p - 1], e) < 0
        val after = p + 1 >= entries.size || View.comparePlace(plan, entries[p + 1], e) > 0
        return before && after
    }

    // Every dependency an entry recorded, its kept numbers' included.
    internal fun index(e: Entry) {
        indexDeps(e)
        for (d in e.held.keys) byDep.getOrPut(d, ::keySet).add(e.key)
    }

    private fun indexDeps(e: Entry) {
        for (d in e.deps) byDep.getOrPut(d, ::keySet).add(e.key)
    }

    private fun unindex(e: Entry) {
        for (d in e.deps) {
            val ks = byDep[d] ?: continue
            ks.remove(e.key)
            if (ks.isEmpty()) byDep.remove(d)
        }
    }

    // What the kept numbers of the entry under `key` came to hold or let go, in order.
    internal fun applyOps(key: List<Value>, ops: List<Pair<Pair<NodeId, Value>, Boolean>>) {
        for ((d, holds) in ops) {
            if (holds) byDep.getOrPut(d, ::keySet).add(key)
            else {
                val ks = byDep[d] ?: continue
                ks.remove(key)
                if (ks.isEmpty()) byDep.remove(d)
            }
        }
    }

    // §1.5, 3–4 One entry's new state against its old, as patches against the
    // window the client holds (`View::settle`).
    internal fun settle(key: List<Value>, new: Entry?, out: MutableList<Patch>) {
        val lim = limit()
        val old = byKey.remove(key)
        val was = if (old != null && old.admitted) positionIn(0, entries.size, old) else null
        val sameDeps = old != null && new != null && old.deps == new.deps
        if (old != null && !sameDeps) unindex(old)
        val now =
            when {
                new == null || !new.admitted -> null
                was == null -> positionIn(0, entries.size, new)
                fitsAt(was, new) -> was
                was > 0 && View.comparePlace(plan, entries[was - 1], new) > 0 ->
                    positionIn(0, was, new)
                else -> positionIn(was + 1, entries.size, new) - 1
            }
        val changed = old == null || new == null || old.node != new.node
        when {
            was != null && now == null -> entries.removeAt(was)
            was == null && now != null -> entries.add(now, emptyList())
            was != null && now != null && was != now -> entries.add(now, entries.removeAt(was))
        }
        if (now != null && new != null) {
            val n = plan.order.size
            val slot = entries[now]
            if (slot.size != n + new.key.size || slot.subList(0, n) != new.order)
                entries[now] = View.place(new)
        }
        if (new != null) {
            if (!sameDeps) indexDeps(new)
            byKey[key] = new
        }
        when {
            was == null && now == null -> {}
            was != null && now == null ->
                if (was < lim) {
                    out.add(Patch.Remove(was))
                    if (entries.size >= lim) out.add(Patch.Insert(lim - 1, nodeAt(lim - 1)))
                }
            was == null && now != null ->
                if (now < lim) {
                    out.add(Patch.Insert(now, nodeAt(now)))
                    if (entries.size > lim) out.add(Patch.Remove(lim))
                }
            was == now -> if (was!! < lim && changed) out.add(Patch.Update(was, nodeAt(was)))
            else -> {
                val p = was!!
                val q = now!!
                when {
                    p < lim && q < lim -> {
                        out.add(Patch.Remove(p))
                        out.add(Patch.Insert(q, nodeAt(q)))
                    }
                    p < lim -> {
                        out.add(Patch.Remove(p))
                        out.add(Patch.Insert(lim - 1, nodeAt(lim - 1)))
                    }
                    q < lim -> {
                        out.add(Patch.Insert(q, nodeAt(q)))
                        out.add(Patch.Remove(lim))
                    }
                }
            }
        }
    }
}

/** §1.5 Hydrate, push, and the contract (`view.rs`). */
public object Views {
    /** R9 `shape`: what a view of `plan` keeps as numbers. */
    public fun shape(sch: Schema, plan: Plan, helpers: List<Function>): Shape =
        Shapes.shape(sch, plan, helpers)

    /** §1.5 `hydrate`: pull everything, and keep it. */
    public fun hydrate(sch: Schema, plan: Plan, env: ViewEnv, st: Store): MaintainedView {
        val shape = Shapes.shape(sch, plan, env.helpers)
        val scope = env.scope(sch)
        val pulled = pullRoot(Cx(sch, plan, shape, scope, st))
        val groups = TreeMap<List<Value>, TreeSet<List<Value>>>(KeyOrder)
        val s = plan.source
        if (s is Source.Group) {
            val (tbl, filter) = View.source(sch, plan, emptyList(), scope)
            for (r in View.fetch(st, plan.table, filter)) groups
                .getOrPut(View.groupOf(s.by, r), ::keySet)
                .add(tbl.keyOf(r))
        }
        val v =
            MaintainedView(
                plan, env, shape, ArrayList(), TreeMap(KeyOrder), TreeMap(depOrder), groups)
        for (e in pulled) {
            if (e.admitted) v.entries.add(View.place(e))
            v.index(e)
            v.byKey[e.key] = e
        }
        return v
    }

    /** §1.6 `rebuild`: hydrate again, in the same environment. */
    public fun rebuild(sch: Schema, st: Store, view: MaintainedView): MaintainedView =
        hydrate(sch, view.plan, view.env, st)

    /**
     * `contract`: a maintained view is right when it is indistinguishable from one hydrated now.
     */
    public fun contract(sch: Schema, st: Store, view: MaintainedView): Boolean =
        try {
            rebuild(sch, st, view) == view
        } catch (_: EvalFault) {
            false
        }

    // A group's members as numbers, from its rows.
    private fun memberNums(cx: Cx, aggs: List<Pair<Agg, Sym>>, rows: List<Value>): List<Num> {
        val nums = R9.zero(cx, aggs)
        for (r in rows) R9.putIn(aggs, nums, R9.terms(cx, aggs, r))
        return nums
    }

    // §1.5, 3 and R9 One candidate of the root plan, built over the numbers `held` keeps
    // (`entry_at`).
    private fun entryAt(
        cx: Cx,
        key: List<Value>,
        row: Cand,
        members: Members,
        held: HeldMap,
        ops: Ops
    ): Entry {
        val plan = cx.plan
        val face = cx.shape.root
        val deps = ArrayList<Pair<NodeId, Value>>()
        if (plan.isBare) {
            return Entry(
                key,
                deps,
                plan.order.map { (k, _) -> row.field((k as Key.Column).column) },
                true,
                row.value,
                held,
                emptyList())
        }
        val node = cx.scope.node()
        plan.row?.let { node.bind(it, row.value) }
        var nums: List<Num> = emptyList()
        when (members) {
            is Members.None -> {}
            is Members.ListM -> plan.members?.let { node.bind(it, members.v) }
            is Members.Nums -> {
                R9.bindNums(node, face.members ?: emptyList(), members.ns)
                nums = members.ns
            }
        }
        var id = 0
        for (l in plan.lookups) {
            val k = l.key.map { node.eval(it) }
            if (k.any { it is Value.VNull }) node.bind(l.sym, Value.VNull)
            else {
                node.bind(l.sym, cx.st.get(l.table, k) ?: Value.VNull)
                deps.add(id to Value.VList(k))
            }
            id += 1
        }
        val fields: MutableMap<String, Value>? =
            if (plan.project == null) LinkedHashMap(row.value.fields) else null
        for ((j, r) in plan.related.withIndex()) {
            val on = r.on.map { (c, e) -> c to node.eval(e) }
            if (face.kept[j]) {
                val dep = Value.VList(on.map { it.second })
                R9.ensure(cx, id, dep, null, held, ops)
                val stale = held.keys.filter { it.first == id && it.second != dep }
                for (k in stale) R9.release(cx, k, null, held, ops)
                R9.bindNums(node, cx.kept(id).aggs, held[id to dep]!!.nums)
            } else {
                deps.add(id to Value.VList(on.map { it.second }))
                val kids = View.pullAt(cx.sch, r.plan, id + 1, on, cx.scope, cx.st)
                for (kid in kids) deps.addAll(kid.deps)
                val list = Value.VList(View.answer(r.plan, kids))
                fields?.put(r.name, list)
                node.bind(r.sym, list)
            }
            id += 1 + View.nodeCount(r.plan)
        }
        val admitted = face.having?.let { R9.boolOf(node.eval(it)) } ?: true
        val value =
            when {
                !admitted -> Value.VNull
                face.project != null -> node.eval(face.project)
                else -> Value.VStruct(fields ?: emptyMap())
            }
        val order =
            plan.order.zip(face.order).map { (kd, e) ->
                when (val k = kd.first) {
                    is Key.Column -> row.field(k.column)
                    is Key.ExprK ->
                        node.eval(
                            e
                                ?: throw EvalFault.Bug(
                                    EvalError.TypeError("an expression order key with no face")))
                }
            }
        return Entry(key, deps, order, admitted, value, held, nums)
    }

    // §1.5 and R9 Every candidate of the root plan, pulled with its shape.
    private fun pullRoot(cx: Cx): List<Entry> {
        val plan = cx.plan
        val (tbl, filter) = View.source(cx.sch, plan, emptyList(), cx.scope)
        val rows = View.fetch(cx.st, plan.table, filter)
        val out = ArrayList<Entry>()
        val ops = ArrayList<Pair<Pair<NodeId, Value>, Boolean>>()
        when (val s = plan.source) {
            is Source.Table ->
                for (row in rows) out.add(
                    entryAt(cx, tbl.keyOf(row), Cand(row), Members.None, HeldMap(), ops))
            is Source.Group -> {
                val groups = TreeMap<List<Value>, MutableList<Value>>(KeyOrder)
                for (row in
                    rows.sortedWith { a, b -> KeyOrder.compare(tbl.keyOf(a), tbl.keyOf(b)) }) groups
                    .getOrPut(View.groupOf(s.by, row), ::ArrayList)
                    .add(row)
                for ((k, members) in groups) {
                    val aggs = cx.shape.root.members
                    val m =
                        if (aggs != null) Members.Nums(memberNums(cx, aggs, members))
                        else Members.ListM(Value.VList(members))
                    out.add(entryAt(cx, k, Cand(View.groupKey(s.by, k)), m, HeldMap(), ops))
                }
            }
        }
        out.sortWith { a, b -> View.compareEntries(plan, a, b) }
        return out
    }

    /**
     * §1.5 `push_all`: the changes of one settle pushed through the view, `st` already the store
     * after all of them. Every entry a change touches is rebuilt from `st` once — its kept numbers
     * moved by the change's terms rather than recounted — and settled against the old one in key
     * order into patches. A fault leaves the view stale: `rebuild` it.
     */
    public fun pushAll(
        sch: Schema,
        st: Store,
        changes: List<Change>,
        view: MaintainedView
    ): List<Patch> {
        val scope = view.env.scope(sch)
        val cx = Cx(sch, view.plan, view.shape, scope, st)
        val t = touched(cx, changes, view)
        val rebuilt = ArrayList<Triple<List<Value>, Entry?, Ops>>()
        for ((k, hits) in t.keys) {
            val old = view.byKey[k]
            val held = old?.held ?: HeldMap()
            // The held numbers move to the rebuilt entry; the old one keeps none.
            if (old != null) view.byKey[k] = old.copy(held = HeldMap())
            val ops: Ops = ArrayList()
            sweep(cx, held, hits, ops)
            rebuilt.add(Triple(k, rootOf(cx, k, held, t, view.groups, view.byKey[k], ops), ops))
        }
        for ((g, m) in t.groups) {
            val members = view.groups.getOrPut(g, ::keySet)
            members.removeAll(m.left)
            members.addAll(m.arrived)
            if (members.isEmpty()) view.groups.remove(g)
        }
        val out = ArrayList<Patch>()
        for ((k, e, ops) in rebuilt) {
            view.settle(k, e, out)
            view.applyOps(k, ops)
        }
        return out
    }

    // §1.5, 1–2 What the changes touch.
    private fun touched(cx: Cx, changes: List<Change>, view: MaintainedView): Touched {
        val plan = cx.plan
        val tbl =
            cx.sch.lookupTable(plan.table)
                ?: throw EvalFault.Bug(EvalError.UnknownTable(plan.table))
        val ns = View.nodes(plan)
        val t = Touched()
        val memberAggs =
            cx.shape.root.members?.takeIf { aggs -> aggs.any { it.first !is Agg.Count } }
        for (ch in changes) {
            val (old, new) =
                when (ch) {
                    is Change.Add -> null to ch.row
                    is Change.Remove -> ch.row to null
                    is Change.Edit -> ch.old to ch.new
                }
            if (ch.table == plan.table) {
                when (val s = plan.source) {
                    is Source.Table ->
                        listOfNotNull(old, new).forEach {
                            t.keys.getOrPut(tbl.keyOf(it), ::ArrayList)
                        }
                    // In change order, so an edit within a group, out of one and
                    // into another, leaves the members right — and a kept sum
                    // moved by each row that really left or arrived.
                    is Source.Group ->
                        for ((r, arrives) in listOf(old to false, new to true)) {
                            if (r == null) continue
                            val g = View.groupOf(s.by, r)
                            val base = view.groups[g]
                            val members = t.groups.getOrPut(g, ::Moves)
                            val moved =
                                if (!arrives) members.leave(base, tbl.keyOf(r))
                                else
                                    View.admits(cx.rootFilter, r) &&
                                        members.arrive(base, tbl.keyOf(r))
                            if (moved && memberAggs != null) {
                                val terms = R9.terms(cx, memberAggs, r)
                                val tr =
                                    t.sums.getOrPut(g) {
                                        Track(
                                            view.byKey[g]
                                                ?.takeIf { it.members.size == memberAggs.size }
                                                ?.members
                                                ?.toMutableList(),
                                            false)
                                    }
                                val nums = tr.nums
                                if (nums != null) {
                                    if (arrives) R9.putIn(memberAggs, nums, terms)
                                    else tr.stale = R9.takeOut(memberAggs, nums, terms) || tr.stale
                                }
                            }
                            t.keys.getOrPut(g, ::ArrayList)
                        }
                }
            }
            for ((id, n) in ns.withIndex()) {
                if (n.table != ch.table) continue
                val kept = cx.shape.kept.containsKey(id)
                val dOld = old?.let { n.dependency(cx.sch, it) }
                val dNew = new?.let { n.dependency(cx.sch, it) }
                val both = dOld != null && dOld == dNew
                for ((d, isOld) in listOf(dOld to true, dNew to false)) {
                    if (d == null || (both && !isOld)) continue
                    val ks = view.byDep[id to d] ?: continue
                    for (k in ks) {
                        val hits = t.keys.getOrPut(k, ::ArrayList)
                        if (kept)
                            hits.add(
                                Hit(
                                    id,
                                    d,
                                    if (isOld) old else null,
                                    if (!isOld || both) new else null))
                    }
                }
            }
        }
        return t
    }

    // §1.13, R9 An entry's kept numbers brought up to the store, from the hits
    // its dependencies took, deepest first (`sweep`).
    private fun sweep(cx: Cx, held: HeldMap, hits: List<Hit>, ops: Ops) {
        val dirty = TreeMap<Pair<NodeId, Value>, Dirt>(depOrder)
        for (hit in hits) {
            val k = hit.id to hit.on
            val h = held[k] ?: continue
            val d = dirty.getOrPut(k, ::Dirt)
            if (d.before == null) d.before = h.nums.toList()
            val c = cx.related(hit.id).plan
            if (c.related.isEmpty()) {
                val aggs = cx.kept(hit.id).aggs
                hit.old
                    ?.let { R9.rowTerms(cx, hit.id, it) }
                    ?.let { d.stale = R9.takeOut(aggs, h.nums, it) || d.stale }
                hit.new?.let { R9.rowTerms(cx, hit.id, it) }?.let { R9.putIn(aggs, h.nums, it) }
            } else {
                val tbl =
                    cx.sch.lookupTable(c.table)
                        ?: throw EvalFault.Bug(EvalError.UnknownTable(c.table))
                for (row in listOfNotNull(hit.old, hit.new)) d.reread.add(tbl.keyOf(row))
            }
        }
        while (dirty.isNotEmpty()) {
            val last = dirty.pollLastEntry()
            val k = last.key
            val d = last.value
            val h = held.remove(k) ?: continue
            val before = d.before ?: h.nums.toList()
            val (id, on) = k
            val r = cx.related(id)
            val ids = cx.kept(id).face.ids
            val aggs = cx.kept(id).aggs
            if (d.stale) R9.extremes(cx, r.plan, R9.pinsOf(r, on), aggs, h.nums)
            for (key in d.reread) {
                val old = h.kids.remove(key)
                val row =
                    cx.st.get(r.plan.table, key)?.takeIf {
                        View.admits(cx.filters[id], it) &&
                            Node.RelatedN(r).dependency(cx.sch, it) == on
                    }
                val new = row?.let { R9.buildKid(cx, id, on, key, it, held, ops) }
                if (old != null) {
                    val me = Triple(id, on, key)
                    for ((j, pair) in ids.zip(old.subs).withIndex()) {
                        val (jid, sub) = pair
                        if (new == null || new.subs[j] != sub)
                            R9.release(cx, jid to sub, me, held, ops)
                    }
                    R9.kidTerms(cx, id, old)?.let { R9.takeOut(aggs, h.nums, it) }
                }
                if (new != null) {
                    R9.kidTerms(cx, id, new)?.let { R9.putIn(aggs, h.nums, it) }
                    h.kids[key] = new
                }
            }
            for (key in d.reeval) {
                if (key in d.reread) continue
                val kid = h.kids[key] ?: continue
                val (admitted, node) = R9.kidNode(cx, id, kid.row, kid.subs, held)
                if (admitted == kid.admitted && node == kid.node) continue
                val oldTerms = R9.kidTerms(cx, id, kid)
                val now = kid.copy(admitted = admitted, node = node)
                h.kids[key] = now
                val newTerms = R9.kidTerms(cx, id, now)
                oldTerms?.let { R9.takeOut(aggs, h.nums, it) }
                newTerms?.let { R9.putIn(aggs, h.nums, it) }
            }
            if (h.nums != before) {
                for (p in h.parents) if (p != null)
                    dirty.getOrPut(p.first to p.second, ::Dirt).reeval.add(p.third)
            }
            held[k] = h
        }
    }

    // §1.5, 1 and R9 The entry under `k` as the store now decides it: gone, new,
    // or rebuilt over the numbers `held` keeps.
    private fun rootOf(
        cx: Cx,
        k: List<Value>,
        held: HeldMap,
        t: Touched,
        viewGroups: Map<List<Value>, TreeSet<List<Value>>>,
        old: Entry?,
        ops: Ops
    ): Entry? {
        val plan = cx.plan
        fun gone(): Entry? {
            for (d in held.keys) ops.add(d to false)
            return null
        }
        return when (val s = plan.source) {
            is Source.Table -> {
                val row = cx.st.get(plan.table, k)
                if (row != null && View.admits(cx.rootFilter, row))
                    entryAt(cx, k, Cand(row), Members.None, held, ops)
                else gone()
            }
            is Source.Group -> {
                val base = viewGroups[k]
                val moves = t.groups[k]
                val size = moves?.size(base) ?: (base?.size ?: 0)
                if (size == 0) return gone()
                fun rows(): List<Value> =
                    (moves?.keys(base) ?: base?.toList() ?: emptyList()).mapNotNull {
                        cx.st.get(plan.table, it)
                    }
                val aggs = cx.shape.root.members
                val members =
                    if (aggs == null) Members.ListM(Value.VList(rows()))
                    else {
                        val track = t.sums[k]
                        val moved: MutableList<Num>? =
                            when {
                                old != null && track == null && old.members.size == aggs.size ->
                                    old.members.toMutableList()
                                track?.nums != null -> {
                                    val nums = track.nums.toMutableList()
                                    if (track.stale) R9.extremes(cx, plan, s.by.zip(k), aggs, nums)
                                    nums
                                }
                                else -> null
                            }
                        Members.Nums(
                            when {
                                moved != null -> {
                                    for (i in aggs.indices) if (aggs[i].first is Agg.Count)
                                        moved[i] = Num.N(BigInteger.valueOf(size.toLong()))
                                    moved
                                }
                                aggs.all { it.first is Agg.Count } ->
                                    List(aggs.size) { Num.N(BigInteger.valueOf(size.toLong())) }
                                else -> memberNums(cx, aggs, rows())
                            })
                    }
                entryAt(cx, k, Cand(View.groupKey(s.by, k)), members, held, ops)
            }
        }
    }

    /** Apply each patch in order: the definition a client's list is held to (`splice`). */
    public fun splice(ps: List<Patch>, xs: List<Value>): List<Value> {
        val vs = ArrayList(xs)
        for (p in ps) {
            when (p) {
                is Patch.Insert -> vs.add(minOf(p.at, vs.size), p.node)
                is Patch.Remove -> if (p.at < vs.size) vs.removeAt(p.at)
                is Patch.Update -> if (p.at < vs.size) vs[p.at] = p.node
            }
        }
        return vs
    }

    /**
     * A patch as the vectors write one: `{t: "insert", at, node}`, `{t: "remove", at}`, `{t:
     * "update", at, node}`.
     */
    public fun patchValue(p: Patch): Value =
        when (p) {
            is Patch.Insert ->
                Value.record(
                    "t" to Value.VText("insert"),
                    "at" to Value.VInt(p.at.toLong()),
                    "node" to p.node)
            is Patch.Remove ->
                Value.record("t" to Value.VText("remove"), "at" to Value.VInt(p.at.toLong()))
            is Patch.Update ->
                Value.record(
                    "t" to Value.VText("update"),
                    "at" to Value.VInt(p.at.toLong()),
                    "node" to p.node)
        }
}
