// §3 Ark IR, as `rust/ark/src/ir/mod.rs` defines it: the program a domain
// is. A module carries a schema, routers, functions and the types of its
// live frames. A function is a procedure on a router (a mutator or a
// query), a middleware (a guard, a provide or a scope) or a helper, with a
// body in a small imperative core over a pure expression language. Spec
// version 4: a query is a `Plan` and nothing else.
package dev.arkdb

import java.util.SortedMap

/** The version of the specification this runtime implements (`ir::SPEC_VERSION`). */
public const val SPEC_VERSION: Long = 4

/** A local variable, alpha-normalised: the `n`th binding in a function (`ir::Sym`). */
public typealias Sym = Long

/** A block of statements. */
public typealias Block = List<Stmt>

internal typealias Exprs = List<Expr>

public data class Module(
    val spec: Long,
    val schema: Schema,
    /** In declaration order; a helper may be called only by functions after it. */
    val functions: List<Function>,
    /** §1.1 The routers: groups of procedures, each naming the middleware it may use. */
    val routers: List<Router>,
    /** §3.9 The live section: the frame types an app's realtime channel carries, by name. */
    val live: List<Pair<String, Ty>>,
) {
    /** `Module::lookup_function`. */
    public fun lookupFunction(name: String): Function? = functions.firstOrNull { it.name == name }

    /** `Module::lookup_router`. */
    public fun lookupRouter(name: String): Router? = routers.firstOrNull { it.name == name }
}

/** §1.1 A router: a group of procedures sharing middleware, its `uses` in order. */
public data class Router(val name: String, val uses: List<String>)

/** `ir::FnKind`, spelled on the wire in lowercase. */
public enum class FnKind(public val wire: String) {
    Mutator("mutator"),
    Query("query"),
    Helper("helper"),
    Guard("guard"),
    Provide("provide"),
    /** `docs/plan-guards.md` D2 Middleware that runs nothing: what a person holds. */
    Scope("scope");

    /** A mutator or a query. */
    public val isProcedure: Boolean
        get() = this == Mutator || this == Query

    /** A guard, a provide or a scope. */
    public val isMiddleware: Boolean
        get() = this == Guard || this == Provide || this == Scope

    public companion object {
        public fun parse(s: String): FnKind? = entries.firstOrNull { it.wire == s }
    }
}

/** The non-determinism a mutator is allowed, by type; drawn once and frozen in the entry. */
public sealed class Auto {
    public data class NewId(val table: String) : Auto()

    public object Now : Auto() {
        override fun toString(): String = "Now"
    }
}

/** `ir::Function`. A decoded function carries no author's names; nothing here needs them. */
public data class Function(
    val name: String,
    val kind: FnKind,
    val router: String?,
    val uses: List<String>,
    val autos: List<Pair<String, Auto>>,
    val input: List<Pair<String, Field>>,
    val refine: List<Pair<Expr, String?>>,
    val ret: Ty?,
    val body: Block,
    /** §1.4 A query's whole meaning; `null` for every other kind. */
    val plan: Plan?,
    /** `docs/plan-guards.md` D2 A scope's holds; empty for every other kind. */
    val holds: List<Hold>,
    /** `docs/plan-guards.md` D3 A mutator with a server half. */
    val private: Boolean,
) {
    /** `Function::arg_types`. */
    public val argTypes: List<Pair<String, Ty>>
        get() = input.map { (n, f) -> n to f.ty }
}

/** `docs/plan-guards.md` D2 What a scope holds of one table. */
public data class Hold(val table: String, val filter: Pred?, val columns: Projection)

/** Which columns of a held row a scope keeps (`ir::Projection`). */
public sealed class Projection {
    public object All : Projection() {
        override fun toString(): String = "All"
    }

    public data class Pick(val columns: List<String>) : Projection()

    public data class Exclude(val columns: List<String>) : Projection()

    /** `Projection::keeps`. */
    public fun keeps(column: String): Boolean =
        when (this) {
            is All -> true
            is Pick -> column in columns
            is Exclude -> column !in columns
        }
}

/** §1.3 One field of a procedure's input. */
public data class Field(val ty: Ty, val checks: List<Check>)

/**
 * §1.3 A check on one field; `why` is the message, `null` for the default
 * (`eval::default_message`).
 */
public sealed class Check {
    public object Trim : Check() {
        override fun toString(): String = "Trim"
    }

    public data class MinLen(val n: Long, val why: String?) : Check()

    public data class MaxLen(val n: Long, val why: String?) : Check()

    public data class Range(val lo: Long?, val hi: Long?, val why: String?) : Check()

    public data class NonEmpty(val why: String?) : Check()

    public data class Exists(val why: String?) : Check()

    public data class Refine(val e: Expr, val why: String?) : Check()

    /** The message the author gave, if any (`eval::message`). */
    public val given: String?
        get() =
            when (this) {
                is Trim -> null
                is MinLen -> why
                is MaxLen -> why
                is Range -> why
                is NonEmpty -> why
                is Exists -> why
                is Refine -> why
            }
}

/** §3.1 Statements (`ir::Stmt`). */
public sealed class Stmt {
    public data class Let(val sym: Sym, val e: Expr) : Stmt()

    public data class If(val c: Expr, val then: Block, val els: Block) : Stmt()

    public data class For(val sym: Sym, val over: Expr, val body: Block) : Stmt()

    public data class Insert(val table: String, val row: Expr, val on: List<String>) : Stmt()

    public data class Upsert(val table: String, val row: Expr, val on: List<String>) : Stmt()

    public data class Update(val table: String, val key: Exprs, val sym: Sym, val row: Expr) :
        Stmt()

    public data class Delete(val table: String, val key: Exprs) : Stmt()

    public data class Refuse(val e: Expr) : Stmt()

    public data class Return(val e: Expr?) : Stmt()

    /**
     * `docs/plan-guards.md` D3 A mutator's server half, run only by the authority, after the body.
     */
    public data class Private(val body: Block) : Stmt()
}

/** `ir::strip`: every private block taken out, wherever it is nested, `private` kept. */
public fun strip(f: Function): Function {
    if (!f.private) return f
    fun block(b: Block): Block =
        b.filter { it !is Stmt.Private }
            .map {
                when (it) {
                    is Stmt.If -> Stmt.If(it.c, block(it.then), block(it.els))
                    is Stmt.For -> Stmt.For(it.sym, it.over, block(it.body))
                    else -> it
                }
            }
    return f.copy(body = block(f.body))
}

/** `ir::strip_module`: the module a client loads. */
public fun stripModule(m: Module): Module = m.copy(functions = m.functions.map(::strip))

/** `ir::has_private`: whether a block holds a private block, at any depth. */
public fun hasPrivate(b: Block): Boolean =
    b.any {
        when (it) {
            is Stmt.Private -> true
            is Stmt.If -> hasPrivate(it.then) || hasPrivate(it.els)
            is Stmt.For -> hasPrivate(it.body)
            else -> false
        }
    }

/** §3.2 Expressions (`ir::Expr`). */
public sealed class Expr {
    public data class Lit(val v: Value) : Expr()

    public data class Arg(val name: String) : Expr()

    public data class AutoE(val name: String) : Expr()

    public data class Var(val sym: Sym) : Expr()

    public object CtxUser : Expr() {
        override fun toString(): String = "CtxUser"
    }

    public object CtxSession : Expr() {
        override fun toString(): String = "CtxSession"
    }

    /** `docs/plan-guards.md` D1 Whether the author holds the role. */
    public data class HasRole(val role: String) : Expr()

    public data class Provided(val fn: String) : Expr()

    public data class FieldE(val e: Expr, val name: String) : Expr()

    /** Fields in code point order of their names, which is the order they are evaluated in. */
    public class StructE(fields: kotlin.collections.Map<String, Expr>) : Expr() {
        public val fields: SortedMap<String, Expr> = sortedFields(fields)

        override fun equals(other: kotlin.Any?): Boolean =
            other is StructE && fields == other.fields

        override fun hashCode(): Int = fields.hashCode()

        override fun toString(): String = "Struct($fields)"
    }

    public data class ListE(val items: Exprs) : Expr()

    public data class SomeE(val e: Expr) : Expr()

    public data class NoneE(val ty: Ty) : Expr()

    public data class Match(val e: Expr, val sym: Sym, val some: Expr, val none: Expr) : Expr()

    public data class IfE(val c: Expr, val then: Expr, val els: Expr) : Expr()

    public data class OpE(val op: Op, val args: Exprs) : Expr()

    public data class CmpE(val op: CmpOp, val l: Expr, val r: Expr) : Expr()

    public data class Call(val fn: String, val args: Exprs) : Expr()

    public data class StdE(val fn: StdFn, val args: Exprs) : Expr()

    public data class MapE(val over: Expr, val sym: Sym, val body: Expr) : Expr()

    public data class FilterE(val over: Expr, val sym: Sym, val body: Expr) : Expr()

    public data class AnyE(val over: Expr, val sym: Sym, val body: Expr) : Expr()

    public data class AllE(val over: Expr, val sym: Sym, val body: Expr) : Expr()

    public data class SortBy(val over: Expr, val sym: Sym, val key: Expr) : Expr()

    public data class Fold(
        val over: Expr,
        val init: Expr,
        val acc: Sym,
        val sym: Sym,
        val body: Expr
    ) : Expr()

    public data class Select(val plan: Plan) : Expr()

    public data class Get(val table: String, val key: Exprs) : Expr()

    public data class ExistsE(val table: String, val key: Exprs) : Expr()
}

/** Arithmetic and boolean operators; integer arithmetic is checked (`ir::Op`). */
public enum class Op(public val wire: String) {
    Add("add"),
    Sub("sub"),
    Mul("mul"),
    Div("div"),
    Mod("mod"),
    Neg("neg"),
    And("and"),
    Or("or"),
    Not("not");

    public companion object {
        public fun parse(s: String): Op? = entries.firstOrNull { it.wire == s }
    }
}

/** Comparison under the total order (`ir::CmpOp`). */
public enum class CmpOp(public val wire: String) {
    Eq("eq"),
    Ne("ne"),
    Lt("lt"),
    Le("le"),
    Gt("gt"),
    Ge("ge");

    public companion object {
        public fun parse(s: String): CmpOp? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * §1.3 A plan: a tree of reads, what a query is, what `select` pulls and what a view maintains
 * (`ir::Plan`). Each node is evaluated in this order: the source rows the filter admits; `row` (and
 * `members`) bound; the lookups in order; the related plans; `having`; `project`; the order keys.
 */
public data class Plan(
    val source: Source,
    val filter: Pred?,
    val row: Sym?,
    val members: Sym?,
    val lookups: List<Lookup>,
    val related: List<Related>,
    val having: Expr?,
    val project: Expr?,
    val order: List<Pair<Key, Dir>>,
    val limit: Long?,
) {
    /** The table the plan reads. */
    public val table: String
        get() = source.table

    /** `Plan::is_v3_shaped`: the shape a mutator's read may have. */
    public val isV3Shaped: Boolean
        get() =
            source is Source.Table &&
                members == null &&
                lookups.isEmpty() &&
                having == null &&
                project == null &&
                order.all { it.first is Key.Column } &&
                related.all { r ->
                    val on = r.on.singleOrNull()
                    val e = on?.second
                    on != null &&
                        row != null &&
                        e is Expr.FieldE &&
                        e.e == Expr.Var(row) &&
                        r.plan.isV3Shaped
                }

    /** `Plan::is_bare`: v3-shaped with nothing beneath. */
    public val isBare: Boolean
        get() = isV3Shaped && related.isEmpty()

    public companion object {
        /** `Plan::from`: every row of a table, in key order. */
        public fun from(table: String): Plan =
            Plan(
                Source.Table(table),
                null,
                null,
                null,
                emptyList(),
                emptyList(),
                null,
                null,
                emptyList(),
                null)
    }
}

/** Where a plan's nodes come from. */
public sealed class Source {
    public abstract val table: String

    public data class Table(override val table: String) : Source()

    public data class Group(override val table: String, val by: List<String>) : Source()
}

/** A row by key from another table, bound to `sym` as an option. */
public data class Lookup(val name: String, val sym: Sym, val table: String, val key: Exprs)

/** A child plan beneath each node, bound to `sym` as the list of its nodes. */
public data class Related(
    val name: String,
    val sym: Sym,
    val on: List<Pair<String, Expr>>,
    val plan: Plan
)

/** One key of an order: a column of the source row, or an expression over the node's binders. */
public sealed class Key {
    public data class Column(val column: String) : Key()

    public data class ExprK(val e: Expr) : Key()
}

/** A filter over one row; the right-hand sides may not mention the row (`ir::Pred`). */
public sealed class Pred {
    public data class Cmp(val column: String, val op: CmpOp, val e: Expr) : Pred()

    public data class In(val column: String, val items: List<Expr>) : Pred()

    public data class All(val items: List<Pred>) : Pred()

    public data class AnyP(val items: List<Pred>) : Pred()

    public data class Not(val e: Pred) : Pred()

    /** `docs/plan-db.md` D4 The text column holds the needle, both sides folded. */
    public data class Has(val column: String, val e: Expr) : Pred()

    /** `docs/plan-guards.md` D2 A scope's leaf: a `Bool` that reads no row. */
    public data class When(val e: Expr) : Pred()

    /** `docs/plan-guards.md` D2 A scope's one reach into another table. */
    public data class Exists(val table: String, val column: String, val pred: Pred) : Pred()
}

/** §3.4 The standard library, by name, spelled as the constructors' `show` (`ir::StdFn`). */
public enum class StdFn(public val arity: Int) {
    Trim(1),
    IsEmpty(1),
    Concat(1),
    Lower(1),
    IsAlnum(1),
    Chars(1),
    TextLen(1),
    StartsWith(2),
    SplitOnce(2),
    TextOfInt(1),
    Hex(1),
    Min(2),
    Max(2),
    Clamp(3),
    Abs(1),
    Fnv1a64(1),
    Sha256(1),
    IdOfText(1),
    TextOfId(1),
    NilId(0),
    Utf8(1),
    First(1),
    Last(1),
    Len(1),
    Contains(2),
    Reverse(1),
    IsSome(1),
    UnwrapOr(2),
    Unwrap(1);

    public companion object {
        public fun parse(s: String): StdFn? = entries.firstOrNull { it.name == s }
    }
}
