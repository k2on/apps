// §2 The schema (`rust/ark/src/schema.rs`): which tables exist, their
// columns and key, which indexes are unique, which columns reference which
// table, and which text columns are searched through a trigram index. A
// value in the module, not DDL; one set of tables and one log.
package dev.arkdb

import java.util.SortedMap

/** §2.1 The static types of the IR (`schema.rs`, `Ty`). */
public sealed class Ty {
    public object TBool : Ty() {
        override fun toString(): String = "Bool"
    }

    public object TInt : Ty() {
        override fun toString(): String = "Int"
    }

    public object TText : Ty() {
        override fun toString(): String = "Text"
    }

    public object TBytes : Ty() {
        override fun toString(): String = "Bytes"
    }

    public data class TId(val table: String) : Ty() {
        override fun toString(): String = "Id(${debugString(table)})"
    }

    public data class TEnum(val variants: List<String>) : Ty() {
        override fun toString(): String = "Enum(${debugStrings(variants)})"
    }

    public data class TOption(val of: Ty) : Ty() {
        override fun toString(): String = "Option($of)"
    }

    public data class TList(val of: Ty) : Ty() {
        override fun toString(): String = "List($of)"
    }

    /** Fields in code point order of their names. */
    public class TStruct(fields: Map<String, Ty>) : Ty() {
        public val fields: SortedMap<String, Ty> = sortedFields(fields)

        override fun equals(other: Any?): Boolean = other is TStruct && fields == other.fields

        override fun hashCode(): Int = fields.hashCode()

        override fun toString(): String =
            "Struct({" +
                fields.entries.joinToString(", ") { debugString(it.key) + ": " + it.value } +
                "})"
    }

    /**
     * §2.2 A type of the module's type section, by its name (`Ty::Named`): a record or a sum a live
     * state is made of (`docs/plan-live.md`). `{"t":"named","name"}`.
     */
    public data class TNamed(val name: String) : Ty() {
        override fun toString(): String = "Named(${debugString(name)})"
    }

    /**
     * §2.2 A list of at least one element: a list to the checker, held to its one element wherever
     * a value crosses a boundary. `{"t":"non_empty_list","of"}`.
     */
    public data class TNonEmpty(val of: Ty) : Ty() {
        override fun toString(): String = "NonEmpty($of)"
    }

    /**
     * §2.2 A sum of records: one of the named variants, each a record of its own fields. Its value
     * is a struct of the variant's fields with the variant's name under `t`, so no variant has a
     * field `t`. `{"t":"sum","variants":{name: {field: ty}}}`. Variants and fields in code point
     * order of their names.
     */
    public class TSum(variants: Map<String, Map<String, Ty>>) : Ty() {
        public val variants: SortedMap<String, SortedMap<String, Ty>> =
            sortedFields(variants.mapValues { sortedFields(it.value) })

        override fun equals(other: Any?): Boolean = other is TSum && variants == other.variants

        override fun hashCode(): Int = variants.hashCode()

        override fun toString(): String =
            "Sum({" +
                variants.entries.joinToString(", ") { (n, fs) ->
                    debugString(n) +
                        ": {" +
                        fs.entries.joinToString(", ") { debugString(it.key) + ": " + it.value } +
                        "}"
                } +
                "})"
    }

    /**
     * §2.2 One value per device standing in a live instance, by device id: a struct keyed by the
     * ids, a slot that holds nothing a key that is not there. `{"t":"per_device","of"}`.
     */
    public data class TPerDevice(val of: Ty) : Ty() {
        override fun toString(): String = "PerDevice($of)"
    }

    /**
     * §2.2 A field of a live state's record that is ephemeral (`docs/plan-live.md`, "Ordered and
     * ephemeral fields"): left out of the state's hash and of what a server keeps.
     * `{"t":"ephemeral","of"}`.
     */
    public data class TEphemeral(val of: Ty) : Ty() {
        override fun toString(): String = "Ephemeral($of)"
    }

    /** `is_scalar`: the types a column may have, before nullability. */
    public val isScalar: Boolean
        get() =
            when (this) {
                is TBool,
                is TInt,
                is TText,
                is TBytes,
                is TId,
                is TEnum -> true
                else -> false
            }
}

/** A column; a nullable column's values are `Null` or a value of `ty`. */
public data class Column(val name: String, val ty: Ty, val nullable: Boolean) {
    /** `Column::column_ty`: the static type, nullability included. */
    public val columnTy: Ty
        get() = if (nullable) Ty.TOption(ty) else ty
}

/** A uniqueness constraint when `unique`, a statement of intent otherwise. */
public data class Index(val columns: List<String>, val unique: Boolean)

/** `column REFERENCES table(key)`. */
public data class Ref(val column: String, val table: String)

/** Ascending or descending, for an order. */
public enum class Dir {
    Asc,
    Desc,
}

/**
 * A table (`schema.rs`, `Table`). `text` is `docs/plan-db.md` D4's text indexes: each a text column
 * whose folded value's trigrams name the rows that hold them, kept beside `indexes` as the Rust
 * keeps them (on the wire an `index` node of kind `text`, after the others).
 */
public data class Table(
    val name: String,
    val columns: List<Column>,
    val key: List<String>,
    val indexes: List<Index>,
    val refs: List<Ref>,
    val text: List<String> = emptyList(),
) {
    private val byName: Map<String, Column> = columns.associateBy { it.name }

    /** The column names, as a set: what a row laid out as this table's has (`Row::is_of`). */
    public val columnNames: Set<String> = columns.mapTo(LinkedHashSet()) { it.name }

    /** `Table::column`. */
    public fun column(name: String): Column? = byName[name]

    /** `Table::row_ty`: a struct of every column. */
    public val rowTy: Ty
        get() = Ty.TStruct(columns.associate { it.name to it.columnTy })

    /** `Table::key_ty`: the key columns' types, in key order. */
    public val keyTy: List<Ty>
        get() = key.mapNotNull { column(it)?.columnTy }

    /** `Table::key_of`: the key columns' values in key order; a missing column reads as `Null`. */
    public fun keyOf(row: Row): List<Value> = key.map { row.fields[it] ?: Value.VNull }

    /** Whether a row is laid out as this table's rows are: exactly its columns (`Row::is_of`). */
    public fun holds(row: Row): Boolean =
        row.fields.size == columns.size && columns.all { row.fields.containsKey(it.name) }
}

/** The tables, in declaration order — which is also the order a state hash walks them. */
public data class Schema(val tables: List<Table>) {
    private val byName: Map<String, Table> = tables.associateBy { it.name }

    /** `Schema::lookup_table` (the first of a name, as `find` is). */
    public fun lookupTable(name: String): Table? = tables.firstOrNull { it.name == name }

    /** §2.2 Every relationship in the schema (`Schema::relations`). */
    public fun relations(): List<Relation> =
        tables.flatMap { t -> t.refs.map { r -> Relation(r.table, t.name, r.column) } }

    /** `Schema::children_of`: the relationships reaching down from a table. */
    public fun childrenOf(parent: String): List<Relation> =
        relations().filter { it.parent == parent }

    /** `Schema::parent_of`: the relationships reaching up from a table. */
    public fun parentOf(child: String): List<Relation> = relations().filter { it.child == child }

    public companion object {
        /** `Schema::empty`. */
        public val empty: Schema = Schema(emptyList())
    }
}

/** §2.2 `child.column REFERENCES parent`. */
public data class Relation(val parent: String, val child: String, val column: String)

/** §2.3 Why a schema is not well formed (`schema.rs`, `SchemaError`). */
public sealed class SchemaError {
    public data class DuplicateTable(val table: String) : SchemaError()

    public data class DuplicateColumn(val table: String, val column: String) : SchemaError()

    public data class NoKey(val table: String) : SchemaError()

    public data class UnknownKeyColumn(val table: String, val column: String) : SchemaError()

    public data class NullableKey(val table: String, val column: String) : SchemaError()

    public data class NonScalarColumn(val table: String, val column: String) : SchemaError()

    public data class UnknownIndexColumn(val table: String, val column: String) : SchemaError()

    public data class UnknownRefColumn(val table: String, val column: String) : SchemaError()

    public data class UnknownRefTable(val table: String, val parent: String) : SchemaError()

    public data class RefToCompositeKey(val table: String, val parent: String) : SchemaError()

    public data class RefTypeMismatch(
        val table: String,
        val column: String,
        val want: Ty,
        val got: Ty
    ) : SchemaError()

    public data class IdColumnWithoutRef(val table: String, val column: String) : SchemaError()

    public data class IdNamesWrongTable(val table: String, val column: String) : SchemaError()

    public data class TextIndexNotText(val table: String, val column: String) : SchemaError()
}

private fun dups(names: List<String>): List<String> {
    val counts = sortedMapOf<String, Int>(CodePointOrder)
    for (n in names) counts[n] = (counts[n] ?: 0) + 1
    return counts.filter { it.value > 1 }.keys.toList()
}

/** `check_schema`: every rule a schema breaks, in the spec's order. */
public fun checkSchema(sch: Schema): List<SchemaError> {
    val errs = ArrayList<SchemaError>()
    val dupTables = dups(sch.tables.map { it.name })
    for (t in sch.tables) if (t.name in dupTables) errs.add(SchemaError.DuplicateTable(t.name))
    for (t in sch.tables) perTable(sch, t, errs)
    return errs
}

private fun perTable(sch: Schema, t: Table, errs: MutableList<SchemaError>) {
    val tn = t.name
    for (c in dups(t.columns.map { it.name })) errs.add(SchemaError.DuplicateColumn(tn, c))
    if (t.key.isEmpty()) errs.add(SchemaError.NoKey(tn))
    for (k in t.key) if (t.column(k) == null) errs.add(SchemaError.UnknownKeyColumn(tn, k))
    for (k in t.key) if (t.column(k)?.nullable == true) errs.add(SchemaError.NullableKey(tn, k))
    for (c in t.columns) if (!c.ty.isScalar) errs.add(SchemaError.NonScalarColumn(tn, c.name))
    for (ix in t.indexes) for (c in ix.columns) if (t.column(c) == null)
        errs.add(SchemaError.UnknownIndexColumn(tn, c))
    for ((i, c) in t.text.withIndex()) {
        val col = t.column(c)
        when {
            col == null -> errs.add(SchemaError.UnknownIndexColumn(tn, c))
            col.ty != Ty.TText || t.text.subList(0, i).contains(c) ->
                errs.add(SchemaError.TextIndexNotText(tn, c))
        }
    }
    for (r in t.refs) perRef(sch, t, r, errs)
    for (c in t.columns) {
        val ty = c.ty
        if (ty is Ty.TId) {
            val isRef = t.refs.any { it.column == c.name }
            val isOwnKey = ty.table == t.name && c.name in t.key
            if (!isRef && !isOwnKey) errs.add(SchemaError.IdColumnWithoutRef(tn, c.name))
        }
    }
}

private fun perRef(sch: Schema, t: Table, r: Ref, errs: MutableList<SchemaError>) {
    val c = t.column(r.column)
    val p = sch.lookupTable(r.table)
    when {
        c == null -> errs.add(SchemaError.UnknownRefColumn(t.name, r.column))
        p == null -> errs.add(SchemaError.UnknownRefTable(t.name, r.table))
        else -> {
            val keyTy = p.keyTy
            if (keyTy.size == 1) {
                val want = if (keyTy[0] is Ty.TId) Ty.TId(r.table) else keyTy[0]
                if (c.ty != want)
                    errs.add(SchemaError.RefTypeMismatch(t.name, r.column, want, c.ty))
                val ty = c.ty
                if (ty is Ty.TId && ty.table != r.table)
                    errs.add(SchemaError.IdNamesWrongTable(t.name, r.column))
            } else {
                errs.add(SchemaError.RefToCompositeKey(t.name, r.table))
            }
        }
    }
}
