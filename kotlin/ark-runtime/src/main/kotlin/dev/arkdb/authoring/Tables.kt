// AUTHORING.md §2.3 and §2.5: rows, the module's tables, and their queries. A
// table is `db.<table>`, a `Table<Row>`; a row class declares its columns
// once in its companion's `columns()`, and its constructor takes them in
// that order.
package dev.arkdb.authoring

import dev.arkdb.CmpOp
import dev.arkdb.Column
import dev.arkdb.Dir
import dev.arkdb.Expr
import dev.arkdb.Fault
import dev.arkdb.IR
import dev.arkdb.Index
import dev.arkdb.Plan
import dev.arkdb.Pred as VPred
import dev.arkdb.Ref
import dev.arkdb.Related
import dev.arkdb.Relation
import dev.arkdb.Stmt
import dev.arkdb.Table as IrTable
import dev.arkdb.Ty
import dev.arkdb.Value
import dev.arkdb.Writes
import java.lang.reflect.Constructor
import java.lang.reflect.Method
import java.lang.reflect.ParameterizedType
import java.util.concurrent.ConcurrentHashMap
import kotlin.reflect.KClass
import kotlin.reflect.KType

// Keys -------------------------------------------------------------------------

/** The shape of a row's key: its columns' types, in key order. */
public interface Key
public class Key1<A : Data> private constructor() : Key
public class Key2<A : Data, B : Data> private constructor() : Key
public class Key3<A : Data, B : Data, C : Data> private constructor() : Key

// Declarations -----------------------------------------------------------------

/** A row of a table, keyed by `K`. Its companion is a `Row.Of` naming the table and its columns. */
public interface Row<K : Key> : Data {
    public interface Of<T : Row<*>> {
        public val NAME: String
        public fun columns(): Columns<T>
    }
}

/**
 * A struct of the vocabulary's values that is not a row of a table: what a
 * query or a helper builds and returns (`TStruct`). Its companion is a
 * `Record.Of` saying its fields once, in declaration order, and its
 * constructor takes them in that order:
 *
 * ```
 * class ArtistsEntry(val art: Text, val name: Text, val tracks: Int) : Record {
 *     companion object : Record.Of<ArtistsEntry> {
 *         override fun fields(): Fields<ArtistsEntry> =
 *             fields<ArtistsEntry>().field("art", text()).field("name", text()).field("tracks", int())
 *     }
 * }
 * ```
 *
 * The IR's struct type is a map, so the canonical form has the fields in
 * name order whatever order they are declared in.
 */
public interface Record : Data {
    public interface Of<R : Record> {
        public fun fields(): Fields<R>
    }
}

/** A record's fields, by name and type, in declaration order. */
public class Fields<R : Record> internal constructor(internal val defs: KList<Pair<String, FieldSpec<*>>>) {
    /**
     * The next field, of the type the field builder names (`text()`,
     * `int()`, `opt(..)`, `id<T>()`, `record<R>()`, …). A record's field
     * carries no checks: a check belongs to an input.
     */
    public fun field(name: String, spec: FieldSpec<*>): Fields<R> {
        if (spec.checks.isNotEmpty()) throw Fault.bug("authoring: $name: a record's field carries no checks")
        return Fields(defs + (name to spec))
    }
}

/** `fields<ArtistsEntry>()`, the start of a record's field list. */
public fun <R : Record> fields(): Fields<R> = Fields(emptyList())

/** A field of a record type (in a record, or in a list or option of one). */
public inline fun <reified R : Record> record(): FieldSpec<R> = recordIn(R::class.java)

@PublishedApi
internal fun <R : Record> recordIn(cls: Class<*>): FieldSpec<R> = FieldSpec({ Kind.KRecord(RecordInfo.of(cls)) }, emptyList())

/**
 * The module's tables: a class whose constructor takes them, in the
 * schema's order. A module has one; every router of it is over the same
 * class. A companion is optional, and when written is a `Tables.Of`.
 */
public interface Tables {
    public interface Of
}

/** A column of `T` holding a `V`. */
public class Col<T : Row<*>, V : Data> @PublishedApi internal constructor(
    public val name: String,
    internal val type: KType,
) {
    public fun eq(v: V): Pred<T> = Pred.Cmp(name, CmpOp.Eq, v)
    public fun ne(v: V): Pred<T> = Pred.Cmp(name, CmpOp.Ne, v)
    public fun lt(v: V): Pred<T> = Pred.Cmp(name, CmpOp.Lt, v)
    public fun le(v: V): Pred<T> = Pred.Cmp(name, CmpOp.Le, v)
    public fun gt(v: V): Pred<T> = Pred.Cmp(name, CmpOp.Gt, v)
    public fun ge(v: V): Pred<T> = Pred.Cmp(name, CmpOp.Ge, v)
    public fun isIn(vararg vs: V): Pred<T> = Pred.In(name, vs.toList())
    public fun isIn(vs: KList<V>): Pred<T> = Pred.In(name, vs)
    public fun asc(): Order<T> = Order(name, Dir.Asc)
    public fun desc(): Order<T> = Order(name, Dir.Desc)
}

public fun <T : Row<*>> Col<T, Text>.eq(v: String): Pred<T> = eq(lit(v))
public fun <T : Row<*>> Col<T, Text>.ne(v: String): Pred<T> = ne(lit(v))
public fun <T : Row<*>> Col<T, Int>.eq(v: KInt): Pred<T> = eq(lit(v))
public fun <T : Row<*>> Col<T, Int>.ne(v: KInt): Pred<T> = ne(lit(v))
public fun <T : Row<*>> Col<T, Int>.lt(v: KInt): Pred<T> = lt(lit(v))
public fun <T : Row<*>> Col<T, Int>.le(v: KInt): Pred<T> = le(lit(v))
public fun <T : Row<*>> Col<T, Int>.gt(v: KInt): Pred<T> = gt(lit(v))
public fun <T : Row<*>> Col<T, Int>.ge(v: KInt): Pred<T> = ge(lit(v))
public fun <T : Row<*>> Col<T, Bool>.eq(v: Boolean): Pred<T> = eq(lit(v))

/** `col<Playlist, Text>("name")`. */
public inline fun <reified T : Row<*>, reified V : Data> col(name: String): Col<T, V> = Col(name, kotlin.reflect.typeOf<V>())

/** A relationship from `P` down to its children `C`, by a reference `C` declares. */
public class Rel<P : Row<*>, C : Row<*>> @PublishedApi internal constructor(
    public val name: String,
    internal val child: Class<*>,
)

/** `rel<Playlist, PlaylistItem>("playlist_item")`. */
public inline fun <reified P : Row<*>, reified C : Row<*>> rel(name: String): Rel<P, C> = Rel(name, C::class.java)

/** One column of an order. */
public class Order<T : Row<*>> internal constructor(internal val column: String, internal val dir: Dir)

/** A filter over rows of `T`; the right-hand sides are values, evaluated once before the scan. */
public sealed class Pred<T : Row<*>> {
    internal class Cmp<T : Row<*>>(val column: String, val op: CmpOp, val v: Data) : Pred<T>()
    internal class In<T : Row<*>>(val column: String, val vs: KList<Data>) : Pred<T>()
    internal class All<T : Row<*>>(val ps: KList<Pred<T>>) : Pred<T>()
    internal class AnyOf<T : Row<*>>(val ps: KList<Pred<T>>) : Pred<T>()
    internal class Not<T : Row<*>>(val p: Pred<T>) : Pred<T>()

    public fun and(p: Pred<T>): Pred<T> = All(listOf(this, p))
    public fun or(p: Pred<T>): Pred<T> = AnyOf(listOf(this, p))
    public fun not(): Pred<T> = Not(this)

    internal fun ir(): IR.Pred = when (this) {
        is Cmp -> IR.Pred.Cmp(column, op, exprOf(v))
        is In -> IR.Pred.In(column, vs.map { exprOf(it) })
        is All -> IR.Pred.All(ps.map { it.ir() })
        is AnyOf -> IR.Pred.Any(ps.map { it.ir() })
        is Not -> IR.Pred.Not(p.ir())
    }

    internal fun native(): VPred = when (this) {
        is Cmp -> VPred.Cmp(column, op, valueOf(v))
        is In -> VPred.In(column, vs.map { valueOf(it) })
        is All -> VPred.All(ps.map { it.native() })
        is AnyOf -> VPred.Any(ps.map { it.native() })
        is Not -> VPred.Not(p.native())
    }
}

/** The columns of a row, said once: types, nullability, references, key and indexes. */
public class Columns<T : Row<*>> @PublishedApi internal constructor() {
    internal class Def(val col: Col<*, *>, val kind: String, var nullable: Boolean = false, var refs: Class<*>? = null, val variants: KList<String> = emptyList())

    internal val defs = ArrayList<Def>()
    internal var key: KList<String> = emptyList()
    internal val indexes = ArrayList<Index>()

    private fun add(c: Col<T, *>, kind: String, variants: KList<String> = emptyList()): Columns<T> {
        defs.add(Def(c, kind, variants = variants))
        return this
    }

    public fun id(c: Col<T, *>): Columns<T> = add(c, "id")
    public fun text(c: Col<T, *>): Columns<T> = add(c, "text")
    public fun int(c: Col<T, *>): Columns<T> = add(c, "int")
    public fun bool(c: Col<T, *>): Columns<T> = add(c, "bool")
    public fun bytes(c: Col<T, *>): Columns<T> = add(c, "bytes")
    public fun enum(c: Col<T, *>, vararg variants: String): Columns<T> = add(c, "enum", variants.toList())

    /** The column just added may be null. */
    public fun nullable(): Columns<T> {
        (defs.lastOrNull() ?: throw Fault.bug("authoring: .nullable() before any column")).nullable = true
        return this
    }

    /** The id column just added references `P`'s key. */
    public inline fun <reified P : Row<*>> refs(): Columns<T> = refsOf(P::class.java)

    @PublishedApi
    internal fun refsOf(p: Class<*>): Columns<T> {
        (defs.lastOrNull() ?: throw Fault.bug("authoring: .refs() before any column")).refs = p
        return this
    }

    public fun key(vararg cols: Col<T, *>): Columns<T> {
        key = cols.map { it.name }
        return this
    }

    public fun unique(vararg cols: Col<T, *>): Columns<T> {
        indexes.add(Index(cols.map { it.name }, true))
        return this
    }

    public fun index(vararg cols: Col<T, *>): Columns<T> {
        indexes.add(Index(cols.map { it.name }, false))
        return this
    }
}

/** `columns<Playlist>()`, the start of a row's column list. */
public fun <T : Row<*>> columns(): Columns<T> = Columns()

// What a row class is, read once ------------------------------------------------

internal class RowInfo<T : Row<*>> private constructor(val cls: Class<T>) {
    private val companion: Row.Of<T> by lazy {
        @Suppress("UNCHECKED_CAST")
        (cls.getField("Companion").get(null) as? Row.Of<T>)
            ?: throw Fault.bug("authoring: ${cls.name}'s companion is not a Row.Of")
    }

    val name: String by lazy { companion.NAME }

    private val cols: Columns<T> by lazy { companion.columns() }

    val table: IrTable by lazy {
        val columns = cols.defs.map { d -> Column(d.col.name, tyOf(d), d.nullable) }
        val refs = cols.defs.mapNotNull { d -> d.refs?.let { Ref(d.col.name, of(it).name) } }
        IrTable(name, columns, cols.key.ifEmpty { throw Fault.bug("authoring: $name has no .key(..)") }, cols.indexes.toList(), refs)
    }

    val rowTy: Ty.TStruct get() = table.rowTy

    private fun tyOf(d: Columns.Def): Ty = when (d.kind) {
        "text" -> Ty.TText
        "int" -> Ty.TInt
        "bool" -> Ty.TBool
        "bytes" -> Ty.TBytes
        "enum" -> Ty.TEnum(d.variants)
        else -> {
            // An id names the table its type argument is: Id<Playlist> is TId "playlist".
            val t = d.col.type
            val idOf = if (t.classifier == Opt::class) t.arguments[0].type!! else t
            val target = idOf.arguments.firstOrNull()?.type?.classifier as? KClass<*>
                ?: throw Fault.bug("authoring: $name.${d.col.name} is declared .id but is not an Id<T>")
            Ty.TId(of(target.java).name)
        }
    }

    private val ctor: Constructor<*> by lazy {
        cls.constructors.firstOrNull { it.parameterCount == cols.defs.size && !it.isSynthetic }
            ?: throw Fault.bug("authoring: ${cls.name} needs a constructor of its ${cols.defs.size} columns in columns() order")
    }

    private val getters: KList<Method> by lazy {
        cols.defs.map { d ->
            val g = "get" + camel(d.col.name).replaceFirstChar { it.uppercaseChar() }
            try {
                cls.getMethod(g)
            } catch (e: NoSuchMethodException) {
                throw Fault.bug("authoring: ${cls.name} has no property ${camel(d.col.name)} for column ${d.col.name}")
            }
        }
    }

    private val kinds: KList<Kind> by lazy {
        table.columns.map { c -> Kind.ofTy(c.columnTy) }
    }

    /** A row value of this table over a term: each column a field of it. */
    fun make(t: Term, k: Kind): T {
        val run = Run.current()
        val args = table.columns.mapIndexed { i, c ->
            val ft = when (t) {
                is Term.N -> Term.N((t.v as Value.VStruct)[c.name])
                is Term.E -> Term.E(Expr.Field(t.e, c.name))
            }
            kinds[i].make(ft)
        }
        @Suppress("UNCHECKED_CAST")
        val row = ctor.newInstance(*args.toTypedArray()) as T
        run.remember(row, t)
        return row
    }

    /** A row value built by the author: a struct of its fields. */
    fun termOfFields(row: Any): Term {
        val vals = getters.map { it.invoke(row) as Data }
        return if (Run.current().native) {
            Term.N(Value.VStruct(table.columns.zip(vals).associate { (c, v) -> c.name to valueOf(v) }))
        } else {
            Term.E(Expr.Struct(table.columns.zip(vals).associate { (c, v) -> c.name to exprOf(v) }))
        }
    }

    companion object {
        private val cache = ConcurrentHashMap<Class<*>, RowInfo<*>>()

        fun of(c: Class<*>): RowInfo<*> = cache.getOrPut(c) {
            @Suppress("UNCHECKED_CAST")
            RowInfo(c as Class<Row<*>>)
        }

        fun camel(snake: String): String {
            val sb = StringBuilder()
            var up = false
            for (ch in snake) {
                if (ch == '_') {
                    up = true
                } else {
                    sb.append(if (up) ch.uppercaseChar() else ch)
                    up = false
                }
            }
            return sb.toString()
        }
    }
}

/** What a record class is, read once: its fields, their kinds, and how to make one. */
internal class RecordInfo<R : Record> private constructor(val cls: Class<R>) {
    private val defs: KList<Pair<String, FieldSpec<*>>> by lazy {
        @Suppress("UNCHECKED_CAST")
        ((cls.getField("Companion").get(null) as? Record.Of<R>) ?: throw Fault.bug("authoring: ${cls.name}'s companion is not a Record.Of"))
            .fields().defs
    }

    val names: KList<String> get() = defs.map { it.first }

    private val kinds: KList<Kind> by lazy { defs.map { it.second.kind() } }

    val ty: Ty.TStruct by lazy { Ty.TStruct(names.zip(kinds).associate { (n, k) -> n to k.ty }) }

    private val ctor: Constructor<*> by lazy {
        cls.constructors.firstOrNull { it.parameterCount == defs.size && !it.isSynthetic }
            ?: throw Fault.bug("authoring: ${cls.name} needs a constructor of its ${defs.size} fields in fields() order")
    }

    private val getters: KList<Method> by lazy {
        names.map { n ->
            val g = "get" + RowInfo.camel(n).replaceFirstChar { it.uppercaseChar() }
            try {
                cls.getMethod(g)
            } catch (e: NoSuchMethodException) {
                throw Fault.bug("authoring: ${cls.name} has no property ${RowInfo.camel(n)} for field $n")
            }
        }
    }

    /** A record over a term: each field a field of it. */
    fun make(t: Term): R {
        val args = names.mapIndexed { i, n ->
            kinds[i].make(
                when (t) {
                    is Term.N -> Term.N((t.v as? Value.VStruct)?.get(n) ?: Value.VNull)
                    is Term.E -> Term.E(Expr.Field(t.e, n))
                },
            )
        }
        @Suppress("UNCHECKED_CAST")
        val r = ctor.newInstance(*args.toTypedArray()) as R
        Run.current().remember(r, t)
        return r
    }

    /** A record built by the author: a struct of its fields. */
    fun termOfFields(r: Any): Term {
        val vals = getters.map { it.invoke(r) as Data }
        return if (Run.current().native) {
            Term.N(Value.VStruct(names.zip(vals).associate { (n, v) -> n to valueOf(v) }))
        } else {
            Term.E(Expr.Struct(names.zip(vals).associate { (n, v) -> n to exprOf(v) }))
        }
    }

    companion object {
        private val cache = ConcurrentHashMap<Class<*>, RecordInfo<*>>()

        fun of(c: Class<*>): RecordInfo<*> = cache.getOrPut(c) {
            @Suppress("UNCHECKED_CAST")
            RecordInfo(c as Class<Record>)
        }
    }
}

/** What the tables class is: its tables, in constructor order. */
internal class TablesInfo(val cls: Class<*>) {
    private val ctor: Constructor<*> by lazy {
        cls.constructors.firstOrNull { c -> c.genericParameterTypes.all { it is ParameterizedType && it.rawType == Table::class.java } }
            ?: throw Fault.bug("authoring: ${cls.name} needs a constructor of its tables")
    }

    val rows: KList<RowInfo<*>> by lazy {
        ctor.genericParameterTypes.map { t -> RowInfo.of((t as ParameterizedType).actualTypeArguments[0] as Class<*>) }
    }

    val tables: KList<dev.arkdb.Table> by lazy { rows.map { it.table } }

    /** The value a body's `db` is. */
    val instance: Any by lazy { ctor.newInstance(*rows.map { Table<Row<*>>(it) }.toTypedArray()) }
}

/** The kinds of the vocabulary's own types, from a Kotlin type (for `none<T>()`, a column, a helper). */
@PublishedApi
internal object Kinds {
    fun of(t: KType): Kind {
        val c = (t.classifier as? KClass<*>)?.java ?: throw Fault.bug("authoring: no kind for $t")
        fun arg(): KType = t.arguments.firstOrNull()?.type ?: throw Fault.bug("authoring: $t needs its type argument")
        return when {
            c == Bool::class.java -> Kind.KBool
            c == Int::class.java -> Kind.KInt
            c == Text::class.java -> Kind.TEXT
            c == Bytes::class.java -> Kind.KBytes
            c == Id::class.java -> Kind.KId(RowInfo.of((arg().classifier as KClass<*>).java).name)
            c == Opt::class.java -> Kind.KOpt(of(arg()))
            c == List::class.java -> Kind.KList(of(arg()))
            c == Split::class.java -> Kind.KSplit
            Row::class.java.isAssignableFrom(c) -> Kind.KRow(RowInfo.of(c))
            Record::class.java.isAssignableFrom(c) -> Kind.KRecord(RecordInfo.of(c))
            else -> throw Fault.bug("authoring: no kind for $t")
        }
    }
}

// Tables and their queries ------------------------------------------------------

/** A query over one table: `filter`, `orderBy`, `limit`, `with`, then `all()` or `first()`. */
public open class Query<T : Row<*>> internal constructor(
    internal val info: RowInfo<*>,
    internal val where: Pred<T>?,
    internal val order: KList<Order<T>>,
    internal val take: KInt?,
    internal val related: KList<Rel<T, *>>,
) {
    public fun filter(p: Pred<T>): Query<T> = Query(info, where?.and(p) ?: p, order, take, related)
    public fun orderBy(vararg o: Order<T>): Query<T> = Query(info, where, order + o, take, related)
    public fun limit(n: KInt): Query<T> = Query(info, where, order, n, related)
    public fun with(r: Rel<T, *>): Query<T> = Query(info, where, order, take, related + r)

    private fun relations(): KList<Pair<Rel<T, *>, Relation>> = related.map { r ->
        val child = RowInfo.of(r.child)
        val ref = child.table.refs.firstOrNull { it.table == info.name }
            ?: throw Fault.bug("authoring: ${child.name} does not reference ${info.name}")
        r to Relation(info.name, child.name, ref.column)
    }

    private fun kind(): Kind.KRow = Kind.KRow(
        info,
        relations().associate { (r, rel) -> r.name to (Ty.TList(RowInfo.of(r.child).rowTy) as Ty) },
    )

    private fun irPlan(n: KInt?): IR.Plan = IR.Plan(
        info.name,
        where?.ir(),
        order.map { it.column to it.dir },
        n,
        relations().map { (r, rel) -> IR.Related(r.name, rel, IR.Plan(rel.child, null, emptyList(), null, emptyList())) },
    )

    // Natively, the order is completed with the key as the verifier completes
    // the emitted plan's, so both answer a limit over the same total order.
    private fun nativePlan(n: KInt?): Plan {
        val cols = order.map { it.column to it.dir }
        return Plan(
            info.name,
            where?.native(),
            cols + info.table.key.filter { k -> cols.none { it.first == k } }.map { it to Dir.Asc },
            n,
            relations().map { (r, rel) ->
                val child = RowInfo.of(r.child)
                Related(r.name, rel, Plan(rel.child, null, child.table.key.map { it to Dir.Asc }, null, emptyList()))
            },
        )
    }

    /** `SLet s (ESelect plan)`; the rows. */
    public fun all(): List<T> {
        val run = Run.current()
        val k = Kind.KList(kind())
        if (run.native) {
            run.flush()
            return List(Term.N((run as Native).store.select(nativePlan(take))), k)
        }
        val s = (run as Emitting).let(Expr.Select(irPlan(take)))
        return List(Term.E(Expr.Var(s)), k)
    }

    /** `SLet s (ESelect plan{limit = 1})`, `SLet s' (EStd First [EVar s])`; `EVar s'`. */
    public fun first(): Opt<T> {
        val run = Run.current()
        val k = Kind.KOpt(kind())
        if (run.native) {
            run.flush()
            val rows = (run as Native).store.select(nativePlan(1)) as Value.VList
            return Opt(Term.N(rows.items.firstOrNull() ?: Value.VNull), k)
        }
        val e = run as Emitting
        val s = e.let(Expr.Select(irPlan(1)))
        return Opt(Term.E(Expr.Var(e.let(Expr.Std(dev.arkdb.StdFn.First, listOf(Expr.Var(s)))))), k)
    }
}

/** `db.<table>`: a query over all of it, and its writes. Reads and writes by key are the extensions below. */
public class Table<T : Row<*>> internal constructor(info: RowInfo<*>) : Query<T>(info, null, emptyList(), null, emptyList()) {
    /** `SInsert t row []`, or on a unique index with `.on(..)`. */
    public fun insert(row: T): Write<T> = write(false, row)

    /** `SUpsert t row []`, or on a unique index with `.on(..)`. */
    public fun upsert(row: T): Write<T> = write(true, row)

    private fun write(upsert: Boolean, row: T): Write<T> {
        val run = Run.current()
        run.flush()
        val p = Pending(upsert, info.name, row)
        // Under Emit the row's expression is taken now, before anything else is said.
        if (!run.native) termOf(row)
        run.pending = p
        return Write(p)
    }

    internal fun optOf(key: KList<Data>): Opt<T> {
        val run = Run.current()
        val k = Kind.KOpt(Kind.KRow(info))
        if (run.native) {
            run.flush()
            return Opt(Term.N((run as Native).store.get(info.name, key.map { valueOf(it) })), k)
        }
        val s = (run as Emitting).let(Expr.Get(info.name, key.map { exprOf(it) }))
        return Opt(Term.E(Expr.Var(s)), k)
    }

    internal fun existsOf(key: KList<Data>): Bool {
        val run = Run.current()
        if (run.native) {
            run.flush()
            return Bool(Term.N((run as Native).store.exists(info.name, key.map { valueOf(it) })))
        }
        val s = (run as Emitting).let(Expr.Exists(info.name, key.map { exprOf(it) }))
        return Bool(Term.E(Expr.Var(s)))
    }

    internal fun deleteOf(key: KList<Data>): Effect {
        val run = Run.current()
        if (run.native) {
            run.flush()
            (run as Native).store.delete(info.name, key.map { valueOf(it) })
        } else {
            (run as Emitting).stmt(Stmt.Delete(info.name, key.map { exprOf(it) }))
        }
        return Effect.DONE
    }

    internal fun updateOf(key: KList<Data>, f: (T) -> T): Effect {
        val run = Run.current()
        val k = Kind.KRow(info)
        if (run.native) {
            run.flush()
            Writes.update((run as Native).store, info.name, key.map { valueOf(it) }) { old ->
                @Suppress("UNCHECKED_CAST")
                valueOf(f(k.make(Term.N(old)) as T))
            }
            return Effect.DONE
        }
        val e = run as Emitting
        e.flush()
        val ks = key.map { exprOf(it) }
        val s = e.fresh()
        @Suppress("UNCHECKED_CAST")
        val row = e.expression { f(k.make(Term.E(Expr.Var(s))) as T) }
        e.stmt(Stmt.Update(info.name, ks, s, exprOf(row)))
        return Effect.DONE
    }
}

// By key: the key's shape is the row's `Key`, so a wrong order does not compile.

public fun <T : Row<Key1<A>>, A : Data> Table<T>.get(a: A): Opt<T> = optOf(listOf(a))
public fun <T : Row<Key2<A, B>>, A : Data, B : Data> Table<T>.get(a: A, b: B): Opt<T> = optOf(listOf(a, b))
public fun <T : Row<Key3<A, B, C>>, A : Data, B : Data, C : Data> Table<T>.get(a: A, b: B, c: C): Opt<T> = optOf(listOf(a, b, c))

public fun <T : Row<Key1<A>>, A : Data> Table<T>.exists(a: A): Bool = existsOf(listOf(a))
public fun <T : Row<Key2<A, B>>, A : Data, B : Data> Table<T>.exists(a: A, b: B): Bool = existsOf(listOf(a, b))
public fun <T : Row<Key3<A, B, C>>, A : Data, B : Data, C : Data> Table<T>.exists(a: A, b: B, c: C): Bool = existsOf(listOf(a, b, c))

public fun <T : Row<Key1<A>>, A : Data> Table<T>.delete(a: A): Effect = deleteOf(listOf(a))
public fun <T : Row<Key2<A, B>>, A : Data, B : Data> Table<T>.delete(a: A, b: B): Effect = deleteOf(listOf(a, b))
public fun <T : Row<Key3<A, B, C>>, A : Data, B : Data, C : Data> Table<T>.delete(a: A, b: B, c: C): Effect = deleteOf(listOf(a, b, c))

public fun <T : Row<Key1<A>>, A : Data> Table<T>.update(a: A, f: (T) -> T): Effect = updateOf(listOf(a), f)
public fun <T : Row<Key2<A, B>>, A : Data, B : Data> Table<T>.update(a: A, b: B, f: (T) -> T): Effect = updateOf(listOf(a, b), f)
