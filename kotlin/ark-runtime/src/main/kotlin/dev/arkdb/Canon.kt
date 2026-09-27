// §1.3 The canonical encoding (Ark.Canon).
//
// RFC 8949 §4.2.1 deterministic CBOR plus the mapping from Value onto
// CBOR's types: shortest heads, definite lengths, map keys sorted by the
// bytes they encode to, tag 37 over sixteen bytes for an id. `decode`
// accepts canonical bytes and nothing else.
package dev.arkdb

import java.io.ByteArrayOutputStream

public object Canon {
    /** Why some bytes are not the canonical encoding of any value; the first rule broken. */
    public enum class Why {
        NonCanonicalHead, IndefiniteLength, UnsortedKeys, DuplicateKey, NonTextKey, BadTag, BadId,
        Float, BadSimple, BadUtf8, IntOutOfRange, Trailing, Truncated,
    }

    public class DecodeError(public val why: Why) : Exception(why.name)

    // Encoding -----------------------------------------------------------

    /** The canonical bytes of a value. Total: every value has an encoding. */
    public fun encode(v: Value): ByteArray {
        val out = ByteArrayOutputStream()
        build(out, v)
        return out.toByteArray()
    }

    private fun header(out: ByteArrayOutputStream, major: Int, n: Long) {
        val mt = major shl 5
        when {
            n in 0 until 24 -> out.write(mt or n.toInt())
            n in 0 until 0x100 -> {
                out.write(mt or 24)
                out.write(n.toInt())
            }
            n in 0 until 0x10000 -> {
                out.write(mt or 25)
                out.write((n ushr 8).toInt())
                out.write(n.toInt())
            }
            n in 0 until 0x100000000L -> {
                out.write(mt or 26)
                for (s in intArrayOf(24, 16, 8, 0)) out.write((n ushr s).toInt())
            }
            else -> {
                // n is an unsigned 64-bit quantity here; every argument this
                // encoder produces is non-negative as a Long.
                out.write(mt or 27)
                for (s in intArrayOf(56, 48, 40, 32, 24, 16, 8, 0)) out.write((n ushr s).toInt())
            }
        }
    }

    private fun string(out: ByteArrayOutputStream, major: Int, b: ByteArray) {
        header(out, major, b.size.toLong())
        out.write(b, 0, b.size)
    }

    private fun build(out: ByteArrayOutputStream, v: Value) {
        when (v) {
            is Value.VNull -> out.write(0xf6)
            is Value.VBool -> out.write(if (v.value) 0xf5 else 0xf4)
            is Value.VInt -> if (v.value >= 0) header(out, 0, v.value) else header(out, 1, -1L - v.value)
            is Value.VText -> string(out, 3, Utf8.encode(v.value))
            is Value.VBytes -> string(out, 2, v.value)
            is Value.VId -> {
                header(out, 6, 37)
                string(out, 2, v.value.bytes)
            }
            is Value.VList -> {
                header(out, 4, v.items.size.toLong())
                for (x in v.items) build(out, x)
            }
            is Value.VStruct -> {
                // Pairs sorted by the bytes the key encodes to: length first,
                // then bytewise, which is not the order of the names.
                val pairs = v.fields.map { (k, x) -> encode(Value.VText(k)) to x }
                    .sortedWith { a, b -> ByteOrder.compare(a.first, b.first) }
                header(out, 5, pairs.size.toLong())
                for ((kb, x) in pairs) {
                    out.write(kb, 0, kb.size)
                    build(out, x)
                }
            }
        }
    }

    // Decoding -----------------------------------------------------------

    /** A value from its canonical bytes, and only from those. */
    public fun decode(input: ByteArray): Value {
        val p = Parser(input)
        val v = p.value()
        if (p.pos != input.size) throw DecodeError(Why.Trailing)
        return v
    }

    /** Whether some bytes decode, and what they decode to encodes back to exactly them. */
    public fun roundTrip(b: ByteArray): Boolean = try {
        encode(decode(b)).contentEquals(b)
    } catch (e: DecodeError) {
        false
    }

    private const val INT64_LIMIT = Long.MAX_VALUE

    private class Parser(val input: ByteArray) {
        var pos = 0

        fun byte(): Int {
            if (pos >= input.size) throw DecodeError(Why.Truncated)
            return input[pos++].toInt() and 0xff
        }

        fun peek(): Int {
            if (pos >= input.size) throw DecodeError(Why.Truncated)
            return input[pos].toInt() and 0xff
        }

        /** The next n bytes; n is compared as unsigned, since a length near 2^64 is a legal argument. */
        fun chunk(n: Long): ByteArray {
            if (java.lang.Long.compareUnsigned(n, (input.size - pos).toLong()) > 0) throw DecodeError(Why.Truncated)
            val out = input.copyOfRange(pos, pos + n.toInt())
            pos += n.toInt()
            return out
        }

        fun bigEndian(k: Int): Long {
            var acc = 0L
            for (i in 0 until k) acc = (acc shl 8) or byte().toLong()
            return acc
        }

        /** The argument of a head with additional information ai, in shortest form only. */
        fun argument(ai: Int): Long = when {
            ai < 24 -> ai.toLong()
            ai == 24 -> wide(1, 24L)
            ai == 25 -> wide(2, 0x100L)
            ai == 26 -> wide(4, 0x10000L)
            ai == 27 -> wide(8, 0x100000000L)
            ai == 31 -> throw DecodeError(Why.IndefiniteLength)
            else -> throw DecodeError(Why.NonCanonicalHead)
        }

        private fun wide(k: Int, least: Long): Long {
            val n = bigEndian(k)
            if (java.lang.Long.compareUnsigned(n, least) < 0) throw DecodeError(Why.NonCanonicalHead)
            return n
        }

        fun value(): Value {
            val ib = byte()
            val ai = ib and 0x1f
            return when (ib ushr 5) {
                0 -> {
                    val n = argument(ai)
                    if (java.lang.Long.compareUnsigned(n, INT64_LIMIT) > 0) throw DecodeError(Why.IntOutOfRange)
                    Value.VInt(n)
                }
                1 -> {
                    val n = argument(ai)
                    if (java.lang.Long.compareUnsigned(n, INT64_LIMIT) > 0) throw DecodeError(Why.IntOutOfRange)
                    Value.VInt(-1L - n)
                }
                2 -> Value.VBytes(chunk(argument(ai)))
                3 -> Value.VText(text(argument(ai)))
                4 -> {
                    var n = argument(ai)
                    val items = ArrayList<Value>()
                    // One at a time, so an absurd count fails on the input
                    // running out rather than on allocation.
                    while (n != 0L) {
                        items.add(value())
                        n--
                    }
                    Value.VList(items)
                }
                5 -> Value.VStruct(pairs(argument(ai)))
                6 -> {
                    val t = argument(ai)
                    if (t != 37L) throw DecodeError(Why.BadTag)
                    Value.VId(identifier())
                }
                else -> when (ai) {
                    20 -> Value.VBool(false)
                    21 -> Value.VBool(true)
                    22 -> Value.VNull
                    25, 26, 27 -> throw DecodeError(Why.Float)
                    31 -> throw DecodeError(Why.IndefiniteLength)
                    else -> throw DecodeError(Why.BadSimple)
                }
            }
        }

        fun text(n: Long): String {
            val b = chunk(n)
            return Utf8.decodeStrict(b) ?: throw DecodeError(Why.BadUtf8)
        }

        /** n pairs whose keys are text strings in strictly increasing order of their encoded bytes. */
        fun pairs(count: Long): Map<String, Value> {
            val out = LinkedHashMap<String, Value>()
            var prev: ByteArray? = null
            var n = count
            while (n != 0L) {
                val start = pos
                val k = key()
                val raw = input.copyOfRange(start, pos)
                if (prev != null) {
                    val c = ByteOrder.compare(raw, prev)
                    if (c == 0) throw DecodeError(Why.DuplicateKey)
                    if (c < 0) throw DecodeError(Why.UnsortedKeys)
                }
                val v = value()
                out[k] = v
                prev = raw
                n--
            }
            return out
        }

        fun key(): String {
            val ib = peek()
            if ((ib ushr 5) != 3) throw DecodeError(Why.NonTextKey)
            byte()
            return text(argument(ib and 0x1f))
        }

        fun identifier(): Id {
            val ib = byte()
            if ((ib ushr 5) != 2) throw DecodeError(Why.BadId)
            val n = argument(ib and 0x1f)
            if (n != 16L) throw DecodeError(Why.BadId)
            return Id(chunk(n))
        }
    }
}

/**
 * UTF-8, strictly: the encoder refuses a lone surrogate (a `String` can
 * hold one; a value's text cannot, and writing U+FFFD for it would make two
 * distinct strings one value), and the decoder refuses overlong forms,
 * surrogates and anything above U+10FFFF.
 */
public object Utf8 {
    public fun encode(s: String): ByteArray {
        val out = ByteArrayOutputStream(s.length)
        var i = 0
        while (i < s.length) {
            val cp = s.codePointAt(i)
            if (cp in 0xD800..0xDFFF) {
                throw IllegalStateException("text holds a lone surrogate at index $i; not a value")
            }
            when {
                cp < 0x80 -> out.write(cp)
                cp < 0x800 -> {
                    out.write(0xC0 or (cp ushr 6))
                    out.write(0x80 or (cp and 0x3F))
                }
                cp < 0x10000 -> {
                    out.write(0xE0 or (cp ushr 12))
                    out.write(0x80 or ((cp ushr 6) and 0x3F))
                    out.write(0x80 or (cp and 0x3F))
                }
                else -> {
                    out.write(0xF0 or (cp ushr 18))
                    out.write(0x80 or ((cp ushr 12) and 0x3F))
                    out.write(0x80 or ((cp ushr 6) and 0x3F))
                    out.write(0x80 or (cp and 0x3F))
                }
            }
            i += Character.charCount(cp)
        }
        return out.toByteArray()
    }

    /** The text, or null if the bytes are not valid UTF-8. */
    public fun decodeStrict(b: ByteArray): String? {
        val sb = StringBuilder(b.size)
        var i = 0
        while (i < b.size) {
            val b0 = b[i].toInt() and 0xff
            val cp: Int
            val len: Int
            when {
                b0 < 0x80 -> {
                    cp = b0
                    len = 1
                }
                b0 in 0xC2..0xDF -> {
                    if (i + 1 >= b.size) return null
                    val b1 = b[i + 1].toInt() and 0xff
                    if (b1 and 0xC0 != 0x80) return null
                    cp = ((b0 and 0x1F) shl 6) or (b1 and 0x3F)
                    len = 2
                }
                b0 in 0xE0..0xEF -> {
                    if (i + 2 >= b.size) return null
                    val b1 = b[i + 1].toInt() and 0xff
                    val b2 = b[i + 2].toInt() and 0xff
                    if (b1 and 0xC0 != 0x80 || b2 and 0xC0 != 0x80) return null
                    cp = ((b0 and 0x0F) shl 12) or ((b1 and 0x3F) shl 6) or (b2 and 0x3F)
                    if (cp < 0x800) return null
                    if (cp in 0xD800..0xDFFF) return null
                    len = 3
                }
                b0 in 0xF0..0xF4 -> {
                    if (i + 3 >= b.size) return null
                    val b1 = b[i + 1].toInt() and 0xff
                    val b2 = b[i + 2].toInt() and 0xff
                    val b3 = b[i + 3].toInt() and 0xff
                    if (b1 and 0xC0 != 0x80 || b2 and 0xC0 != 0x80 || b3 and 0xC0 != 0x80) return null
                    cp = ((b0 and 0x07) shl 18) or ((b1 and 0x3F) shl 12) or ((b2 and 0x3F) shl 6) or (b3 and 0x3F)
                    if (cp < 0x10000 || cp > 0x10FFFF) return null
                    len = 4
                }
                else -> return null
            }
            sb.appendCodePoint(cp)
            i += len
        }
        return sb.toString()
    }
}

/** SHA-256, through the platform's digest; the one JVM API beyond java.util and java.io this runtime uses. */
public object Sha256 {
    public fun hash(bytes: ByteArray): ByteArray =
        java.security.MessageDigest.getInstance("SHA-256").digest(bytes)
}
