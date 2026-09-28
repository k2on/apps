// The plan a runtime executes and a view maintains: Ark.IR.Plan with every
// right-hand side already evaluated (Ark.View.ViewPlan). A native query
// builds one through these builders and hands it to `Store.select`.
package dev.arkdb

/** A filter with its right-hand sides evaluated (Ark.View.Filter). */
public sealed class Pred {
    public data class Cmp(val column: String, val op: CmpOp, val value: Value) : Pred()
    public data class In(val column: String, val values: List<Value>) : Pred()
    public data class All(val items: List<Pred>) : Pred()
    public data class Any(val items: List<Pred>) : Pred()
    public data class Not(val p: Pred) : Pred()

    /** Whether a row passes; a column the row lacks reads as `VNull`. */
    public fun admits(row: Row): Boolean = when (this) {
        is Cmp -> op.holds(compareValue(row[column], value))
        is In -> values.any { compareValue(row[column], it) == 0 }
        is All -> items.all { it.admits(row) }
        is Any -> items.any { it.admits(row) }
        is Not -> !p.admits(row)
    }

    public companion object {
        public fun cmp(column: String, op: CmpOp, value: Value): Pred = Cmp(column, op, value)

        public fun inList(column: String, values: List<Value>): Pred = In(column, values.toList())

        public fun all(items: List<Pred>): Pred = All(items.toList())

        public fun any(items: List<Pred>): Pred = Any(items.toList())

        public fun not(p: Pred): Pred = Not(p)
    }
}

/** A relationship read beneath each row of a plan, as the field `name`. */
public data class Related(val name: String, val relation: Relation, val plan: Plan)

public data class Plan(
    val table: String,
    val filter: Pred?,
    /** Order columns; a view appends the key columns ascending to make it total. */
    val order: List<Pair<String, Dir>>,
    val limit: Int?,
    val related: List<Related>,
) {
    /** Keep the rows the predicate admits. A second call narrows further (both must hold). */
    public fun filter(p: Pred): Plan = copy(filter = if (filter == null) p else Pred.All(listOf(filter, p)))

    public fun orderBy(column: String, dir: Dir): Plan = copy(order = order + (column to dir))

    public fun limit(n: Int): Plan = copy(limit = n)

    public fun limit(n: Long): Plan = copy(limit = Math.toIntExact(n))

    public fun related(name: String, parent: String, child: String, column: String, plan: Plan): Plan =
        copy(related = related + Related(name, Relation(parent, child, column), plan))

    public companion object {
        public fun from(table: String): Plan = Plan(table, null, emptyList(), null, emptyList())
    }
}
