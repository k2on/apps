//! The vectors' JSON: a value is its own JSON (`ark::json`), a row its
//! object of columns as the store holds it, an object's keys in code-point
//! order where the vector writer builds it, a vector file one named part
//! per line.

#![allow(dead_code)]

use ark::hash::hex;
use ark::schema::{Schema, Table};

/// A JSON string: only `"`, `\` and the C0 controls escaped.
pub fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
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

pub fn array(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}

/// An object from parts already in code-point order of their keys.
pub fn object(parts: &[(&str, String)]) -> String {
    debug_assert!(parts.windows(2).all(|w| w[0].0 < w[1].0), "keys in code-point order");
    format!(
        "{{{}}}",
        parts.iter().map(|(k, v)| format!("{}:{v}", quoted(k))).collect::<Vec<_>>().join(",")
    )
}

/// A vector's named parts, one per line.
pub fn obj(parts: &[(&str, String)]) -> String {
    let lines: Vec<String> = parts.iter().map(|(k, v)| format!("  \"{k}\": {v}")).collect();
    format!("{{\n{}\n}}\n", lines.join(",\n"))
}

pub fn int(n: i64) -> String {
    n.to_string()
}

/// A row: its JSON, as it is.
pub fn row(_: &Table, b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("a row is JSON")
}

/// The schema: its JSON, as its hash is taken over.
pub fn schema(s: &Schema) -> String {
    serde_json::to_string(s).unwrap()
}

pub fn hexed(b: &[u8]) -> String {
    quoted(&hex(b))
}

/// A store as the dialect writes one: tables by name in code-point order,
/// rows in key order.
pub fn store(sch: &Schema, s: &ark::store::Store) -> String {
    let mut tables: Vec<_> = sch.tables.iter().collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    let parts: Vec<(&str, String)> = tables.iter().map(|t| (&t.name[..], array(s.rows(&t.name).map(|r| row(t, r))))).collect();
    object(&parts)
}

/// A change in the protocol's fact form.
pub fn change(sch: &Schema, c: &ark::store::Change) -> String {
    use ark::store::Change;
    let t = sch.table(c.table()).unwrap().1;
    let tag = |s: &str| quoted(s);
    match c {
        Change::Add { row: r, .. } => object(&[("row", row(t, r)), ("t", tag("add")), ("table", quoted(&t.name))]),
        Change::Remove { row: r, .. } => object(&[("row", row(t, r)), ("t", tag("remove")), ("table", quoted(&t.name))]),
        Change::Edit { old, new, .. } => object(&[("new", row(t, new)), ("old", row(t, old)), ("t", tag("edit")), ("table", quoted(&t.name))]),
    }
}
