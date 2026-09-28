// §1.4 (AUTHORING.md) The three table writes, over any `Store`: insert,
// upsert and update. Each is a `put` after a match, so every constraint is
// still decided by the store's `put` alone, and the interpreter and a
// native body write through exactly this code.
package dev.arkdb

public object Writes {
    /**
     * The existing row a new one matches on `on` — the table's key when
     * `on` is empty, otherwise a unique index's columns. A NULL in a matched
     * column matches nothing, as it clashes with nothing in a unique index.
     */
    public fun match(st: Store, table: String, row: Row, on: List<String>): Row? {
        val tbl = st.schema.lookupTable(table) ?: throw Fault.Refuse(Refusal.NoSuchTable(table))
        val cols = on.ifEmpty { tbl.key }
        val want = cols.map { row[it] }
        if (want.any { it is Value.VNull }) return null
        if (cols == tbl.key) return st.get(table, want) as? Row
        return st.scan(table).firstOrNull { r -> cols.map { r[it] } == want }
    }

    /** `SInsert`: write the row unless one matches on the columns. */
    public fun insert(st: Store, table: String, row: Value, on: List<String>) {
        val r = row as? Row ?: throw Fault.bug("insert: expected Struct, got ${row.show().take(60)}")
        if (match(st, table, r, on) != null) return
        st.put(table, r)
    }

    /** `SUpsert`: write the row; a matching row keeps its key columns and takes the rest from the new one. */
    public fun upsert(st: Store, table: String, row: Value, on: List<String>) {
        val r = row as? Row ?: throw Fault.bug("upsert: expected Struct, got ${row.show().take(60)}")
        val old = match(st, table, r, on)
        if (old == null) {
            st.put(table, r)
            return
        }
        st.put(table, keepKey(st, table, old, r))
    }

    /**
     * `SUpdate`: the row under `key`, handed to `f`, replaced by what `f`
     * returns with the key columns of the row it replaces. A no-op when
     * there is no such row, and `f` is then not called.
     */
    public fun update(st: Store, table: String, key: List<Value>, f: (Row) -> Value) {
        val old = st.get(table, key) as? Row ?: return
        val r = f(old) as? Row ?: throw Fault.bug("update: expected Struct")
        st.put(table, keepKey(st, table, old, r))
    }

    private fun keepKey(st: Store, table: String, old: Row, new: Row): Row {
        val tbl = st.schema.lookupTable(table) ?: throw Fault.Refuse(Refusal.NoSuchTable(table))
        val m = LinkedHashMap<String, Value>(new.fields)
        for (k in tbl.key) m[k] = old[k]
        return Value.VStruct(m)
    }
}
