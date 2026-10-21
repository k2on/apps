// The typed half of the bridge (`docs/plan-v5.md` §1.16, N3), beside the
// bindings UniFFI writes: what the Kotlin `ark-dev gen` writes for a domain
// stands on — a row id, a view's rows read as their type with what it did,
// and JSON as the kernel reads and writes it (Android's `org.json`).
package dev.arkdb.ffi

import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

/** A row id: its sixteen bytes in hex. */
@JvmInline
value class Id(val hex: String) {
    companion object {
        val nil = Id("0".repeat(32))
    }
}

/** A view's rows read as `T`, and what it did since last taken. */
class View<T>(val handle: ViewHandle, private val read: (Any?) -> T) : AutoCloseable {
    fun rows(): List<T> = handle.rows().map { read(J.parse(it)) }

    fun take(): Update<T> =
        when (val u = handle.take()) {
            is ViewUpdate.Unchanged -> Update.Unchanged
            is ViewUpdate.Reset -> Update.Reset(u.rows.map { read(J.parse(it)) })
            is ViewUpdate.Patched -> Update.Patched(u.patches.map { p ->
                when (p) {
                    is Patch.Insert -> Change.Insert(p.at.toInt(), read(J.parse(p.row)))
                    is Patch.Remove -> Change.Remove(p.at.toInt())
                    is Patch.Update -> Change.Update(p.at.toInt(), read(J.parse(p.row)))
                }
            })
        }

    override fun close() = handle.close()
}

sealed class Update<out T> {
    data object Unchanged : Update<Nothing>()
    data class Patched<T>(val changes: List<Change<T>>) : Update<T>()
    data class Reset<T>(val rows: List<T>) : Update<T>()
}

sealed class Change<out T> {
    data class Insert<T>(val at: Int, val row: T) : Change<T>()
    data class Remove(val at: Int) : Change<Nothing>()
    data class Update<T>(val at: Int, val row: T) : Change<T>()
}

/** [changes] made to the list a screen holds. */
fun <T> MutableList<T>.splice(changes: List<Change<T>>) {
    for (c in changes) when (c) {
        is Change.Insert -> add(c.at, c.row)
        is Change.Remove -> removeAt(c.at)
        is Change.Update -> set(c.at, c.row)
    }
}

/** JSON as the kernel reads and writes it. */
object J {
    fun parse(s: String): Any? = JSONTokener(s).nextValue()
    fun int(j: Any?): Long = (j as Number).toLong()
    fun text(j: Any?): String = j as String
    fun bool(j: Any?): Boolean = j as Boolean
    fun <T> opt(j: Any?, f: (Any?) -> T): T? = if (j == null || j == JSONObject.NULL) null else f(j)
    fun <T> list(j: Any?, f: (Any?) -> T): List<T> = (j as JSONArray).let { a -> List(a.length()) { f(a.get(it)) } }
    fun <T> sum(j: Any?, f: (String, JSONObject?) -> T): T =
        if (j is String) f(j, null) else (j as JSONObject).keys().next().let { k -> f(k, j.optJSONObject(k)) }
    fun <T> wopt(v: T?, f: (T) -> String): String = if (v == null) "null" else f(v)
    fun <T> wlist(v: List<T>, f: (T) -> String): String = v.joinToString(",", "[", "]", transform = f)
    fun q(s: String): String = buildString {
        append('"')
        for (c in s) when (c) {
            '"' -> append("\\\"")
            '\\' -> append("\\\\")
            '\n' -> append("\\n")
            '\r' -> append("\\r")
            '\t' -> append("\\t")
            '\b' -> append("\\b")
            '\u000C' -> append("\\f")
            else -> if (c < ' ') append("\\u%04x".format(c.code)) else append(c)
        }
        append('"')
    }
}
