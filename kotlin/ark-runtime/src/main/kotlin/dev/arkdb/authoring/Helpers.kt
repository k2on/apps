// AUTHORING.md §2.2 `ECall`: a helper is an ordinary Kotlin function of the
// vocabulary's types whose body is `helper(..)`. Called under Emit it is
// `ECall name args`, and its first call in a module build emits the helper
// itself (its body over `EArg`s of its parameters) into the module,
// immediately before the first function that calls it — so a helper a
// helper calls comes first. Under Native it is its body, run on the values.
//
//     fun movementKey(workId: Text, no: Int): Text =
//         helper("movement_key", "work_id" to workId, "no" to no) { workId, no ->
//             concat(list(workId, lit("#"), no.toText()))
//         }
//
// The names are the helper's parameters in the IR (`fnInput`), written once
// beside the values; the types are the Kotlin types of the values and of
// the result, as written, so a helper's signature does not depend on what a
// caller happens to pass. A helper reads nothing and refuses nothing (the
// verifier holds it to that); a body that captures a value from its caller
// rather than taking it as a parameter emits a symbol or an argument its own
// function does not have, and the verifier says so.
package dev.arkdb.authoring

import dev.arkdb.Eval
import dev.arkdb.Expr
import dev.arkdb.Fault
import dev.arkdb.Field
import dev.arkdb.FnKind
import dev.arkdb.Function
import dev.arkdb.MemoryStore
import dev.arkdb.Schema as IrSchema
import dev.arkdb.Stmt
import kotlin.reflect.KType
import kotlin.reflect.typeOf

/** One parameter of a helper: its name in the IR, the value passed, and its Kotlin type. */
@PublishedApi
internal class Param(val name: String, val value: Data, val type: KType)

public inline fun <reified A : Data, reified R : Data> helper(name: String, a: Pair<String, A>, noinline body: (A) -> R): R {
    @Suppress("UNCHECKED_CAST")
    return helperIn(name, listOf(Param(a.first, a.second, typeOf<A>())), typeOf<R>()) { xs -> body(xs[0] as A) }
}

public inline fun <reified A : Data, reified B : Data, reified R : Data> helper(
    name: String,
    a: Pair<String, A>,
    b: Pair<String, B>,
    noinline body: (A, B) -> R,
): R {
    @Suppress("UNCHECKED_CAST")
    return helperIn(name, listOf(Param(a.first, a.second, typeOf<A>()), Param(b.first, b.second, typeOf<B>())), typeOf<R>()) { xs ->
        body(xs[0] as A, xs[1] as B)
    }
}

public inline fun <reified A : Data, reified B : Data, reified C : Data, reified R : Data> helper(
    name: String,
    a: Pair<String, A>,
    b: Pair<String, B>,
    c: Pair<String, C>,
    noinline body: (A, B, C) -> R,
): R {
    @Suppress("UNCHECKED_CAST")
    return helperIn(
        name,
        listOf(Param(a.first, a.second, typeOf<A>()), Param(b.first, b.second, typeOf<B>()), Param(c.first, c.second, typeOf<C>())),
        typeOf<R>(),
    ) { xs -> body(xs[0] as A, xs[1] as B, xs[2] as C) }
}

public inline fun <reified A : Data, reified B : Data, reified C : Data, reified D : Data, reified R : Data> helper(
    name: String,
    a: Pair<String, A>,
    b: Pair<String, B>,
    c: Pair<String, C>,
    d: Pair<String, D>,
    noinline body: (A, B, C, D) -> R,
): R {
    @Suppress("UNCHECKED_CAST")
    return helperIn(
        name,
        listOf(
            Param(a.first, a.second, typeOf<A>()),
            Param(b.first, b.second, typeOf<B>()),
            Param(c.first, c.second, typeOf<C>()),
            Param(d.first, d.second, typeOf<D>()),
        ),
        typeOf<R>(),
    ) { xs -> body(xs[0] as A, xs[1] as B, xs[2] as C, xs[3] as D) }
}

public inline fun <reified A : Data, reified B : Data, reified C : Data, reified D : Data, reified E : Data, reified R : Data> helper(
    name: String,
    a: Pair<String, A>,
    b: Pair<String, B>,
    c: Pair<String, C>,
    d: Pair<String, D>,
    e: Pair<String, E>,
    noinline body: (A, B, C, D, E) -> R,
): R {
    @Suppress("UNCHECKED_CAST")
    return helperIn(
        name,
        listOf(
            Param(a.first, a.second, typeOf<A>()),
            Param(b.first, b.second, typeOf<B>()),
            Param(c.first, c.second, typeOf<C>()),
            Param(d.first, d.second, typeOf<D>()),
            Param(e.first, e.second, typeOf<E>()),
        ),
        typeOf<R>(),
    ) { xs -> body(xs[0] as A, xs[1] as B, xs[2] as C, xs[3] as D, xs[4] as E) }
}

public inline fun <reified A : Data, reified B : Data, reified C : Data, reified D : Data, reified E : Data, reified F : Data, reified R : Data> helper(
    name: String,
    a: Pair<String, A>,
    b: Pair<String, B>,
    c: Pair<String, C>,
    d: Pair<String, D>,
    e: Pair<String, E>,
    f: Pair<String, F>,
    noinline body: (A, B, C, D, E, F) -> R,
): R {
    @Suppress("UNCHECKED_CAST")
    return helperIn(
        name,
        listOf(
            Param(a.first, a.second, typeOf<A>()),
            Param(b.first, b.second, typeOf<B>()),
            Param(c.first, c.second, typeOf<C>()),
            Param(d.first, d.second, typeOf<D>()),
            Param(e.first, e.second, typeOf<E>()),
            Param(f.first, f.second, typeOf<F>()),
        ),
        typeOf<R>(),
    ) { xs -> body(xs[0] as A, xs[1] as B, xs[2] as C, xs[3] as D, xs[4] as E, xs[5] as F) }
}

/** `ECall name args` under Emit (emitting the helper on its first call in a build); the body on the values under Native. */
@PublishedApi
internal fun <R : Data> helperIn(name: String, params: KList<Param>, ret: KType, body: (KList<Data>) -> R): R {
    val run = Run.current()
    if (run.native) return body(params.map { it.value })
    val rk = Kinds.of(ret)
    val reg = Helpers.current.get()
    if (reg == null || reg.seen.add(name)) {
        // The helper's own function, over its parameters, in a run of its own.
        val kinds = params.map { Kinds.of(it.type) }
        val e = Emitting()
        val block = Run.under(e) {
            e.block {
                val r = body(params.mapIndexed { i, p -> kinds[i].make(Term.E(Expr.Arg(p.name))) })
                e.stmt(Stmt.Return(exprOf(r)))
            }
        }
        if (e.autos.isNotEmpty()) throw Fault.bug("authoring: the helper $name draws an auto; a helper has no ctx")
        val fn = Function(
            name,
            FnKind.Helper,
            emptyList(),
            params.mapIndexed { i, p -> p.name to Field(kinds[i].ty, emptyList()) },
            rk.ty,
            block,
            emptyMap(),
            router = null,
            uses = emptyList(),
            refine = emptyList(),
        )
        reg?.ready?.add(fn)
    }
    @Suppress("UNCHECKED_CAST")
    return rk.make(Term.E(Expr.Call(name, params.map { exprOf(it.value) }))) as R
}

/** The helpers a module build has met, and those emitted and not yet placed. */
internal class Helpers {
    val seen = HashSet<String>()
    val ready = ArrayList<Function>()

    /** The helpers emitted since the last call, in the order they finished: a helper a helper calls first. */
    fun drain(): KList<Function> {
        val out = ready.toList()
        ready.clear()
        return out
    }

    companion object {
        val current: ThreadLocal<Helpers?> = ThreadLocal()

        fun <A> during(f: (Helpers) -> A): A {
            val before = current.get()
            val reg = Helpers()
            current.set(reg)
            try {
                return f(reg)
            } finally {
                current.set(before)
            }
        }
    }
}

/**
 * Run pure vocabulary natively, outside any procedure: the value `f`
 * computes, or the refusal it reached. What lets a host compute what a
 * helper computes — a derived key, say — with the helper's own definition
 * rather than a copy of it. There is no store: a read inside is a bug, as a
 * read outside a procedure is.
 */
public fun <T : Data> evaluate(f: () -> T): Eval.Answer = try {
    Run.under(Native(MemoryStore(IrSchema(emptyList())), emptyMap())) { Eval.Answer.Ok(valueOf(f())) }
} catch (r: Fault.Refuse) {
    Eval.Answer.Refused(r.refusal)
}
