// §5 The standard library (Ark.Std): one function per Ark.IR.StdFn over
// Values. Unicode goes through the pinned tables and nothing of the
// platform; a type mismatch is a bug, a fault is a verdict.
package dev.arkdb

import dev.arkdb.std.UnicodeTables

public object Std {
    // text ---------------------------------------------------------------

    /** Strip White_Space from both ends, by code point. */
    public fun trim(a: Value): Value {
        val s = text(a, StdFn.Trim)
        var start = 0
        while (start < s.length) {
            val cp = s.codePointAt(start)
            if (!UnicodeTables.isWhiteSpace(cp)) break
            start += Character.charCount(cp)
        }
        var end = s.length
        while (end > start) {
            val cp = s.codePointBefore(end)
            if (!UnicodeTables.isWhiteSpace(cp)) break
            end -= Character.charCount(cp)
        }
        return Value.VText(s.substring(start, end))
    }

    public fun isEmpty(a: Value): Value = Value.VBool(text(a, StdFn.IsEmpty).isEmpty())

    /** Of a list of texts. */
    public fun concat(a: Value): Value {
        val xs = list(a, StdFn.Concat)
        val sb = StringBuilder()
        for (x in xs) sb.append(text(x, StdFn.Concat))
        return Value.VText(sb.toString())
    }

    /** Simple lowercase mapping, code point by code point. */
    public fun lower(a: Value): Value {
        val s = text(a, StdFn.Lower)
        val sb = StringBuilder(s.length)
        var i = 0
        while (i < s.length) {
            val cp = s.codePointAt(i)
            sb.appendCodePoint(UnicodeTables.toLowerSimple(cp))
            i += Character.charCount(cp)
        }
        return Value.VText(sb.toString())
    }

    /** Non-empty and every code point Alphabetic or numeric. */
    public fun isAlnum(a: Value): Value {
        val s = text(a, StdFn.IsAlnum)
        if (s.isEmpty()) return Value.VBool(false)
        var i = 0
        while (i < s.length) {
            val cp = s.codePointAt(i)
            if (!UnicodeTables.isAlphanumeric(cp)) return Value.VBool(false)
            i += Character.charCount(cp)
        }
        return Value.VBool(true)
    }

    /** The code points, each as a one-character text. */
    public fun chars(a: Value): Value {
        val s = text(a, StdFn.Chars)
        val out = ArrayList<Value>()
        var i = 0
        while (i < s.length) {
            val cp = s.codePointAt(i)
            out.add(Value.VText(String(Character.toChars(cp))))
            i += Character.charCount(cp)
        }
        return Value.VList(out)
    }

    /** In code points. */
    public fun textLen(a: Value): Value {
        val s = text(a, StdFn.TextLen)
        return Value.VInt(s.codePointCount(0, s.length).toLong())
    }

    public fun startsWith(t: Value, p: Value): Value =
        Value.VBool(text(t, StdFn.StartsWith).startsWith(text(p, StdFn.StartsWith)))

    /** At the first occurrence: `Some {before, after}` or `None`; an empty separator is `None`. */
    public fun splitOnce(t: Value, sep: Value): Value {
        val s = text(t, StdFn.SplitOnce)
        val d = text(sep, StdFn.SplitOnce)
        if (d.isEmpty()) return Value.VNull
        val i = s.indexOf(d)
        if (i < 0) return Value.VNull
        return Value.record("before" to Value.VText(s.substring(0, i)), "after" to Value.VText(s.substring(i + d.length)))
    }

    /** Decimal, with a leading `-` for negatives. */
    public fun textOfInt(n: Value): Value = Value.VText(int(n, StdFn.TextOfInt).toString())

    /** Lowercase, two digits per byte. */
    public fun hex(b: Value): Value = Value.VText(Hex.encode(bytes(b, StdFn.Hex)))

    // int ----------------------------------------------------------------

    public fun min(a: Value, b: Value): Value = Value.VInt(minOf(int(a, StdFn.Min), int(b, StdFn.Min)))

    public fun max(a: Value, b: Value): Value = Value.VInt(maxOf(int(a, StdFn.Max), int(b, StdFn.Max)))

    public fun clamp(x: Value, lo: Value, hi: Value): Value {
        val v = int(x, StdFn.Clamp)
        val l = int(lo, StdFn.Clamp)
        val h = int(hi, StdFn.Clamp)
        if (l > h) throw Fault.refuse("clamp: lower bound above upper bound")
        return Value.VInt(maxOf(l, minOf(h, v)))
    }

    public fun abs(n: Value): Value {
        val v = int(n, StdFn.Abs)
        if (v == Long.MIN_VALUE) throw Fault.refuse("integer overflow")
        return Value.VInt(if (v < 0) -v else v)
    }

    // hash ---------------------------------------------------------------

    /** FNV-1a, 64-bit, of a text's UTF-8 bytes, as the u64 reinterpreted. */
    public fun fnv1a64(t: Value): Value = Value.VInt(fnv1a64(Utf8.encode(text(t, StdFn.Fnv1a64))))

    public fun sha256(b: Value): Value = Value.VBytes(Sha256.hash(bytes(b, StdFn.Sha256)))

    // id -----------------------------------------------------------------

    /** Parse 8-4-4-4-12 hex; `None` if not an id. */
    public fun idOfText(t: Value): Value = Id.ofText(text(t, StdFn.IdOfText))?.let { Value.VId(it) } ?: Value.VNull

    /** Canonical lowercase 8-4-4-4-12. */
    public fun textOfId(i: Value): Value = Value.VText(id(i, StdFn.TextOfId).text)

    public fun nilId(): Value = Value.VId(Id.nil)

    /** A text's UTF-8 bytes. */
    public fun utf8(t: Value): Value = Value.VBytes(Utf8.encode(text(t, StdFn.Utf8)))

    // list and option -----------------------------------------------------

    public fun first(xs: Value): Value = list(xs, StdFn.First).firstOrNull() ?: Value.VNull

    public fun last(xs: Value): Value = list(xs, StdFn.Last).lastOrNull() ?: Value.VNull

    public fun len(xs: Value): Value = Value.VInt(list(xs, StdFn.Len).size.toLong())

    /** Under `compareValue`. */
    public fun contains(xs: Value, v: Value): Value =
        Value.VBool(list(xs, StdFn.Contains).any { compareValue(v, it) == 0 })

    public fun reverse(xs: Value): Value = Value.VList(list(xs, StdFn.Reverse).reversed())

    public fun isSome(v: Value): Value = Value.VBool(!v.isNull())

    public fun unwrapOr(v: Value, d: Value): Value = if (v.isNull()) d else v

    // By name, for the interpreter ----------------------------------------

    /** Apply a standard function to already-evaluated arguments; arity and type mismatches are bugs. */
    public fun call(f: StdFn, args: List<Value>): Value {
        if (args.size != f.arity) throw Fault.bug("Arity ${HsShow.text("$f/${args.size}")}")
        return when (f) {
            StdFn.Trim -> trim(args[0])
            StdFn.IsEmpty -> isEmpty(args[0])
            StdFn.Concat -> concat(args[0])
            StdFn.Lower -> lower(args[0])
            StdFn.IsAlnum -> isAlnum(args[0])
            StdFn.Chars -> chars(args[0])
            StdFn.TextLen -> textLen(args[0])
            StdFn.StartsWith -> startsWith(args[0], args[1])
            StdFn.SplitOnce -> splitOnce(args[0], args[1])
            StdFn.TextOfInt -> textOfInt(args[0])
            StdFn.Hex -> hex(args[0])
            StdFn.Min -> min(args[0], args[1])
            StdFn.Max -> max(args[0], args[1])
            StdFn.Clamp -> clamp(args[0], args[1], args[2])
            StdFn.Abs -> abs(args[0])
            StdFn.Fnv1a64 -> fnv1a64(args[0])
            StdFn.Sha256 -> sha256(args[0])
            StdFn.IdOfText -> idOfText(args[0])
            StdFn.TextOfId -> textOfId(args[0])
            StdFn.NilId -> nilId()
            StdFn.Utf8 -> utf8(args[0])
            StdFn.First -> first(args[0])
            StdFn.Last -> last(args[0])
            StdFn.Len -> len(args[0])
            StdFn.Contains -> contains(args[0], args[1])
            StdFn.Reverse -> reverse(args[0])
            StdFn.IsSome -> isSome(args[0])
            StdFn.UnwrapOr -> unwrapOr(args[0], args[1])
        }
    }

    // The primitives behind them ------------------------------------------

    /** FNV-1a, 64-bit: offset basis 0xcbf29ce484222325, prime 0x100000001b3, over the bytes in order. */
    public fun fnv1a64(bytes: ByteArray): Long {
        var h = -3750763034362895579L // 0xcbf29ce484222325
        for (b in bytes) {
            h = h xor (b.toLong() and 0xff)
            h *= 0x100000001b3L
        }
        return h
    }

    public fun textOfId(i: Id): String = i.text

    public fun idOfText(s: String): Id? = Id.ofText(s)

    private fun mismatch(f: StdFn): Nothing = throw Fault.bug("TypeError ${HsShow.text(f.name)}")

    private fun text(v: Value, f: StdFn): String = (v as? Value.VText)?.value ?: mismatch(f)

    private fun int(v: Value, f: StdFn): Long = (v as? Value.VInt)?.value ?: mismatch(f)

    private fun bytes(v: Value, f: StdFn): ByteArray = (v as? Value.VBytes)?.value ?: mismatch(f)

    private fun id(v: Value, f: StdFn): Id = (v as? Value.VId)?.value ?: mismatch(f)

    private fun list(v: Value, f: StdFn): List<Value> = (v as? Value.VList)?.items ?: mismatch(f)
}
