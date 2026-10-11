// The JSON dialect ArkDB values cross text in (`ark::json`): the vectors are
// written in it, and so is every line a `harken-peer` reads and answers.
//
// JSON cannot say bytes, ids or 64-bit integers, so those three are wrapped —
// `{"$bytes": hex}`, `{"$id": "8-4-4-4-12"}`, `{"$int": "decimal"}` — and
// text, booleans and `null` are themselves. A value is printed on one line
// with no spaces, a struct's keys in code-point order: `Value.show`, which is
// this dialect already.
//
// `decode` reads it back and is a little wider than `json` prints, as the
// reference's is: a bare JSON integer is an `Int` too, because a person typing
// a command writes `3`, and whitespace anywhere JSON allows it is skipped. A
// fraction or an exponent is refused rather than rounded — there are no floats
// in the value model (§3). An object is a wrapper only when it has exactly one
// key and that key is one of the three; `{"$int": 3}` is an error.
package dev.arkdb

/** Why a text is not the dialect: what was wrong, and the offset it was noticed at. */
public class JsonError(public val at: Int, public val why: String) :
    Exception("$why at offset $at")

public object Json {
    /** Nesting deeper than this is refused rather than recursed into. */
    private const val DEPTH: Int = 128

    /** A value, on one line (`json::json`). */
    public fun json(v: Value): String = v.show()

    /** A JSON string: only `"`, `\` and the C0 controls escaped (`json::quoted`). */
    public fun quoted(s: String): String = Value.text(s).show()

    /** Already-printed JSON, as an array on one line (`json::array`). */
    public fun array(items: List<String>): String = items.joinToString(",", "[", "]")

    /** One value in the dialect, and nothing after it but whitespace (`json::decode`). */
    public fun decode(text: String): Value {
        val p = Parser(text.toByteArray(Charsets.UTF_8))
        val v = p.value(0)
        p.skip()
        if (p.at != p.s.size) throw p.fail("something after the value")
        return v
    }

    private class Parser(val s: ByteArray) {
        var at = 0

        fun fail(why: String): JsonError = JsonError(at, why)

        fun peek(): Int = if (at < s.size) s[at].toInt() and 0xff else -1

        fun skip() {
            while (at < s.size && peek().toChar() in " \t\n\r") at++
        }

        fun eat(c: Char) {
            skip()
            if (peek() == c.code) at++ else throw fail("expected `$c`")
        }

        fun word(w: String, v: Value): Value {
            val b = w.toByteArray()
            if (at + b.size <= s.size && (b.indices).all { s[at + it] == b[it] }) {
                at += b.size
                return v
            }
            throw fail("not a value")
        }

        fun value(depth: Int): Value {
            if (depth > DEPTH) throw fail("nested too deeply")
            skip()
            return when (val c = peek()) {
                -1 -> throw fail("expected a value, found the end")
                'n'.code -> word("null", Value.VNull)
                't'.code -> word("true", Value.VBool(true))
                'f'.code -> word("false", Value.VBool(false))
                '"'.code -> Value.VText(string())
                '['.code -> arr(depth)
                '{'.code -> obj(depth)
                else ->
                    if (c == '-'.code || c in '0'.code..'9'.code) number()
                    else throw fail("not a value")
            }
        }

        fun arr(depth: Int): Value {
            at++
            val xs = ArrayList<Value>()
            skip()
            if (peek() == ']'.code) {
                at++
                return Value.VList(xs)
            }
            while (true) {
                xs.add(value(depth + 1))
                skip()
                when (peek()) {
                    ','.code -> at++
                    ']'.code -> {
                        at++
                        return Value.VList(xs)
                    }
                    else -> throw fail("expected `,` or `]`")
                }
            }
        }

        fun obj(depth: Int): Value {
            val start = at
            at++
            val m = LinkedHashMap<String, Value>()
            skip()
            if (peek() == '}'.code) {
                at++
                return Value.VStruct(m)
            }
            while (true) {
                skip()
                if (peek() != '"'.code) throw fail("expected a key")
                val k = string()
                eat(':')
                val v = value(depth + 1)
                if (m.put(k, v) != null) throw fail("a key twice")
                skip()
                when (peek()) {
                    ','.code -> at++
                    '}'.code -> {
                        at++
                        break
                    }
                    else -> throw fail("expected `,` or `}`")
                }
            }
            if (m.size == 1) {
                val (k, v) = m.entries.first()
                fun wrapped(what: String, ok: Value?): Value =
                    ok ?: throw JsonError(start, "`$k` wraps $what")
                val t = (v as? Value.VText)?.value
                when (k) {
                    "\$int" ->
                        return wrapped(
                            if (t == null) "text" else "a decimal integer",
                            t?.let { decimal(it) }?.let { Value.VInt(it) })
                    "\$bytes" ->
                        return wrapped(
                            if (t == null) "text" else "hex",
                            t?.let { runCatching { Value.VBytes(Hex.decode(it)) }.getOrNull() })
                    "\$id" ->
                        return wrapped(
                            if (t == null) "text" else "an 8-4-4-4-12 id",
                            t?.let { Id.ofText(it) }?.let { Value.VId(it) })
                }
            }
            return Value.VStruct(m)
        }

        // Rust's `i64::from_str`: an optional sign, then digits only.
        fun decimal(t: String): Long? {
            val body = t.removePrefix("-").removePrefix("+")
            if (body.isEmpty() || !body.all { it in '0'..'9' }) return null
            return t.toLongOrNull()
        }

        fun number(): Value {
            val start = at
            if (peek() == '-'.code) at++
            while (peek() in '0'.code..'9'.code) at++
            if (peek() == '.'.code || peek() == 'e'.code || peek() == 'E'.code)
                throw fail("a fraction or an exponent: the value model has no floats")
            val digits = String(s, start, at - start, Charsets.US_ASCII)
            return digits.toLongOrNull()?.let { Value.VInt(it) }
                ?: throw JsonError(start, "not a 64-bit integer")
        }

        fun hex4(): Int {
            if (at + 4 > s.size) throw fail("a short \\u escape")
            val h = String(s, at, 4, Charsets.US_ASCII)
            val n = h.toIntOrNull(16)
            if (n == null || h.any { it == '+' || it == '-' })
                throw fail("a \\u escape that is not hex")
            at += 4
            return n
        }

        fun string(): String {
            at++
            val out = java.io.ByteArrayOutputStream()
            while (true) {
                when (val c = peek()) {
                    -1 -> throw fail("a string with no end")
                    '"'.code -> {
                        at++
                        val bytes = out.toByteArray()
                        val dec =
                            Charsets.UTF_8.newDecoder()
                                .onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
                                .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
                        return try {
                            dec.decode(java.nio.ByteBuffer.wrap(bytes)).toString()
                        } catch (e: java.nio.charset.CharacterCodingException) {
                            throw fail("a string that is not UTF-8")
                        }
                    }
                    '\\'.code -> {
                        at++
                        if (at >= s.size) throw fail("an escape with no end")
                        val e = peek()
                        at++
                        val code: Int =
                            when (e) {
                                '"'.code -> '"'.code
                                '\\'.code -> '\\'.code
                                '/'.code -> '/'.code
                                'b'.code -> 8
                                'f'.code -> 12
                                'n'.code -> '\n'.code
                                'r'.code -> '\r'.code
                                't'.code -> '\t'.code
                                'u'.code -> {
                                    val hi = hex4()
                                    if (hi in 0xd800 until 0xdc00) {
                                        if (!(peek() == '\\'.code &&
                                            at + 1 < s.size &&
                                            s[at + 1] == 'u'.code.toByte()))
                                            throw fail("half a surrogate pair")
                                        at += 2
                                        val lo = hex4()
                                        if (lo !in 0xdc00 until 0xe000)
                                            throw fail("half a surrogate pair")
                                        0x10000 + ((hi - 0xd800) shl 10) + (lo - 0xdc00)
                                    } else if (hi in 0xdc00 until 0xe000) {
                                        throw fail("a \\u escape that is no character")
                                    } else hi
                                }
                                else -> throw fail("an unknown escape")
                            }
                        out.write(String(Character.toChars(code)).toByteArray(Charsets.UTF_8))
                    }
                    else -> {
                        if (c < ' '.code) throw fail("a control character in a string")
                        out.write(c)
                        at++
                    }
                }
            }
        }
    }
}
