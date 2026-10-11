// How the Rust reference prints a text where its words are part of the
// meaning: a refusal's `Display` (`store.rs`, `impl Display for Refusal`)
// writes its names with `{:?}`, and that text is what a `reject` frame and
// a trace carry. So the escaping of `<str as Debug>` is written down here
// once.

/// A text as Rust's `{:?}` writes it: in double quotes, with `\"`, `\\`,
/// `\0`, `\t`, `\r`, `\n`, and `\u{hex}` for a control character, a mark that
/// extends a grapheme, or anything else Rust does not print as itself. The
/// test of "printable" is the platform's general category here, which agrees
/// with Rust's tables on every character a name or a message in this
/// repository holds.
public func debugString(_ s: String) -> String {
    var out = "\""
    for u in s.unicodeScalars {
        switch u.value {
        case 0: out += "\\0"
        case 9: out += "\\t"
        case 13: out += "\\r"
        case 10: out += "\\n"
        case 34: out += "\\\""
        case 92: out += "\\\\"
        default:
            if escaped(u) {
                out += "\\u{" + String(u.value, radix: 16) + "}"
            } else {
                out.unicodeScalars.append(u)
            }
        }
    }
    out += "\""
    return out
}

/// A list of texts as Rust's `{:?}` writes a `Vec<String>`: `["a", "b"]`.
public func debugStrings(_ xs: [String]) -> String {
    "[" + xs.map(debugString).joined(separator: ", ") + "]"
}

private func escaped(_ u: Unicode.Scalar) -> Bool {
    let cp = u.value
    if cp >= 0x20 && cp <= 0x7e { return false }
    if cp < 0x20 || cp == 0x7f { return true }
    switch u.properties.generalCategory {
    case .control, .format, .unassigned, .surrogate, .privateUse, .lineSeparator, .paragraphSeparator,
        .spaceSeparator, .nonspacingMark, .enclosingMark:
        return true
    default:
        return false
    }
}
