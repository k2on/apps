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
        Ty::Named(n) => object(&[("name", quoted(n)), ("t", tag("named"))]),
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

pub fn hexed(b: &[u8]) -> String {
    quoted(&hex(b))
}

/// A value of the language under its type: an int, text as itself, an id
/// (`Id<…>`) as one, other bytes as `$bytes`, a list or a tuple as an
/// array, a sum as `{"fields", "variant"}`.
pub fn val(cx: &ark5::check::Types, t: &Ty, v: &ark5::interp::V) -> String {
    use ark5::interp::V;
    match (t, v) {
        (Ty::Named(n), V::Bytes(b)) if n.starts_with("Id<") && b.len() == 16 => id(b),
        (Ty::Named(n), V::Bytes(b)) if cx.decl(n).is_some_and(|d| cx.texty(d)) => quoted(&String::from_utf8_lossy(b)),
        (Ty::Named(n), v) => val(cx, &cx.decl(n).unwrap().ty, v),
        (_, V::Int(n)) => int(*n),
        (_, V::Bytes(b)) => format!("{{\"$bytes\":\"{}\"}}", hex(b)),
        (Ty::List(e), V::List(xs)) => array(xs.iter().map(|x| val(cx, e, x))),
        (Ty::Tuple(ts), V::Tup(xs)) => array(ts.iter().zip(xs).map(|(t, x)| val(cx, t, x))),
        (Ty::Sum(vs), V::Sum(i, xs)) => object(&[
            ("fields", array(vs[*i as usize].iter().zip(xs).map(|(t, x)| val(cx, t, x)))),
            ("variant", int(*i as i64)),
        ]),
        (t, v) => panic!("{v:?} is not a {t:?}"),
    }
}

/// A store as the dialect writes one: tables by name in code-point order,
/// rows in key order.
pub fn store(sch: &Schema, s: &ark5::store::Store) -> String {
    let mut tables: Vec<_> = sch.tables.iter().collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    let parts: Vec<(&str, String)> = tables.iter().map(|t| (&t.name[..], array(s.rows(&t.name).map(|r| row(t, r))))).collect();
    object(&parts)
}

/// A change in the protocol's fact form.
pub fn change(sch: &Schema, c: &ark5::store::Change) -> String {
    use ark5::store::Change;
    let t = sch.table(c.table()).unwrap().1;
    let tag = |s: &str| quoted(s);
    match c {
        Change::Add { row: r, .. } => object(&[("row", row(t, r)), ("t", tag("add")), ("table", quoted(&t.name))]),
        Change::Remove { row: r, .. } => object(&[("row", row(t, r)), ("t", tag("remove")), ("table", quoted(&t.name))]),
        Change::Edit { old, new, .. } => object(&[("new", row(t, new)), ("old", row(t, old)), ("t", tag("edit")), ("table", quoted(&t.name))]),
    }
}
