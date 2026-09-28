// AUTHORING.md §3: the two backends behind one API, chosen by the run a body
// is executed under. `Emitting` records IR — every closure runs once over
// fresh symbols, a read becomes an `SLet`, an auto is registered by name.
// `Native` executes against a store — reads run at once, a closure runs
// only when its branch is taken, an auto is the entry's. A body cannot
// tell which it is under.
package dev.arkdb.authoring

import dev.arkdb.Args
import dev.arkdb.Auto
import dev.arkdb.Expr
import dev.arkdb.Fault
import dev.arkdb.Stmt
import dev.arkdb.Store
import dev.arkdb.Value
import dev.arkdb.Writes
import java.util.IdentityHashMap

/** What a mutator's body, a guard's, and every control closure return: the statements already said. */
public open class Effect internal constructor() {
    internal companion object {
        val DONE: Effect = Effect()
    }
}

/** A write that may still be narrowed to a unique index with `.on(...)`. */
public class Write<T : Row<*>> internal constructor(private val pending: Pending) : Effect() {
    /** Match on these columns (a declared unique index) rather than the key. */
    public fun on(vararg cols: Col<T, *>): Effect {
        val run = Run.current()
        if (run.pending !== pending) throw Fault.bug("authoring: .on(..) must follow its insert or upsert directly")
        pending.on = cols.map { it.name }
        run.flush()
        return DONE
    }
}

/** An insert or upsert said but not yet carried out, because `.on` may still follow. */
internal class Pending(val upsert: Boolean, val table: String, val row: Data) {
    var on: KList<String> = emptyList()
}

internal abstract class Run {
    abstract val native: Boolean

    /** The term a row value was made from, so that handing it on hands on that term. */
    private val origins = IdentityHashMap<Any, Term>()

    var pending: Pending? = null

    fun origin(d: Any): Term? = origins[d]

    fun remember(d: Any, t: Term) {
        origins[d] = t
    }

    /** Carry out the pending write, if there is one: before any other statement, read or end. */
    abstract fun flush()

    companion object {
        private val current = ThreadLocal<Run?>()

        fun current(): Run = current.get() ?: throw Fault.bug("authoring: the vocabulary is used outside a module's run")

        fun <A> under(run: Run, f: () -> A): A {
            val before = current.get()
            current.set(run)
            try {
                return f()
            } finally {
                current.set(before)
            }
        }
    }
}

/** Executes: every read against `store`, every write into it. */
internal class Native(val store: Store, val autos: Args) : Run() {
    override val native: Boolean get() = true

    override fun flush() {
        val p = pending ?: return
        pending = null
        val row = valueOf(p.row)
        if (p.upsert) Writes.upsert(store, p.table, row, p.on) else Writes.insert(store, p.table, row, p.on)
    }

    fun auto(name: String): Value = autos[name] ?: throw Fault.bug("MissingAuto ${dev.arkdb.HsShow.text(name)}")
}

/** Records: statements into the innermost block, symbols fresh, autos as met. */
internal class Emitting : Run() {
    override val native: Boolean get() = false

    private var next = 0
    private val blocks = ArrayDeque<MutableList<Stmt>>()
    private var expressionDepth = 0
    val autos = LinkedHashMap<String, Auto>()
    /** No host shows a builder its names; the printer derives them (AUTHORING.md §6), so none are kept. */
    val names = HashMap<KInt, String>()

    fun fresh(): KInt = next++

    fun stmt(s: Stmt) {
        if (expressionDepth > 0) throw Fault.bug("authoring: a statement (a read, a write, a refusal) inside an expression's closure")
        flush()
        blocks.last().add(s)
    }

    /** `SLet s e`; the symbol. */
    fun let(e: Expr): KInt {
        flush()
        val s = fresh()
        stmt(Stmt.Let(s, e))
        return s
    }

    /** Run an expression's closure: no statement may be said inside it. */
    fun <A> expression(f: () -> A): A {
        expressionDepth++
        try {
            return f()
        } finally {
            expressionDepth--
        }
    }

    /** Run a statement closure into a block of its own. */
    fun block(f: () -> Unit): KList<Stmt> {
        flush()
        blocks.addLast(ArrayList())
        try {
            f()
            flush()
        } catch (t: Throwable) {
            blocks.removeLast()
            throw t
        }
        return blocks.removeLast()
    }

    override fun flush() {
        val p = pending ?: return
        pending = null
        val row = exprOf(p.row)
        blocks.last().add(if (p.upsert) Stmt.Upsert(p.table, row, p.on) else Stmt.Insert(p.table, row, p.on))
    }

    /**
     * Register an auto by name. Naming it again is reading the same frozen
     * value again — `ctx.now("added_ms")` in three rows of one entry is one
     * time — so a repeated name of the same kind is that auto; a repeated
     * name of another kind is an error.
     */
    fun auto(name: String, a: Auto) {
        val had = autos[name]
        when {
            had == null -> autos[name] = a
            had == a -> Unit
            else -> throw Fault.bug("authoring: the auto ${dev.arkdb.HsShow.text(name)} is drawn as $had and again as $a; one name is one auto")
        }
    }
}

// §2.4 Control ------------------------------------------------------------------

/** `SIf c t []`: the body only when `c` holds. */
public fun `when`(c: Bool, then: () -> Effect): Effect = branch(c, then, null)

/** `SIf c [] e`: the body only when `c` does not hold. */
public fun unless(c: Bool, els: () -> Effect): Effect = branch(c, null, els)

/** `SIf c t e`. */
public fun ifElse(c: Bool, then: () -> Effect, els: () -> Effect): Effect = branch(c, then, els)

private fun branch(c: Bool, then: (() -> Effect)?, els: (() -> Effect)?): Effect {
    val run = Run.current()
    if (run.native) {
        run.flush()
        val taken = if ((valueOf(c) as Value.VBool).value) then else els
        if (taken != null) {
            taken()
            run.flush()
        }
        return Effect.DONE
    }
    val e = run as Emitting
    val ce = exprOf(c)
    val t = if (then != null) e.block { then() } else emptyList()
    val f = if (els != null) e.block { els() } else emptyList()
    e.stmt(Stmt.If(ce, t, f))
    return Effect.DONE
}

/** `SFor x xs body`. */
public fun <T : Data> forEach(xs: List<T>, body: (T) -> Effect): Effect {
    val run = Run.current()
    if (run.native) {
        run.flush()
        for (v in (valueOf(xs) as Value.VList).items) {
            @Suppress("UNCHECKED_CAST")
            body(Kind.known(xs.of).make(Term.N(v)) as T)
            run.flush()
        }
        return Effect.DONE
    }
    val e = run as Emitting
    val over = exprOf(xs)
    val x = e.fresh()
    @Suppress("UNCHECKED_CAST")
    val b = e.block { body(Kind.known(xs.of).make(Term.E(Expr.Var(x))) as T) }
    e.stmt(Stmt.For(x, over, b))
    return Effect.DONE
}

/** `SRefuse (ELit why)`: the entry's verdict. */
public fun refuse(why: String): Effect {
    val run = Run.current()
    if (run.native) {
        run.flush()
        throw Fault.refuse(why)
    }
    (run as Emitting).stmt(Stmt.Refuse(Expr.Lit(Value.VText(why))))
    return Effect.DONE
}
