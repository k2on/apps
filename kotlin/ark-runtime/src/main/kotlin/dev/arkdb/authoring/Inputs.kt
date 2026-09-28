// AUTHORING.md §1.3 and §2.5: a procedure's input, as a class whose
// companion gives its schema — each field's type and checks, and checks
// over the whole input. The same schema is the IR's `fnInput`/`fnRefine`
// under Emit and the checks themselves under Native.
package dev.arkdb.authoring

import dev.arkdb.Args
import dev.arkdb.Check
import dev.arkdb.Eval
import dev.arkdb.Expr
import dev.arkdb.Fault
import dev.arkdb.Field
import dev.arkdb.Std
import dev.arkdb.Store
import dev.arkdb.Value
import java.lang.reflect.Constructor
import java.util.concurrent.ConcurrentHashMap

/** A procedure's input. Its companion is an `Input.Of` giving its schema; its constructor takes the fields in order. */
public interface Input {
    public interface Of<I : Input> {
        public fun schema(): Schema<I>
    }
}

/** One check, as the author wrote it. */
internal sealed class Checker {
    object Trim : Checker()
    class MinLen(val n: KInt, val why: String?) : Checker()
    class MaxLen(val n: KInt, val why: String?) : Checker()
    class Range(val lo: Long?, val hi: Long?, val why: String?) : Checker()
    class NonEmpty(val why: String?) : Checker()
    class Exists(val why: String?) : Checker()
    class Refine(val f: (Data) -> Bool, val why: String?) : Checker()
}

/** A field's type and its checks, built as `text().trim().min(1, "why")`. */
public class FieldSpec<V : Data> internal constructor(
    internal val kind: () -> Kind,
    internal val checks: KList<Checker>,
) {
    private fun and(c: Checker): FieldSpec<V> = FieldSpec(kind, checks + c)

    /** Text: normalised before every later check and before the body. */
    public fun trim(): FieldSpec<V> = and(Checker.Trim)

    /** Text: at least `n` code points. */
    public fun min(n: KInt): FieldSpec<V> = and(Checker.MinLen(n, null))

    /** Text: at most `n` code points. */
    public fun max(n: KInt): FieldSpec<V> = and(Checker.MaxLen(n, null))

    /** Int: `lo <= v <= hi`. */
    public fun range(lo: Long, hi: Long): FieldSpec<V> = and(Checker.Range(lo, hi, null))

    public fun atLeast(lo: Long): FieldSpec<V> = and(Checker.Range(lo, null, null))

    public fun atMost(hi: Long): FieldSpec<V> = and(Checker.Range(null, hi, null))

    /** List: at least one element. */
    public fun nonEmpty(): FieldSpec<V> = and(Checker.NonEmpty(null))

    /** Id: a row with that key exists. */
    public fun exists(): FieldSpec<V> = and(Checker.Exists(null))

    /** Any: the closure, over the field's value, holds. */
    public fun refine(f: (V) -> Bool): FieldSpec<V> {
        @Suppress("UNCHECKED_CAST")
        return and(Checker.Refine(f as (Data) -> Bool, null))
    }

    /** The message of the check just before, in place of its default. */
    public fun why(message: String): FieldSpec<V> {
        val last = checks.lastOrNull() ?: throw Fault.bug("authoring: .why(..) with no check before it")
        val c = when (last) {
            is Checker.Trim -> throw Fault.bug("authoring: trim cannot fail, so it has no message")
            is Checker.MinLen -> Checker.MinLen(last.n, message)
            is Checker.MaxLen -> Checker.MaxLen(last.n, message)
            is Checker.Range -> Checker.Range(last.lo, last.hi, message)
            is Checker.NonEmpty -> Checker.NonEmpty(message)
            is Checker.Exists -> Checker.Exists(message)
            is Checker.Refine -> Checker.Refine(last.f, message)
        }
        return FieldSpec(kind, checks.dropLast(1) + c)
    }
}

public fun text(): FieldSpec<Text> = FieldSpec({ Kind.TEXT }, emptyList())
public fun int(): FieldSpec<Int> = FieldSpec({ Kind.KInt }, emptyList())
public fun bool(): FieldSpec<Bool> = FieldSpec({ Kind.KBool }, emptyList())
public fun bytes(): FieldSpec<Bytes> = FieldSpec({ Kind.KBytes }, emptyList())
public fun enum(vararg variants: String): FieldSpec<Text> = FieldSpec({ Kind.KText(dev.arkdb.Ty.TEnum(variants.toList())) }, emptyList())

/** An id of `T`. */
public inline fun <reified T : Row<*>> id(): FieldSpec<Id<T>> = idIn(T::class.java)

@PublishedApi
internal fun <T : Row<*>> idIn(cls: Class<*>): FieldSpec<Id<T>> = FieldSpec({ Kind.KId(RowInfo.of(cls).name) }, emptyList())

/** An option of the field; its checks apply when it is `Some`. */
public fun <V : Data> opt(f: FieldSpec<V>): FieldSpec<Opt<V>> = FieldSpec({ Kind.KOpt(f.kind()) }, f.checks)

/** A list of the field's type; checks on the list go after (`.nonEmpty()`). */
public fun <V : Data> list(f: FieldSpec<V>): FieldSpec<List<V>> {
    if (f.checks.isNotEmpty()) throw Fault.bug("authoring: checks inside list(..) have nowhere to go; check the list itself")
    return FieldSpec({ Kind.KList(f.kind()) }, emptyList())
}

/** A named field of an input. */
public class FieldDef internal constructor(internal val name: String, internal val spec: FieldSpec<*>)

public fun field(name: String, spec: FieldSpec<*>): FieldDef = FieldDef(name, spec)

/** An input's schema: its fields in order, and checks over the whole of it. */
public class Schema<I : Input> internal constructor(
    internal val fields: KList<FieldDef>,
    internal val refines: KList<Pair<(I) -> Bool, String?>>,
) {
    /** A check over the whole input, run after every field's. */
    public fun refine(f: (I) -> Bool): Schema<I> = Schema(fields, refines + (f to null))

    /** The message of the refinement just before, in place of the default. */
    public fun why(message: String): Schema<I> {
        val last = refines.lastOrNull() ?: throw Fault.bug("authoring: .why(..) with no refine before it")
        return Schema(fields, refines.dropLast(1) + (last.first to message))
    }
}

/** `obj(field(..), field(..))`: an input's schema. */
public fun <I : Input> obj(vararg fields: FieldDef): Schema<I> = Schema(fields.toList(), emptyList())

// What an input class is, read once ----------------------------------------------

internal class InputInfo<I : Input> private constructor(val cls: Class<I>) {
    val schema: Schema<I> by lazy {
        @Suppress("UNCHECKED_CAST")
        ((cls.getField("Companion").get(null) as? Input.Of<I>) ?: throw Fault.bug("authoring: ${cls.name}'s companion is not an Input.Of"))
            .schema()
    }

    private val ctor: Constructor<*> by lazy {
        cls.constructors.firstOrNull { it.parameterCount == schema.fields.size && !it.isSynthetic }
            ?: throw Fault.bug("authoring: ${cls.name} needs a constructor of its ${schema.fields.size} fields in schema order")
    }

    val names: KList<String> get() = schema.fields.map { it.name }

    fun kind(name: String): Kind = schema.fields.first { it.name == name }.spec.kind()

    /** The input value over a term per field. */
    fun make(term: (String) -> Term): I {
        val args = schema.fields.map { f -> f.spec.kind().make(term(f.name)) }
        @Suppress("UNCHECKED_CAST")
        return ctor.newInstance(*args.toTypedArray()) as I
    }

    /** The IR's `fnInput` and `fnRefine`, emitted under the current run. */
    fun ir(e: Emitting, checks: Boolean): Pair<KList<Pair<String, Field>>, KList<Pair<Expr, String?>>> {
        val fields = schema.fields.map { f ->
            val k = f.spec.kind()
            val inner = (k as? Kind.KOpt)?.of ?: k
            val cs = if (!checks) emptyList() else f.spec.checks.map { c ->
                when (c) {
                    is Checker.Trim -> Check.Trim
                    is Checker.MinLen -> Check.MinLen(c.n, c.why)
                    is Checker.MaxLen -> Check.MaxLen(c.n, c.why)
                    is Checker.Range -> Check.Range(c.lo, c.hi, c.why)
                    is Checker.NonEmpty -> Check.NonEmpty(c.why)
                    is Checker.Exists -> Check.Exists(c.why)
                    is Checker.Refine -> Check.Refine(exprOf(e.expression { c.f(inner.make(Term.E(Expr.Arg(f.name)))) }), c.why)
                }
            }
            f.name to Field(k.ty, cs)
        }
        val refine = if (!checks) emptyList() else schema.refines.map { (f, why) ->
            exprOf(e.expression { f(make { Term.E(Expr.Arg(it)) }) }) to why
        }
        return fields to refine
    }

    /**
     * The checks, natively: each field's in order (trim first where it is), a
     * failure refusing with its message; then the whole-input ones. The input
     * as the body sees it.
     */
    fun check(args: Args, st: Store): Args {
        val out = LinkedHashMap(args)
        for (f in schema.fields) {
            var v = out[f.name] ?: throw Fault.bug("MissingArg ${dev.arkdb.HsShow.text(f.name)}")
            if (v is Value.VNull) continue
            val k = f.spec.kind()
            val inner = (k as? Kind.KOpt)?.of ?: k
            for (c in f.spec.checks) {
                val (ok, irc) = when (c) {
                    is Checker.Trim -> {
                        v = Std.trim(v)
                        true to Check.Trim
                    }
                    is Checker.MinLen -> ((Std.textLen(v) as Value.VInt).value >= c.n) to Check.MinLen(c.n, c.why)
                    is Checker.MaxLen -> ((Std.textLen(v) as Value.VInt).value <= c.n) to Check.MaxLen(c.n, c.why)
                    is Checker.Range -> {
                        val n = (v as Value.VInt).value
                        ((c.lo == null || n >= c.lo) && (c.hi == null || n <= c.hi)) to Check.Range(c.lo, c.hi, c.why)
                    }
                    is Checker.NonEmpty -> (v as Value.VList).items.isNotEmpty() to Check.NonEmpty(c.why)
                    is Checker.Exists -> {
                        val t = (inner as Kind.KId).table
                        (st.exists(t, listOf(v)) as Value.VBool).value to Check.Exists(c.why)
                    }
                    is Checker.Refine -> (valueOf(c.f(inner.make(Term.N(v)))) as Value.VBool).value to Check.Refine(dev.arkdb.Expr.Lit(Value.VBool(true)), c.why)
                }
                if (!ok) throw Fault.refuse(irc.why ?: Eval.defaultMessage(f.name, irc, (inner as? Kind.KId)?.table))
            }
            out[f.name] = v
        }
        for ((f, why) in schema.refines) {
            val ok = (valueOf(f(make { Term.N(out.getValue(it)) })) as Value.VBool).value
            if (!ok) throw Fault.refuse(why ?: Eval.DEFAULT_REFINE_MESSAGE)
        }
        return out
    }

    companion object {
        private val cache = ConcurrentHashMap<Class<*>, InputInfo<*>>()

        fun of(c: Class<*>): InputInfo<*> = cache.getOrPut(c) {
            @Suppress("UNCHECKED_CAST")
            InputInfo(c as Class<Input>)
        }
    }
}
