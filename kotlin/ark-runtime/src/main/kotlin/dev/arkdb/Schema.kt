// §2 The schema (Ark.Schema): one set of tables, and one log. Every
// reference is checked, and any function may read any table.
package dev.arkdb

import java.util.SortedMap

/** §2.1 The static types of the IR. */
public sealed class Ty {
    public object TBool : Ty() {
        override fun toString(): String = "TBool"
    }

    public object TInt : Ty() {
        override fun toString(): String = "TInt"
    }

    public object TText : Ty() {
        override fun toString(): String = "TText"
    }

    public object TBytes : Ty() {
        override fun toString(): String = "TBytes"
    }

    public data class TId(val table: String) : Ty()

    public data class TEnum(val variants: List<String>) : Ty()

    public data class TOption(val of: Ty) : Ty()

    public data class TList(val of: Ty) : Ty()

    /** Fields in code point order of their names. */
    public class TStruct(fields: Map<String, Ty>) : Ty() {
        public val fields: SortedMap<String, Ty> = sortedFields(fields)
        override fun equals(other: Any?): Boolean = other is TStruct && fields == other.fields
        override fun hashCode(): Int = fields.hashCode()
        override fun toString(): String = "TStruct$fields"
    }

    /** The types a column may have (before nullability). */
    public val isScalar: Boolean
        get() = when (this) {
            is TBool, is TInt, is TText, is TBytes, is TId, is TEnum -> true
            else -> false
        }
}

public data class Column(val name: String, val ty: Ty, val nullable: Boolean) {
    /** The static type of the column, nullability included. */
    public val columnTy: Ty get() = if (nullable) Ty.TOption(ty) else ty
}

/** A uniqueness constraint when `unique`, a statement of intent otherwise. */
public data class Index(val columns: List<String>, val unique: Boolean)

/** `column REFERENCES table(key)`. */
public data class Ref(val column: String, val table: String)

public data class Table(
    val name: String,
    val columns: List<Column>,
    val key: List<String>,
    val indexes: List<Index>,
    val refs: List<Ref>,
) {
    public fun column(n: String): Column? = columns.firstOrNull { it.name == n }

    /** The type of a whole row: a struct of every column. */
    public val rowTy: Ty.TStruct get() = Ty.TStruct(columns.associate { it.name to it.columnTy })

    /** The types of the key columns, in key order. */
    public val keyTy: List<Ty> get() = key.mapNotNull { column(it)?.columnTy }

    /** The key of a row: the key columns' values in key order. */
    public fun keyOf(row: Row): List<Value> = key.map { row[it] }
}

/** The tables, in declaration order — which is also the order a state hash walks them. */
public data class Schema(val tables: List<Table>) {
    public fun lookupTable(n: String): Table? = tables.firstOrNull { it.name == n }

    /** §2.2 Every relationship: each reference read both ways. */
    public val relations: List<Relation>
        get() = tables.flatMap { t -> t.refs.map { r -> Relation(r.table, t.name, r.column) } }

    /** The relationships reaching down from a table. */
    public fun childrenOf(parent: String): List<Relation> = relations.filter { it.parent == parent }

    /** The relationships reaching up from a table. */
    public fun parentOf(child: String): List<Relation> = relations.filter { it.child == child }
}

/** A reference as two relationships: `column` is the child column holding the parent's key. */
public data class Relation(val parent: String, val child: String, val column: String)

/** Ascending or descending, for an order. */
public enum class Dir { Asc, Desc }

/** §2.3 Well-formedness. */
public sealed class SchemaError {
    public data class DuplicateTable(val table: String) : SchemaError()
    public data class DuplicateColumn(val table: String, val column: String) : SchemaError()
    public data class NoKey(val table: String) : SchemaError()
    public data class UnknownKeyColumn(val table: String, val column: String) : SchemaError()
    public data class NullableKey(val table: String, val column: String) : SchemaError()
    public data class NonScalarColumn(val table: String, val column: String) : SchemaError()
    public data class UnknownIndexColumn(val table: String, val column: String) : SchemaError()
    public data class UnknownRefColumn(val table: String, val column: String) : SchemaError()
    public data class UnknownRefTable(val table: String, val ref: String) : SchemaError()
    public data class RefToCompositeKey(val table: String, val ref: String) : SchemaError()
    public data class RefTypeMismatch(val table: String, val column: String, val want: Ty, val got: Ty) : SchemaError()
    public data class IdColumnWithoutRef(val table: String, val column: String) : SchemaError()
    public data class IdNamesWrongTable(val table: String, val column: String) : SchemaError()
}

public fun checkSchema(sch: Schema): List<SchemaError> {
    val out = ArrayList<SchemaError>()
    val tables = sch.tables
    val dupTables = dups(tables.map { it.name })
    for (t in tables) if (t.name in dupTables) out.add(SchemaError.DuplicateTable(t.name))
    for (t in tables) {
        for (c in dups(t.columns.map { it.name })) out.add(SchemaError.DuplicateColumn(t.name, c))
        if (t.key.isEmpty()) out.add(SchemaError.NoKey(t.name))
        for (k in t.key) if (t.column(k) == null) out.add(SchemaError.UnknownKeyColumn(t.name, k))
        for (k in t.key) {
            val c = t.column(k)
            if (c != null && c.nullable) out.add(SchemaError.NullableKey(t.name, k))
        }
        for (c in t.columns) if (!c.ty.isScalar) out.add(SchemaError.NonScalarColumn(t.name, c.name))
        for (ix in t.indexes) for (c in ix.columns) if (t.column(c) == null) out.add(SchemaError.UnknownIndexColumn(t.name, c))
        for (r in t.refs) {
            val c = t.column(r.column)
            val p = sch.lookupTable(r.table)
            when {
                c == null -> out.add(SchemaError.UnknownRefColumn(t.name, r.column))
                p == null -> out.add(SchemaError.UnknownRefTable(t.name, r.table))
                else -> {
                    val kt = p.keyTy
                    if (kt.size == 1) {
                        val pk = kt[0]
                        val want = if (pk is Ty.TId) Ty.TId(r.table) else pk
                        if (c.ty != want) out.add(SchemaError.RefTypeMismatch(t.name, r.column, want, c.ty))
                        val ct = c.ty
                        if (ct is Ty.TId && ct.table != r.table) out.add(SchemaError.IdNamesWrongTable(t.name, r.column))
                    } else {
                        out.add(SchemaError.RefToCompositeKey(t.name, r.table))
                    }
                }
            }
        }
        // An id column must be a key of its own table or a reference.
        for (c in t.columns) {
            val ct = c.ty
            if (ct is Ty.TId && t.refs.none { it.column == c.name } && !(ct.table == t.name && c.name in t.key)) {
                out.add(SchemaError.IdColumnWithoutRef(t.name, c.name))
            }
        }
    }
    return out
}

/** The names that appear more than once, in code point order. */
private fun dups(xs: List<String>): List<String> =
    xs.groupingBy { it }.eachCount().filter { it.value > 1 }.keys.sortedWith(CodePointOrder)
