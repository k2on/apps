// §3 Ark IR (Ark.IR): the program a domain is.
package dev.arkdb

import java.util.SortedMap

/** The version of this specification a module was written against. */
public const val SPEC_VERSION: Int = 1

public data class Module(
    val spec: Int,
    val schema: Schema,
    /** In declaration order; a helper may be called only by functions after it. */
    val functions: List<Function>,
    /** §3.9 The live section: the frame types an app's realtime channel carries. */
    val live: List<Pair<String, Ty>>,
) {
    public fun lookupFunction(n: String): Function? = functions.firstOrNull { it.name == n }
}

public enum class FnKind { Mutator, Query, Helper }

/** The non-determinism a mutator is allowed, by type. */
public sealed class Auto {
    public data class NewId(val table: String) : Auto()
    public object Now : Auto() {
        override fun toString(): String = "Now"
    }
}

/** A local variable, alpha-normalised. */
public typealias Sym = Int

public data class Function(
    val name: String,
    val kind: FnKind,
    val scope: String?,
    val autos: List<Pair<String, Auto>>,
    val args: List<Pair<String, Ty>>,
    val ret: Ty?,
    val body: List<Stmt>,
    /** The author's names for symbols; not hashed, not required. */
    val names: Map<Sym, String>,
)

/** §3.1 Statements. */
public sealed class Stmt {
    public data class Let(val sym: Sym, val e: Expr) : Stmt()
    public data class If(val c: Expr, val then: List<Stmt>, val els: List<Stmt>) : Stmt()
    public data class For(val sym: Sym, val over: Expr, val body: List<Stmt>) : Stmt()
    public data class Put(val table: String, val row: Expr) : Stmt()
    public data class Delete(val table: String, val key: List<Expr>) : Stmt()
    public data class Refuse(val e: Expr) : Stmt()
    public data class Return(val e: Expr?) : Stmt()
}

/** §3.2 Expressions. */
public sealed class Expr {
    public data class Lit(val v: Value) : Expr()
    public data class Arg(val name: String) : Expr()
    public data class Auto(val name: String) : Expr()
    public data class Var(val sym: Sym) : Expr()
    public object CtxUser : Expr() {
        override fun toString(): String = "CtxUser"
    }
    public object CtxSession : Expr() {
        override fun toString(): String = "CtxSession"
    }
    public data class Field(val e: Expr, val name: String) : Expr()
    public class Struct(fields: kotlin.collections.Map<String, Expr>) : Expr() {
        public val fields: SortedMap<String, Expr> = sortedFields(fields)
        override fun equals(other: kotlin.Any?): Boolean = other is Struct && fields == other.fields
        override fun hashCode(): Int = fields.hashCode()
        override fun toString(): String = "Struct$fields"
    }
    public data class ListE(val items: List<Expr>) : Expr()
    public data class Some(val e: Expr) : Expr()
    public data class None(val ty: Ty) : Expr()
    public data class Match(val e: Expr, val sym: Sym, val some: Expr, val none: Expr) : Expr()
    public data class If(val c: Expr, val then: Expr, val els: Expr) : Expr()
    public data class Op(val op: dev.arkdb.Op, val args: List<Expr>) : Expr()
    public data class Cmp(val op: CmpOp, val l: Expr, val r: Expr) : Expr()
    public data class Call(val fn: String, val args: List<Expr>) : Expr()
    public data class Std(val fn: StdFn, val args: List<Expr>) : Expr()
    public data class Map(val over: Expr, val sym: Sym, val body: Expr) : Expr()
    public data class Filter(val over: Expr, val sym: Sym, val body: Expr) : Expr()
    public data class Any(val over: Expr, val sym: Sym, val body: Expr) : Expr()
    public data class All(val over: Expr, val sym: Sym, val body: Expr) : Expr()
    public data class SortBy(val over: Expr, val sym: Sym, val key: Expr) : Expr()
    public data class Fold(val over: Expr, val init: Expr, val acc: Sym, val sym: Sym, val body: Expr) : Expr()
    public data class Select(val plan: IR.Plan) : Expr()
    public data class Get(val table: String, val key: List<Expr>) : Expr()
    public data class Exists(val table: String, val key: List<Expr>) : Expr()
}

/** Arithmetic and boolean operators; the integer ones are checked. */
public enum class Op(public val wire: String) {
    Add("add"), Sub("sub"), Mul("mul"), Div("div"), Mod("mod"), Neg("neg"), And("and"), Or("or"), Not("not");

    public companion object {
        public fun ofWire(s: String): Op? = entries.firstOrNull { it.wire == s }
    }
}

/** Comparison under the total order. */
public enum class CmpOp(public val wire: String) {
    Eq("eq"), Ne("ne"), Lt("lt"), Le("le"), Gt("gt"), Ge("ge");

    /** Whether the comparison holds of an ordering result. */
    public fun holds(o: Int): Boolean = when (this) {
        Eq -> o == 0
        Ne -> o != 0
        Lt -> o < 0
        Le -> o <= 0
        Gt -> o > 0
        Ge -> o >= 0
    }

    public companion object {
        public fun ofWire(s: String): CmpOp? = entries.firstOrNull { it.wire == s }
    }
}

/** §3.3 Plans and predicates as the IR carries them: right-hand sides are expressions. */
public object IR {
    public data class Plan(
        val table: String,
        val filter: Pred?,
        val order: List<Pair<String, Dir>>,
        val limit: Int?,
        val related: List<Related>,
    )

    public data class Related(val name: String, val relation: Relation, val plan: Plan)

    public sealed class Pred {
        public data class Cmp(val column: String, val op: CmpOp, val e: Expr) : Pred()
        public data class In(val column: String, val items: List<Expr>) : Pred()
        public data class All(val items: List<Pred>) : Pred()
        public data class Any(val items: List<Pred>) : Pred()
        public data class Not(val e: Pred) : Pred()
    }
}

/** §3.4 The standard library, by name; spelled as Haskell's `show` spells the constructors. */
public enum class StdFn(public val arity: Int) {
    Trim(1), IsEmpty(1), Concat(1), Lower(1), IsAlnum(1), Chars(1), TextLen(1), StartsWith(2), SplitOnce(2),
    TextOfInt(1), Hex(1),
    Min(2), Max(2), Clamp(3), Abs(1),
    Fnv1a64(1), Sha256(1),
    IdOfText(1), TextOfId(1), NilId(0), Utf8(1),
    First(1), Last(1), Len(1), Contains(2), Reverse(1), IsSome(1), UnwrapOr(2);

    public companion object {
        public fun ofName(s: String): StdFn? = entries.firstOrNull { it.name == s }
    }
}
