//! Percent-encoding, for the two places a name goes into a URL: a path segment
//! and a fragment.

/// Percent-encode one segment onto `out`.
///
/// Real names are full of spaces, ampersands and the occasional `#`, and the
/// `#` is the one that is silently destructive: everything after it is a
/// fragment, so a request goes out for a path that stops mid-name and the
/// server answers 404. Everything outside RFC 3986's unreserved set is
/// escaped, which covers those, the `/` that would otherwise read as a
/// separator, and every non-ASCII byte.
pub fn encode(part: &str, out: &mut String) {
    for b in part.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

/// [`encode`], as a new string.
pub fn encoded(part: &str) -> String {
    let mut out = String::new();
    encode(part, &mut out);
    out
}

/// Percent-decode.
///
/// A truncated or malformed escape is kept as the bytes it was rather than
/// refused, and bytes that are not UTF-8 are replaced rather than panicked on:
/// a fragment somebody hand-edited is not guaranteed to be either, and a name
/// that half decodes is better than no page.
pub fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(byte) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A name as a readable segment beside an id — `Joint / Savings #2` is
/// `joint-savings-2`: lowercase ASCII letters and digits, anything else one
/// `-`, none at either end. Only for reading; a route is never found by its
/// slug, so a name with no letters in it has an empty one, and is left out.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// A row's id as a path carries it (ark's `Id::hex`): 32 hex digits, in
/// lowercase whatever case they came in; `None` for anything else, a name
/// among them.
pub fn id(segment: &str) -> Option<String> {
    (segment.len() == 32 && segment.bytes().all(|b| b.is_ascii_hexdigit())).then(|| segment.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slug is readable and has nothing a path would escape; an id is only
    /// ever 32 hex digits. Falsified by accepting 31: a name like
    /// `deadbeef…` one short would read as an id.
    #[test]
    fn a_slug_reads_and_an_id_is_hex() {
        assert_eq!(slug("Joint / Savings #2"), "joint-savings-2");
        assert_eq!(slug("  Café—au lait "), "caf-au-lait");
        assert_eq!(slug("%%%"), "");
        let hex = "0123456789ABCDEF0123456789abcdef";
        assert_eq!(id(hex).as_deref(), Some("0123456789abcdef0123456789abcdef"));
        assert_eq!(id(&hex[1..]), None);
        assert_eq!(id("Checking"), None);
    }

    /// Real libraries are full of these, and `#` is the destructive one.
    /// Falsified by adding `b'#'` to the unreserved set: the `%23` goes.
    #[test]
    fn the_awkward_characters_in_a_name_are_escaped() {
        assert_eq!(
            encoded("Boléro #1 & 2.mp3"),
            "Bol%C3%A9ro%20%231%20%26%202.mp3",
            "a raw # ends the request mid-name"
        );
        assert_eq!(decode(&encoded("Boléro / Pavane #1")), "Boléro / Pavane #1");
    }
}
