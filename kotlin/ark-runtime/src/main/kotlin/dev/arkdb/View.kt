// §13 Incremental views (Ark.View): a plan kept up to date, reporting what
// it did to its own list as positions.
package dev.arkdb

/** The plan a view maintains: `Plan`, with every right-hand side a value. */
public typealias ViewPlan = Plan

/** A filter with its right-hand sides evaluated. */
public typealias Filter = Pred

/** What `push` did to the view's list, as positions into the list as it stands when each is applied. */
public sealed class Patch {
    public abstract val at: Int

    public data class Insert(override val at: Int, val node: Value) : Patch()
    public data class Remove(override val at: Int) : Patch()
    public data class Update(override val at: Int, val node: Value) : Patch()

    /** The patch as the vectors write it. */
    public fun toValue(): Value = when (this) {
        is Insert -> Value.record("t" to Value.VText("insert"), "at" to Value.VInt(at.toLong()), "node" to node)
        is Remove -> Value.record("t" to Value.VText("remove"), "at" to Value.VInt(at.toLong()))
        is Update -> Value.record("t" to Value.VText("update"), "at" to Value.VInt(at.toLong()), "node" to node)
    }
}

/** A maintained plan: the plan and its nodes, each beside the row it was built over, in the plan's order. */
public class View(public val plan: Plan, public val nodes: List<Pair<Row, Value>>) {
    /** The current nodes, in order. */
    public val rows: List<Value> get() = nodes.map { it.second }

    override fun equals(other: Any?): Boolean = other is View && plan == other.plan && nodes == other.nodes
    override fun hashCode(): Int = plan.hashCode() * 31 + nodes.hashCode()

    public companion object {
        /** Pull everything: what `select` answers for the plan, as a view. */
        public fun hydrate(sch: Schema, vp: Plan, st: Store): View = View(vp, pull(sch, vp, st))

        /** §13.2 The plan's order, then the key ascending: total. */
        public fun compareRows(tbl: Table, cols: List<Pair<String, Dir>>): Comparator<Row> = Comparator { a, b ->
            val o = Select.orderBy(cols).compare(a, b)
            if (o != 0) o else KeyOrder.compare(tbl.keyOf(a), tbl.keyOf(b))
        }

        /** What a list of patches means to whoever holds a copy of the list. */
        public fun splice(ps: List<Patch>, xs: List<Value>): List<Value> {
            val vs = ArrayList(xs)
            for (p in ps) when (p) {
                is Patch.Insert -> vs.add(p.at, p.node)
                is Patch.Remove -> vs.removeAt(p.at)
                is Patch.Update -> vs[p.at] = p.node
            }
            return vs
        }

        // `select` over an evaluated plan; a table the schema lacks is empty, so a view is total.
        private fun pull(sch: Schema, vp: Plan, st: Store): List<Pair<Row, Value>> {
            val tbl = sch.lookupTable(vp.table) ?: return emptyList()
            val admitted = st.scan(vp.table).filter { vp.filter?.admits(it) ?: true }
            val ordered = admitted.sortedWith(compareRows(tbl, vp.order))
            val taken = if (vp.limit != null) ordered.take(vp.limit) else ordered
            return taken.map { it to nodeOf(sch, vp, st, tbl, it) }
        }

        // A node over a row: its columns plus one field per relationship (the field wins a clash).
        private fun nodeOf(sch: Schema, vp: Plan, st: Store, tbl: Table, row: Row): Value {
            if (vp.related.isEmpty()) return row
            val pk = parentKey(tbl, row)
            val fields = LinkedHashMap<String, Value>(row.fields)
            for (r in vp.related) {
                val pin = Pred.Cmp(r.relation.column, CmpOp.Eq, pk)
                val f = r.plan.filter
                val child = r.plan.copy(filter = if (f == null) pin else Pred.All(listOf(pin, f)))
                fields[r.name] = Value.VList(pull(sch, child, st).map { it.second })
            }
            return Value.VStruct(fields)
        }

        private fun parentKey(tbl: Table, row: Row): Value {
            val ks = tbl.keyOf(row)
            return if (ks.size == 1) ks[0] else Value.VList(ks)
        }
    }

    /** A rebase rolled the store back and reported nothing: hydrate again. */
    public fun rebuild(sch: Schema, st: Store): View = hydrate(sch, plan, st)

    /** §13.7 The contract: indistinguishable from a view hydrated now. */
    public fun contract(sch: Schema, st: Store): Boolean = this == hydrate(sch, plan, st)

    /**
     * §13.5 A change arrives; the store is already at the state after it. A
     * change in the plan's own table moves the list; one in a table read
     * beneath it updates the parent nodes; both when the table is both.
     */
    public fun push(sch: Schema, st: Store, ch: Change): Pair<View, List<Patch>> {
        val tbl = sch.lookupTable(plan.table) ?: return this to emptyList()
        val (v1, ps1) = if (ch.table == plan.table) pushTop(sch, st, tbl, ch) else this to emptyList()
        val (v2, ps2) = v1.pushBelow(sch, st, tbl, ch)
        return v2 to (ps1 + ps2)
    }

    private fun with(ns: List<Pair<Row, Value>>): View = View(plan, ns)

    private fun pushTop(sch: Schema, st: Store, tbl: Table, ch: Change): Pair<View, List<Patch>> {
        val keep: (Row) -> Boolean = { plan.filter?.admits(it) ?: true }
        val order = compareRows(tbl, plan.order)
        fun key(r: Row) = tbl.keyOf(r)
        fun position(row: Row): Int? = nodes.indexOfFirst { key(it.first) == key(row) }.takeIf { it >= 0 }
        fun insertPos(row: Row, ns: List<Pair<Row, Value>>): Int = ns.takeWhile { order.compare(it.first, row) < 0 }.size
        fun build(row: Row): Pair<Row, Value> = row to nodeOf(sch, plan, st, tbl, row)
        val full = plan.limit == nodes.size
        fun insertAt(i: Int, x: Pair<Row, Value>, xs: List<Pair<Row, Value>>) = xs.take(i) + x + xs.drop(i)
        fun deleteAt(i: Int, xs: List<Pair<Row, Value>>) = xs.take(i) + xs.drop(i + 1)

        // The pull: the first admitted row beyond the bound (the last row held).
        fun refill(ns: List<Pair<Row, Value>>): Pair<Row, Value>? {
            val bound = ns.lastOrNull()?.first
            val candidates = st.scan(plan.table).filter { r -> keep(r) && (bound == null || order.compare(bound, r) < 0) }
            val first = candidates.sortedWith(order).firstOrNull() ?: return null
            return build(first)
        }

        fun add(row: Row): Pair<View, List<Patch>> {
            if (!keep(row)) return this to emptyList()
            val j = insertPos(row, nodes)
            val lim = plan.limit
            if (lim != null && j >= lim) return this to emptyList()
            val n = build(row)
            val ns = insertAt(j, n, nodes)
            return if (lim != null && ns.size > lim) {
                with(ns.take(lim)) to listOf(Patch.Insert(j, n.second), Patch.Remove(lim))
            } else {
                with(ns) to listOf(Patch.Insert(j, n.second))
            }
        }

        fun removeAt(i: Int): Pair<View, List<Patch>> {
            val ns = deleteAt(i, nodes)
            val n = if (full) refill(ns) else null
            return if (n != null) {
                with(ns + n) to listOf(Patch.Remove(i), Patch.Insert(ns.size, n.second))
            } else {
                with(ns) to listOf(Patch.Remove(i))
            }
        }

        fun edit(i: Int, new: Row): Pair<View, List<Patch>> {
            val ns = deleteAt(i, nodes)
            val j = insertPos(new, ns)
            val n = build(new)
            val hidden = if (full && j == ns.size) refill(ns) else null
            return when {
                hidden != null && key(hidden.first) != key(new) ->
                    with(ns + hidden) to listOf(Patch.Remove(i), Patch.Insert(j, hidden.second))
                j == i -> with(insertAt(j, n, ns)) to listOf(Patch.Update(i, n.second))
                else -> with(insertAt(j, n, ns)) to listOf(Patch.Remove(i), Patch.Insert(j, n.second))
            }
        }

        return when (ch) {
            is Change.Add -> add(ch.row)
            is Change.Remove -> position(ch.row)?.let { removeAt(it) } ?: (this to emptyList())
            is Change.Edit -> {
                val i = position(ch.old)
                val k = keep(ch.new)
                when {
                    i == null && !k -> this to emptyList()
                    i == null -> add(ch.new)
                    !k -> removeAt(i)
                    else -> edit(i, ch.new)
                }
            }
        }
    }

    // §13.6 A change beneath the plan: an update of every parent node it could have moved.
    private fun pushBelow(sch: Schema, st: Store, tbl: Table, ch: Change): Pair<View, List<Patch>> {
        val t = ch.table
        val direct = plan.related.filter { it.relation.child == t }.map { it.relation.column }
        fun descendants(c: Plan): List<String> = c.related.map { it.plan.table } + c.related.flatMap { descendants(it.plan) }
        val deeper = t in plan.related.flatMap { descendants(it.plan) }
        if (direct.isEmpty() && !deeper) return this to emptyList()
        val joins = HashSet<Value>()
        for (col in direct) for (row in ch.rows) row.fields[col]?.let { joins.add(it) }
        val out = ArrayList<Pair<Row, Value>>(nodes.size)
        val patches = ArrayList<Patch>()
        for ((i, node) in nodes.withIndex()) {
            val (row, old) = node
            val affected = deeper || parentKey(tbl, row) in joins
            if (affected) {
                val new = nodeOf(sch, plan, st, tbl, row)
                if (new != old) {
                    out.add(row to new)
                    patches.add(Patch.Update(i, new))
                    continue
                }
            }
            out.add(node)
        }
        return with(out) to patches
    }
}
