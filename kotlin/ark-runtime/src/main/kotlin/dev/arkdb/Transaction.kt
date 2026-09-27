// A transaction: what generated code writes through when it runs in place
// of the interpreter (GENERATED.md item 4: "an overlay over a store that
// records the transaction, so a body that faults commits nothing").
//
// It is backed by a fork of the base rather than by a diff, for the same
// reason `Eval.applyClosure` works on a fork: every constraint — not-null,
// uniqueness, references — is decided in `MemoryStore.write`, once, and a
// diff store would have to decide them a second time against a merged
// picture of two stores. The base is never touched, so a body that throws
// has committed nothing, and `commit` hands back the store after the body
// and what it changed, exactly as `Eval.Applied.Ok` does.
package dev.arkdb

public class TransactionStore(public val base: MemoryStore) : Store {
    private val work: MemoryStore = base.fork()
    private var closed: Boolean = false

    override val schema: Schema get() = base.schema

    /** What the body has written so far, in order. */
    public val changes: List<Change> get() = work.changes

    override fun get(table: String, key: List<Value>): Value = work.get(table, key)

    override fun exists(table: String, key: List<Value>): Value = work.exists(table, key)

    override fun scan(table: String): List<Row> = work.scan(table)

    override fun put(table: String, row: Value) {
        open()
        work.put(table, row)
    }

    override fun delete(table: String, key: List<Value>) {
        open()
        work.delete(table, key)
    }

    override fun applyChange(change: Change) {
        open()
        work.applyChange(change)
    }

    private fun open() {
        if (closed) throw Fault.bug("TransactionStore: written after commit")
    }

    /** The store after the body and its changes; the transaction is over. */
    public fun commit(): Eval.Applied.Ok {
        closed = true
        return Eval.Applied.Ok(work, work.changes.toList())
    }

    public companion object {
        /**
         * Run a body as a mutator over a store, the way `Eval.applyClosure`
         * runs a closure: the store after it with its changes, or the verdict
         * if it refused. A `Fault.Bug` propagates, as it does there.
         */
        public fun run(base: MemoryStore, body: (Store) -> Unit): Eval.Applied {
            val tx = TransactionStore(base)
            try {
                body(tx)
            } catch (f: Fault.Refuse) {
                return Eval.Applied.Refused(f.refusal)
            }
            return tx.commit()
        }
    }
}
