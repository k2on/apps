// AUTHORING.md §2.5 and §6: routers, middleware, procedures, and the module.
// `Module(routers).emit()` runs every body once under Emit and returns the
// verified module's bytes; `procedures()` is every procedure, by the hash
// an entry names it by, ready to run under Native.
package dev.arkdb.authoring

import dev.arkdb.Args
import dev.arkdb.Auto
import dev.arkdb.Canon
import dev.arkdb.Ctx as PeerCtx
import dev.arkdb.Encode
import dev.arkdb.Eval
import dev.arkdb.Expr
import dev.arkdb.Fault
import dev.arkdb.FnHash
import dev.arkdb.FnKind
import dev.arkdb.Function
import dev.arkdb.Hash
import dev.arkdb.MemoryStore
import dev.arkdb.Procedure
import dev.arkdb.SPEC_VERSION
import dev.arkdb.Schema as IrSchema
import dev.arkdb.Stmt
import dev.arkdb.Value
import dev.arkdb.Verify

/** Who is asking: the verified user and the login, as values; and the entry's autos, by name. */
public class Ctx internal constructor(public val user: Text, public val session: Text) {
    /** `EAuto name`, declared `Now`: the clock at the originating peer, frozen in the entry. */
    public fun now(name: String): Int {
        val run = Run.current()
        if (run.native) return Int(Term.N((run as Native).auto(name)))
        (run as Emitting).auto(name, Auto.Now)
        return Int(Term.E(Expr.Auto(name)))
    }

    /** `EAuto name`, declared `NewId T`: a fresh id drawn at the originating peer. */
    public inline fun <reified T : Row<*>> newId(name: String): Id<T> = newIdIn(name, T::class.java)

    @PublishedApi
    internal fun <T : Row<*>> newIdIn(name: String, cls: Class<*>): Id<T> {
        val run = Run.current()
        val k = Kind.KId(RowInfo.of(cls).name)
        if (run.native) return Id(Term.N((run as Native).auto(name)), k)
        (run as Emitting).auto(name, Auto.NewId(k.table))
        return Id(Term.E(Expr.Auto(name)), k)
    }
}

/** A guard or a provide, declared on a router. */
internal class Middleware(
    val name: String,
    val provide: Boolean,
    val input: InputInfo<*>?,
    val body: (Ctx, Any, Any?) -> Any?,
) {
    /** What a provide returns, known once it has been emitted. */
    var provided: Kind? = null
}

/** A mutation or a query on a router. */
public class Route<S : Tables> internal constructor(
    internal val name: String,
    internal val kind: FnKind,
    internal val input: InputInfo<*>?,
    internal val uses: KList<Middleware>,
    internal val body: (Ctx, Any, Any?, Any?) -> Any?,
)

/** The middleware a procedure is built on, oldest first; `Router` is the chain with none. */
public open class Chain<S : Tables> internal constructor(private val root: Router<S>?, internal val uses: KList<Middleware>) {
    @Suppress("UNCHECKED_CAST")
    internal val router: Router<S> get() = root ?: (this as Router<S>)

    /** A guard: runs before the body, may refuse, returns nothing. */
    public fun guard(name: String, f: (Ctx, S) -> Effect): Chain<S> {
        @Suppress("UNCHECKED_CAST")
        val mw = Middleware(name, false, null) { c, db, _ -> f(c, db as S) }
        router.declare(mw)
        return Chain(router, uses + mw)
    }

    /** A provide: runs before the body, may refuse, and hands the body a value. */
    public inline fun <reified I : Input, P : Data> provide(name: String, noinline f: (Ctx, S, I) -> P): Provides<S, P> =
        provideIn(name, I::class.java, f)

    @PublishedApi
    internal fun <I : Input, P : Data> provideIn(name: String, cls: Class<*>, f: (Ctx, S, I) -> P): Provides<S, P> {
        @Suppress("UNCHECKED_CAST")
        val mw = Middleware(name, true, InputInfo.of(cls)) { c, db, i -> f(c, db as S, i as I) }
        router.declare(mw)
        return Provides(router, uses + mw)
    }

    public inline fun <reified I : Input> input(): Takes<S, I> = takes(I::class.java)

    @PublishedApi
    internal fun <I : Input> takes(cls: Class<*>): Takes<S, I> = Takes(router, uses, InputInfo.of(cls))

    public fun mutation(name: String, f: (Ctx, S, Unit) -> Effect): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Mutator, null, uses) { c, db, _, _ -> f(c, db as S, Unit) }
    }

    public fun <R : Data> query(name: String, f: (Ctx, S, Unit) -> R): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Query, null, uses) { c, db, _, _ -> f(c, db as S, Unit) }
    }
}

/** A chain whose last provide hands the body a `P`. */
public class Provides<S : Tables, P : Data> internal constructor(internal val router: Router<S>, internal val uses: KList<Middleware>) {
    public fun guard(name: String, f: (Ctx, S) -> Effect): Provides<S, P> {
        @Suppress("UNCHECKED_CAST")
        val mw = Middleware(name, false, null) { c, db, _ -> f(c, db as S) }
        router.declare(mw)
        return Provides(router, uses + mw)
    }

    public inline fun <reified I : Input> input(): TakesProvided<S, I, P> = takes(I::class.java)

    @PublishedApi
    internal fun <I : Input> takes(cls: Class<*>): TakesProvided<S, I, P> = TakesProvided(router, uses, InputInfo.of(cls))

    public fun mutation(name: String, f: (Ctx, S, Unit, P) -> Effect): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Mutator, null, uses) { c, db, _, p -> f(c, db as S, Unit, p as P) }
    }

    public fun <R : Data> query(name: String, f: (Ctx, S, Unit, P) -> R): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Query, null, uses) { c, db, _, p -> f(c, db as S, Unit, p as P) }
    }
}

/** A chain with an input type. */
public class Takes<S : Tables, I : Input> internal constructor(
    internal val router: Router<S>,
    internal val uses: KList<Middleware>,
    internal val input: InputInfo<*>,
) {
    public fun mutation(name: String, f: (Ctx, S, I) -> Effect): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Mutator, input, uses) { c, db, i, _ -> f(c, db as S, i as I) }
    }

    public fun <R : Data> query(name: String, f: (Ctx, S, I) -> R): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Query, input, uses) { c, db, i, _ -> f(c, db as S, i as I) }
    }
}

/** A chain with an input type whose last provide hands the body a `P`. */
public class TakesProvided<S : Tables, I : Input, P : Data> internal constructor(
    internal val router: Router<S>,
    internal val uses: KList<Middleware>,
    internal val input: InputInfo<*>,
) {
    public fun mutation(name: String, f: (Ctx, S, I, P) -> Effect): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Mutator, input, uses) { c, db, i, p -> f(c, db as S, i as I, p as P) }
    }

    public fun <R : Data> query(name: String, f: (Ctx, S, I, P) -> R): Route<S> {
        @Suppress("UNCHECKED_CAST")
        return Route(name, FnKind.Query, input, uses) { c, db, i, p -> f(c, db as S, i as I, p as P) }
    }
}

/** A router over the tables `S`: its middleware, as declared, and its routes. */
public class Router<S : Tables> @PublishedApi internal constructor(internal val name: String, tablesClass: Class<*>) :
    Chain<S>(null, emptyList()) {
    internal val tables: TablesInfo = TablesInfo(tablesClass)
    internal val middleware = ArrayList<Middleware>()
    internal val routes = ArrayList<Route<S>>()

    override fun toString(): String = "Router($name)"

    internal fun declare(mw: Middleware) {
        if (middleware.any { it.name == mw.name }) throw Fault.bug("authoring: middleware ${mw.name} is declared twice on $name")
        middleware.add(mw)
    }

    /** The router, with these routes in this order. */
    public fun routes(vararg rs: Route<S>): Router<S> {
        routes.addAll(rs)
        return this
    }

}

/** `router<Harken>("playlists")`. */
public inline fun <reified S : Tables> router(name: String): Router<S> = Router(name, S::class.java)

// The module ------------------------------------------------------------------------

/** A domain: its routers, in order. */
public class Module(vararg routers: Router<*>) {
    private val routers: KList<Router<*>> = routers.toList()

    /** The module as IR, verified: orders completed and every function normalised (§6). */
    public val ir: dev.arkdb.Module by lazy { Verify.verify(build()) }

    /** The module's canonical bytes: what `.ark` holds. */
    public fun emit(): ByteArray = Canon.encode(Encode.toValue(ir))

    /** The hash of the module (`Ark.Hash.moduleHash`). */
    public val hash: ByteArray get() = Hash.moduleHash(ir)

    /** Every procedure, natively, by the hash an entry names its function by. */
    public fun procedures(): KList<Pair<FnHash, Procedure>> = routers.flatMap { r ->
        r.routes.map { route ->
            val fn = ir.lookupFunction(route.name) ?: throw Fault.bug("authoring: ${route.name} was not emitted")
            Hash.functionHash(Hash.closure(ir, fn)) to (NativeProcedure(r, route, fn) as Procedure)
        }
    }

    private fun build(): dev.arkdb.Module {
        // A module has one set of tables: every router is over the same class.
        val first = routers.firstOrNull()
        for (r in routers) {
            if (first != null && r.tables.cls != first.tables.cls) {
                throw Fault.bug(
                    "authoring: router ${first.name} is over ${first.tables.cls.name} and router ${r.name} over " +
                        "${r.tables.cls.name}: a module has one set of tables",
                )
            }
        }
        // A helper is emitted into the module immediately before the first
        // function that calls it.
        val fns = ArrayList<Function>()
        Helpers.during { helpers ->
            for (r in routers) {
                for (mw in r.middleware) {
                    val f = emitMiddleware(r, mw)
                    fns.addAll(helpers.drain())
                    fns.add(f)
                }
                for (route in r.routes) {
                    val f = emitRoute(r, route)
                    fns.addAll(helpers.drain())
                    fns.add(f)
                }
            }
        }
        val rs = routers.map { r -> dev.arkdb.Router(r.name, r.middleware.map { it.name }) }
        return dev.arkdb.Module(SPEC_VERSION, IrSchema(first?.tables?.tables ?: emptyList()), fns, emptyList(), rs)
    }

    private fun emitCtx(): Ctx = Ctx(Text(Term.E(Expr.CtxUser), Kind.TEXT), Text(Term.E(Expr.CtxSession), Kind.TEXT))

    private fun emitMiddleware(r: Router<*>, mw: Middleware): Function {
        val e = Emitting()
        return Run.under(e) {
            val input = mw.input?.ir(e, checks = false)?.first ?: emptyList()
            var ret: Kind? = null
            val body = e.block {
                val v = mw.body(emitCtx(), r.tables.instance, mw.input?.make { Term.E(Expr.Arg(it)) })
                if (mw.provide) {
                    val d = v as Data
                    ret = kindOf(d)
                    e.stmt(Stmt.Return(exprOf(d)))
                }
            }
            mw.provided = ret
            Function(
                mw.name,
                if (mw.provide) FnKind.Provide else FnKind.Guard,
                e.autos.toList().map { it.first to it.second },
                input,
                ret?.ty,
                body,
                e.names.toMap(),
                router = null,
                uses = emptyList(),
                refine = emptyList(),
            )
        }
    }

    private fun emitRoute(r: Router<*>, route: Route<*>): Function {
        val e = Emitting()
        return Run.under(e) {
            val (input, refine) = route.input?.ir(e, checks = true) ?: (emptyList<Pair<String, dev.arkdb.Field>>() to emptyList())
            val provider = route.uses.lastOrNull { it.provide }
            var ret: Kind? = null
            val body = e.block {
                val given = route.input?.make { Term.E(Expr.Arg(it)) }
                val provided = provider?.let { p ->
                    (p.provided ?: throw Fault.bug("authoring: ${p.name} is used before it is declared")).make(Term.E(Expr.Provided(p.name)))
                }
                val v = route.body(emitCtx(), r.tables.instance, given, provided)
                if (route.kind == FnKind.Query) {
                    val d = v as Data
                    ret = kindOf(d)
                    e.stmt(Stmt.Return(exprOf(d)))
                }
            }
            Function(
                route.name,
                route.kind,
                e.autos.toList().map { it.first to it.second },
                input,
                ret?.ty,
                body,
                e.names.toMap(),
                router = r.name,
                uses = route.uses.map { it.name },
                refine = refine,
            )
        }
    }
}

/**
 * A procedure run natively (§3): the input checked, each middleware in its
 * chain's order, the body — all against one fork of the store, as
 * `Eval.applyClosure` runs the same function's closure.
 */
internal class NativeProcedure(
    private val router: Router<*>,
    private val route: Route<*>,
    private val fn: Function,
) : Procedure {
    override val name: String get() = route.name
    override val kind: FnKind get() = route.kind

    private fun <A> run(ctx: PeerCtx, autos: Args, args: Args, st: MemoryStore, f: (Any?) -> A): A {
        for ((a, _) in fn.autos) if (a !in autos) throw Fault.Bug("MissingAuto ${dev.arkdb.HsShow.text(a)}")
        for ((a, _) in fn.input) if (a !in args) throw Fault.Bug("MissingArg ${dev.arkdb.HsShow.text(a)}")
        return Run.under(Native(st, autos)) {
            val run = Run.current()
            val checked = route.input?.check(args, st) ?: args
            val c = Ctx(Text(Term.N(Value.VText(ctx.user)), Kind.TEXT), Text(Term.N(Value.VText(ctx.session)), Kind.TEXT))
            val db = router.tables.instance
            var provided: Any? = null
            for (mw in route.uses) {
                val given = mw.input?.make { Term.N(checked.getValue(it)) }
                val v = mw.body(c, db, given)
                run.flush()
                if (mw.provide) provided = v
            }
            val given = route.input?.make { Term.N(checked.getValue(it)) }
            val v = route.body(c, db, given, provided)
            run.flush()
            f(v)
        }
    }

    override fun apply(sch: dev.arkdb.Schema, ctx: PeerCtx, autos: Args, args: Args, st: MemoryStore): Eval.Applied {
        if (route.kind != FnKind.Mutator) throw Fault.Bug("WrongKind ${dev.arkdb.HsShow.text(name)} ${route.kind}")
        val work = st.fork()
        try {
            run(ctx, autos, args, work) { }
        } catch (f: Fault.Refuse) {
            return Eval.Applied.Refused(f.refusal)
        }
        return Eval.Applied.Ok(work, work.changes.toList())
    }

    override fun query(sch: dev.arkdb.Schema, ctx: PeerCtx, args: Args, st: MemoryStore): Eval.Answer {
        if (route.kind != FnKind.Query) throw Fault.Bug("WrongKind ${dev.arkdb.HsShow.text(name)} ${route.kind}")
        return try {
            Eval.Answer.Ok(run(ctx, emptyMap(), args, st.fork()) { v -> valueOf(v as Data) })
        } catch (f: Fault.Refuse) {
            Eval.Answer.Refused(f.refusal)
        }
    }
}
