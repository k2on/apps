//! The JSON the vectors are written in (`spec/README.md`, "The vectors"),
//! printed by hand so that the bytes are this file's to decide.
//!
//! JSON cannot say bytes, ids or 64-bit integers, so those three are
//! wrapped — `{"$bytes": hex}`, `{"$id": "8-4-4-4-12"}`, `{"$int":
//! "decimal"}` — and text, booleans and `null` are themselves. A value is
//! printed on one line with no spaces, a struct's keys in code-point order
//! (the order the value already holds them in). A vector file is an object
//! of named parts, one per line, two-space indented, ending in a newline:
//! the layout `spec/app/Vectors.hs` wrote, kept so that a regeneration that
//! changes nothing shows nothing in a diff.

use ark::stdlib::text_of_id;
use ark::value::{hex, Value};

/// A value, on one line.
pub fn json(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => format!("{{\"$int\":\"{n}\"}}"),
        Value::Text(t) => quoted(t),
        Value::Bytes(b) => format!("{{\"$bytes\":\"{}\"}}", hex(b)),
        Value::Id(i) => format!("{{\"$id\":\"{}\"}}", text_of_id(i)),
        Value::List(xs) => array(xs.iter().map(json)),
        Value::Struct(m) => {
            let fields: Vec<String> = m.iter().map(|(k, x)| format!("{}:{}", quoted(k), json(x))).collect();
            format!("{{{}}}", fields.join(","))
        }
    }
}

/// A JSON string. Only `"`, `\` and the C0 controls are escaped; everything
/// else, non-ASCII included, is written as itself in UTF-8.
pub fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Already-printed JSON, as an array on one line.
pub fn array(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}

/// A vector's named parts, one per line. Nested inside an array it keeps
/// its own newlines, trailing one included, as the files have always had.
pub fn obj(parts: &[(&str, String)]) -> String {
    let lines: Vec<String> = parts.iter().map(|(k, v)| format!("  \"{k}\": {v}")).collect();
    format!("{{\n{}\n}}\n", lines.join(",\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialect in one value: every wrapper, an escape of each kind,
    /// and a key order that is not insertion order. Falsified by printing
    /// `\u` escapes in uppercase hex, which `spec/vectors` never has.
    #[test]
    fn the_dialect() {
        let v = Value::record(vec![
            ("b", Value::List(vec![Value::int(-1), Value::Bytes(vec![0xab, 1]), Value::Id([0; 16])])),
            ("a", Value::text("q\"\\\u{1f}水")),
            ("c", Value::Null),
            ("d", Value::Bool(true)),
        ]);
        assert_eq!(
            json(&v),
            r#"{"a":"q\"\\\u001f水","b":[{"$int":"-1"},{"$bytes":"ab01"},{"$id":"00000000-0000-0000-0000-000000000000"}],"c":null,"d":true}"#
        );
        assert_eq!(obj(&[("x", "1".into()), ("y", "[]".into())]), "{\n  \"x\": 1,\n  \"y\": []\n}\n");
    }
}
