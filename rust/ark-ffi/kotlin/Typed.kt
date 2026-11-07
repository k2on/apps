// The typed half of the bridge (`docs/plan-v5.md` §1.16, N3), beside the
// bindings UniFFI writes: what the Kotlin `arkdb gen` writes for a domain
// stands on — a row id, a value read from its bytes in the kernel's format
// (how a view's rows cross), a view's rows read as their type with what it
// did, and JSON as the kernel reads and writes it (how an input crosses,
// and the app's own JSON is read: Android's `org.json`).
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

private const val HEX = "0123456789abcdef"

/**
 * A value read from its bytes in the kernel's value format (`ark::format`, postcard): an integer
 * zigzagged into a varint, text and bytes after their length, a list after its count, an option
 * after a 0 or a 1, a sum's variant as its index, a record's fields in order.
 */
class Reader(private val b: ByteArray) {
    private var at = 0

    private fun byte(): Int = b[at++].toInt() and 0xff

    private fun varint(): Long {
        var v = 0L
        var shift = 0
        while (true) {
            val x = byte()
            v = v or ((x and 0x7f).toLong() shl shift)
            if (x < 0x80) return v
            shift += 7
        }
    }

    private fun bytes(): ByteArray = varint().toInt().let { n -> b.copyOfRange(at, at + n).also { at += n } }

    fun variant(): Int = varint().toInt()

    fun int(): Long = varint().let { (it ushr 1) xor -(it and 1) }

    fun bool(): Boolean = byte() != 0

    fun text(): String = String(bytes(), Charsets.UTF_8)

    fun hex(): String = buildString { for (x in bytes()) append(HEX[(x.toInt() shr 4) and 0xf]).append(HEX[x.toInt() and 0xf]) }

    fun id(): Id = Id(hex())

    fun <T> opt(f: (Reader) -> T): T? = if (byte() == 0) null else f(this)

    fun <T> list(f: (Reader) -> T): List<T> = List(varint().toInt()) { f(this) }
}

/**
 * A view's rows read as `T`, and what it did since last taken (its [Op]s and the rows that came
 * with them): each row its bytes read by `read`. A view made again says so as its changes: its
 * rows removed and inserted.
 */
class View<T>(private val handle: ViewHandle, private val read: (Reader) -> T) : AutoCloseable {
    fun rows(): List<T> = handle.rows().map { read(Reader(it)) }

    fun take(): Update<T> =
        handle.take().let { t ->
            if (t.ops.isEmpty()) Update.Unchanged
            else t.rows.iterator().let { r ->
                Update.Patched(t.ops.map { o ->
                    when (o) {
                        is Op.Insert -> Change.Insert(o.at.toInt(), read(Reader(r.next())))
                        is Op.Remove -> Change.Remove(o.at.toInt())
                        is Op.Update -> Change.Update(o.at.toInt(), read(Reader(r.next())))
                    }
                })
            }
        }

    override fun close() = handle.close()
}

sealed class Update<out T> {
    data object Unchanged : Update<Nothing>()
    data class Patched<T>(val changes: List<Change<T>>) : Update<T>()
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
