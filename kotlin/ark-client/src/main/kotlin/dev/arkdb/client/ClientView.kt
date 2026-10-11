// A query held as a list and kept up to date (`rust/ark-client/src/view.rs`).
//
// Hydrate once, then hand every `Changes` to `update` and splice what it
// returns into whatever the screen built from the rows. Every query is
// maintained: a query is a plan, and `Views` keeps a plan's entries, so a
// change costs the entries it touches whatever the size of the list.
//
// A query's middleware — its input checks, guards and provides — runs before
// the hydrate, over the same store, and decides the scope the plan is pulled
// in (§1.7). The view keeps the tables that middleware reads (`Reads`); a
// change to one of them re-runs it before anything is pushed, and a different
// outcome — another provided value, a refusal appearing or clearing, or
// another user signed in — re-hydrates and reports `Reset`. A rebase is
// changes like any other; `Changes.Rebuilt` — the optimistic store replaced
// whole — is a re-hydrate and `Reset`.
package dev.arkdb.client

import dev.arkdb.Changes
import dev.arkdb.Closure
import dev.arkdb.Ctx
import dev.arkdb.Eval
import dev.arkdb.EvalFault
import dev.arkdb.FnKind
import dev.arkdb.MaintainedView
import dev.arkdb.Patch
import dev.arkdb.Reads
import dev.arkdb.Refusal
import dev.arkdb.Value
import dev.arkdb.ViewEnv
import dev.arkdb.Views
import dev.arkdb.debugEvalError

/** What an update did to the list (`view::Update`). */
public sealed class Update {
    public object Unchanged : Update()

    /** Apply these, in order, to the list as it stood ([ClientView.splice]). */
    public data class Patched(val patches: List<Patch>) : Update()

    /** The list was read again: take [ClientView.rows] whole. */
    public object Reset : Update()
}

/** A maintained query (`ark_client::View`). */
public class ClientView
internal constructor(
    public val name: String,
    public val args: Map<String, Value>,
    private var ctx: Ctx,
    /** The tables the middleware reads (§1.7). */
    private val guards: Set<String>,
    // The checked input and the provided values, or the verdict that leaves the list empty.
    private var outcome: Outcome,
) {
    internal sealed class Outcome {
        data class Ran(val args: Map<String, Value>, val provided: Map<String, Value>) : Outcome()

        data class Refused(val refusal: Refusal) : Outcome()
    }

    private var held: MaintainedView? = null

    /** The list, as it stands. */
    public var rows: List<Value> = emptyList()
        private set

    /** Whether the view holds the plan's entries: every query's does, except while it refuses. */
    public val incremental: Boolean
        get() = held != null

    /** Run the middleware again and hydrate whole. */
    public fun rehydrate(peer: Peer) {
        val c = closure(peer, name)
        ctx = peer.ctx
        outcome = middleware(peer, name, c, args)
        hydrate(peer, c)
    }

    private fun hydrate(peer: Peer, c: Closure) {
        val o = outcome
        if (o !is Outcome.Ran) {
            held = null
            rows = emptyList()
            return
        }
        val plan = c.function.plan ?: throw PeerError("$name: a query with no plan")
        val env = ViewEnv(c.helpers, ctx, o.args, o.provided)
        val v =
            try {
                Views.hydrate(peer.storeSchema, plan, env, peer.store)
            } catch (e: EvalFault) {
                throw fault(name, e)
            }
        rows = v.rows()
        held = v
    }

    /**
     * Bring the list up to date with what moved: `changes` is what [Peer.takeChanges] returned,
     * handed to every view in turn.
     */
    public fun update(peer: Peer, changes: Changes): Update {
        val chs =
            when (changes) {
                is Changes.Rebuilt -> {
                    rehydrate(peer)
                    return Update.Reset
                }
                is Changes.Applied -> changes.changes
            }
        // §1.7 The middleware first: whoever is signed in, and whatever the
        // tables it reads now say.
        if (peer.ctx != ctx || chs.any { it.table in guards }) {
            val c = closure(peer, name)
            val o = middleware(peer, name, c, args)
            if (peer.ctx != ctx || o != outcome) {
                ctx = peer.ctx
                outcome = o
                hydrate(peer, c)
                return Update.Reset
            }
        }
        val v = held ?: return Update.Unchanged
        if (chs.isEmpty()) return Update.Unchanged
        val patches =
            try {
                Views.pushAll(peer.storeSchema, peer.store, chs, v)
            } catch (e: EvalFault) {
                // A fault leaves the engine's view as it was, and stale: read
                // it again, and say so.
                rehydrate(peer)
                return Update.Reset
            }
        if (patches.isEmpty()) return Update.Unchanged
        rows = Views.splice(patches, rows)
        return Update.Patched(patches)
    }

    public companion object {
        internal fun open(peer: Peer, name: String, args: Map<String, Value>): ClientView {
            val c = closure(peer, name)
            val guards = HashSet(Reads.reads(c.function))
            for (u in c.function.uses) {
                // A scope runs nothing on a device (`docs/plan-guards.md`
                // D2): what it reads decides what the authority serves, not
                // whether this view's middleware refuses.
                c.helpers
                    .firstOrNull { it.name == u && it.kind != FnKind.Scope }
                    ?.let { guards.addAll(Reads.reads(it)) }
            }
            val o = middleware(peer, name, c, args)
            if (o is Outcome.Refused) throw Refused(o.refusal)
            val v = ClientView(name, args, peer.ctx, guards, o)
            v.hydrate(peer, c)
            return v
        }

        /**
         * Apply patches in order to a list as it stood (`view::splice`): what a screen holding its
         * own list of the rows — mapped to its own type — does with [Update.Patched].
         */
        public fun <T> splice(xs: MutableList<T>, ps: List<Patch>, of: (Value) -> T) {
            for (p in ps) {
                when (p) {
                    is Patch.Insert -> xs.add(p.at, of(p.node))
                    is Patch.Remove -> xs.removeAt(p.at)
                    is Patch.Update -> xs[p.at] = of(p.node)
                }
            }
        }

        private fun closure(peer: Peer, name: String): Closure {
            val (h, _) = peer.domain.query(name)
            return peer.domain.closures[dev.arkdb.Hex.encode(h)]!!
        }

        // The middleware over the peer's store, as the peer's user: a verdict
        // is an outcome, a bug is an error.
        private fun middleware(
            peer: Peer,
            name: String,
            c: Closure,
            args: Map<String, Value>
        ): Outcome =
            try {
                val (a, p) = Eval.middleware(peer.storeSchema, c, peer.ctx, args, peer.store)
                Outcome.Ran(a, p)
            } catch (e: EvalFault.Verdict) {
                Outcome.Refused(e.refusal)
            } catch (e: EvalFault.Bug) {
                throw PeerError("bug: $name: ${debugEvalError(e.error)}")
            }

        private fun fault(name: String, e: EvalFault): PeerError =
            when (e) {
                is EvalFault.Verdict -> Refused(e.refusal)
                is EvalFault.Bug -> PeerError("bug: $name: ${debugEvalError(e.error)}")
            }
    }
}
