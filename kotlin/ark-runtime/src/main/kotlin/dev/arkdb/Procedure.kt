// What a peer runs natively (AUTHORING.md §3): a procedure of a module,
// written in this language against the authoring vocabulary and executed
// directly rather than interpreted. It must mean exactly what `Eval` says
// its closure means; `dev.arkdb.authoring` is the one implementation, and
// the runtime's tests hold the two to agreement on every procedure of the
// demo.
package dev.arkdb

public interface Procedure {
    public val name: String

    /** `Mutator` or `Query`. */
    public val kind: FnKind

    /** Apply a mutator to a store: `Eval.applyClosure`'s answer, computed natively. */
    public fun apply(sch: Schema, ctx: Ctx, autos: Args, args: Args, st: MemoryStore): Eval.Applied

    /** Run a query: `Eval.queryClosure`'s answer, computed natively. */
    public fun query(sch: Schema, ctx: Ctx, args: Args, st: MemoryStore): Eval.Answer
}
