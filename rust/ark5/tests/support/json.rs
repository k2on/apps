//! The JSON dialect of `rust/ark/src/json.rs`, printed under a type: the
//! human form of the vectors (`spec/README.md`, "The vectors"). `{"$int":
//! "decimal"}`, `{"$bytes": hex}`, `{"$id": "8-4-4-4-12"}`, text as itself,
//! an object's keys in code-point order, a vector file one named part per
//! line. A row is a tuple, so it prints as an array in schema order, each
//! column by its declared type's name: `Int`, `Text`, `Id<t>`, else bytes.

#![allow(dead_code)]

use ark5::codec::{skip, Reader};
use ark5::hash::hex;
use ark5::schema::{Schema, Space, Table};
use ark5::ty::Ty;

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
    format!("{{\"$int\":\"{n}\"}}")
}

fn id(b: &[u8]) -> String {
    let h = hex(b);
    format!("{{\"$id\":\"{}-{}-{}-{}-{}\"}}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// One column's value, read from the reader by its declared type.
fn column(decl: &str, ty: &Ty, r: &mut Reader) -> String {
    match ty {
        Ty::Int => int(r.int().unwrap()),
        Ty::Bytes if decl == "Text" => quoted(&r.text().unwrap()),
        Ty::Bytes if decl.starts_with("Id<") => id(&r.bytes().unwrap()),
        Ty::Bytes => format!("{{\"$bytes\":\"{}\"}}", hex(&r.bytes().unwrap())),
        _ => {
            let s = r.at;
            skip(ty, r).unwrap();
            format!("{{\"$bytes\":\"{}\"}}", hex(&r.bytes[s..r.at]))
        }
    }
}

/// A row, as the tuple it is.
pub fn row(t: &Table, b: &[u8]) -> String {
    let mut r = Reader::new(b);
    array(t.columns.iter().map(|c| column(&c.decl, &c.ty, &mut r)))
}

pub fn ty(t: &Ty) -> String {
    let tag = |s: &str| quoted(s);
    match t {
        Ty::Int => object(&[("t", tag("int"))]),
        Ty::Bytes => object(&[("t", tag("bytes"))]),
        Ty::List(e) => object(&[("of", ty(e)), ("t", tag("list"))]),
        Ty::Tuple(ts) => object(&[("fields", array(ts.iter().map(ty))), ("t", tag("tuple"))]),
        Ty::Sum(vs) => object(&[("t", tag("sum")), ("variants", array(vs.iter().map(|v| array(v.iter().map(ty)))))]),
    }
}

pub fn schema(s: &Schema) -> String {
    array(s.tables.iter().map(|t| {
        let name = |i: &usize| quoted(&t.columns[*i].name);
        object(&[
            (
                "columns",
                array(t.columns.iter().map(|c| {
                    let default = c.default.as_ref().map_or("null".into(), |d| format!("{{\"$bytes\":\"{}\"}}", hex(d)));
                    object(&[
                        ("decl", quoted(&c.decl)),
                        ("default", default),
                        ("name", quoted(&c.name)),
                        ("ty", ty(&c.ty)),
                    ])
                })),
            ),
            (
                "indexes",
                array(
                    t.indexes
                        .iter()
                        .map(|i| object(&[("columns", array(i.columns.iter().map(name))), ("unique", i.unique.to_string())])),
                ),
            ),
            ("key", int(t.key as i64)),
            ("name", quoted(&t.name)),
            (
                "refs",
                array(t.refs.iter().map(|r| object(&[("column", name(&r.column)), ("table", quoted(&r.table))]))),
            ),
            ("space", quoted(if t.space == Space::Durable { "durable" } else { "volatile" })),
        ])
    }))
}
