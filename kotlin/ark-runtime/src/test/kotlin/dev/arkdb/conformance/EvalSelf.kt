// The interpreter's spec-2 additions, on a module written out by hand:
// input checks and their default messages, middleware in a procedure's own
// order, insert/upsert/update, a query refused, and the form validator.
package dev.arkdb.conformance

import dev.arkdb.Check
import dev.arkdb.CmpOp
import dev.arkdb.Column
import dev.arkdb.Ctx
import dev.arkdb.Eval
import dev.arkdb.Expr
import dev.arkdb.Field
import dev.arkdb.FnKind
import dev.arkdb.Function
import dev.arkdb.Hash
import dev.arkdb.IR
import dev.arkdb.Index
import dev.arkdb.MemoryStore
import dev.arkdb.Module
import dev.arkdb.Refusal
import dev.arkdb.Router
import dev.arkdb.SPEC_VERSION
import dev.arkdb.Schema
import dev.arkdb.StdFn
import dev.arkdb.Stmt
import dev.arkdb.Table
import dev.arkdb.Ty
import dev.arkdb.Value
import dev.arkdb.Verify

object EvalSelf {
    private fun <T> eq(got: T, want: T, what: String) {
        if (got != want) throw AssertionError("$what:\n  got  $got\n  want $want")
    }

    private val sch = Schema(
        listOf(
            Table(
                "t",
                listOf(Column("id", Ty.TInt, false), Column("name", Ty.TText, false), Column("n", Ty.TInt, false)),
                listOf("id"),
                listOf(Index(listOf("name"), true)),
                emptyList(),
            ),
        ),
    )

    private fun fn(
        name: String,
        kind: FnKind,
        input: List<Pair<String, Field>>,
        body: List<Stmt>,
        uses: List<String> = emptyList(),
        ret: Ty? = null,
        refine: List<Pair<Expr, String?>> = emptyList(),
    ) = Function(
        name, kind, emptyList(), input, ret, body, emptyMap(),
        router = if (kind == FnKind.Mutator || kind == FnKind.Query) "r" else null, uses = uses, refine = refine,
    )

    private fun row(id: Expr, name: Expr, n: Expr) = Expr.Struct(mapOf("id" to id, "name" to name, "n" to n))
    private fun arg(n: String) = Expr.Arg(n)
    private fun lit(n: Long) = Expr.Lit(Value.VInt(n))
    private fun lit(s: String) = Expr.Lit(Value.VText(s))

    private val module: Module by lazy {
        val input = listOf(
            "id" to Field(Ty.TInt, listOf(Check.Range(1, 99, null))),
            "name" to Field(Ty.TText, listOf(Check.Trim, Check.MinLen(1, null), Check.MaxLen(5, null))),
            "n" to Field(Ty.TInt, listOf(Check.Refine(Expr.Cmp(CmpOp.Ne, arg("n"), lit(13)), "not thirteen"))),
        )
        val refine = listOf(Expr.Cmp(CmpOp.Lt, arg("n"), arg("id")) to null as String?)
        val fns = listOf(
            fn("gate", FnKind.Guard, emptyList(), listOf(Stmt.If(Expr.Std(StdFn.IsEmpty, listOf(Expr.CtxUser)), listOf(Stmt.Refuse(lit("sign in"))), emptyList()))),
            fn("same", FnKind.Provide, listOf("id" to Field(Ty.TInt, emptyList())),
                listOf(Stmt.Let(0, Expr.Get("t", listOf(arg("id")))), Stmt.Return(Expr.Var(0))), ret = Ty.TOption(sch.lookupTable("t")!!.rowTy)),
            fn("ins", FnKind.Mutator, input, listOf(Stmt.Insert("t", row(arg("id"), arg("name"), arg("n")), listOf("name"))), uses = listOf("gate"), refine = refine),
            fn("ups", FnKind.Mutator, input, listOf(Stmt.Upsert("t", row(arg("id"), arg("name"), arg("n")), listOf("name"))), uses = listOf("gate")),
            fn("bump", FnKind.Mutator, listOf("id" to Field(Ty.TInt, emptyList())),
                listOf(Stmt.Update("t", listOf(arg("id")), 0, row(lit(0), Expr.Field(Expr.Var(0), "name"), Expr.Op(dev.arkdb.Op.Add, listOf(Expr.Field(Expr.Var(0), "n"), lit(1))))))),
            fn("was", FnKind.Query, listOf("id" to Field(Ty.TInt, emptyList())),
                listOf(Stmt.Return(Expr.Std(StdFn.IsSome, listOf(Expr.Provided("same"))))), uses = listOf("gate", "same"), ret = Ty.TBool),
        )
        Verify.verify(Module(SPEC_VERSION, sch, fns, emptyList(), listOf(Router("r", listOf("gate", "same")))))
    }

    private fun c(name: String) = Hash.closure(module, module.lookupFunction(name)!!)

    fun run() {
        val alice = Ctx("alice", "a")
        var st = MemoryStore(sch)
        fun apply(name: String, args: Map<String, Value>, ctx: Ctx = alice): Eval.Applied {
            val a = Eval.applyClosure(sch, c(name), ctx, emptyMap(), args, st)
            if (a is Eval.Applied.Ok) st = a.store
            return a
        }
        fun args(id: Long, name: String, n: Long) = mapOf("id" to Value.int(id), "name" to Value.text(name), "n" to Value.int(n))
        fun refused(why: String) = Eval.Applied.Refused(Refusal.Refused(why)) as Eval.Applied
        eq(apply("ins", args(0, "a", 0)), refused("id: between 1 and 99"), "range, default message")
        eq(apply("ins", args(1, "  ", 0)), refused("name: at least 1 characters"), "trim before min")
        eq(apply("ins", args(1, "abcdef", 0)), refused("name: at most 5 characters"), "max")
        eq(apply("ins", args(20, "a", 13)), refused("not thirteen"), "a refine with a message")
        eq(apply("ins", args(2, "a", 5)), refused("invalid"), "a whole-input refine, default")
        eq(apply("ins", args(2, "a", 1), Ctx("", "x")), refused("sign in"), "the guard runs after the checks")
        val ok = apply("ins", args(2, " a ", 1))
        eq((ok as Eval.Applied.Ok).changes.size, 1, "an insert adds")
        eq(st.get("t", listOf(Value.int(2))).field("name"), Value.text("a"), "the body sees the trimmed value")
        eq((apply("ins", args(3, "a", 1)) as Eval.Applied.Ok).changes.size, 0, "an insert on a matching unique index is a no-op")
        eq((apply("ups", args(3, "a", 2)) as Eval.Applied.Ok).changes.map { it::class.simpleName }, listOf("Edit"), "an upsert on the index edits the match")
        eq(st.scan("t").map { it["id"] to it["n"] }, listOf(Value.int(2) to Value.int(2)), "keeping the match's key")
        apply("bump", mapOf("id" to Value.int(2)))
        eq(st.scan("t").map { it["id"] to it["n"] }, listOf(Value.int(2) to Value.int(3)), "an update keeps the key and reads the old row")
        eq((apply("bump", mapOf("id" to Value.int(7))) as Eval.Applied.Ok).changes, emptyList(), "an update of nothing is a no-op")
        eq(Eval.queryClosure(sch, c("was"), alice, mapOf("id" to Value.int(2)), st), Eval.Answer.Ok(Value.bool(true)) as Eval.Answer, "a provide's value")
        eq(Eval.queryClosure(sch, c("was"), Ctx("", "x"), mapOf("id" to Value.int(2)), st), Eval.Answer.Refused(Refusal.Refused("sign in")) as Eval.Answer, "a query refused by a guard")
        // The form validator: present fields only, normalised values beside the messages.
        val partial = Eval.check(sch, c("ins"), mapOf("name" to Value.text("  toolong  ")), st)
        eq(partial.messages, listOf("name" to "name: at most 5 characters"), "one field checked")
        eq(partial.values["name"], Value.text("toolong"), "trimmed")
        val whole = Eval.check(sch, c("ins"), args(4, " b ", 9), st)
        eq(whole.messages, listOf("" to "invalid"), "the whole-input refine once every field is present")
        eq(Eval.check(sch, c("ins"), args(4, "b", 1), st).ok, true, "a valid input")
    }
}
