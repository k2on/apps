// How the Rust reference prints a text where its words are part of the
// meaning: a refusal's `Display` (`store.rs`, `impl Display for Refusal`)
// writes its names with `{:?}`, and that text is what a `reject` frame and
// a trace carry. So the escaping of `<str as Debug>` is written down here
// once.
package dev.arkdb

/**
 * A text as Rust's `{:?}` writes it: in double quotes, with `\"`, `\\`, `\0`, `\t`, `\r`, `\n`, and
 * `\u{hex}` for a control character, a mark that extends a grapheme, or anything else Rust does not
 * print as itself. The test of "printable" is the platform's character type here, which agrees with
 * Rust's tables on every character a name or a message in this repository holds.
 */
public fun debugString(s: String): String {
    val sb = StringBuilder(s.length + 2)
    sb.append('"')
    var i = 0
    while (i < s.length) {
        val cp = s.codePointAt(i)
        i += Character.charCount(cp)
        when (cp) {
            0 -> sb.append("\\0")
            '\t'.code -> sb.append("\\t")
            '\r'.code -> sb.append("\\r")
            '\n'.code -> sb.append("\\n")
            '"'.code -> sb.append("\\\"")
            '\\'.code -> sb.append("\\\\")
            else ->
                if (escaped(cp)) sb.append("\\u{").append(Integer.toHexString(cp)).append('}')
                else sb.appendCodePoint(cp)
        }
    }
    return sb.append('"').toString()
}

/** A list of texts as Rust's `{:?}` writes a `Vec<String>`: `["a", "b"]`. */
public fun debugStrings(xs: List<String>): String =
    xs.joinToString(", ", "[", "]") { debugString(it) }

private fun escaped(cp: Int): Boolean {
    if (cp in 0x20..0x7e) return false
    if (cp < 0x20 || cp == 0x7f) return true
    return when (Character.getType(cp).toByte()) {
        Character.CONTROL,
        Character.FORMAT,
        Character.UNASSIGNED,
        Character.SURROGATE,
        Character.PRIVATE_USE,
        Character.LINE_SEPARATOR,
        Character.PARAGRAPH_SEPARATOR,
        Character.SPACE_SEPARATOR,
        Character.NON_SPACING_MARK,
        Character.ENCLOSING_MARK -> true
        else -> false
    }
}
