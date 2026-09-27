// §1 Values (Ark.Value).
//
// The eight run-time values and the one total order over them. Everything
// here must agree byte for byte with the Rust and Swift runtimes, in
// memory, on the wire, in an index key and under a hash.
package dev.arkdb

import java.util.SortedMap
import java.util.TreeMap

/**
 * Text compared by Unicode code point (§1.2), which is UTF-8 byte order.
 * Kotlin's `String.compareTo` compares UTF-16 code units and puts U+FF5E
 * before U+1F3B5 where code points put it after; nothing in this runtime
 * orders text through it.
 */
public object CodePointOrder : Comparator<String> {
    override fun compare(a: String, b: String): Int {
        var i = 0
        var j = 0
        while (i < a.length && j < b.length) {
            val ca = a.codePointAt(i)
            val cb = b.codePointAt(j)
            if (ca != cb) return if (ca < cb) -1 else 1
            i += Character.charCount(ca)
            j += Character.charCount(cb)
        }
        return when {
            i < a.length -> 1
            j < b.length -> -1
            else -> 0
        }
    }
}

/** Unsigned lexicographic order over bytes, a prefix first. */
public object ByteOrder : Comparator<ByteArray> {
    override fun compare(a: ByteArray, b: ByteArray): Int {
        val n = minOf(a.size, b.size)
        for (i in 0 until n) {
            val x = a[i].toInt() and 0xff
            val y = b[i].toInt() and 0xff
            if (x != y) return if (x < y) -1 else 1
        }
        return a.size.compareTo(b.size)
    }
}

/** Lowercase hex, two digits per byte, and back. */
public object Hex {
    private const val DIGITS = "0123456789abcdef"

    public fun encode(bytes: ByteArray): String {
        val sb = StringBuilder(bytes.size * 2)
        for (b in bytes) {
            val w = b.toInt() and 0xff
            sb.append(DIGITS[w ushr 4]).append(DIGITS[w and 0x0f])
        }
        return sb.toString()
    }

    public fun decode(hex: String): ByteArray {
        require(hex.length % 2 == 0) { "hex has an odd number of digits: $hex" }
        val out = ByteArray(hex.length / 2)
        for (i in out.indices) {
            out[i] = ((digit(hex[2 * i]) shl 4) or digit(hex[2 * i + 1])).toByte()
        }
        return out
    }

    /** Whether a character is an ASCII hex digit, as Haskell's `isHexDigit`. */
    public fun isHexDigit(c: Char): Boolean =
        (c in '0'..'9') || (c in 'a'..'f') || (c in 'A'..'F')

    internal fun digit(c: Char): Int = when (c) {
        in '0'..'9' -> c - '0'
        in 'a'..'f' -> c - 'a' + 10
        in 'A'..'F' -> c - 'A' + 10
        else -> throw IllegalArgumentException("not a hex digit: '$c'")
    }
}

/** Sixteen bytes: the run-time form of an id. The table it names is a static type. */
public class Id(bytes: ByteArray) : Comparable<Id> {
    public val bytes: ByteArray = bytes.copyOf()

    init {
        require(bytes.size == 16) { "an id is exactly sixteen bytes, got ${bytes.size}" }
    }

    /** Lowercase 8-4-4-4-12, as `Std.textOfId` spells it. */
    public val text: String
        get() {
            val h = Hex.encode(bytes)
            return h.substring(0, 8) + "-" + h.substring(8, 12) + "-" + h.substring(12, 16) + "-" +
                h.substring(16, 20) + "-" + h.substring(20)
        }

    /** Thirty-two lowercase hex digits, no dashes. */
    public val hex: String get() = Hex.encode(bytes)

    override fun equals(other: Any?): Boolean = other is Id && bytes.contentEquals(other.bytes)
    override fun hashCode(): Int = bytes.contentHashCode()
    override fun compareTo(other: Id): Int = ByteOrder.compare(bytes, other.bytes)
    override fun toString(): String = text

    public companion object {
        /** The nil id: sixteen zero bytes. */
        public val nil: Id = Id(ByteArray(16))

        /** Thirty-two hex digits, in either case. */
        public fun ofHex(hex: String): Id = Id(Hex.decode(hex))

        /** Parse 8-4-4-4-12 hex in either case; null if that is not what the text is. */
        public fun ofText(text: String): Id? {
            val parts = text.split("-")
            if (parts.size != 5) return null
            val lengths = intArrayOf(8, 4, 4, 4, 12)
            for (i in 0 until 5) {
                if (parts[i].length != lengths[i]) return null
                if (!parts[i].all { Hex.isHexDigit(it) }) return null
            }
            return Id(Hex.decode(parts.joinToString("")))
        }
    }
}

/** Arguments (or autos) by name. */
public typealias Args = Map<String, Value>

/** A row: every column of its table, by name. Never partial once stored. */
public typealias Row = Value.VStruct

/**
 * A run-time value. `VNull` is the absent case of an option; `VStruct` is
 * a named record whose field names are its identity, so two structs with
 * the same fields and values are equal whatever order they were built in.
 */
public sealed class Value : Comparable<Value> {
    public object VNull : Value() {
        override fun toString(): String = "null"
    }

    public data class VBool(val value: Boolean) : Value()

    public data class VInt(val value: Long) : Value()

    public data class VText(val value: String) : Value()

    public class VBytes(bytes: ByteArray) : Value() {
        public val value: ByteArray = bytes.copyOf()
        override fun equals(other: Any?): Boolean = other is VBytes && value.contentEquals(other.value)
        override fun hashCode(): Int = value.contentHashCode()
        override fun toString(): String = "bytes(${Hex.encode(value)})"
    }

    public data class VId(val value: Id) : Value()

    public data class VList(val items: List<Value>) : Value()

    /** Fields in code point order of their names, which is `Data.Map`'s order over `Text`. */
    public class VStruct(fields: Map<String, Value>) : Value() {
        public val fields: SortedMap<String, Value> = sortedFields(fields)

        /** The field, or `VNull` where the struct lacks it — how a row's column is read. */
        public operator fun get(name: String): Value = fields[name] ?: VNull

        public fun has(name: String): Boolean = fields.containsKey(name)

        override fun equals(other: Any?): Boolean = other is VStruct && fields == other.fields
        override fun hashCode(): Int = fields.hashCode()
        override fun toString(): String = show()
    }

    /** `Null < Bool < Int < Text < Bytes < Id < List < Struct`. */
    public val rank: Int
        get() = when (this) {
            is VNull -> 0
            is VBool -> 1
            is VInt -> 2
            is VText -> 3
            is VBytes -> 4
            is VId -> 5
            is VList -> 6
            is VStruct -> 7
        }

    override fun compareTo(other: Value): Int = compareValue(this, other)

    // Accessors: a verified module never mismatches, so a mismatch is a bug
    // and fails fatally rather than faulting.

    public fun isNull(): Boolean = this is VNull

    public fun asBool(): Boolean = (this as? VBool)?.value ?: mismatch("Bool")

    public fun asInt(): Long = (this as? VInt)?.value ?: mismatch("Int")

    public fun asText(): String = (this as? VText)?.value ?: mismatch("Text")

    public fun asBytes(): ByteArray = (this as? VBytes)?.value ?: mismatch("Bytes")

    public fun asId(): Id = (this as? VId)?.value ?: mismatch("Id")

    public fun asList(): List<Value> = (this as? VList)?.items ?: mismatch("List")

    public fun asStruct(): VStruct = (this as? VStruct) ?: mismatch("Struct")

    public fun field(name: String): Value {
        val s = this as? VStruct ?: mismatch("Struct")
        return s.fields[name] ?: throw IllegalStateException("no field \"$name\" in ${show()}")
    }

    private fun mismatch(want: String): Nothing =
        throw IllegalStateException("expected $want, got ${show().take(60)}")

    /** A printable form: the vectors' JSON, with the `$int`/`$bytes`/`$id` wrappers. */
    public fun show(): String = when (this) {
        is VNull -> "null"
        is VBool -> if (value) "true" else "false"
        is VInt -> "{\"\$int\":\"$value\"}"
        is VText -> quote(value)
        is VBytes -> "{\"\$bytes\":\"${Hex.encode(value)}\"}"
        is VId -> "{\"\$id\":\"${value.text}\"}"
        is VList -> items.joinToString(",", "[", "]") { it.show() }
        is VStruct -> fields.entries.joinToString(",", "{", "}") { quote(it.key) + ":" + it.value.show() }
    }

    public companion object {
        /** `Value.null()` in the contract; Kotlin spells the call `Value.\`null\`()`. */
        public fun `null`(): Value = VNull

        public fun bool(b: Boolean): Value = VBool(b)

        public fun int(i: Long): Value = VInt(i)

        public fun text(s: String): Value = VText(s)

        public fun bytes(b: ByteArray): Value = VBytes(b)

        public fun bytesHex(hex: String): Value = VBytes(Hex.decode(hex))

        public fun id(i: Id): Value = VId(i)

        public fun idHex(hex: String): Value = VId(Id.ofHex(hex))

        public fun list(items: List<Value>): Value = VList(items.toList())

        public fun list(vararg items: Value): Value = VList(items.toList())

        public fun record(fields: List<Pair<String, Value>>): Value = VStruct(fields.toMap())

        public fun record(vararg fields: Pair<String, Value>): Value = VStruct(fields.toMap())

        public fun struct(fields: Map<String, Value>): VStruct = VStruct(fields)

        /** `Some v` is `v` and `None` is `VNull`: an option is flat. */
        public fun opt(v: Value?): Value = v ?: VNull

        internal fun quote(s: String): String {
            val sb = StringBuilder("\"")
            for (c in s) {
                when {
                    c == '"' -> sb.append("\\\"")
                    c == '\\' -> sb.append("\\\\")
                    c < ' ' -> sb.append("\\u").append(String.format("%04x", c.code))
                    else -> sb.append(c)
                }
            }
            return sb.append('"').toString()
        }
    }
}

/** A map of fields in code point order of their names. */
public fun <V> sortedFields(fields: Map<String, V>): SortedMap<String, V> {
    if (fields is TreeMap<String, V> && fields.comparator() === CodePointOrder) return fields
    val m = TreeMap<String, V>(CodePointOrder)
    m.putAll(fields)
    return m
}

public fun <V> sortedFields(fields: List<Pair<String, V>>): SortedMap<String, V> {
    val m = TreeMap<String, V>(CodePointOrder)
    for ((k, v) in fields) m[k] = v
    return m
}

/**
 * §1.2 The total order. Values of different types order by rank; within a
 * type: ints numerically, text by code point, bytes and ids bytewise, lists
 * lexicographically (a prefix first), structs as the association list
 * sorted by field name, names before values.
 */
public fun compareValue(a: Value, b: Value): Int {
    val ra = a.rank
    val rb = b.rank
    if (ra != rb) return ra.compareTo(rb)
    return when (a) {
        is Value.VNull -> 0
        is Value.VBool -> a.value.compareTo((b as Value.VBool).value)
        is Value.VInt -> a.value.compareTo((b as Value.VInt).value)
        is Value.VText -> CodePointOrder.compare(a.value, (b as Value.VText).value)
        is Value.VBytes -> ByteOrder.compare(a.value, (b as Value.VBytes).value)
        is Value.VId -> a.value.compareTo((b as Value.VId).value)
        is Value.VList -> {
            val xs = a.items
            val ys = (b as Value.VList).items
            val n = minOf(xs.size, ys.size)
            for (i in 0 until n) {
                val o = compareValue(xs[i], ys[i])
                if (o != 0) return o
            }
            xs.size.compareTo(ys.size)
        }
        is Value.VStruct -> {
            val xs = a.fields.entries.iterator()
            val ys = (b as Value.VStruct).fields.entries.iterator()
            while (xs.hasNext() && ys.hasNext()) {
                val x = xs.next()
                val y = ys.next()
                val ok = CodePointOrder.compare(x.key, y.key)
                if (ok != 0) return ok
                val ov = compareValue(x.value, y.value)
                if (ov != 0) return ov
            }
            when {
                xs.hasNext() -> 1
                ys.hasNext() -> -1
                else -> 0
            }
        }
    }
}

/** The order as a comparator, for sorts and maps that must follow the spec. */
public object ValueOrder : Comparator<Value> {
    override fun compare(a: Value, b: Value): Int = compareValue(a, b)
}

/** A key's order: the key columns' values, lexicographically. */
public object KeyOrder : Comparator<List<Value>> {
    override fun compare(a: List<Value>, b: List<Value>): Int {
        val n = minOf(a.size, b.size)
        for (i in 0 until n) {
            val o = compareValue(a[i], b[i])
            if (o != 0) return o
        }
        return a.size.compareTo(b.size)
    }
}
