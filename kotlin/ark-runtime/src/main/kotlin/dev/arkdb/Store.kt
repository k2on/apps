// §4 The store (Ark.Store).
//
// What a mutator writes through and a query reads from. Constraints —
// not-null, uniqueness, references — are enforced here, identically
// everywhere, as deterministic refusals; an index changes nothing.
package dev.arkdb

import java.util.SortedMap
import java.util.TreeMap

/** §4.1 What a write reports: which row it touched and how. */
public sealed class Change {
    public abstract val table: String

    public data class Add(override val table: String, val row: Row) : Change()
    public data class Remove(override val table: String, val row: Row) : Change()
    public data class Edit(override val table: String, val old: Row, val new: Row) : Change()

    /** The rows a change is about: both versions of an edit. */
    public val rows: List<Row>
        get() = when (this) {
            is Add -> listOf(row)
            is Remove -> listOf(row)
            is Edit -> listOf(old, new)
        }
}

/** What applying an entry changed, in order. */
public typealias Facts = List<Change>

/**
 * The interface a procedure writes through (Ark.Store, Ark.Eval §6.6).
 * `select` is generic over `scan` and `schema`, so any implementation gets
 * exactly `Ark.Eval.select`'s nodes.
 */
public interface Store {
    public val schema: Schema

    /** The row, or `VNull` if there is none. */
    public fun get(table: String, key: List<Value>): Value

    public fun exists(table: String, key: List<Value>): Value

    /** The plan's nodes, exactly as `Ark.Eval.select` builds them. */
    public fun select(plan: Plan): Value = Value.VList(Select.run(this, plan).map { it.second })

    /** Write a row; faults with the store's refusal. */
    public fun put(table: String, row: Value)

    /** Delete by key; a missing row is a no-op, a referenced row a refusal. */
    public fun delete(table: String, key: List<Value>)

    /** Every row of a table, in key order under `compareValue`. */
    public fun scan(table: String): List<Row>

    /** §4.5 Apply a change as a fact: no constraint is checked. */
    public fun applyChange(change: Change)
}

/**
 * The store as a map of maps, as the spec has it. Mutable, so a caller that
 * needs a value (the peer's confirmed store, a transaction that may roll
 * back) works on a `fork` and keeps the original. Every write through
 * `put`/`delete` is recorded in `changes`, in order.
 */
public class MemoryStore(override val schema: Schema) : Store {
    private val tables: HashMap<String, TreeMap<List<Value>, Row>> = HashMap()

    /** What `put` and `delete` reported since this store was made or forked. */
    public val changes: MutableList<Change> = ArrayList()

    /** A copy with the same rows and no changes recorded. */
    public fun fork(): MemoryStore {
        val s = MemoryStore(schema)
        for ((t, rows) in tables) {
            val m = TreeMap<List<Value>, Row>(KeyOrder)
            m.putAll(rows)
            s.tables[t] = m
        }
        return s
    }

    public fun rows(table: String): SortedMap<List<Value>, Row> = tables[table] ?: TreeMap(KeyOrder)

    private fun rowsMut(table: String): TreeMap<List<Value>, Row> = tables.getOrPut(table) { TreeMap(KeyOrder) }

    /** Every table of the schema, in schema order. */
    public val tableNames: List<String> get() = schema.tables.map { it.name }

    public val isEmpty: Boolean get() = tables.values.all { it.isEmpty() }

    public fun getRow(table: String, key: List<Value>): Row? = tables[table]?.get(key)

    override fun get(table: String, key: List<Value>): Value = getRow(table, key) ?: Value.VNull

    override fun exists(table: String, key: List<Value>): Value = Value.VBool(getRow(table, key) != null)

    override fun scan(table: String): List<Row> = rows(table).values.toList()

    override fun put(table: String, row: Value) {
        val r = row as? Row ?: throw Fault.bug("put: expected Struct, got ${row.show().take(60)}")
        write(table, r)?.let { changes.add(it) }
    }

    override fun delete(table: String, key: List<Value>) {
        remove(table, key)?.let { changes.add(it) }
    }

    /**
     * §4.3 Write a row. Nullable columns the row leaves out are filled with
     * `VNull` first; then the row must be exactly the table's columns with
     * values of their types, no non-nullable column may be null, every unique
     * index must stay unique against every other row, every reference must
     * find its parent. A new key is an `Add`; an identical row is nothing at
     * all; anything else an `Edit`. Throws `Fault.Refuse`.
     */
    public fun write(table: String, row0: Row): Change? {
        val tbl = schema.lookupTable(table) ?: throw Fault.Refuse(Refusal.NoSuchTable(table))
        val row = complete(tbl, row0)
        wellTyped(tbl, row)
        val k = tbl.keyOf(row)
        val here = rowsMut(table)
        val old = here[k]
        for (ix in tbl.indexes) if (ix.unique) unique(tbl, here, k, row, ix)
        for (r in tbl.refs) parentExists(tbl, row, r)
        return when {
            old == null -> {
                here[k] = row
                Change.Add(table, row)
            }
            old == row -> null
            else -> {
                here[k] = row
                Change.Edit(table, old, row)
            }
        }
    }

    /** §4.4 Delete by key; throws `Fault.Refuse` when a row still references it. */
    public fun remove(table: String, key: List<Value>): Change? {
        val tbl = schema.lookupTable(table) ?: throw Fault.Refuse(Refusal.NoSuchTable(table))
        val here = tables[table] ?: return null
        val row = here[key] ?: return null
        for (rel in schema.childrenOf(table)) noChild(tbl, key, rel)
        here.remove(key)
        return Change.Remove(table, row)
    }

    override fun applyChange(change: Change) {
        when (change) {
            is Change.Add -> insertRaw(change.table, change.row)
            is Change.Edit -> insertRaw(change.table, change.new)
            is Change.Remove -> {
                val tbl = schema.lookupTable(change.table) ?: return
                tables[change.table]?.remove(tbl.keyOf(change.row))
            }
        }
    }

    private fun insertRaw(table: String, row: Row) {
        val tbl = schema.lookupTable(table) ?: return
        rowsMut(table)[tbl.keyOf(row)] = row
    }

    /** The store after the changes, as a new store; this one is untouched. */
    public fun applyChanges(changes: List<Change>): MemoryStore {
        val s = fork()
        for (ch in changes) s.applyChange(ch)
        return s
    }

    /** The store as the vectors write it: every table of the schema, its rows in key order. */
    public fun toValue(): Value = Value.VStruct(tableNames.associateWith { t -> Value.VList(scan(t)) })

    override fun equals(other: Any?): Boolean {
        if (other !is MemoryStore || other.schema != schema) return false
        for (t in tableNames) if (rows(t) != other.rows(t)) return false
        return true
    }

    override fun hashCode(): Int = tableNames.fold(schema.hashCode()) { h, t -> h * 31 + rows(t).hashCode() }

    override fun toString(): String = toValue().show()

    // Constraints ---------------------------------------------------------

    /** Fill in every nullable column the row left out, as `VNull`. */
    private fun complete(tbl: Table, row: Row): Row {
        var missing = false
        for (c in tbl.columns) if (c.nullable && !row.has(c.name)) missing = true
        if (!missing) return row
        val m = LinkedHashMap<String, Value>(row.fields)
        for (c in tbl.columns) if (c.nullable && !m.containsKey(c.name)) m[c.name] = Value.VNull
        return Value.VStruct(m)
    }

    private fun wellTyped(tbl: Table, row: Row) {
        val want = tbl.columns.map { it.name }
        val have = row.fields.keys.toList()
        if (have.toSet() != want.toSet()) {
            throw Fault.Refuse(
                Refusal.MalformedRow(tbl.name, "columns ${HsShow.texts(have)} are not ${HsShow.texts(want)}"),
            )
        }
        for (c in tbl.columns) {
            val v = row.fields[c.name] ?: throw Fault.Refuse(Refusal.MalformedRow(tbl.name, c.name))
            if (v is Value.VNull) {
                if (!c.nullable) throw Fault.Refuse(Refusal.NotNull(tbl.name, c.name))
            } else if (!ofType(c.ty, v)) {
                throw Fault.Refuse(Refusal.MalformedRow(tbl.name, c.name + " has the wrong type"))
            }
        }
    }

    private fun unique(tbl: Table, here: TreeMap<List<Value>, Row>, k: List<Value>, row: Row, ix: Index) {
        val mine = ix.columns.map { row.fields[it] }
        // A NULL is not equal to anything, itself included, so two rows both
        // NULL in a unique column do not clash.
        if (mine.any { it == null || it is Value.VNull }) return
        for ((ok, other) in here) {
            if (ok == k) continue
            if (ix.columns.map { other.fields[it] } == mine) {
                throw Fault.Refuse(Refusal.UniqueViolation(tbl.name, ix.columns))
            }
        }
    }

    private fun parentExists(tbl: Table, row: Row, r: Ref) {
        val v = row.fields[r.column] ?: throw Fault.Refuse(Refusal.MalformedRow(tbl.name, r.column))
        if (v is Value.VNull) return
        if (getRow(r.table, listOf(v)) == null) {
            throw Fault.Refuse(Refusal.MissingParent(tbl.name, r.column, r.table))
        }
    }

    private fun noChild(tbl: Table, k: List<Value>, rel: Relation) {
        if (k.size != 1) return
        val kv = k[0]
        if (scan(rel.child).any { it.fields[rel.column] == kv }) {
            throw Fault.Refuse(Refusal.StillReferenced(tbl.name, rel.child))
        }
    }

    public companion object {
        /** Whether a value inhabits a scalar type; any sixteen bytes inhabit any `TId`. */
        public fun ofType(t: Ty, v: Value): Boolean = when (t) {
            is Ty.TBool -> v is Value.VBool
            is Ty.TInt -> v is Value.VInt
            is Ty.TText -> v is Value.VText
            is Ty.TBytes -> v is Value.VBytes
            is Ty.TId -> v is Value.VId
            is Ty.TEnum -> v is Value.VText && v.value in t.variants
            is Ty.TOption -> v is Value.VNull || ofType(t.of, v)
            else -> false
        }

        /** A store holding the rows a vector or a snapshot lists, per table. */
        public fun of(schema: Schema, rows: Map<String, List<Value>>): MemoryStore {
            val s = MemoryStore(schema)
            for ((t, vs) in rows) for (v in vs) if (v is Row) s.applyChange(Change.Add(t, v))
            return s
        }
    }
}

/** §6.6 `select` over an evaluated plan: scan, filter, sort, take, attach. */
public object Select {
    /** The order a plan sorts by: each column under `compareValue`, flipped where descending. */
    public fun orderBy(cols: List<Pair<String, Dir>>): Comparator<Row> = Comparator { a, b ->
        for ((c, d) in cols) {
            val o = compareValue(a[c], b[c])
            if (o != 0) return@Comparator if (d == Dir.Desc) -o else o
        }
        0
    }

    /** Each admitted row with its node, in the plan's order, cut to the limit. */
    public fun run(store: Store, plan: Plan): List<Pair<Row, Value>> {
        val tbl = store.schema.lookupTable(plan.table) ?: throw Fault.bug("UnknownTable ${HsShow.text(plan.table)}")
        val keep = plan.filter
        val admitted = store.scan(plan.table).filter { keep == null || keep.admits(it) }
        val ordered = admitted.sortedWith(orderBy(plan.order))
        val taken = if (plan.limit != null) ordered.take(plan.limit) else ordered
        return taken.map { it to attach(store, tbl, plan.related, it) }
    }

    /** A node: the row plus one field per relationship holding the child nodes; the field wins a name clash. */
    public fun attach(store: Store, tbl: Table, rels: List<Related>, row: Row): Value {
        if (rels.isEmpty()) return row
        val key = tbl.keyOf(row)
        val pk = if (key.size == 1) key[0] else throw Fault.bug("CompositeParentKey ${HsShow.text(tbl.name)}")
        val fields = LinkedHashMap<String, Value>(row.fields)
        for (r in rels) {
            val pin = Pred.Cmp(r.relation.column, CmpOp.Eq, pk)
            val f = r.plan.filter
            val child = r.plan.copy(filter = if (f == null) pin else Pred.All(listOf(pin, f)))
            fields[r.name] = Value.VList(run(store, child).map { it.second })
        }
        return Value.VStruct(fields)
    }
}
