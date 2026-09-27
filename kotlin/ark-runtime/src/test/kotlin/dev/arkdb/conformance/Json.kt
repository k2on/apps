// The vectors' JSON, read straight into Values: `{"$int": "…"}`,
// `{"$bytes": "hex"}` and `{"$id": "uuid"}` are the wrappers spec/README.md
// describes; everything else is itself. A tiny parser, no dependency.
package dev.arkdb.conformance

import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.Value

object Json {
    fun parse(text: String): Value {
        val p = Parser(text)
        p.ws()
        val v = p.value()
        p.ws()
        if (p.i != text.length) p.fail("trailing input")
        return v
    }

    private class Parser(val s: String) {
        var i = 0

        fun fail(what: String): Nothing = throw IllegalArgumentException("json: $what at offset $i")

        fun ws() {
            while (i < s.length && s[i].isWhitespace()) i++
        }

        fun value(): Value {
            if (i >= s.length) fail("unexpected end")
            return when (val c = s[i]) {
                '{' -> obj()
                '[' -> arr()
                '"' -> Value.VText(str())
                't' -> lit("true", Value.VBool(true))
                'f' -> lit("false", Value.VBool(false))
                'n' -> lit("null", Value.VNull)
                else -> if (c == '-' || c.isDigit()) number() else fail("unexpected '$c'")
            }
        }

        fun lit(word: String, v: Value): Value {
            if (!s.startsWith(word, i)) fail("expected $word")
            i += word.length
            return v
        }

        // A bare number never appears in a vector (ints are wrapped), but a
        // parser that cannot read one is a parser that fails on the next spec.
        fun number(): Value {
            val start = i
            if (s[i] == '-') i++
            while (i < s.length && (s[i].isDigit() || s[i] in ".eE+-")) i++
            val t = s.substring(start, i)
            return t.toLongOrNull()?.let { Value.VInt(it) } ?: fail("not an integer: $t")
        }

        fun str(): String {
            if (s[i] != '"') fail("expected string")
            i++
            val sb = StringBuilder()
            while (true) {
                if (i >= s.length) fail("unterminated string")
                val c = s[i++]
                when (c) {
                    '"' -> return sb.toString()
                    '\\' -> {
                        val e = s[i++]
                        when (e) {
                            '"' -> sb.append('"')
                            '\\' -> sb.append('\\')
                            '/' -> sb.append('/')
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000c')
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'u' -> {
                                val h = s.substring(i, i + 4)
                                i += 4
                                sb.append(h.toInt(16).toChar())
                            }
                            else -> fail("bad escape \\$e")
                        }
                    }
                    else -> sb.append(c)
                }
            }
        }

        fun arr(): Value {
            i++ // [
            val out = ArrayList<Value>()
            ws()
            if (s[i] == ']') {
                i++
                return Value.VList(out)
            }
            while (true) {
                ws()
                out.add(value())
                ws()
                when (s[i++]) {
                    ',' -> continue
                    ']' -> return Value.VList(out)
                    else -> fail("expected , or ]")
                }
            }
        }

        fun obj(): Value {
            i++ // {
            val out = LinkedHashMap<String, Value>()
            ws()
            if (s[i] == '}') {
                i++
                return Value.VStruct(out)
            }
            while (true) {
                ws()
                val k = str()
                ws()
                if (s[i++] != ':') fail("expected :")
                ws()
                out[k] = value()
                ws()
                when (s[i++]) {
                    ',' -> continue
                    '}' -> break
                    else -> fail("expected , or }")
                }
            }
            if (out.size == 1) {
                val (k, v) = out.entries.first()
                val t = (v as? Value.VText)?.value
                when (k) {
                    "\$int" -> return Value.VInt(t?.toLong() ?: fail("\$int wants a decimal string"))
                    "\$bytes" -> return Value.VBytes(Hex.decode(t ?: fail("\$bytes wants hex")))
                    "\$id" -> return Value.VId(Id.ofText(t ?: fail("\$id wants text")) ?: fail("\$id is not an id: $t"))
                }
            }
            return Value.VStruct(out)
        }
    }
}
