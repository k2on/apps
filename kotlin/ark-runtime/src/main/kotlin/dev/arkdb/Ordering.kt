// §3.12 Which transitions need an order (`rust/ark/src/ir/ordering.rs`,
// `docs/plan-live.md`, "Ordered and ephemeral fields"), by static
// inspection, as `Reads` says what a query reads.
//
// - **ephemeral**: it reads nothing of the state, nothing of the context but
//   `ctx.device`, and writes only the caller's own slot of ephemeral fields
//   (`state.f[ctx.device]`). The server relays it with no sequence and no
//   hash, and the newest value from a device wins.
// - **ordered**: every other one, sequenced per instance.
//
// And one rule binds the ordered kind: it reads no ephemeral field. It may
// write the caller's own slot and pass the rest through. Hooks are ordered.
//
// "Writes only" and "passes through" are read off the shape of what the body
// returns: the state itself, or a struct of every field of the state's
// record, each `state.f` — or, for an ephemeral `f`,
// `set_slot(state.f, ctx.device, v)` — through any `if` whose condition is
// allowed where it stands.
package dev.arkdb

/** The two kinds of transition (`ordering::Kind`), named as a vector names them. */
public enum class Kind(public val wire: String) {
    /** Sequenced per instance; the server's order is every replica's. */
    Ordered("ordered"),
    /** Relayed with no order: writes the caller's own ephemeral slots only. */
    Ephemeral("ephemeral"),
}

public object Ordering {
    /**
     * `classify`: the kind of a declared transition of a state whose resolved type is `state`,
     * ephemeral markers kept. A hook is always ordered.
     */
    public fun classify(state: Ty, t: Transition): Kind {
        val eph = Types.ephemeralFields(state)
        if (state !is Ty.TStruct) return Kind.Ordered
        if (eph.isEmpty() || t.name == ON_JOIN || t.name == ON_LEAVE || t.name == ON_RESTORE)
            return Kind.Ordered
        val names = state.fields.keys
        val checksQuiet =
            t.input.flatMap { it.second.checks }.all { c -> c !is Check.Refine || quiet(c.e) }
        val wrote = BooleanArray(1)
        val ok =
            checksQuiet &&
                t.guards.all { quiet(it.first) } &&
                blockEphemeral(t.body, names, eph, wrote) &&
                wrote[0]
        return if (ok) Kind.Ephemeral else Kind.Ordered
    }

    // Reads nothing of the state and nothing of the context but the device.
    private fun quiet(e: Expr): Boolean =
        !mentions(e) {
            it is Expr.LiveState ||
                it is Expr.CtxUser ||
                it is Expr.CtxSession ||
                it is Expr.CtxNow ||
                it is Expr.HasRole
        }

    private fun blockEphemeral(
        b: Block,
        names: Set<String>,
        eph: Set<String>,
        wrote: BooleanArray
    ): Boolean =
        b.all { s ->
            when (s) {
                is Stmt.Return -> s.e != null && returnsOwnSlots(s.e, names, eph, wrote)
                is Stmt.If ->
                    quiet(s.c) &&
                        blockEphemeral(s.then, names, eph, wrote) &&
                        blockEphemeral(s.els, names, eph, wrote)
                is Stmt.For -> quiet(s.over) && blockEphemeral(s.body, names, eph, wrote)
                is Stmt.Let -> quiet(s.e)
                is Stmt.Refuse -> quiet(s.e)
                else -> false
            }
        }

    // What an ephemeral transition may return: the state unchanged, or every field passed through
    // but ephemeral ones written at the caller's slot, through any `if` that reads nothing it may
    // not.
    private fun returnsOwnSlots(
        e: Expr,
        names: Set<String>,
        eph: Set<String>,
        wrote: BooleanArray
    ): Boolean =
        when (e) {
            is Expr.LiveState -> true
            is Expr.IfE ->
                quiet(e.c) &&
                    returnsOwnSlots(e.then, names, eph, wrote) &&
                    returnsOwnSlots(e.els, names, eph, wrote)
            is Expr.StructE ->
                e.fields.keys == names &&
                    e.fields.all { (k, v) ->
                        if (passedThrough(k, v)) return@all true
                        val x = ownSlotWrite(k, v)
                        if (x != null && k in eph && quiet(x)) {
                            wrote[0] = true
                            true
                        } else false
                    }
            else -> false
        }

    // `state.k`, as `k` itself.
    private fun passedThrough(k: String, v: Expr): Boolean =
        v is Expr.FieldE && v.e == Expr.LiveState && v.name == k

    // `set_slot(state.k, ctx.device, x)`: the value written, `x`.
    private fun ownSlotWrite(k: String, v: Expr): Expr? =
        if (v is Expr.SetSlot && passedThrough(k, v.e) && v.key == Expr.CtxDevice) v.v else null

    /**
     * `ordered_reads_ephemeral`: the first ephemeral field an ordered transition reads, by name, or
     * `"the whole state"` where it uses the state as a value; null where it reads none.
     */
    public fun orderedReadsEphemeral(state: Ty, t: Transition): String? {
        val eph = Types.ephemeralFields(state)
        if (eph.isEmpty()) return null
        val names = (state as? Ty.TStruct)?.fields?.keys ?: return null
        for ((_, f) in t.input) for (c in f.checks) if (c is Check.Refine)
            reads(c.e, eph)?.let {
                return it
            }
        for ((e, _) in t.guards) reads(e, eph)?.let {
            return it
        }
        return blockReads(t.body, names, eph)
    }

    private fun blockReads(b: Block, names: Set<String>, eph: Set<String>): String? {
        for (s in b) {
            val r =
                when (s) {
                    is Stmt.Return -> s.e?.let { returnedReads(it, names, eph) }
                    is Stmt.If ->
                        reads(s.c, eph)
                            ?: blockReads(s.then, names, eph)
                            ?: blockReads(s.els, names, eph)
                    is Stmt.For -> reads(s.over, eph) ?: blockReads(s.body, names, eph)
                    is Stmt.Let -> reads(s.e, eph)
                    is Stmt.Refuse -> reads(s.e, eph)
                    else -> null
                }
            if (r != null) return r
        }
        return null
    }

    // A returned value: the state itself is a pass-through; a struct of every field may pass an
    // ephemeral field through or write the caller's slot of it; anything else is read as any
    // expression is.
    private fun returnedReads(e: Expr, names: Set<String>, eph: Set<String>): String? =
        when {
            e is Expr.LiveState -> null
            e is Expr.IfE ->
                reads(e.c, eph)
                    ?: returnedReads(e.then, names, eph)
                    ?: returnedReads(e.els, names, eph)
            e is Expr.StructE && e.fields.keys == names -> {
                var out: String? = null
                for ((k, v) in e.fields) {
                    val r =
                        if (k in eph && passedThrough(k, v)) null
                        else if (k in eph && ownSlotWrite(k, v) != null)
                            reads(ownSlotWrite(k, v)!!, eph)
                        else reads(v, eph)
                    if (r != null) {
                        out = r
                        break
                    }
                }
                out
            }
            else -> reads(e, eph)
        }

    // The first ephemeral field `e` reads: `state.f` for an ephemeral `f`, or the state used whole.
    private fun reads(e: Expr, eph: Set<String>): String? =
        when {
            e is Expr.FieldE && e.e == Expr.LiveState -> if (e.name in eph) e.name else null
            e is Expr.LiveState -> "the whole state"
            else -> children(e).firstNotNullOfOrNull { reads(it, eph) }
        }

    // Whether any subexpression of `e` is one `hit` answers for.
    private fun mentions(e: Expr, hit: (Expr) -> Boolean): Boolean =
        hit(e) || children(e).any { mentions(it, hit) }
}
