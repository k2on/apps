// `docs/plan-guards.md` D4 The authority's raw writes (`rust/ark/src/raw.rs`):
// two functions in every module by construction, as the standard library
// is, that write a row or take one away with no domain logic between —
//
//     ark.put_row(table: Text, row: Struct)      the row, judged as `put` is
//     ark.delete_row(table: Text, key: List)     the row under the key, as `delete` is
//
// named by fixed hashes, `sha256(enc(Text name))`, which no closure's hash
// can be. Only a server's authority authors one; every peer replays one,
// natively, here — which is all a client does with them.
package dev.arkdb

/** Which of the two an entry names (`raw::Raw`). */
public enum class Raw(public val wire: String) {
    PutRow("ark.put_row"),
    DeleteRow("ark.delete_row");

    /** Its fixed hash. */
    public val hash: FnHash
        get() = hashOf(wire)

    public companion object {
        /** The actor of every raw write a server makes (`raw::AUTHOR`). */
        public const val AUTHOR: String = "authority"

        /** `hash_of`: `sha256(enc(Text name))`, how a function no module carries is named. */
        public fun hashOf(name: String): FnHash = Sha256.hash(Canon.encode(Value.VText(name)))

        private val hashes: List<Pair<Raw, FnHash>> by lazy { entries.map { it to it.hash } }

        /** `of`: which raw write a hash names, if it names one. */
        public fun of(fh: ByteArray): Raw? =
            hashes.firstOrNull { it.second.contentEquals(fh) }?.first

        /** `is_raw`. */
        public fun isRaw(fh: ByteArray): Boolean = of(fh) != null

        /** `table_of`: the table its arguments name, or the function's name where they do not. */
        public fun tableOf(raw: Raw, args: Map<String, Value>): String =
            (args["table"] as? Value.VText)?.value ?: raw.wire

        /**
         * `call_of`: the raw write that makes `change`, and its arguments — an `Add` or an `Edit`
         * puts the new row, a `Remove` deletes the row's key.
         */
        public fun callOf(schema: Schema, change: Change): Pair<Raw, Map<String, Value>> {
            val args = LinkedHashMap<String, Value>()
            args["table"] = Value.VText(change.table)
            return when (change) {
                is Change.Add -> PutRow to args.apply { put("row", change.row) }
                is Change.Edit -> PutRow to args.apply { put("row", change.new) }
                is Change.Remove -> {
                    val key = schema.lookupTable(change.table)?.keyOf(change.row) ?: emptyList()
                    DeleteRow to args.apply { put("key", Value.VList(key)) }
                }
            }
        }

        /**
         * `apply`: a raw write over a store, judged by the table's constraints exactly as a
         * mutator's `put` and `delete` are — its changes, applied, or the verdict with the store
         * untouched; arguments that are not the function's are a bug.
         */
        public fun apply(raw: Raw, args: Map<String, Value>, store: Store): Ran {
            val table =
                when (val t = args["table"]) {
                    is Value.VText -> t.value
                    null -> throw EvalFault.Bug(EvalError.MissingArg("table"))
                    else ->
                        throw EvalFault.Bug(
                            EvalError.TypeError("${raw.wire}: table is a text, not ${t.show()}"))
                }
            val written =
                when (raw) {
                    PutRow -> {
                        val row =
                            when (val r = args["row"]) {
                                is Value.VStruct -> r
                                null -> throw EvalFault.Bug(EvalError.MissingArg("row"))
                                else ->
                                    throw EvalFault.Bug(
                                        EvalError.TypeError(
                                            "${raw.wire}: row is a struct, not ${r.show()}"))
                            }
                        store.put(table, Rules.rowFor(store, table, row))
                    }
                    DeleteRow -> {
                        val key =
                            when (val k = args["key"]) {
                                is Value.VList -> k.items
                                null -> throw EvalFault.Bug(EvalError.MissingArg("key"))
                                else ->
                                    throw EvalFault.Bug(
                                        EvalError.TypeError(
                                            "${raw.wire}: key is a list, not ${k.show()}"))
                            }
                        store.delete(table, key)
                    }
                }
            return when (written) {
                is Written.Ok -> Ran.Ok(listOfNotNull(written.change))
                is Written.Refused -> Ran.Refused(written.refusal)
            }
        }
    }
}
