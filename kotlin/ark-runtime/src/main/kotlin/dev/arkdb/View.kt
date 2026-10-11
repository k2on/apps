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
internal class Cand(val value: Row) {
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

    /** Where an entry sorts, as one list: its order keys, then its key (`place`). */
    public fun place(e: Entry): List<Value> = e.order + e.key

    /** `compare_place`: `compareEntries` of the entry a place was taken from, and `e`. */
    public fun comparePlace(plan: Plan, place: List<Value>, e: Entry): Int {
        val n = minOf(plan.order.size, place.size)
        for ((i, kd) in plan.order.withIndex()) {
            val o =
                compareValue(
                    place.getOrElse(i) { Value.VNull }, e.order.getOrElse(i) { Value.VNull })
            val od = if (kd.second == Dir.Desc) -o else o
            if (od != 0) return od
        }
        return KeyOrder.compare(place.subList(n, place.size), e.key)
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
    internal fun entry(
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

private val depOrder: Comparator<Pair<NodeId, Value>> =
    compareBy<Pair<NodeId, Value>> { it.first }
        .thenComparator { a, b -> compareValue(a.second, b.second) }

/**
 * §1.5 A plan kept up to date (`view::View`): the plan, its environment, and one `Entry` per
 * candidate, with the two indexes that route a change. Every field is a function of the plan, the
 * environment and the store, which `contract` checks against a fresh hydrate.
 *
 * The reference also keeps, for a related list or a group's members whose every use is a count, a
 * sum or an extreme, those numbers rather than the list, and moves them by each change's terms
 * (`docs/plan-perf.md` R9, `Shape`). That is a cost, not a meaning — the contract holds either way
 * — and it is not ported: an entry a change touches is rebuilt, and its lists folded again, under
 * the same §13 rule for a sum.
 */
public class MaintainedView
internal constructor(
    public val plan: Plan,
    public val env: ViewEnv,
    // The admitted entries in answer order, not cut to the limit, each as its place.
    internal val entries: MutableList<List<Value>>,
    internal val byKey: java.util.TreeMap<List<Value>, Entry>,
    internal val byDep: java.util.TreeMap<Pair<NodeId, Value>, java.util.TreeSet<List<Value>>>,
    internal val groups: java.util.TreeMap<List<Value>, java.util.TreeSet<List<Value>>>,
) {
    /** The answer as it stands: the admitted entries' nodes in order, cut to the limit. */
    public fun rows(): List<Value> = (0 until minOf(entries.size, limit())).map(::nodeAt)

    override fun equals(other: Any?): Boolean =
        other is MaintainedView &&
            plan == other.plan &&
            env == other.env &&
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

    // Where an entry sits, or would sit, among the admitted: a binary search over the places.
    private fun positionIn(from: Int, to: Int, e: Entry): Int {
        var lo = from
        var hi = to
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (View.comparePlace(plan, entries[mid], e) < 0) lo = mid + 1 else hi = mid
        }
        return lo
    }

    // Whether `e` belongs at `p`: after the entry before and before the entry after.
    private fun fitsAt(p: Int, e: Entry): Boolean {
        val before = p == 0 || View.comparePlace(plan, entries[p - 1], e) < 0
        val after = p + 1 >= entries.size || View.comparePlace(plan, entries[p + 1], e) > 0
        return before && after
    }

    internal fun indexDeps(e: Entry) {
        for (d in e.deps) byDep.getOrPut(d) { java.util.TreeSet(KeyOrder) }.add(e.key)
    }

    private fun unindex(e: Entry) {
        for (d in e.deps) {
            val ks = byDep[d] ?: continue
            ks.remove(e.key)
            if (ks.isEmpty()) byDep.remove(d)
        }
    }

    // §1.5, 3–4 One entry's new state against its old, as patches against the
    // window the client holds (`View::settle`): a moved entry is a `Remove` at
    // its old place and an `Insert` at its new one; one that stays is an
    // `Update` when its node changed and nothing when it did not; under a
    // limit, an entry leaving the window lets the next one in and one
    // entering pushes the last one out.
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

// R9's group bookkeeping without its numbers: what the changes did to one
// group's members, over the kept set (`view::Moves`).
private class Moves {
    val arrived = java.util.TreeSet(KeyOrder)
    val left = java.util.TreeSet(KeyOrder)

    fun has(base: Set<List<Value>>?, k: List<Value>): Boolean =
        k in arrived || (k !in left && base?.contains(k) == true)

    fun leave(base: Set<List<Value>>?, k: List<Value>) {
        if (!has(base, k)) return
        if (!arrived.remove(k)) left.add(k)
    }

    fun arrive(base: Set<List<Value>>?, k: List<Value>) {
        if (has(base, k)) return
        if (!left.remove(k)) arrived.add(k)
    }

    fun keys(base: Set<List<Value>>?): List<List<Value>> {
        val out = java.util.TreeSet(KeyOrder)
        base?.forEach { if (it !in left) out.add(it) }
        out.addAll(arrived)
        return out.toList()
    }
}

/** §1.5 Hydrate, push, and the contract (`view.rs`, "The view" and "Maintenance"). */
public object Views {
    /** §1.5 `hydrate`: pull everything, and keep it. */
    public fun hydrate(sch: Schema, plan: Plan, env: ViewEnv, st: Store): MaintainedView {
        val scope = env.scope(sch)
        val pulled = View.pull(sch, plan, scope, st)
        val groups = java.util.TreeMap<List<Value>, java.util.TreeSet<List<Value>>>(KeyOrder)
        val s = plan.source
        if (s is Source.Group) {
            val (tbl, filter) = View.source(sch, plan, emptyList(), scope)
            for (r in View.fetch(st, plan.table, filter)) groups
                .getOrPut(View.groupOf(s.by, r)) { java.util.TreeSet(KeyOrder) }
                .add(tbl.keyOf(r))
        }
        val v =
            MaintainedView(
                plan,
                env,
                ArrayList(),
                java.util.TreeMap(KeyOrder),
                java.util.TreeMap(depOrder),
                groups)
        for (e in pulled) {
            if (e.admitted) v.entries.add(View.place(e))
            v.indexDeps(e)
            v.byKey[e.key] = e
        }
        return v
    }

    /** §1.6 `rebuild`: hydrate again, in the same environment. */
    public fun rebuild(sch: Schema, st: Store, view: MaintainedView): MaintainedView =
        hydrate(sch, view.plan, view.env, st)

    /**
     * A maintained view is right when it is indistinguishable from one hydrated now (`contract`).
     */
    public fun contract(sch: Schema, st: Store, view: MaintainedView): Boolean =
        try {
            rebuild(sch, st, view) == view
        } catch (_: EvalFault) {
            false
        }

    /**
     * §1.5 `push_all`: the changes of one settle pushed through the view, `st` already the store
     * after all of them. Every entry a change touches — its source row, its group, or a dependency
     * it recorded at any depth — is rebuilt from `st`, once, and settled against the old one in key
     * order into patches. A fault leaves the view stale: `rebuild` it.
     */
    public fun pushAll(
        sch: Schema,
        st: Store,
        changes: List<Change>,
        view: MaintainedView
    ): List<Patch> {
        val plan = view.plan
        val scope = view.env.scope(sch)
        val tbl =
            sch.lookupTable(plan.table) ?: throw EvalFault.Bug(EvalError.UnknownTable(plan.table))
        val rootFilter = plan.filter?.let { View.evalPred(it) { e -> scope.eval(e) } }
        val ns = View.nodes(plan)
        val keys = java.util.TreeSet(KeyOrder)
        val moves = java.util.TreeMap<List<Value>, Moves>(KeyOrder)
        for (ch in changes) {
            val (old, new) =
                when (ch) {
                    is Change.Add -> null to ch.row
                    is Change.Remove -> ch.row to null
                    is Change.Edit -> ch.old to ch.new
                }
            if (ch.table == plan.table) {
                when (val s = plan.source) {
                    is Source.Table -> listOfNotNull(old, new).forEach { keys.add(tbl.keyOf(it)) }
                    // In change order, so an edit within a group, out of one and into
                    // another, all leave the members right.
                    is Source.Group ->
                        for ((r, arrives) in listOf(old to false, new to true)) {
                            if (r == null) continue
                            val g = View.groupOf(s.by, r)
                            val base = view.groups[g]
                            val m = moves.getOrPut(g, ::Moves)
                            if (!arrives) m.leave(base, tbl.keyOf(r))
                            else if (View.admits(rootFilter, r)) m.arrive(base, tbl.keyOf(r))
                            keys.add(g)
                        }
                }
            }
            for ((id, n) in ns.withIndex()) {
                if (n.table != ch.table) continue
                val dOld = old?.let { n.dependency(sch, it) }
                val dNew = new?.let { n.dependency(sch, it) }
                for (d in listOfNotNull(dOld, dNew.takeIf { it != dOld })) view.byDep[id to d]
                    ?.let { keys.addAll(it) }
            }
        }
        val rebuilt =
            keys.map { k ->
                k to rootOf(sch, st, plan, scope, tbl, rootFilter, k, moves[k], view.groups[k])
            }
        for ((g, m) in moves) {
            val members = view.groups.getOrPut(g) { java.util.TreeSet(KeyOrder) }
            members.removeAll(m.left)
            members.addAll(m.arrived)
            if (members.isEmpty()) view.groups.remove(g)
        }
        val out = ArrayList<Patch>()
        for ((k, e) in rebuilt) view.settle(k, e, out)
        return out
    }

    // §1.5, 1 The entry under `k` as the store now decides it: gone, new, or rebuilt.
    private fun rootOf(
        sch: Schema,
        st: Store,
        plan: Plan,
        scope: Scope,
        tbl: Table,
        rootFilter: Filter?,
        k: List<Value>,
        moves: Moves?,
        base: Set<List<Value>>?,
    ): Entry? =
        when (val s = plan.source) {
            is Source.Table -> {
                val row = st.get(plan.table, k)
                if (row != null && View.admits(rootFilter, row))
                    View.entry(sch, plan, 0, scope, st, k, Cand(row), null)
                else null
            }
            is Source.Group -> {
                val members =
                    (moves?.keys(base) ?: base?.toList() ?: emptyList()).mapNotNull {
                        st.get(plan.table, it)
                    }
                if (members.isEmpty()) null
                else
                    View.entry(
                        sch,
                        plan,
                        0,
                        scope,
                        st,
                        k,
                        Cand(View.groupKey(s.by, k)),
                        Value.VList(members))
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
