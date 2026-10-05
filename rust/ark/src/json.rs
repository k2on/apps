//! The JSON dialect ArkDB values cross text in: the vectors are written in
//! it (`spec/README.md`, "The vectors"), and so is every line `harken-peer`
//! reads and answers — one copy, here, so the two cannot drift.
//!
//! JSON cannot say bytes, ids or 64-bit integers, so those three are
//! wrapped — `{"$bytes": hex}`, `{"$id": "8-4-4-4-12"}`, `{"$int":
//! "decimal"}` — and text, booleans and `null` are themselves. A value is
//! printed on one line with no spaces, a struct's keys in code-point order
//! (the order the value already holds them in), by hand, so that the bytes
//! are this file's to decide. A vector file is an object of named parts,
//! one per line, two-space indented, ending in a newline ([`obj`]), so that
//! a regeneration that changes nothing shows nothing in a diff.
//!
//! [`decode`] reads the dialect back, and is deliberately a little wider
//! than [`json`] prints: a bare JSON integer is an `Int` too, because a
//! person typing a command at a peer writes `3` and not
//! `{"$int":"3"}`, and whitespace anywhere JSON allows it is skipped. A
//! fraction or an exponent is refused rather than rounded — there are no
//! floats in the value model (§3). An object is a wrapper only when it has
//! exactly one key and that key is one of the three; `{"$int": 3}` (a
//! number, not text) is an error rather than a struct with a strange field.

use std::collections::BTreeMap;

use crate::stdlib::{id_of_text, text_of_id};
use crate::value::{decode_hex, hex, Value};

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

/// Why a text is not the dialect: what was wrong, and the byte offset it
/// was noticed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub at: usize,
    pub why: String,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.why, self.at)
    }
}

impl std::error::Error for JsonError {}

/// One value in the dialect, and nothing after it but whitespace.
pub fn decode(text: &str) -> Result<Value, JsonError> {
    let mut p = Parser { s: text.as_bytes(), at: 0 };
    let v = p.value(0)?;
    p.skip();
    if p.at != p.s.len() {
        return Err(p.fail("something after the value"));
    }
    Ok(v)
}

/// Nesting deeper than this is refused rather than recursed into: a line
/// from a pipe is not a reason for a stack overflow.
const DEPTH: usize = 128;

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn fail(&self, why: &str) -> JsonError {
        JsonError {
            at: self.at,
            why: why.into(),
        }
    }

    fn skip(&mut self) {
        while matches!(self.s.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), JsonError> {
        self.skip();
        if self.s.get(self.at) == Some(&c) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.fail(&format!("expected `{}`", c as char)))
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, JsonError> {
        if self.s[self.at..].starts_with(w.as_bytes()) {
            self.at += w.len();
            Ok(v)
        } else {
            Err(self.fail("not a value"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > DEPTH {
            return Err(self.fail("nested too deeply"));
        }
        self.skip();
        match self.s.get(self.at) {
            None => Err(self.fail("expected a value, found the end")),
            Some(b'n') => self.word("null", Value::Null),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::from),
            Some(b'[') => {
                self.at += 1;
                let mut xs = vec![];
                self.skip();
                if self.s.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Value::from(xs));
                }
                loop {
                    xs.push(self.value(depth + 1)?);
                    self.skip();
                    match self.s.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Value::from(xs));
                        }
                        _ => return Err(self.fail("expected `,` or `]`")),
                    }
                }
            }
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.fail("not a value")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        let start = self.at;
        self.at += 1;
        let mut m: BTreeMap<String, Value> = BTreeMap::new();
        self.skip();
        if self.s.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Value::from(m));
        }
        loop {
            self.skip();
            if self.s.get(self.at) != Some(&b'"') {
                return Err(self.fail("expected a key"));
            }
            let k = self.string()?;
            self.eat(b':')?;
            let v = self.value(depth + 1)?;
            if m.insert(k, v).is_some() {
                return Err(self.fail("a key twice"));
            }
            self.skip();
            match self.s.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(self.fail("expected `,` or `}`")),
            }
        }
        if m.len() == 1 {
            let (k, v) = m.iter().next().expect("one field");
            let wrapped = |what: &str, ok: Option<Value>| {
                ok.ok_or_else(|| JsonError {
                    at: start,
                    why: format!("`{k}` wraps {what}"),
                })
            };
            match (k.as_str(), v) {
                ("$int", Value::Text(s)) => return wrapped("a decimal integer", s.parse().ok().map(Value::Int)),
                ("$bytes", Value::Text(s)) => return wrapped("hex", decode_hex(s).map(Value::bytes)),
                ("$id", Value::Text(s)) => return wrapped("an 8-4-4-4-12 id", id_of_text(s).map(Value::Id)),
                ("$int" | "$bytes" | "$id", _) => return wrapped("text", None),
                _ => {}
            }
        }
        Ok(Value::from(m))
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.at;
        if self.s.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        while matches!(self.s.get(self.at), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if matches!(self.s.get(self.at), Some(b'.' | b'e' | b'E')) {
            return Err(self.fail("a fraction or an exponent: the value model has no floats"));
        }
        let digits = std::str::from_utf8(&self.s[start..self.at]).expect("ASCII");
        digits.parse().map(Value::Int).map_err(|_| JsonError {
            at: start,
            why: "not a 64-bit integer".into(),
        })
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let h = self.s.get(self.at..self.at + 4).ok_or_else(|| self.fail("a short \\u escape"))?;
        let h = std::str::from_utf8(h).map_err(|_| self.fail("a \\u escape that is not hex"))?;
        let n = u32::from_str_radix(h, 16).map_err(|_| self.fail("a \\u escape that is not hex"))?;
        self.at += 4;
        Ok(n)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1;
        let mut out: Vec<u8> = vec![];
        loop {
            match self.s.get(self.at) {
                None => return Err(self.fail("a string with no end")),
                Some(b'"') => {
                    self.at += 1;
                    return String::from_utf8(out).map_err(|_| self.fail("a string that is not UTF-8"));
                }
                Some(b'\\') => {
                    self.at += 1;
                    let c = *self.s.get(self.at).ok_or_else(|| self.fail("an escape with no end"))?;
                    self.at += 1;
                    let ch = match c {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&hi) {
                                // A surrogate pair: the low half must follow.
                                if !self.s[self.at..].starts_with(b"\\u") {
                                    return Err(self.fail("half a surrogate pair"));
                                }
                                self.at += 2;
                                let lo = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&lo) {
                                    return Err(self.fail("half a surrogate pair"));
                                }
                                0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)
                            } else {
                                hi
                            };
                            char::from_u32(code).ok_or_else(|| self.fail("a \\u escape that is no character"))?
                        }
                        _ => return Err(self.fail("an unknown escape")),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                Some(c) if *c < b' ' => return Err(self.fail("a control character in a string")),
                Some(c) => {
                    out.push(*c);
                    self.at += 1;
                }
            }
        }
    }
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
            ("b", Value::list(vec![Value::int(-1), Value::bytes(vec![0xab, 1]), Value::Id([0; 16])])),
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

    /// What is printed reads back as itself, for every kind of value and
    /// the integer extremes. Falsified by reading `$int` with `i32`: the
    /// extremes fail.
    #[test]
    fn what_is_printed_reads_back() {
        let mut id = [0u8; 16];
        id[0] = 0xde;
        id[15] = 0x01;
        let v = Value::record(vec![
            ("ints", Value::list(vec![Value::int(i64::MIN), Value::int(0), Value::int(i64::MAX)])),
            ("text", Value::text("tab\there \"quoted\" \\ 水 \u{1f3b5} \u{0}")),
            ("bytes", Value::bytes(vec![0, 255, 16])),
            ("id", Value::Id(id)),
            ("empty", Value::list(vec![])),
            ("nested", Value::record(vec![("x", Value::Null), ("y", Value::Bool(false))])),
            ("nothing", Value::record::<String>(vec![])),
        ]);
        assert_eq!(decode(&json(&v)).unwrap(), v);
    }

    /// The wider half: bare integers, whitespace, escapes JSON has and the
    /// printer never writes. Falsified by treating a bare number as text.
    #[test]
    fn a_person_may_type_it_plainly() {
        let v = decode(" { \"n\" : 3 , \"s\":\"a\\/b\\u00e9\\ud83c\\udfb5\\n\", \"l\":[ -7 ,true,null] }\n").unwrap();
        assert_eq!(
            v,
            Value::record(vec![
                ("n", Value::int(3)),
                ("s", Value::text("a/bé\u{1f3b5}\n")),
                ("l", Value::list(vec![Value::int(-7), Value::Bool(true), Value::Null])),
            ])
        );
        // One key that merely looks like a wrapper's is a struct.
        assert!(matches!(decode(r#"{"$other":"x"}"#).unwrap(), Value::Struct(_)));
        // A wrapper beside another key is a struct too.
        assert!(matches!(decode(r#"{"$int":"1","b":2}"#).unwrap(), Value::Struct(_)));
    }

    /// What is not the dialect is refused, with where. Falsified by
    /// accepting a trailing value (the first case parses).
    #[test]
    fn what_is_not_the_dialect_is_refused() {
        for bad in [
            "1 2",
            "1.5",
            "1e3",
            "99999999999999999999",
            r#"{"$int":3}"#,
            r#"{"$int":"x"}"#,
            r#"{"$bytes":"abc"}"#,
            r#"{"$id":"nope"}"#,
            r#"{"a":1,"a":2}"#,
            "[1,]",
            "\"open",
            "\"\u{1}\"",
            "\"\\ud800\"",
            "",
            "nul",
        ] {
            assert!(decode(bad).is_err(), "{bad:?} was read");
        }
        let deep = "[".repeat(DEPTH + 2) + &"]".repeat(DEPTH + 2);
        assert!(decode(&deep).is_err(), "nesting is bounded");
        assert_eq!(decode("[1,]").unwrap_err().at, 3);
    }
}
