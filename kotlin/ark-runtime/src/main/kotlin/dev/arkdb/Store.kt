// §4 The store, as `rust/ark/src/store.rs` defines it: an ordered
// key-value backend under get, scan, put, delete and the commit a batch of
// them makes. Not-null, uniqueness and references are enforced here,
// identically everywhere, as deterministic refusals; an index changes
// nothing but what a read costs.
//
// A row is a `Value.VStruct` of every column (§4). The Rust lays a row out
// positionally behind its table's names (`docs/plan-perf.md` R11), which is
// representation: "a row laid out as the table's" is, here, a struct whose
// fields are exactly the table's columns (`Table.holds`), and a row of no
// table is any other struct.
package dev.arkdb

import java.util.NavigableMap
import java.util.TreeMap
import java.util.TreeSet

/** §4.1 What a write reports (`store::Change`). */
public sealed class Change {
    public abstract val table: String

    public data class Add(override val table: String, val row: Row) : Change()

    public data class Remove(override val table: String, val row: Row) : Change()

    /** Old, new. */
    public data class Edit(override val table: String, val old: Row, val new: Row) : Change()
}

/**
 * §4.2 A refusal: a verdict about the write, reached identically by every replica
 * (`store::Refusal`).
 */
public sealed class Refusal {
    public data class NoSuchTable(val table: String) : Refusal()

    public data class MalformedRow(val table: String, val why: String) : Refusal()

    public data class NotNull(val table: String, val column: String) : Refusal()

    public data class UniqueViolation(val table: String, val columns: List<String>) : Refusal()

    public data class MissingParent(val table: String, val column: String, val parent: String) :
        Refusal()

    public data class StillReferenced(val table: String, val child: String) : Refusal()

    /** An explicit `refuse`, or a checked arithmetic fault. */
    public data class Refused(val why: String) : Refusal()

    /**
     * Not this login's to do, naming the table it would have written (`docs/plan-guards.md` D4).
     */
    public data class Forbidden(val table: String) : Refusal()

    /** `impl Display for Refusal`: the text a server puts in a `reject` frame. */
    public fun show(): String =
        when (this) {
            is NoSuchTable -> "NoSuchTable ${debugString(table)}"
            is MalformedRow -> "MalformedRow ${debugString(table)} ${debugString(why)}"
            is NotNull -> "NotNull ${debugString(table)} ${debugString(column)}"
            is UniqueViolation -> "UniqueViolation ${debugString(table)} ${debugStrings(columns)}"
            is MissingParent ->
                "MissingParent ${debugString(table)} ${debugString(column)} ${debugString(parent)}"
            is StillReferenced -> "StillReferenced ${debugString(table)} ${debugString(child)}"
            is Refused -> "Refused ${debugString(why)}"
            is Forbidden -> "Forbidden ${debugString(table)}"
        }
}

/** A bound of a span, under the total order (`std::ops::Bound`). */
public sealed class Bound {
    public object Unbounded : Bound()

    public data class Included(val v: Value) : Bound()

    public data class Excluded(val v: Value) : Bound()
}

/**
 * One column a read's filter holds between two bounds (`store::Span`, `docs/plan-perf.md` R6). A
 * hint, as the equalities are: `keep` still decides.
 */
public data class Span(val column: String, val lo: Bound, val hi: Bound)

/** A row filter, as the store is handed one. */
public typealias Keep = (Row) -> Boolean

/**
 * The store's interface (`store::Store`): a backend implements the primitives, and the constraints
 * and the reads are given. `scan` returns rows in key order under the total order, and a table with
 * no rows is empty rather than absent.
 */
public interface Store {
    public val schema: Schema

    /** The row under a key, if any. */
    public fun get(table: String, key: List<Value>): Row?

    /** Every row of a table, in key order. */
    public fun scan(table: String): List<Row>

    /** The rows `keep` admits, in key order. */
    public fun scanWhere(table: String, keep: Keep): List<Row> = scan(table).filter(keep)

    /** `scan_where_eq`: told which columns `keep` holds equal and which between bounds (hints). */
    public fun scanWhereEq(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        keep: Keep
    ): List<Row> = scanWhere(table, keep)

    /**
     * `scan_ordered` (R1): the first `limit` rows `keep` admits in `order` — each column under its
     * direction, then the key ascending — read from an index that holds them so; `null` when none
     * does.
     */
    public fun scanOrdered(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        order: List<Pair<String, Dir>>,
        keep: Keep,
        limit: Int,
    ): List<Row>? = null

    /** §4.5 Apply a change as a fact: raw, unjudged. An unknown table is ignored. */
    public fun applyChange(change: Change)

    public fun applyChanges(changes: List<Change>) {
        for (c in changes) applyChange(c)
    }

    /** §4.3 `put`: completed, judged, and written. A new key is an `Add`, an equal row nothing. */
    public fun put(table: String, row: Row): Written {
        val w = Rules.judgePut(this, table, row)
        if (w is Written.Ok) w.change?.let { applyChange(it) }
        return w
    }

    /** §4.4 `delete` by key: a missing row is nothing; a referenced one a refusal. */
    public fun delete(table: String, key: List<Value>): Written {
        val w = Rules.judgeDelete(this, table, key)
        if (w is Written.Ok) w.change?.let { applyChange(it) }
        return w
    }

    public fun exists(table: String, key: List<Value>): Boolean = get(table, key) != null

    /** The row under a key as a value, or `Null`. */
    public fun getValue(table: String, key: List<Value>): Value = get(table, key) ?: Value.VNull

    /** Every table's rows, as the vectors write a store: table name to the list of rows. */
    public fun storeValue(): Value =
        Value.VStruct(schema.tables.associate { t -> t.name to Value.VList(scan(t.name)) })

    /**
     * `docs/plan-db.md` D4 `scan_where_text`: told also which text columns the filter holds to
     * contain which (folded) needles, as a disjunction of conjunctions. A hint; `keep` still
     * decides.
     */
    public fun scanWhereText(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        has: List<List<Pair<String, String>>>,
        keep: Keep,
    ): List<Row> = scanWhereEq(table, eq, spans, keep)

    /** §8.1 The digest this store keeps for a table, if it keeps one. */
    public fun digest(table: String): Digest? = null
}

/** What a write did: a change (or none), or the refusal that stopped it. */
public sealed class Written {
    public data class Ok(val change: Change?) : Written()

    public data class Refused(val refusal: Refusal) : Written()
}

private class Refuse(val refusal: Refusal) : Exception(null, null, false, false)

/** The rules every backend judges a write by (`store.rs`, "The rules"). */
public object Rules {
    /**
     * `complete`: every nullable column the row left out, as `Null`. A row that names a column the
     * table lacks or leaves out one it requires stays as it is otherwise, for `judgePut` to refuse.
     */
    public fun complete(tbl: Table, row: Row): Row {
        if (tbl.holds(row)) return row
        val missing = tbl.columns.filter { it.nullable && !row.fields.containsKey(it.name) }
        if (missing.isEmpty()) return row
        val m = TreeMap<String, Value>(CodePointOrder)
        m.putAll(row.fields)
        for (c in missing) m[c.name] = Value.VNull
        return Value.VStruct(m)
    }

    /**
     * `row_for`: a struct written to a table, as the table's row — laid out, its omitted nullable
     * columns `Null`, when it names only columns and leaves out only nullable ones; as it is
     * otherwise.
     */
    public fun rowFor(st: Store, table: String, m: Row): Row {
        val tbl = st.schema.lookupTable(table) ?: return m
        val fits =
            m.fields.keys.all { tbl.column(it) != null } &&
                tbl.columns.all { it.nullable || m.fields.containsKey(it.name) }
        return if (fits) complete(tbl, m) else m
    }

    /**
     * `project_row` (§12 behind): a fact's row laid out as `tbl`'s — a column the table lacks
     * dropped, a nullable column the row lacks `Null` — or a refusal for a required column it
     * lacks.
     */
    public fun projectRow(tbl: Table, row: Row): Pair<Row?, Refusal?> {
        if (tbl.holds(row)) return row to null
        val out = LinkedHashMap<String, Value>()
        for (c in tbl.columns) {
            val v = row.fields[c.name]
            when {
                v != null -> out[c.name] = v
                c.nullable -> out[c.name] = Value.VNull
                else -> return null to Refusal.MalformedRow(tbl.name, "${c.name} is missing")
            }
        }
        return Value.VStruct(out) to null
    }

    /** `judge_put`: what `put` would report, without writing. */
    public fun judgePut(st: Store, tn: String, row0: Row): Written =
        try {
            val tbl = st.schema.lookupTable(tn) ?: throw Refuse(Refusal.NoSuchTable(tn))
            val row = complete(tbl, row0)
            wellTyped(tbl, row)
            val k = tbl.keyOf(row)
            for (ix in tbl.indexes) if (ix.unique) unique(st, tbl, k, row, ix)
            for (r in tbl.refs) parentExists(st, tbl, row, r)
            val old = st.get(tn, k)
            Written.Ok(
                when {
                    old == null -> Change.Add(tn, row)
                    old == row -> null
                    else -> Change.Edit(tn, old, row)
                })
        } catch (r: Refuse) {
            Written.Refused(r.refusal)
        }

    /** `matching`: the row a write's column list matches (the key when empty), if any. */
    public fun matching(st: Store, tbl: Table, row: Row, on: List<String>): Row? {
        if (on.isEmpty()) return st.get(tbl.name, tbl.keyOf(row))
        val want = on.map { it to (row.fields[it] ?: Value.VNull) }
        if (want.any { it.second is Value.VNull }) return null
        return st.scanWhereEq(tbl.name, want, emptyList()) { r ->
                want.all { (c, v) -> r.fields[c] == v }
            }
            .firstOrNull()
    }

    /** §1.4 `insert`: the row, unless one matches on the columns. */
    public fun insert(st: Store, tn: String, row0: Row, on: List<String>): Written {
        val tbl = st.schema.lookupTable(tn) ?: return Written.Refused(Refusal.NoSuchTable(tn))
        val row = complete(tbl, row0)
        if (matching(st, tbl, row, on) != null) return Written.Ok(null)
        return st.put(tn, row)
    }

    /** §1.4 `upsert`: the row; if one matches on the columns, its key columns kept. */
    public fun upsert(st: Store, tn: String, row0: Row, on: List<String>): Written {
        val tbl = st.schema.lookupTable(tn) ?: return Written.Refused(Refusal.NoSuchTable(tn))
        var row = complete(tbl, row0)
        val old = matching(st, tbl, row, on)
        if (old != null) {
            val m = TreeMap<String, Value>(CodePointOrder)
            m.putAll(row.fields)
            for (k in tbl.key) {
                val v = old.fields[k]
                if (v != null && m.containsKey(k)) m[k] = v
            }
            row = Value.VStruct(m)
        }
        return st.put(tn, row)
    }

    /** §1.4 `update`, once the new row is computed: it must keep the key. */
    public fun update(st: Store, tn: String, key: List<Value>, row0: Row): Written {
        val tbl = st.schema.lookupTable(tn) ?: return Written.Refused(Refusal.NoSuchTable(tn))
        val row = complete(tbl, row0)
        if (tbl.keyOf(row) != key)
            return Written.Refused(Refusal.MalformedRow(tn, "update changed the key"))
        return st.put(tn, row)
    }

    /** `judge_delete`: what `delete` would report, without writing. */
    public fun judgeDelete(st: Store, tn: String, k: List<Value>): Written {
        val tbl = st.schema.lookupTable(tn) ?: return Written.Refused(Refusal.NoSuchTable(tn))
        val row = st.get(tn, k) ?: return Written.Ok(null)
        for (rel in st.schema.childrenOf(tn)) {
            if (k.size == 1) {
                val kv = k[0]
                val held =
                    st.scanWhereEq(rel.child, listOf(rel.column to kv), emptyList()) { r ->
                        r.fields[rel.column] == kv
                    }
                if (held.isNotEmpty())
                    return Written.Refused(Refusal.StillReferenced(tbl.name, rel.child))
            }
        }
        return Written.Ok(Change.Remove(tn, row))
    }

    // A row is exactly the table's columns, each of the column's type.
    private fun wellTyped(tbl: Table, row: Row) {
        if (!tbl.holds(row)) {
            val have = row.fields.keys.sortedWith(CodePointOrder)
            val want = tbl.columns.map { it.name }
            throw Refuse(
                Refusal.MalformedRow(
                    tbl.name, "columns ${debugStrings(have)} are not ${debugStrings(want)}"))
        }
        for (c in tbl.columns) {
            val v = row.fields[c.name]!!
            if (v is Value.VNull) {
                if (!c.nullable) throw Refuse(Refusal.NotNull(tbl.name, c.name))
            } else if (!ofType(c.ty, v)) {
                throw Refuse(Refusal.MalformedRow(tbl.name, "${c.name} has the wrong type"))
            }
        }
    }

    /** `of_type`: whether a value inhabits a scalar type. Ids are untyped at run time. */
    public fun ofType(t: Ty, v: Value): Boolean =
        when {
            t is Ty.TBool -> v is Value.VBool
            t is Ty.TInt -> v is Value.VInt
            t is Ty.TText -> v is Value.VText
            t is Ty.TBytes -> v is Value.VBytes
            t is Ty.TId -> v is Value.VId
            t is Ty.TEnum -> v is Value.VText && v.value in t.variants
            t is Ty.TOption -> v is Value.VNull || ofType(t.of, v)
            else -> false
        }

    /** `compare_rows`: each column under its direction, then the key ascending. */
    public fun compareRows(tbl: Table, order: List<Pair<String, Dir>>, a: Row, b: Row): Int {
        for ((c, d) in order) {
            val o = compareValue(a.fields[c] ?: Value.VNull, b.fields[c] ?: Value.VNull)
            val od = if (d == Dir.Desc) -o else o
            if (od != 0) return od
        }
        for (k in tbl.key) {
            val o = compareValue(a.fields[k] ?: Value.VNull, b.fields[k] ?: Value.VNull)
            if (o != 0) return o
        }
        return 0
    }

    // No other row holds the index's columns equal to this row's.
    private fun unique(st: Store, tbl: Table, k: List<Value>, row: Row, ix: Index) {
        val mine = ix.columns.map { it to (row.fields[it] ?: Value.VNull) }
        // A NULL equals nothing, itself included.
        if (mine.any { it.second is Value.VNull }) return
        val clash =
            st.scanWhereEq(tbl.name, mine, emptyList()) { r ->
                tbl.keyOf(r) != k && mine.all { (c, v) -> r.fields[c] == v }
            }
        if (clash.isNotEmpty()) throw Refuse(Refusal.UniqueViolation(tbl.name, ix.columns))
    }

    private fun parentExists(st: Store, tbl: Table, row: Row, r: Ref) {
        when (val v = row.fields[r.column]) {
            null -> throw Refuse(Refusal.MalformedRow(tbl.name, r.column))
            is Value.VNull -> {}
            else ->
                if (!st.exists(r.table, listOf(v)))
                    throw Refuse(Refusal.MissingParent(tbl.name, r.column, r.table))
        }
    }
}

/**
 * `docs/plan-db.md` D4 A text, folded as `lower` folds it: each character to its simple lowercase
 * mapping under the pinned Unicode tables, one for one (`store::fold`).
 */
public fun fold(s: String): String {
    val sb = StringBuilder(s.length)
    var i = 0
    while (i < s.length) {
        val cp = s.codePointAt(i)
        sb.appendCodePoint(UnicodeTables.toLowerSimple(cp))
        i += Character.charCount(cp)
    }
    return sb.toString()
}

/** D4 The trigrams of a folded text: every run of three characters (not bytes), once each. */
public fun trigrams(folded: String): Set<String> {
    val cps = folded.codePoints().toArray()
    val out = TreeSet<String>(CodePointOrder)
    for (i in 0..cps.size - 3) out.add(String(cps, i, 3))
    return out
}

private fun keyTree(): TreeSet<List<Value>> = TreeSet(KeyOrder)

// One secondary index: the keys of the rows under each value of its columns.
// The Rust posts ordinals (D7.2); keys answer the same reads, in key order.
private class Secondary(val columns: List<String>) {
    val rows: TreeMap<List<Value>, TreeSet<List<Value>>> = TreeMap(KeyOrder)

    fun keyOf(row: Row): List<Value> = columns.map { row.fields[it] ?: Value.VNull }

    fun set(key: List<Value>, old: Row?, new: Row?) {
        val was = old?.let(::keyOf)
        val now = new?.let(::keyOf)
        if (was == now) return
        if (was != null) {
            val list = rows[was]
            if (list != null) {
                list.remove(key)
                if (list.isEmpty()) rows.remove(was)
            }
        }
        if (now != null) rows.getOrPut(now, ::keyTree).add(key)
    }

    // The buckets whose values begin with `prefix` and, given a span on the
    // next column, lie between its bounds. `prefix ++ [v, []]` is above every
    // bucket continuing `prefix ++ [v]` (a list outranks every scalar).
    fun under(prefix: List<Value>, span: Span?): NavigableMap<List<Value>, TreeSet<List<Value>>> {
        fun at(v: Value?, top: Boolean): List<Value> {
            val k = ArrayList(prefix)
            if (v != null) k.add(v)
            if (top) k.add(Value.VList(emptyList()))
            return k
        }
        val lo = span?.lo ?: Bound.Unbounded
        val hi = span?.hi ?: Bound.Unbounded
        val (l, lIncl) =
            when (lo) {
                is Bound.Unbounded -> at(null, false) to true
                is Bound.Included -> at(lo.v, false) to true
                is Bound.Excluded -> at(lo.v, true) to false
            }
        val h =
            when (hi) {
                is Bound.Unbounded -> at(null, true)
                is Bound.Included -> at(hi.v, true)
                is Bound.Excluded -> at(hi.v, false)
            }
        if (KeyOrder.compare(l, h) >= 0) return rows.subMap(l, true, l, false)
        return rows.subMap(l, lIncl, h, false)
    }

    // R1 Whether this index holds a read's rows in the read's order: how many
    // leading columns the read holds equal, and which way to walk it.
    fun serves(
        tbl: Table,
        eq: List<Pair<String, Value>>,
        order: List<Pair<String, Dir>>
    ): Pair<Int, Dir>? {
        fun isEq(c: String) = eq.any { it.first == c }
        val n = columns.takeWhile { isEq(it) }.size
        val lead = columns.subList(0, n)
        val rest = columns.subList(n, columns.size)
        if (!eq.all { (c, _) -> c in lead }) return null
        val ord = order.filter { !isEq(it.first) }.iterator()
        var dir: Dir? = null
        for (c in rest) {
            if (!ord.hasNext()) return null
            val (o, d) = ord.next()
            if (o != c || (dir != null && dir != d)) return null
            dir = d
        }
        val tail = tbl.key.filter { !isEq(it) && it !in rest }.iterator()
        while (ord.hasNext()) {
            val (o, d) = ord.next()
            if (d != Dir.Asc || !tail.hasNext() || tail.next() != o) return null
        }
        return n to (dir ?: Dir.Asc)
    }

    fun copy(): Secondary {
        val s = Secondary(columns)
        for ((k, v) in rows) s.rows[k] = TreeSet(v)
        return s
    }
}

// D4 A text index: for each trigram of one text column's folded value, the
// keys of the rows whose value holds it.
private class Trigrams(val column: String) {
    val postings: TreeMap<String, TreeSet<List<Value>>> = TreeMap(CodePointOrder)

    fun of(row: Row?): Set<String> {
        val v = row?.fields?.get(column)
        return if (v is Value.VText) trigrams(fold(v.value)) else emptySet()
    }

    fun set(key: List<Value>, old: Row?, new: Row?) {
        if (old?.fields?.get(column) == new?.fields?.get(column) && (old != null) == (new != null))
            return
        val was = of(old)
        val now = of(new)
        for (g in was) {
            if (g in now) continue
            val list = postings[g] ?: continue
            list.remove(key)
            if (list.isEmpty()) postings.remove(g)
        }
        for (g in now) if (g !in was) postings.getOrPut(g, ::keyTree).add(key)
    }

    fun copy(): Trigrams {
        val t = Trigrams(column)
        for ((k, v) in postings) t.postings[k] = TreeSet(v)
        return t
    }
}

// One table: its rows by key, its secondaries and text indexes, and its
// digest (§8.1), which moves with every row written.
private class TableState(
    val rows: TreeMap<List<Value>, Row>,
    val indexes: List<Secondary>,
    val texts: List<Trigrams>,
    val digest: Digest,
) {
    fun set(t: String, k: List<Value>, row: Row?) {
        val old: Row?
        if (row != null) {
            old = rows.put(k, row)
        } else {
            old = rows.remove(k) ?: return
        }
        if (old != row) {
            if (old != null) digest.sub(leaf(t, old))
            if (row != null) digest.add(leaf(t, row))
        }
        for (ix in texts) ix.set(k, old, row)
        for (ix in indexes) ix.set(k, old, row)
    }

    fun copy(): TableState =
        TableState(TreeMap(rows), indexes.map { it.copy() }, texts.map { it.copy() }, digest.copy())

    companion object {
        fun of(tbl: Table): TableState {
            val seen = HashSet<List<String>>()
            val lists = tbl.indexes.map { it.columns } + tbl.refs.map { listOf(it.column) }
            val indexes =
                lists
                    .filter { it.isNotEmpty() && it != tbl.key && seen.add(it) }
                    .map { Secondary(it) }
            return TableState(TreeMap(KeyOrder), indexes, tbl.text.map { Trigrams(it) }, Digest())
        }
    }
}

// The values `eq` holds `columns` equal to, in the columns' order.
private fun held(columns: List<String>, eq: List<Pair<String, Value>>): List<Value> =
    columns.map { c -> eq.firstOrNull { it.first == c }?.second ?: Value.VNull }

// The span on the column after an index's first `n`, if any.
private fun bounding(columns: List<String>, n: Int, spans: List<Span>): Span? {
    val c = columns.getOrNull(n) ?: return null
    return spans.firstOrNull { it.column == c }
}

// The key `eq` names, when it holds every key column equal.
private fun keyHeld(tbl: Table, eq: List<Pair<String, Value>>): List<Value>? {
    val all = tbl.key.isNotEmpty() && tbl.key.all { c -> eq.any { it.first == c } }
    return if (all) held(tbl.key, eq) else null
}

/**
 * The spec's store (`store::MemoryStore`): the rows of every table by key, an index per declared
 * index and per reference column, a trigram index per text index, and each table's digest. The
 * indexes are derived and say nothing the rows do not: two stores are equal when their rows are.
 */
public class MemoryStore
private constructor(
    override val schema: Schema,
    private val tables: MutableMap<String, TableState>
) : Store {
    /** A second store with the same rows, written independently. */
    public fun copy(): MemoryStore =
        MemoryStore(schema, tables.mapValuesTo(LinkedHashMap()) { it.value.copy() })

    override fun equals(other: Any?): Boolean =
        other is MemoryStore &&
            schema == other.schema &&
            tables.keys == other.tables.keys &&
            tables.all { (t, st) -> st.rows == other.tables[t]!!.rows }

    override fun hashCode(): Int = tables.values.sumOf { it.rows.hashCode() }

    override fun toString(): String = "MemoryStore(" + storeValue().show() + ")"

    /** The rows of a table, by key. */
    public fun rows(table: String): Map<List<Value>, Row> = tables[table]?.rows ?: emptyMap()

    /** Whether any table holds a row. */
    public val isEmpty: Boolean
        get() = tables.values.all { it.rows.isEmpty() }

    override fun get(table: String, key: List<Value>): Row? = tables[table]?.rows?.get(key)

    override fun exists(table: String, key: List<Value>): Boolean =
        tables[table]?.rows?.containsKey(key) == true

    override fun scan(table: String): List<Row> =
        tables[table]?.rows?.values?.toList() ?: emptyList()

    override fun scanWhere(table: String, keep: Keep): List<Row> =
        tables[table]?.rows?.values?.filter(keep) ?: emptyList()

    // The index to read `eq` through and the values to look up in it.
    private fun lookup(
        st: TableState,
        eq: List<Pair<String, Value>>,
        spans: List<Span>
    ): Triple<Secondary, List<Value>, Span?>? {
        fun isHeld(c: String) = eq.any { it.first == c }
        var best: Triple<Secondary, List<Value>, Span?>? = null
        var bestSize = Int.MAX_VALUE
        for (ix in st.indexes) {
            if (!ix.columns.all(::isHeld)) continue
            val vals = held(ix.columns, eq)
            val size = ix.rows[vals]?.size ?: 0
            if (size < bestSize) {
                best = Triple(ix, vals, null)
                bestSize = size
            }
        }
        if (best != null) return best
        var most = -1
        for (ix in st.indexes) {
            val n = ix.columns.takeWhile(::isHeld).size
            val span = bounding(ix.columns, n, spans)
            if (n == 0 && span == null) continue
            val score = n + if (span != null) 1 else 0
            if (score > most) {
                most = score
                best = Triple(ix, held(ix.columns.subList(0, n), eq), span)
            }
        }
        return best
    }

    private fun byKey(tbl: Table?, rows: MutableList<Row>): List<Row> {
        if (tbl != null && rows.size > 1)
            rows.sortWith { a, b -> Rules.compareRows(tbl, emptyList(), a, b) }
        return rows
    }

    override fun scanWhereEq(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        keep: Keep
    ): List<Row> {
        val tbl = schema.lookupTable(table)
        val st = tables[table] ?: return emptyList()
        if (tbl != null) {
            val k = keyHeld(tbl, eq)
            if (k != null) return listOfNotNull(st.rows[k]?.takeIf(keep))
        }
        val (ix, vals, span) = lookup(st, eq, spans) ?: return scanWhere(table, keep)
        val out = ArrayList<Row>()
        if (vals.size == ix.columns.size) {
            val keys = ix.rows[vals] ?: return emptyList()
            for (k in keys) st.rows[k]!!.let { if (keep(it)) out.add(it) }
        } else {
            for (keys in ix.under(vals, span).values) for (k in keys) st.rows[k]!!.let {
                if (keep(it)) out.add(it)
            }
        }
        return byKey(tbl, out)
    }

    override fun scanOrdered(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        order: List<Pair<String, Dir>>,
        keep: Keep,
        limit: Int,
    ): List<Row>? {
        val tbl = schema.lookupTable(table) ?: return null
        val st = tables[table] ?: return null
        var found: Triple<Secondary, Int, Dir>? = null
        for (ix in st.indexes) {
            val s = ix.serves(tbl, eq, order) ?: continue
            found = Triple(ix, s.first, s.second)
            break
        }
        val (ix, n, dir) = found ?: return null
        val out = ArrayList<Row>()
        if (limit == 0 || st.rows.isEmpty()) return out
        val range = ix.under(held(ix.columns.subList(0, n), eq), bounding(ix.columns, n, spans))
        val buckets = if (dir == Dir.Asc) range.values else range.descendingMap().values
        for (keys in buckets) {
            for (k in keys) {
                val r = st.rows[k]!!
                if (keep(r)) {
                    out.add(r)
                    if (out.size == limit) return out
                }
            }
        }
        return out
    }

    override fun applyChange(change: Change) {
        val tbl = schema.lookupTable(change.table) ?: return
        val st = tables[change.table] ?: return
        when (change) {
            is Change.Add -> st.set(change.table, tbl.keyOf(change.row), change.row)
            is Change.Edit -> st.set(change.table, tbl.keyOf(change.new), change.new)
            is Change.Remove -> st.set(change.table, tbl.keyOf(change.row), null)
        }
    }

    override fun digest(table: String): Digest? = tables[table]?.digest?.copy()

    // D4 The keys a text index says may hold the needles: per branch, the
    // postings of every trigram of every needle on an indexed column,
    // intersected; the branches together. `null` when a branch has none.
    private fun textKeys(st: TableState, has: List<List<Pair<String, String>>>): Set<List<Value>>? {
        if (st.texts.isEmpty()) return null
        val out = keyTree()
        for (branch in has) {
            val lists = ArrayList<TreeSet<List<Value>>?>()
            for ((c, needle) in branch) {
                val ix = st.texts.firstOrNull { it.column == c } ?: continue
                for (g in trigrams(needle)) lists.add(ix.postings[g])
            }
            if (lists.isEmpty()) return null
            if (lists.any { it == null }) continue
            val sorted = lists.map { it!! }.sortedBy { it.size }
            for (k in sorted[0]) if (sorted.all { k in it }) out.add(k)
        }
        return out
    }

    override fun scanWhereText(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        has: List<List<Pair<String, String>>>,
        keep: Keep,
    ): List<Row> {
        val tbl = schema.lookupTable(table)
        val wholeKey = tbl != null && keyHeld(tbl, eq) != null
        val st = tables[table]
        val keys = if (st != null && !wholeKey) textKeys(st, has) else null
        if (st == null || keys == null) return scanWhereEq(table, eq, spans, keep)
        return keys.mapNotNull { st.rows[it]?.takeIf(keep) }
    }

    public companion object {
        /** `MemoryStore::empty`: one empty table per table of the schema. */
        public fun empty(schema: Schema): MemoryStore {
            val tables = LinkedHashMap<String, TableState>()
            for (t in schema.tables) if (t.name !in tables) tables[t.name] = TableState.of(t)
            return MemoryStore(schema, tables)
        }

        /** `MemoryStore::from_value`: a store from the vectors' shape, each row applied raw. */
        public fun fromValue(schema: Schema, v: Value): MemoryStore {
            val st = empty(schema)
            if (v is Value.VStruct) {
                for ((t, rows) in v.fields) {
                    if (rows !is Value.VList) continue
                    for (r in rows.items) if (r is Value.VStruct) st.applyChange(Change.Add(t, r))
                }
            }
            return st
        }
    }
}

/**
 * An optimistic overlay over a base store (`store::Overlay`): `(table, key) -> row or none`,
 * consulted first on every read, the base untouched until the caller commits the changes.
 */
public class Overlay(private val base: Store) : Store {
    private val writes = TreeMap<String, TreeMap<List<Value>, Row?>>(CodePointOrder)

    override val schema: Schema
        get() = base.schema

    private fun written(table: String): TreeMap<List<Value>, Row?>? =
        writes[table]?.takeIf { it.isNotEmpty() }

    override fun get(table: String, key: List<Value>): Row? {
        val w = writes[table]
        if (w != null && w.containsKey(key)) return w[key]
        return base.get(table, key)
    }

    override fun exists(table: String, key: List<Value>): Boolean {
        val w = writes[table]
        if (w != null && w.containsKey(key)) return w[key] != null
        return base.exists(table, key)
    }

    override fun scan(table: String): List<Row> {
        val tbl = schema.lookupTable(table) ?: return emptyList()
        val ws = written(table) ?: return base.scan(table)
        val merged = TreeMap<List<Value>, Row>(KeyOrder)
        for (r in base.scan(table)) merged[tbl.keyOf(r)] = r
        for ((k, w) in ws) if (w != null) merged[k] = w else merged.remove(k)
        return merged.values.toList()
    }

    override fun scanWhere(table: String, keep: Keep): List<Row> =
        scanWhereEq(table, emptyList(), emptyList(), keep)

    override fun scanWhereEq(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        keep: Keep
    ): List<Row> = scanWhereText(table, eq, spans, emptyList(), keep)

    override fun scanWhereText(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        has: List<List<Pair<String, String>>>,
        keep: Keep,
    ): List<Row> {
        val tbl = schema.lookupTable(table) ?: return emptyList()
        val k = keyHeld(tbl, eq)
        if (k != null) return listOfNotNull(get(table, k)?.takeIf(keep))
        fun fromBase(keep: Keep): List<Row> =
            if (has.isEmpty()) base.scanWhereEq(table, eq, spans, keep)
            else base.scanWhereText(table, eq, spans, has, keep)
        val ws = written(table) ?: return fromBase(keep)
        val merged = TreeMap<List<Value>, Row>(KeyOrder)
        for (r in fromBase { r -> !ws.containsKey(tbl.keyOf(r)) && keep(r) }) merged[tbl.keyOf(r)] =
            r
        for ((key, w) in ws) if (w != null && keep(w)) merged[key] = w
        return merged.values.toList()
    }

    override fun scanOrdered(
        table: String,
        eq: List<Pair<String, Value>>,
        spans: List<Span>,
        order: List<Pair<String, Dir>>,
        keep: Keep,
        limit: Int,
    ): List<Row>? {
        val tbl = schema.lookupTable(table) ?: return null
        val ws = written(table) ?: return base.scanOrdered(table, eq, spans, order, keep, limit)
        val rows =
            base
                .scanOrdered(
                    table,
                    eq,
                    spans,
                    order,
                    { r -> !ws.containsKey(tbl.keyOf(r)) && keep(r) },
                    limit)
                ?.toMutableList() ?: return null
        for (w in ws.values) if (w != null && keep(w)) rows.add(w)
        rows.sortWith { a, b -> Rules.compareRows(tbl, order, a, b) }
        return if (rows.size > limit) rows.subList(0, limit) else rows
    }

    override fun applyChange(change: Change) {
        val tbl = schema.lookupTable(change.table) ?: return
        val (row, present) =
            when (change) {
                is Change.Add -> change.row to true
                is Change.Edit -> change.new to true
                is Change.Remove -> change.row to false
            }
        writes.getOrPut(change.table) { TreeMap(KeyOrder) }[tbl.keyOf(row)] =
            if (present) row else null
    }

    override fun digest(table: String): Digest? {
        val d = base.digest(table) ?: return null
        for ((k, w) in writes[table] ?: return d) {
            val old = base.get(table, k)
            if (old == w) continue
            if (old != null) d.sub(leaf(table, old))
            if (w != null) d.add(leaf(table, w))
        }
        return d
    }
}
