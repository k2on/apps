// A change in the protocol's fact form (`rust/ark/src/protocol.rs`,
// `change_value` and `change_from_value`): `{"t": "add", "table", "row"}`,
// `{"t": "remove", "table", "row"}`, `{"t": "edit", "table", "old",
// "new"}` — how the vectors write a mutator's changes and a view's
// batches, and how an authority's facts cross the wire.
package dev.arkdb

/** A frame or a fact whose shape is wrong (`protocol::DecodeError`). */
public class FactError(what: String) : Exception(what)

public object Facts {
    /** `change_value`. */
    public fun changeValue(c: Change): Value {
        val m = LinkedHashMap<String, Value>()
        m["table"] = Value.VText(c.table)
        when (c) {
            is Change.Add -> {
                m["t"] = Value.VText("add")
                m["row"] = c.row
            }
            is Change.Remove -> {
                m["t"] = Value.VText("remove")
                m["row"] = c.row
            }
            is Change.Edit -> {
                m["t"] = Value.VText("edit")
                m["old"] = c.old
                m["new"] = c.new
            }
        }
        return Value.VStruct(m)
    }

    /** `change_from_value`. */
    public fun changeFromValue(v: Value): Change {
        val m = (v as? Value.VStruct)?.fields ?: throw FactError("a change is a struct")
        fun need(k: String): Value = m[k] ?: throw FactError("missing field $k")
        fun text(k: String): String =
            (need(k) as? Value.VText)?.value ?: throw FactError("$k is not text")
        fun row(k: String): Row = need(k) as? Value.VStruct ?: throw FactError("$k is not a row")
        val t = text("t")
        val table = text("table")
        return when (t) {
            "add" -> Change.Add(table, row("row"))
            "remove" -> Change.Remove(table, row("row"))
            "edit" -> Change.Edit(table, row("old"), row("new"))
            else -> throw FactError("unknown change $t")
        }
    }
}
