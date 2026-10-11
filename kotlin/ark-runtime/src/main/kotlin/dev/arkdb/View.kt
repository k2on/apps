// §13 Views: what a plan means, and a plan kept up to date
// (`rust/ark/src/view.rs`).
//
// `pull` is the one evaluator of plans: a query's value, a mutator's
// `select`, a hydrating view are all what it answers. It returns one
// `Entry` per candidate — a source row the filter admits, or a group —
// carrying its key, the dependencies its subtree read (by plan node), its
// order keys, whether `having` admitted it, and its node. `answer` is the
// list a caller sees; `read` is that answer for a caller that keeps no
// entries.
package dev.arkdb

/**
 * A `Lookup` or a `Related` of a plan tree, by its position in a pre-order walk (`view::NodeId`).
 */
public typealias NodeId = Int

/** One node of a plan tree that reads another table (`view::Node`). */
public sealed class Node {
    public data class LookupN(val l: Lookup) : Node()

    public data class RelatedN(val r: Related) : Node()

    /** The table this node reads. */
    public val table: String
        get() =
            when (this) {
                is LookupN -> l.table
                is RelatedN -> r.plan.table
            }

    /**
     * The dependency value a row of `table` would satisfy: the row's key for a lookup, the values
     * of the `on` columns for a related plan — each as a list.
     */
    public fun dependency(sch: Schema, row: Row): Value =
        when (this) {
            is LookupN -> Value.VList(sch.lookupTable(l.table)?.keyOf(row) ?: emptyList())
            is RelatedN -> Value.VList(r.on.map { row.fields[it.first] ?: Value.VNull })
        }
}

/**
 * §1.5 One candidate of a plan: a source row the filter admits, or a non-empty group
 * (`view::Entry`).
 */
public data class Entry(
    val key: List<Value>,
    /** What the subtree read, at every depth, in order: `(node, dependency)`. */
    val deps: List<Pair<NodeId, Value>>,
    /** The order keys' values, in the plan's order. */
    val order: List<Value>,
    val admitted: Boolean,
    /** The node's value; `Null` when not admitted. */
    val node: Value,
)

// A filter with its right-hand sides evaluated: one constructor per `Pred`.
internal sealed class Filter {
    data class Cmp(val column: String, val op: CmpOp, val v: Value) : Filter()

    data class In(val column: String, val vs: List<Value>) : Filter()

    data class All(val fs: List<Filter>) : Filter()

    data class AnyF(val fs: List<Filter>) : Filter()

    data class Not(val f: Filter) : Filter()

    // D4 The needle, folded once; `null` for one that is not text, which admits nothing.
    data class Has(val column: String, val needle: String?) : Filter()
}

// What an entry is built over: a row of the plan's table, or a group's key as
// the struct of its `by` columns.
private class Cand(val value: Row) {
    // A column, as `Value::field` reads one: fatal when it is missing.
    fun field(c: String): Value =
        value.fields[c] ?: throw IllegalStateException("field: no field $c in ${value.show()}")
}

public object View {
    /** Every lookup and related node of a plan tree, by `NodeId` (`view::nodes`). */
    public fun nodes(plan: Plan): List<Node> {
        val out = ArrayList<Node>()
        fun walk(p: Plan) {
            for (l in p.lookups) out.add(Node.LookupN(l))
            for (r in p.related) {
                out.add(Node.RelatedN(r))
                walk(r.plan)
            }
        }
        walk(plan)
        return out
    }

    /** How many lookup and related nodes a plan tree has. */
    public fun nodeCount(plan: Plan): Int =
        plan.lookups.size + plan.related.sumOf { 1 + nodeCount(it.plan) }

    private fun limitOf(plan: Plan): Int {
        val n = plan.limit ?: return Int.MAX_VALUE
        return if (n <= 0) 0 else if (n > Int.MAX_VALUE) Int.MAX_VALUE else n.toInt()
    }

    /** §1.3 `pull`: every candidate of a plan over a store, ordered. */
    public fun pull(sch: Schema, plan: Plan, scope: Scope, st: Store): List<Entry> =
        pullAt(sch, plan, 0, emptyList(), scope, st)

    /** `answer`: the admitted entries' nodes, in order, cut to the limit. */
    public fun answer(plan: Plan, entries: List<Entry>): List<Value> =
        entries.asSequence().filter { it.admitted }.take(limitOf(plan)).map { it.node }.toList()

    /**
     * §1.4 `read`: the answer of a pull, for a caller that keeps no entries. A bare plan's rows are
     * read through the store's indexes in order where one serves (R1), and otherwise read and
     * sorted.
     */
    public fun read(sch: Schema, plan: Plan, scope: Scope, st: Store): List<Value> {
        if (!plan.isBare) return answer(plan, pull(sch, plan, scope, st))
        val (tbl, filter) = source(sch, plan, emptyList(), scope)
        val order = plan.order.map { (k, d) -> (k as Key.Column).column to d }
        val lim = limitOf(plan)
        if (lim == 0) return emptyList()
        val eq = equalities(filter)
        val sp = spans(filter)
        val keep: Keep = { admits(filter, it) }
        // A text search is read through its text index and then sorted.
        val searched =
            needles(filter)?.any { b -> b.any { trigrams(it.second).isNotEmpty() } } == true
        if (!searched)
            st.scanOrdered(plan.table, eq, sp, order, keep, lim)?.let {
                return it
            }
        val rows =
            fetch(st, plan.table, filter).sortedWith { a, b -> Rules.compareRows(tbl, order, a, b) }
        return if (rows.size > lim) rows.subList(0, lim) else rows
    }

    /** §1.5 `compare_entries`: each order key under its direction, then the entries' keys. */
    public fun compareEntries(plan: Plan, a: Entry, b: Entry): Int {
        for ((i, kd) in plan.order.withIndex()) {
            val o =
                compareValue(
                    a.order.getOrElse(i) { Value.VNull }, b.order.getOrElse(i) { Value.VNull })
            val od = if (kd.second == Dir.Desc) -o else o
            if (od != 0) return od
        }
        return KeyOrder.compare(a.key, b.key)
    }

    // A plan whose node ids start at `base`, its rows pinned by `pins`.
    internal fun pullAt(
        sch: Schema,
        plan: Plan,
        base: NodeId,
        pins: List<Pair<String, Value>>,
        scope: Scope,
        st: Store
    ): List<Entry> {
        val (tbl, filter) = source(sch, plan, pins, scope)
        val rows = fetch(st, plan.table, filter)
        val out = ArrayList<Entry>()
        when (val s = plan.source) {
            is Source.Table ->
                for (row in rows) out.add(
                    entry(sch, plan, base, scope, st, tbl.keyOf(row), Cand(row), null))
            is Source.Group -> {
                val groups = java.util.TreeMap<List<Value>, MutableList<Value>>(KeyOrder)
                for (row in
                    rows.sortedWith { a, b -> KeyOrder.compare(tbl.keyOf(a), tbl.keyOf(b)) }) {
                    groups.getOrPut(groupOf(s.by, row), ::ArrayList).add(row)
                }
                for ((k, members) in groups) {
                    out.add(
                        entry(
                            sch,
                            plan,
                            base,
                            scope,
                            st,
                            k,
                            Cand(groupKey(s.by, k)),
                            Value.VList(members)))
                }
            }
        }
        out.sortWith { a, b -> compareEntries(plan, a, b) }
        return out
    }

    internal fun groupOf(by: List<String>, row: Row): List<Value> =
        by.map { row.fields[it] ?: Value.VNull }

    internal fun groupKey(by: List<String>, k: List<Value>): Row = Value.VStruct(by.zip(k).toMap())

    // The rows a filter admits, in key order, through whatever indexes serve it.
    internal fun fetch(st: Store, table: String, filter: Filter?): List<Row> {
        val eq = equalities(filter)
        val sp = spans(filter)
        val keep: Keep = { admits(filter, it) }
        val has = needles(filter) ?: return st.scanWhereEq(table, eq, sp, keep)
        return st.scanWhereText(table, eq, sp, has, keep)
    }

    // A plan's table, and its filter and its pins as one filter, evaluated.
    internal fun source(
        sch: Schema,
        plan: Plan,
        pins: List<Pair<String, Value>>,
        scope: Scope
    ): Pair<Table, Filter?> {
        val tbl =
            sch.lookupTable(plan.table) ?: throw EvalFault.Bug(EvalError.UnknownTable(plan.table))
        val all = ArrayList<Filter>()
        for ((c, v) in pins) all.add(Filter.Cmp(c, CmpOp.Eq, v))
        plan.filter?.let { all.add(evalPred(it) { e -> scope.eval(e) }) }
        val filter =
            when (all.size) {
                0 -> null
                1 -> all[0]
                else -> Filter.All(all)
            }
        return tbl to filter
    }

    // One candidate, pulled: its lookups, its related plans, its having, its
    // node and its order keys, in that order.
    private fun entry(
        sch: Schema,
        plan: Plan,
        base: NodeId,
        scope: Scope,
        st: Store,
        key: List<Value>,
        row: Cand,
        members: Value?
    ): Entry {
        val deps = ArrayList<Pair<NodeId, Value>>()
        // A bare plan binds nothing: its node is the row.
        if (plan.isBare) {
            val order = plan.order.map { (k, _) -> orderKey(k, row, null) }
            return Entry(key, deps, order, true, row.value)
        }
        val node = scope.node()
        plan.row?.let { node.bind(it, row.value) }
        if (plan.members != null && members != null) node.bind(plan.members, members)
        var id = base
        for (l in plan.lookups) {
            val k = l.key.map { node.eval(it) }
            lookUp(node, l, st, k, id, deps)
            id += 1
        }
        val fields: MutableMap<String, Value>? =
            if (plan.project == null) LinkedHashMap(row.value.fields) else null
        for (r in plan.related) {
            val on = r.on.map { (c, e) -> c to node.eval(e) }
            deps.add(id to Value.VList(on.map { it.second }))
            val kids = pullAt(sch, r.plan, id + 1, on, scope, st)
            for (kid in kids) deps.addAll(kid.deps)
            val list = Value.VList(answer(r.plan, kids))
            fields?.put(r.name, list)
            node.bind(r.sym, list)
            id += 1 + nodeCount(r.plan)
        }
        val admitted =
            when (val h = plan.having) {
                null -> true
                else ->
                    when (val v = node.eval(h)) {
                        is Value.VBool -> v.value
                        else ->
                            throw EvalFault.Bug(
                                EvalError.TypeError("a having is a Bool, not ${v.show()}"))
                    }
            }
        val value =
            when {
                !admitted -> Value.VNull
                plan.project != null -> node.eval(plan.project)
                else -> Value.VStruct(fields ?: emptyMap())
            }
        val order = plan.order.map { (k, _) -> orderKey(k, row, node) }
        return Entry(key, deps, order, admitted, value)
    }

    // A lookup, bound: the row under its key, or `Null` — read, and recorded as a
    // dependency, only when no part of the key is `Null`.
    private fun lookUp(
        node: NodeScope,
        l: Lookup,
        st: Store,
        k: List<Value>,
        id: NodeId,
        deps: MutableList<Pair<NodeId, Value>>
    ) {
        if (k.any { it is Value.VNull }) {
            node.bind(l.sym, Value.VNull)
            return
        }
        node.bind(l.sym, st.get(l.table, k) ?: Value.VNull)
        deps.add(id to Value.VList(k))
    }

    private fun orderKey(k: Key, row: Cand, node: NodeScope?): Value =
        when (k) {
            is Key.Column -> row.field(k.column)
            is Key.ExprK ->
                node?.eval(k.e)
                    ?: throw EvalFault.Bug(
                        EvalError.TypeError("an expression order key on a bare plan"))
        }

    // The filter, evaluated --------------------------------------------------

    internal fun evalPred(p: Pred, ev: (Expr) -> Value): Filter =
        when (p) {
            is Pred.Cmp -> Filter.Cmp(p.column, p.op, ev(p.e))
            is Pred.In -> Filter.In(p.column, p.items.map(ev))
            is Pred.All -> Filter.All(p.items.map { evalPred(it, ev) })
            is Pred.AnyP -> Filter.AnyF(p.items.map { evalPred(it, ev) })
            is Pred.Not -> Filter.Not(evalPred(p.e, ev))
            is Pred.Has -> Filter.Has(p.column, (ev(p.e) as? Value.VText)?.let { fold(it.value) })
            // D2 A scope's leaves, refused in a plan by the verifier: `When` is the
            // constant it evaluates to, and `Exists`, which needs a store, admits nothing.
            is Pred.When ->
                if (ev(p.e) == Value.VBool(true)) Filter.All(emptyList())
                else Filter.AnyF(emptyList())
            is Pred.Exists -> Filter.AnyF(emptyList())
        }

    // D4 The text searches a filter holds however it is satisfied, as a
    // disjunction of conjunctions of (column, folded needle). Past sixteen
    // branches it says nothing, which is only a wider read.
    internal fun needles(f: Filter?): List<List<Pair<String, String>>>? {
        val most = 16
        fun go(f: Filter): List<List<Pair<String, String>>>? {
            when (f) {
                is Filter.Has ->
                    return if (f.needle != null) listOf(listOf(f.column to f.needle)) else null
                is Filter.All -> {
                    var out: List<List<Pair<String, String>>>? = null
                    for (g in f.fs.mapNotNull(::go)) {
                        val bs = out
                        if (bs == null) {
                            out = g
                            continue
                        }
                        val prod = bs.flatMap { b -> g.map { h -> b + h } }
                        if (prod.size > most) return bs
                        out = prod
                    }
                    return out
                }
                is Filter.AnyF -> {
                    val parts = f.fs.map(::go)
                    if (parts.any { it == null }) return null
                    val bs = parts.flatMap { it!! }
                    return if (bs.size <= most) bs else null
                }
                else -> return null
            }
        }
        return f?.let(::go)?.takeIf { it.isNotEmpty() }
    }

    // The columns a filter holds equal to a value however it is satisfied.
    internal fun equalities(f: Filter?): List<Pair<String, Value>> {
        val out = ArrayList<Pair<String, Value>>()
        fun go(f: Filter) {
            when {
                f is Filter.Cmp && f.op == CmpOp.Eq -> out.add(f.column to f.v)
                f is Filter.All -> f.fs.forEach(::go)
                else -> {}
            }
        }
        f?.let(::go)
        return out
    }

    // The columns a filter holds between bounds, one span per column with the
    // tightest bound of each side (R6).
    internal fun spans(f: Filter?): List<Span> {
        val out = ArrayList<Span>()
        fun bound(b: Bound): Value? =
            when (b) {
                is Bound.Unbounded -> null
                is Bound.Included -> b.v
                is Bound.Excluded -> b.v
            }
        // Whether `new` narrows past `old`, `inward` the direction a bound moves to narrow.
        fun tighter(new: Bound, old: Bound, inward: Int): Boolean {
            val o = bound(old) ?: return true
            val n = bound(new) ?: return false
            val by = Integer.signum(compareValue(n, o))
            return by == inward || (by == 0 && new is Bound.Excluded)
        }
        fun go(f: Filter) {
            if (f is Filter.Cmp && f.op in setOf(CmpOp.Ge, CmpOp.Gt, CmpOp.Le, CmpOp.Lt)) {
                var i = out.indexOfFirst { it.column == f.column }
                if (i < 0) {
                    out.add(Span(f.column, Bound.Unbounded, Bound.Unbounded))
                    i = out.size - 1
                }
                val s = out[i]
                if (f.op == CmpOp.Ge || f.op == CmpOp.Gt) {
                    val new = if (f.op == CmpOp.Ge) Bound.Included(f.v) else Bound.Excluded(f.v)
                    if (tighter(new, s.lo, 1)) out[i] = s.copy(lo = new)
                } else {
                    val new = if (f.op == CmpOp.Le) Bound.Included(f.v) else Bound.Excluded(f.v)
                    if (tighter(new, s.hi, -1)) out[i] = s.copy(hi = new)
                }
            } else if (f is Filter.All) f.fs.forEach(::go)
        }
        f?.let(::go)
        return out
    }

    // Whether a row passes the filter; no filter admits every row. A column the
    // row lacks reads as `Null`.
    internal fun admits(f: Filter?, row: Row): Boolean {
        fun go(f: Filter): Boolean {
            fun field(c: String): Value = row.fields[c] ?: Value.VNull
            return when (f) {
                is Filter.Cmp -> cmp(f.op, field(f.column), f.v)
                is Filter.In -> f.vs.any { cmp(CmpOp.Eq, field(f.column), it) }
                is Filter.All -> f.fs.all(::go)
                is Filter.AnyF -> f.fs.any(::go)
                is Filter.Not -> !go(f.f)
                // D4 Both sides folded; a `Null` holds nothing, and every text the empty needle.
                is Filter.Has -> {
                    val s = field(f.column)
                    when {
                        s !is Value.VText || f.needle == null -> false
                        f.needle.isEmpty() -> true
                        else -> fold(s.value).contains(f.needle)
                    }
                }
            }
        }
        return f == null || go(f)
    }
}
