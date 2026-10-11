// §5 The standard library (`rust/ark/src/stdlib.rs`): everything a function
// may compute that is not an operator or a read. `trim`, `lower` and
// `is_alnum` consult the pinned Unicode tables and nothing of the platform.
// Text is walked by code point, never by UTF-16 unit.
package dev.arkdb

/**
 * A fault a standard function reports (`stdlib::StdError`): `Arity` and `TypeMismatch` are bugs.
 */
public sealed class StdError : Exception(null, null, false, false) {
    public class Arity(public val fn: StdFn, public val n: Int) : StdError()

    public class TypeMismatch(public val fn: StdFn) : StdError()

    /** A deterministic verdict. */
    public class Fault(public val why: String) : StdError()
}

public object Std {
    /** `stdlib::std`: a standard function applied to evaluated arguments, or a `StdError`. */
    public fun apply(f: StdFn, args: List<Value>): Value {
        if (args.size != f.arity) throw StdError.Arity(f, args.size)
        val a = args.getOrNull(0)
        val b = args.getOrNull(1)
        val c = args.getOrNull(2)
        fun mismatch(): Nothing = throw StdError.TypeMismatch(f)
        return when (f) {
            StdFn.Trim -> Value.VText(trim((a as? Value.VText ?: mismatch()).value))
            StdFn.IsEmpty -> Value.VBool((a as? Value.VText ?: mismatch()).value.isEmpty())
            StdFn.Concat -> {
                val xs = (a as? Value.VList ?: mismatch()).items
                val sb = StringBuilder()
                for (x in xs) sb.append((x as? Value.VText ?: mismatch()).value)
                Value.VText(sb.toString())
            }
            StdFn.Lower -> Value.VText(fold((a as? Value.VText ?: mismatch()).value))
            StdFn.IsAlnum -> {
                val t = (a as? Value.VText ?: mismatch()).value
                Value.VBool(
                    t.isNotEmpty() && t.codePoints().allMatch(UnicodeTables::isAlphanumeric))
            }
            StdFn.Chars -> {
                val t = (a as? Value.VText ?: mismatch()).value
                Value.VList(
                    t.codePoints().toArray().map { Value.VText(String(Character.toChars(it))) })
            }
            StdFn.TextLen -> {
                val t = (a as? Value.VText ?: mismatch()).value
                Value.VInt(t.codePointCount(0, t.length).toLong())
            }
            StdFn.StartsWith -> {
                if (a !is Value.VText || b !is Value.VText) mismatch()
                Value.VBool(a.value.startsWith(b.value))
            }
            StdFn.SplitOnce -> {
                if (a !is Value.VText || b !is Value.VText) mismatch()
                val sep = b.value
                if (sep.isEmpty()) return Value.VNull
                val at = a.value.indexOf(sep)
                if (at < 0) Value.VNull
                else
                    Value.record(
                        "before" to Value.VText(a.value.substring(0, at)),
                        "after" to Value.VText(a.value.substring(at + sep.length)))
            }
            StdFn.TextOfInt -> Value.VText((a as? Value.VInt ?: mismatch()).value.toString())
            StdFn.Hex -> Value.VText(dev.arkdb.Hex.encode((a as? Value.VBytes ?: mismatch()).value))
            StdFn.Min -> {
                if (a !is Value.VInt || b !is Value.VInt) mismatch()
                Value.VInt(minOf(a.value, b.value))
            }
            StdFn.Max -> {
                if (a !is Value.VInt || b !is Value.VInt) mismatch()
                Value.VInt(maxOf(a.value, b.value))
            }
            StdFn.Clamp -> {
                if (a !is Value.VInt || b !is Value.VInt || c !is Value.VInt) mismatch()
                if (b.value > c.value) throw StdError.Fault("clamp: lower bound above upper bound")
                Value.VInt(maxOf(b.value, minOf(c.value, a.value)))
            }
            StdFn.Abs -> {
                val n = (a as? Value.VInt ?: mismatch()).value
                if (n == Long.MIN_VALUE) throw StdError.Fault("integer overflow")
                Value.VInt(kotlin.math.abs(n))
            }
            StdFn.Fnv1a64 ->
                Value.VInt(fnv1a64(Utf8.encode((a as? Value.VText ?: mismatch()).value)))
            StdFn.Sha256 ->
                Value.VBytes(dev.arkdb.Sha256.hash((a as? Value.VBytes ?: mismatch()).value))
            StdFn.IdOfText ->
                idOfText((a as? Value.VText ?: mismatch()).value)?.let { Value.VId(it) }
                    ?: Value.VNull
            StdFn.TextOfId -> Value.VText((a as? Value.VId ?: mismatch()).value.text)
            StdFn.NilId -> Value.VId(Id.nil)
            StdFn.Utf8 -> Value.VBytes(Utf8.encode((a as? Value.VText ?: mismatch()).value))
            StdFn.First -> (a as? Value.VList ?: mismatch()).items.firstOrNull() ?: Value.VNull
            StdFn.Last -> (a as? Value.VList ?: mismatch()).items.lastOrNull() ?: Value.VNull
            StdFn.Len -> Value.VInt((a as? Value.VList ?: mismatch()).items.size.toLong())
            StdFn.Contains -> Value.VBool((a as? Value.VList ?: mismatch()).items.any { it == b })
            StdFn.Reverse -> Value.VList((a as? Value.VList ?: mismatch()).items.reversed())
            StdFn.IsSome -> Value.VBool(a !is Value.VNull)
            StdFn.UnwrapOr -> if (a is Value.VNull) b!! else a!!
            StdFn.Unwrap -> if (a is Value.VNull) throw StdError.Fault("unwrapped none") else a!!
        }
    }

    /** `trim_matches(is_white_space)`: white space by the pinned tables, off both ends. */
    public fun trim(t: String): String {
        var start = 0
        var end = t.length
        while (start < end) {
            val cp = t.codePointAt(start)
            if (!UnicodeTables.isWhiteSpace(cp)) break
            start += Character.charCount(cp)
        }
        while (end > start) {
            val cp = t.codePointBefore(end)
            if (!UnicodeTables.isWhiteSpace(cp)) break
            end -= Character.charCount(cp)
        }
        return t.substring(start, end)
    }

    /** FNV-1a, 64-bit, over the bytes in order, as the two's-complement `Int` it is stored as. */
    public fun fnv1a64(bytes: ByteArray): Long {
        var h = -0x340d631b7bdddcdbL // 0xcbf29ce484222325
        for (b in bytes) {
            h = h xor (b.toLong() and 0xff)
            h *= 0x00000100000001b3L
        }
        return h
    }

    /** `id_of_text`: 8-4-4-4-12 hex in either case; anything else is `null`. */
    public fun idOfText(t: String): Id? = Id.ofText(t)
}
