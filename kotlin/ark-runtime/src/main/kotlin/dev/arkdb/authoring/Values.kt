// AUTHORING.md §2.1–2.2: the value types of the vocabulary and the
// operations on them. Each value holds a `Term`: under Native the `Value`
// itself, under Emit the `Expr` that computes it (a literal is a known value
// in both). An operation asks the ambient run which it is building, so one
// body text is both the program and its IR.
package dev.arkdb.authoring

import dev.arkdb.CmpOp
import dev.arkdb.Expr
import dev.arkdb.Fault
import dev.arkdb.Op
import dev.arkdb.Ops
import dev.arkdb.Std
import dev.arkdb.StdFn
import dev.arkdb.Ty
import dev.arkdb.Value
import dev.arkdb.compareValue

internal typealias KInt = kotlin.Int
internal typealias KList<T> = kotlin.collections.List<T>

/** How a value is held: known (Native, or a literal under Emit), or an expression (Emit). */
internal sealed class Term {
    class N(val v: Value) : Term()
    class E(val e: Expr) : Term()
}

/** What a value is, enough to hold one of its type again: its IR type and how to wrap a term. */
internal sealed class Kind {
    abstract val ty: Ty
    abstract fun make(t: Term): Data

    object KBool : Kind() {
        override val ty: Ty get() = Ty.TBool
        override fun make(t: Term): Data = Bool(t)
    }

    object KInt : Kind() {
        override val ty: Ty get() = Ty.TInt
        override fun make(t: Term): Data = Int(t)
    }

    /** Text, or an enum's value (a text of the enum's type). */
    class KText(override val ty: Ty) : Kind() {
        override fun make(t: Term): Data = Text(t, this)
    }

    object KBytes : Kind() {
        override val ty: Ty get() = Ty.TBytes
        override fun make(t: Term): Data = Bytes(t)
    }

    class KId(val table: String) : Kind() {
        override val ty: Ty get() = Ty.TId(table)
        override fun make(t: Term): Data = Id<Row<*>>(t, this)
    }

    /** An option; `of` is unknown only for a None computed natively, where nothing asks. */
    class KOpt(val of: Kind?) : Kind() {
        override val ty: Ty get() = Ty.TOption(known(of).ty)
        override fun make(t: Term): Data = Opt<Data>(t, this)
    }

    class KList(val of: Kind?) : Kind() {
        override val ty: Ty get() = Ty.TList(known(of).ty)
        override fun make(t: Term): Data = List<Data>(t, this)
    }

    /** A row of a table; `extra` are the relationships a plan attached beneath it. */
    class KRow(val info: RowInfo<*>, val extra: Map<String, Ty> = emptyMap()) : Kind() {
        override val ty: Ty get() = Ty.TStruct(info.rowTy.fields + extra)
        override fun make(t: Term): Data = info.make(t, this)
    }

    object KSplit : Kind() {
        override val ty: Ty get() = Ty.TStruct(mapOf("before" to Ty.TText, "after" to Ty.TText))
        override fun make(t: Term): Data = Split(t)
    }

    companion object {
        val TEXT: KText = KText(Ty.TText)

        fun known(k: Kind?): Kind = k ?: throw Fault.bug("authoring: the type of an empty value is not known here")

        /** The kind of an IR type, for a column or a field: rows are found by table name. */
        fun ofTy(t: Ty, rowOf: (Ty.TStruct) -> Kind? = { null }): Kind = when (t) {
            is Ty.TBool -> KBool
            is Ty.TInt -> KInt
            is Ty.TText -> TEXT
            is Ty.TEnum -> KText(t)
            is Ty.TBytes -> KBytes
            is Ty.TId -> KId(t.table)
            is Ty.TOption -> KOpt(ofTy(t.of, rowOf))
            is Ty.TList -> KList(ofTy(t.of, rowOf))
            is Ty.TStruct -> rowOf(t) ?: throw Fault.bug("authoring: no row type for $t")
        }
    }
}

/** A value of the vocabulary: one of the types below, or a row. */
public interface Data

/** The scalar and collection values: a term and what it is. */
public abstract class Val internal constructor(internal val term: Term, internal val kind: Kind) : Data {
    override fun toString(): String = when (val t = term) {
        is Term.N -> t.v.show()
        is Term.E -> t.e.toString()
    }
}

// Terms of any value, rows included ------------------------------------------

internal fun kindOf(d: Data): Kind = when (d) {
    is Val -> d.kind
    is Row<*> -> Kind.KRow(RowInfo.of(d.javaClass))
    else -> throw Fault.bug("authoring: not a value of the vocabulary: ${d.javaClass.name}")
}

internal fun termOf(d: Data): Term = when (d) {
    is Val -> d.term
    is Row<*> -> Run.current().origin(d) ?: RowInfo.of(d.javaClass).termOfFields(d)
    else -> throw Fault.bug("authoring: not a value of the vocabulary: ${d.javaClass.name}")
}

/** The expression a value is, under Emit. A known None needs its type, which the kind carries. */
internal fun exprOf(d: Data): Expr = exprOf(termOf(d), kindOf(d))

internal fun exprOf(t: Term, k: Kind): Expr = when (t) {
    is Term.E -> t.e
    is Term.N -> if (t.v is Value.VNull) Expr.None((k as? Kind.KOpt)?.of?.ty ?: Ty.TText) else Expr.Lit(t.v)
}

/** The value a value is, under Native. */
internal fun valueOf(d: Data): Value = when (val t = termOf(d)) {
    is Term.N -> t.v
    is Term.E -> throw Fault.bug("authoring: an expression where a value was expected (a closure captured an emitted value?)")
}

// Building a result: natively, or as an expression.
internal inline fun <R : Data> build(k: Kind, native: () -> Value, emit: () -> Expr): R {
    @Suppress("UNCHECKED_CAST")
    return k.make(if (Run.current().native) Term.N(native()) else Term.E(emit())) as R
}

internal fun std(k: Kind, f: StdFn, vararg xs: Data): Data =
    build(k, { Std.call(f, xs.map { valueOf(it) }) }, { Expr.Std(f, xs.map { exprOf(it) }) })

internal fun cmp(op: CmpOp, a: Data, b: Data): Bool =
    build(Kind.KBool, { Value.VBool(op.holds(compareValue(valueOf(a), valueOf(b)))) }, { Expr.Cmp(op, exprOf(a), exprOf(b)) })

internal fun arith(op: Op, a: Data, b: Data): Int = build(
    Kind.KInt,
    {
        when (op) {
            Op.Add -> Ops.add(valueOf(a), valueOf(b))
            Op.Sub -> Ops.sub(valueOf(a), valueOf(b))
            Op.Mul -> Ops.mul(valueOf(a), valueOf(b))
            Op.Div -> Ops.div(valueOf(a), valueOf(b))
            else -> Ops.mod(valueOf(a), valueOf(b))
        }
    },
    { Expr.Op(op, listOf(exprOf(a), exprOf(b))) },
)

// Literals lift ----------------------------------------------------------------

internal fun lit(b: Boolean): Bool = Bool(Term.N(Value.VBool(b)))
internal fun lit(n: Long): Int = Int(Term.N(Value.VInt(n)))
internal fun lit(n: KInt): Int = lit(n.toLong())
internal fun lit(s: String): Text = Text(Term.N(Value.VText(s)), Kind.TEXT)

// The types ------------------------------------------------------------------

public class Bool internal constructor(term: Term) : Val(term, Kind.KBool) {
    public fun and(b: Bool): Bool = build(Kind.KBool, {
        if ((valueOf(this) as Value.VBool).value) valueOf(b) else Value.VBool(false)
    }, { Expr.Op(Op.And, listOf(exprOf(this), exprOf(b))) })

    public fun or(b: Bool): Bool = build(Kind.KBool, {
        if ((valueOf(this) as Value.VBool).value) Value.VBool(true) else valueOf(b)
    }, { Expr.Op(Op.Or, listOf(exprOf(this), exprOf(b))) })

    public fun not(): Bool = build(Kind.KBool, { Ops.not(valueOf(this)) }, { Expr.Op(Op.Not, listOf(exprOf(this))) })

    public fun and(b: Boolean): Bool = and(lit(b))
    public fun or(b: Boolean): Bool = or(lit(b))
    public fun eq(b: Bool): Bool = cmp(CmpOp.Eq, this, b)
    public fun ne(b: Bool): Bool = cmp(CmpOp.Ne, this, b)
    public fun eq(b: Boolean): Bool = eq(lit(b))
    public fun ne(b: Boolean): Bool = ne(lit(b))
}

public class Int internal constructor(term: Term) : Val(term, Kind.KInt) {
    public fun add(b: Int): Int = arith(Op.Add, this, b)
    public fun sub(b: Int): Int = arith(Op.Sub, this, b)
    public fun mul(b: Int): Int = arith(Op.Mul, this, b)
    public fun div(b: Int): Int = arith(Op.Div, this, b)
    public fun rem(b: Int): Int = arith(Op.Mod, this, b)
    public fun neg(): Int = build(Kind.KInt, { Ops.neg(valueOf(this)) }, { Expr.Op(Op.Neg, listOf(exprOf(this))) })
    public fun add(b: KInt): Int = add(lit(b))
    public fun sub(b: KInt): Int = sub(lit(b))
    public fun mul(b: KInt): Int = mul(lit(b))
    public fun div(b: KInt): Int = div(lit(b))
    public fun rem(b: KInt): Int = rem(lit(b))
    public fun add(b: Long): Int = add(lit(b))
    public fun sub(b: Long): Int = sub(lit(b))
    public fun mul(b: Long): Int = mul(lit(b))

    public fun eq(b: Int): Bool = cmp(CmpOp.Eq, this, b)
    public fun ne(b: Int): Bool = cmp(CmpOp.Ne, this, b)
    public fun lt(b: Int): Bool = cmp(CmpOp.Lt, this, b)
    public fun le(b: Int): Bool = cmp(CmpOp.Le, this, b)
    public fun gt(b: Int): Bool = cmp(CmpOp.Gt, this, b)
    public fun ge(b: Int): Bool = cmp(CmpOp.Ge, this, b)
    public fun eq(b: KInt): Bool = eq(lit(b))
    public fun ne(b: KInt): Bool = ne(lit(b))
    public fun lt(b: KInt): Bool = lt(lit(b))
    public fun le(b: KInt): Bool = le(lit(b))
    public fun gt(b: KInt): Bool = gt(lit(b))
    public fun ge(b: KInt): Bool = ge(lit(b))

    public fun min(b: Int): Int = std(Kind.KInt, StdFn.Min, this, b) as Int
    public fun max(b: Int): Int = std(Kind.KInt, StdFn.Max, this, b) as Int
    public fun min(b: KInt): Int = min(lit(b))
    public fun max(b: KInt): Int = max(lit(b))
    public fun clamp(lo: Int, hi: Int): Int = std(Kind.KInt, StdFn.Clamp, this, lo, hi) as Int
    public fun clamp(lo: KInt, hi: KInt): Int = clamp(lit(lo), lit(hi))
    public fun abs(): Int = std(Kind.KInt, StdFn.Abs, this) as Int
    public fun toText(): Text = std(Kind.TEXT, StdFn.TextOfInt, this) as Text
}

public class Text internal constructor(term: Term, kind: Kind) : Val(term, kind) {
    public fun trim(): Text = std(Kind.TEXT, StdFn.Trim, this) as Text
    public fun isEmpty(): Bool = std(Kind.KBool, StdFn.IsEmpty, this) as Bool
    public fun lower(): Text = std(Kind.TEXT, StdFn.Lower, this) as Text
    public fun len(): Int = std(Kind.KInt, StdFn.TextLen, this) as Int
    public fun startsWith(p: Text): Bool = std(Kind.KBool, StdFn.StartsWith, this, p) as Bool
    public fun startsWith(p: String): Bool = startsWith(lit(p))
    public fun splitOnce(p: Text): Opt<Split> = std(Kind.KOpt(Kind.KSplit), StdFn.SplitOnce, this, p) as Opt<Split>
    public fun splitOnce(p: String): Opt<Split> = splitOnce(lit(p))
    public fun chars(): List<Text> = std(Kind.KList(Kind.TEXT), StdFn.Chars, this) as List<Text>
    public fun isAlnum(): Bool = std(Kind.KBool, StdFn.IsAlnum, this) as Bool
    public fun utf8(): Bytes = std(Kind.KBytes, StdFn.Utf8, this) as Bytes
    public fun fnv1a64(): Int = std(Kind.KInt, StdFn.Fnv1a64, this) as Int

    public fun eq(b: Text): Bool = cmp(CmpOp.Eq, this, b)
    public fun ne(b: Text): Bool = cmp(CmpOp.Ne, this, b)
    public fun lt(b: Text): Bool = cmp(CmpOp.Lt, this, b)
    public fun le(b: Text): Bool = cmp(CmpOp.Le, this, b)
    public fun gt(b: Text): Bool = cmp(CmpOp.Gt, this, b)
    public fun ge(b: Text): Bool = cmp(CmpOp.Ge, this, b)
    public fun eq(b: String): Bool = eq(lit(b))
    public fun ne(b: String): Bool = ne(lit(b))
    public fun lt(b: String): Bool = lt(lit(b))
    public fun le(b: String): Bool = le(lit(b))
    public fun gt(b: String): Bool = gt(lit(b))
    public fun ge(b: String): Bool = ge(lit(b))
}

public class Bytes internal constructor(term: Term) : Val(term, Kind.KBytes) {
    public fun hex(): Text = std(Kind.TEXT, StdFn.Hex, this) as Text
    public fun sha256(): Bytes = std(Kind.KBytes, StdFn.Sha256, this) as Bytes
    public fun eq(b: Bytes): Bool = cmp(CmpOp.Eq, this, b)
    public fun ne(b: Bytes): Bool = cmp(CmpOp.Ne, this, b)
}

/** An id of a row of `T`. */
public class Id<T : Row<*>> internal constructor(term: Term, kind: Kind) : Val(term, kind) {
    public fun toText(): Text = std(Kind.TEXT, StdFn.TextOfId, this) as Text
    public fun eq(b: Id<T>): Bool = cmp(CmpOp.Eq, this, b)
    public fun ne(b: Id<T>): Bool = cmp(CmpOp.Ne, this, b)
    public fun lt(b: Id<T>): Bool = cmp(CmpOp.Lt, this, b)
    public fun le(b: Id<T>): Bool = cmp(CmpOp.Le, this, b)
    public fun gt(b: Id<T>): Bool = cmp(CmpOp.Gt, this, b)
    public fun ge(b: Id<T>): Bool = cmp(CmpOp.Ge, this, b)
}

/** `SplitOnce`'s answer: the text before the separator and after it. */
public class Split internal constructor(term: Term) : Val(term, Kind.KSplit) {
    public val before: Text get() = field("before")
    public val after: Text get() = field("after")

    private fun field(n: String): Text = build(Kind.TEXT, {
        (valueOf(this) as Value.VStruct)[n]
    }, { Expr.Field(exprOf(this), n) })
}

/** An option of `T`. Flat, as the IR's is: `Some v` is `v` and `None` is null. */
public class Opt<T : Data> internal constructor(term: Term, kind: Kind) : Val(term, kind) {
    internal val of: Kind? get() = (kind as Kind.KOpt).of

    private fun some(): T? {
        val v = valueOf(this)
        @Suppress("UNCHECKED_CAST")
        return if (v is Value.VNull) null else Kind.known(of).make(Term.N(v)) as T
    }

    public fun isSome(): Bool = std(Kind.KBool, StdFn.IsSome, this) as Bool

    /** `EStd UnwrapOr [opt, d]`. */
    public fun unwrapOr(d: T): T {
        @Suppress("UNCHECKED_CAST")
        return std(of ?: kindOf(d), StdFn.UnwrapOr, this, d) as T
    }

    /** `EMatch opt x e d`: `f` of the value, or `d`. */
    public fun <R : Data> mapOr(d: R, f: (T) -> R): R {
        val run = Run.current()
        if (run.native) return some()?.let(f) ?: d
        val e = run as Emitting
        val x = e.fresh()
        @Suppress("UNCHECKED_CAST")
        val r = e.expression { f(Kind.known(of).make(Term.E(Expr.Var(x))) as T) }
        @Suppress("UNCHECKED_CAST")
        return kindOf(r).make(Term.E(Expr.Match(exprOf(this), x, exprOf(r), exprOf(d)))) as R
    }

    public fun mapOr(d: KInt, f: (T) -> Int): Int = mapOr(lit(d), f)
    public fun mapOr(d: Long, f: (T) -> Int): Int = mapOr(lit(d), f)
    public fun mapOr(d: String, f: (T) -> Text): Text = mapOr(lit(d), f)
    public fun mapOr(d: Boolean, f: (T) -> Bool): Bool = mapOr(lit(d), f)

    /** `EMatch opt x (ESome e) (ENone R)`. */
    public fun <R : Data> map(f: (T) -> R): Opt<R> {
        val run = Run.current()
        if (run.native) {
            val s = some() ?: return Opt(Term.N(Value.VNull), Kind.KOpt(null))
            val r = f(s)
            return Opt(Term.N(valueOf(r)), Kind.KOpt(kindOf(r)))
        }
        val e = run as Emitting
        val x = e.fresh()
        @Suppress("UNCHECKED_CAST")
        val r = e.expression { f(Kind.known(of).make(Term.E(Expr.Var(x))) as T) }
        val rk = kindOf(r)
        return Opt(Term.E(Expr.Match(exprOf(this), x, Expr.Some(exprOf(r)), Expr.None(rk.ty))), Kind.KOpt(rk))
    }

    /** `EMatch opt x (EIf p (ESome (EVar x)) (ENone T)) (ENone T)`. */
    public fun filter(p: (T) -> Bool): Opt<T> {
        val run = Run.current()
        if (run.native) {
            val s = some() ?: return this
            return if ((valueOf(p(s)) as Value.VBool).value) this else Opt(Term.N(Value.VNull), kind)
        }
        val e = run as Emitting
        val x = e.fresh()
        @Suppress("UNCHECKED_CAST")
        val c = e.expression { p(Kind.known(of).make(Term.E(Expr.Var(x))) as T) }
        val t = Kind.known(of).ty
        return Opt(
            Term.E(Expr.Match(exprOf(this), x, Expr.If(exprOf(c), Expr.Some(Expr.Var(x)), Expr.None(t)), Expr.None(t))),
            kind,
        )
    }

    /**
     * The value, or a refusal with `why`: `SLet s opt`,
     * `SIf (EStd IsSome [EVar s]) [] [SRefuse (ELit why)]`, `EStd Unwrap [EVar s]`.
     */
    public fun orRefuse(why: String): T {
        val run = Run.current()
        if (run.native) {
            run.flush()
            return some() ?: throw Fault.refuse(why)
        }
        val e = run as Emitting
        val s = e.let(exprOf(this))
        e.stmt(dev.arkdb.Stmt.If(Expr.Std(StdFn.IsSome, listOf(Expr.Var(s))), emptyList(), listOf(dev.arkdb.Stmt.Refuse(Expr.Lit(Value.VText(why))))))
        @Suppress("UNCHECKED_CAST")
        return Kind.known(of).make(Term.E(Expr.Std(StdFn.Unwrap, listOf(Expr.Var(s))))) as T
    }
}

/** A list of `T`. */
public class List<T : Data> internal constructor(term: Term, kind: Kind) : Val(term, kind) {
    internal val of: Kind? get() = (kind as Kind.KList).of

    private fun items(): KList<T> {
        val vs = (valueOf(this) as Value.VList).items
        @Suppress("UNCHECKED_CAST")
        return vs.map { Kind.known(of).make(Term.N(it)) as T }
    }

    public fun first(): Opt<T> {
        @Suppress("UNCHECKED_CAST")
        return std(Kind.KOpt(of), StdFn.First, this) as Opt<T>
    }

    public fun last(): Opt<T> {
        @Suppress("UNCHECKED_CAST")
        return std(Kind.KOpt(of), StdFn.Last, this) as Opt<T>
    }

    public fun len(): Int = std(Kind.KInt, StdFn.Len, this) as Int
    public fun contains(x: T): Bool = std(Kind.KBool, StdFn.Contains, this, x) as Bool

    public fun reverse(): List<T> {
        @Suppress("UNCHECKED_CAST")
        return std(kind, StdFn.Reverse, this) as List<T>
    }

    // One closure over each element: natively, or once over a fresh symbol.
    private fun <R : Data> each(f: (T) -> R): Pair<KInt, R> {
        val e = Run.current() as Emitting
        val x = e.fresh()
        @Suppress("UNCHECKED_CAST")
        return x to e.expression { f(Kind.known(of).make(Term.E(Expr.Var(x))) as T) }
    }

    public fun <R : Data> map(f: (T) -> R): List<R> {
        if (Run.current().native) {
            val rs = items().map(f)
            return List(Term.N(Value.VList(rs.map { valueOf(it) })), Kind.KList(rs.firstOrNull()?.let { kindOf(it) }))
        }
        val (x, r) = each(f)
        return List(Term.E(Expr.Map(exprOf(this), x, exprOf(r))), Kind.KList(kindOf(r)))
    }

    public fun filter(p: (T) -> Bool): List<T> {
        if (Run.current().native) {
            val keep = items().filter { (valueOf(p(it)) as Value.VBool).value }
            return List(Term.N(Value.VList(keep.map { valueOf(it) })), kind)
        }
        val (x, r) = each(p)
        return List(Term.E(Expr.Filter(exprOf(this), x, exprOf(r))), kind)
    }

    public fun any(p: (T) -> Bool): Bool {
        if (Run.current().native) {
            var r = false
            for (it in items()) if ((valueOf(p(it)) as Value.VBool).value) r = true
            return lit(r)
        }
        val (x, r) = each(p)
        return Bool(Term.E(Expr.Any(exprOf(this), x, exprOf(r))))
    }

    public fun all(p: (T) -> Bool): Bool {
        if (Run.current().native) {
            var r = true
            for (it in items()) if (!(valueOf(p(it)) as Value.VBool).value) r = false
            return lit(r)
        }
        val (x, r) = each(p)
        return Bool(Term.E(Expr.All(exprOf(this), x, exprOf(r))))
    }

    public fun <K : Data> sortBy(key: (T) -> K): List<T> {
        if (Run.current().native) {
            val keyed = items().map { it to valueOf(key(it)) }
            val sorted = keyed.sortedWith { a, b -> compareValue(a.second, b.second) }.map { valueOf(it.first) }
            return List(Term.N(Value.VList(sorted)), kind)
        }
        val (x, r) = each(key)
        return List(Term.E(Expr.SortBy(exprOf(this), x, exprOf(r))), kind)
    }

    /** `EFold xs init acc x body`. */
    public fun <A : Data> fold(init: A, f: (A, T) -> A): A {
        val run = Run.current()
        if (run.native) {
            var acc = init
            for (it in items()) acc = f(acc, it)
            return acc
        }
        val e = run as Emitting
        val a = e.fresh()
        val x = e.fresh()
        val ak = kindOf(init)
        @Suppress("UNCHECKED_CAST")
        val r = e.expression { f(ak.make(Term.E(Expr.Var(a))) as A, Kind.known(of).make(Term.E(Expr.Var(x))) as T) }
        @Suppress("UNCHECKED_CAST")
        return ak.make(Term.E(Expr.Fold(exprOf(this), exprOf(init), a, x, exprOf(r)))) as A
    }

    public fun fold(init: KInt, f: (Int, T) -> Int): Int = fold(lit(init), f)
    public fun fold(init: String, f: (Text, T) -> Text): Text = fold(lit(init), f)
    public fun fold(init: Boolean, f: (Bool, T) -> Bool): Bool = fold(lit(init), f)
}

// Free functions of §2.2 --------------------------------------------------------

/** `ESome x`. */
public fun <T : Data> some(x: T): Opt<T> =
    build(Kind.KOpt(kindOf(x)), { valueOf(x) }, { Expr.Some(exprOf(x)) })

public fun some(x: String): Opt<Text> = some(lit(x))
public fun some(x: KInt): Opt<Int> = some(lit(x))

/** `ENone T`. */
public inline fun <reified T : Data> none(): Opt<T> = noneOf(kotlin.reflect.typeOf<T>())

@PublishedApi
internal fun <T : Data> noneOf(t: kotlin.reflect.KType): Opt<T> {
    val k = Kinds.of(t)
    return build(Kind.KOpt(k), { Value.VNull }, { Expr.None(k.ty) })
}

/** `EList [a, b, …]`; at least one element, whose type is the list's. */
public fun <T : Data> list(first: T, vararg rest: T): List<T> {
    val xs = listOf(first) + rest
    return build(Kind.KList(kindOf(first)), { Value.VList(xs.map { valueOf(it) }) }, { Expr.ListE(xs.map { exprOf(it) }) })
}

public fun list(first: String, vararg rest: String): List<Text> = list(lit(first), *rest.map { lit(it) }.toTypedArray())

/** `EStd Concat [list]`. */
public fun concat(xs: List<Text>): Text = std(Kind.TEXT, StdFn.Concat, xs) as Text

/** `EStd IdOfText [t]`: an id of `T`, if the text is one. */
public inline fun <reified T : Row<*>> idOfText(t: Text): Opt<Id<T>> = idOfTextIn(t, T::class.java)

@PublishedApi
internal fun <T : Row<*>> idOfTextIn(t: Text, cls: Class<*>): Opt<Id<T>> {
    @Suppress("UNCHECKED_CAST")
    return std(Kind.KOpt(Kind.KId(RowInfo.of(cls).name)), StdFn.IdOfText, t) as Opt<Id<T>>
}

/** `EStd NilId []`. */
public inline fun <reified T : Row<*>> nilId(): Id<T> = nilIdIn(T::class.java)

@PublishedApi
internal fun <T : Row<*>> nilIdIn(cls: Class<*>): Id<T> {
    @Suppress("UNCHECKED_CAST")
    return std(Kind.KId(RowInfo.of(cls).name), StdFn.NilId) as Id<T>
}

/** `EIf c a b`: a value chosen, both sides evaluated as values. */
public fun <R : Data> pick(c: Bool, a: R, b: R): R = build(
    kindOf(a),
    { if ((valueOf(c) as Value.VBool).value) valueOf(a) else valueOf(b) },
    { Expr.If(exprOf(c), exprOf(a), exprOf(b)) },
)

public fun pick(c: Bool, a: KInt, b: KInt): Int = pick(c, lit(a), lit(b))
public fun pick(c: Bool, a: String, b: String): Text = pick(c, lit(a), lit(b))
public fun pick(c: Bool, a: Boolean, b: Boolean): Bool = pick(c, lit(a), lit(b))
