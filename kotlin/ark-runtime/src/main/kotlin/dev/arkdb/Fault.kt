// A fault: the two ways a computation stops short (Ark.Eval.Fault).
//
// `Refuse` is a verdict — a deterministic fact about an entry that every
// replica reaches (an explicit refuse, a constraint, an overflow).
// `Bug` is a bug — a module the verifier would have refused, or a native
// procedure that disagrees with Ark.Eval. They are never conflated.
package dev.arkdb

public sealed class Fault(message: String) : Exception(message) {
    /** A verdict, carrying the store's or the mutator's refusal. */
    public class Refuse(public val refusal: Refusal) : Fault(refusal.text)

    /** A bug, never a verdict. */
    public class Bug(public val text: String) : Fault(text)

    public companion object {
        public fun refuse(text: String): Fault = Refuse(Refusal.Refused(text))

        public fun refuse(v: Value): Fault = refuse(v.asText())

        public fun refuse(refusal: Refusal): Fault = Refuse(refusal)

        public fun bug(text: String): Fault = Bug(text)
    }
}

/**
 * §4.2 A refusal is a verdict about a write, reached identically by every
 * replica; it travels in the same channel as a mutator's own `refuse` and a
 * mutator body propagates it. `text` is what a peer shows: the mutator's own
 * text for `Refused`, and a sentence naming a constraint (§12.5).
 */
public sealed class Refusal {
    public data class NoSuchTable(val table: String) : Refusal()
    public data class MalformedRow(val table: String, val what: String) : Refusal()
    public data class NotNull(val table: String, val column: String) : Refusal()
    public data class UniqueViolation(val table: String, val columns: List<String>) : Refusal()
    public data class MissingParent(val table: String, val column: String, val parent: String) : Refusal()
    public data class StillReferenced(val table: String, val child: String) : Refusal()
    public data class Refused(val reason: String) : Refusal()

    /** The text a peer shows for this verdict: the sentence a `Reject` carries (`Protocol.refusalText`). */
    public val text: String
        get() = Protocol.refusalText(this)

    /** The refusal as Haskell's `show` prints it, for a vector to compare. */
    public fun show(): String = when (this) {
        is NoSuchTable -> "NoSuchTable ${HsShow.text(table)}"
        is MalformedRow -> "MalformedRow ${HsShow.text(table)} ${HsShow.text(what)}"
        is NotNull -> "NotNull ${HsShow.text(table)} ${HsShow.text(column)}"
        is UniqueViolation -> "UniqueViolation ${HsShow.text(table)} ${HsShow.texts(columns)}"
        is MissingParent -> "MissingParent ${HsShow.text(table)} ${HsShow.text(column)} ${HsShow.text(parent)}"
        is StillReferenced -> "StillReferenced ${HsShow.text(table)} ${HsShow.text(child)}"
        is Refused -> "Refused ${HsShow.text(reason)}"
    }
}

/** Haskell's `show` for `Text`, `ByteString` and `[Text]`, where the spec prints one into a message. */
public object HsShow {
    private val NAMES = arrayOf(
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "a", "b", "t", "n", "v", "f", "r", "SO", "SI",
        "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS", "RS", "US",
    )

    // One code point, with what the previous escape needs of the next one
    // (`\SO` before an `H`, or a decimal escape before a digit, wants `\&`).
    private fun escape(sb: StringBuilder, cp: Int, prev: String?) {
        val esc: String? = when {
            cp == '"'.code -> "\\\""
            cp == '\\'.code -> "\\\\"
            cp < 0x20 -> "\\" + NAMES[cp]
            cp == 0x7f -> "\\DEL"
            cp > 0x7e -> "\\" + cp
            else -> null
        }
        if (esc != null) {
            sb.append(esc)
            return
        }
        if (prev != null) {
            val digitAfterNumber = prev.length > 1 && prev[1].isDigit() && cp >= '0'.code && cp <= '9'.code
            val hAfterSo = prev == "\\SO" && cp == 'H'.code
            if (digitAfterNumber || hAfterSo) sb.append("\\&")
        }
        sb.appendCodePoint(cp)
    }

    private fun quoted(cps: IntArray): String {
        val sb = StringBuilder("\"")
        var prev: String? = null
        for (cp in cps) {
            val before = sb.length
            escape(sb, cp, prev)
            val piece = sb.substring(before)
            prev = if (piece.startsWith("\\") && piece != "\\&") piece else null
        }
        return sb.append('"').toString()
    }

    public fun text(s: String): String = quoted(s.codePoints().toArray())

    /** A `ByteString`, which Haskell shows as its bytes read as Latin-1 characters. */
    public fun bytes(b: ByteArray): String = quoted(IntArray(b.size) { b[it].toInt() and 0xff })

    public fun texts(xs: List<String>): String = xs.joinToString(",", "[", "]") { text(it) }
}
