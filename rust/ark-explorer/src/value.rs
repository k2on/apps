//! A value as the explorer shows and edits it (`docs/plan-v5.md` §1.1):
//! bytes read under a type, the type said the way a person reads it.
//!
//! v5 has no run-time value and keeps no names but a table's columns: a
//! record is a `Tuple`, an enum a `Sum`, `Bool` and `Option` are sums by
//! shape and `Text` and `Id<t>` are declarations over `Bytes`. [`resolve`]
//! turns a type of the module into a [`Ty`] a form and a cell can draw —
//! `Text` as text, `Id<playlist>` as an id of `playlist`, `Sum [(), ()]` as
//! a bool, `Sum [(), (T)]` as an option — and labels a record's fields from
//! what the module does say: a field refined by its own declaration
//! (`CreatePlaylist.name`) by that name, an id by its table, anything else
//! by its type and place. A list of rows whose types are a table's columns
//! is labelled by the table's column names ([`row_labels`]).
//!
//! [`Value`] is a value of a [`Ty`], read from bytes by [`decode`] and
//! written back by [`encode`]: the codec's own encoding, so what a form
//! sends is what `parse` (the module's form validator) reads.

use ark5::check::Types;
use ark5::codec::{put_bytes, put_int, put_variant, Reader};
use ark5::ir::Rule;
use ark5::module::Module;
use ark5::schema::Table;
use ark5::ty::Ty as Structure;
use ark5::Refusal;

/// A type as the explorer draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Text,
    Bytes,
    Bool,
    /// An id of a table's rows.
    Id(String),
    Option(Box<Ty>),
    List(Box<Ty>),
    /// A record: its fields, labelled.
    Struct(Vec<(String, Ty)>),
    /// An enum, its variants by place, or a sum with fields.
    Enum(Vec<String>),
    Sum(Vec<Vec<Ty>>),
}

impl Ty {
    /// Whether a value of this type is edited in place, on one line.
    pub fn is_scalar(&self) -> bool {
        matches!(self, Ty::Int | Ty::Text | Ty::Bytes | Ty::Bool | Ty::Id(_) | Ty::Enum(_))
    }
}

/// A value of a [`Ty`]. An option holds `Null` or its value itself.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Bytes(Vec<u8>),
    Id([u8; 16]),
    List(Vec<Value>),
    Struct(Vec<Value>),
    Variant(u8, Vec<Value>),
}

impl Value {
    pub fn text(s: impl Into<String>) -> Value {
        Value::Text(s.into())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

/// The name a type is declared by, or its constructor's: what tells a
/// table's columns apart from another record's.
pub fn decl_name(t: &Structure) -> String {
    match t {
        Structure::Named(n) => n.clone(),
        Structure::Int => "Int".into(),
        Structure::Bytes => "Bytes".into(),
        Structure::Sum(vs) if vs.len() == 2 && vs.iter().all(Vec::is_empty) => "Bool".into(),
        _ => String::new(),
    }
}

/// The module's declarations, `Text` among them.
pub fn types(m: &Module) -> Types {
    Types::new(&m.types)
}

/// A module's type as the explorer draws it.
pub fn resolve(cx: &Types, t: &Structure) -> Ty {
    resolve_at(cx, t, 0)
}

fn resolve_at(cx: &Types, t: &Structure, depth: usize) -> Ty {
    if depth > 32 {
        return Ty::Bytes;
    }
    let go = |t: &Structure| resolve_at(cx, t, depth + 1);
    match t {
        Structure::Int => Ty::Int,
        Structure::Bytes => Ty::Bytes,
        Structure::Named(n) if n == "Text" => Ty::Text,
        Structure::Named(n) if n.starts_with("Id<") && n.ends_with('>') => Ty::Id(n[3..n.len() - 1].to_string()),
        Structure::Named(n) => match cx.decl(n) {
            Some(d) if cx.texty(d) => Ty::Text,
            // A declared enum is an enum, whatever its size: only `Bool`'s
            // shape unnamed is a bool.
            Some(d) if matches!(&d.ty, Structure::Sum(vs) if vs.iter().all(Vec::is_empty)) && n != "Bool" => {
                let Structure::Sum(vs) = &d.ty else { unreachable!() };
                Ty::Enum((0..vs.len()).map(|i| format!("#{i}")).collect())
            }
            Some(d) => go(&d.ty),
            None => Ty::Bytes,
        },
        Structure::List(t) => Ty::List(Box::new(go(t))),
        Structure::Tuple(ts) => Ty::Struct(labelled(ts).into_iter().zip(ts).map(|(l, t)| (l, go(t))).collect()),
        Structure::Sum(vs) if vs.len() == 2 && vs.iter().all(Vec::is_empty) => Ty::Bool,
        Structure::Sum(vs) if vs.len() == 2 && vs[0].is_empty() && vs[1].len() == 1 => Ty::Option(Box::new(go(&vs[1][0]))),
        Structure::Sum(vs) if vs.iter().all(Vec::is_empty) => Ty::Enum((0..vs.len()).map(|i| format!("#{i}")).collect()),
        Structure::Sum(vs) => Ty::Sum(vs.iter().map(|f| f.iter().map(go).collect()).collect()),
    }
}

/// A function's input as fields: a record's (named, or a tuple as a row's
/// is), or each parameter's; and whether the input is the record of them
/// rather than the one field itself.
pub fn fields_of(cx: &Types, params: &[Structure]) -> (Vec<Structure>, bool) {
    match params {
        [Structure::Tuple(fs)] => (fs.clone(), true),
        [p @ Structure::Named(n)] => match cx.decl(n).map(|d| &d.ty) {
            Some(Structure::Tuple(fs)) => (fs.clone(), true),
            _ => (vec![p.clone()], false),
        },
        [p] => (vec![p.clone()], false),
        ps => (ps.to_vec(), true),
    }
}

/// A record's fields' labels from their types: a field's own declaration
/// (`AddSong.title`) by its name, an id by its table, the rest by what they
/// are; the same label twice is numbered.
pub fn labelled(ts: &[Structure]) -> Vec<String> {
    let base: Vec<String> = ts
        .iter()
        .map(|t| match t {
            Structure::Named(n) if n.starts_with("Id<") => n[3..n.len() - 1].to_string(),
            Structure::Named(n) => match n.rsplit_once('.') {
                Some((_, f)) => f.to_string(),
                None => n.to_lowercase(),
            },
            Structure::Int => "int".into(),
            Structure::Bytes => "bytes".into(),
            Structure::List(_) => "list".into(),
            Structure::Sum(vs) if vs.len() == 2 && vs.iter().all(Vec::is_empty) => "bool".into(),
            _ => "value".into(),
        })
        .collect();
    base.iter()
        .enumerate()
        .map(|(i, b)| match base.iter().filter(|x| *x == b).count() {
            1 => b.clone(),
            _ => format!("{b} {}", base[..=i].iter().filter(|x| *x == b).count()),
        })
        .collect()
}

/// The type of a table's columns, by their names.
pub fn row_ty(cx: &Types, t: &Table) -> Ty {
    Ty::Struct(t.columns.iter().map(|c| (c.name.clone(), column_ty(cx, c))).collect())
}

/// A column's type: its declaration, else its structure.
pub fn column_ty(cx: &Types, c: &ark5::schema::Column) -> Ty {
    match c.decl.as_str() {
        "Int" => Ty::Int,
        "Bool" => Ty::Bool,
        d if d == "Text" || d.starts_with("Id<") || cx.decl(d).is_some() => resolve(cx, &Structure::Named(d.into())),
        _ => resolve(cx, &c.ty),
    }
}

/// When the elements of a list are some table's rows — their fields'
/// declarations the table's columns', in order — the table's column names.
pub fn row_labels(schema: &ark5::schema::Schema, element: &Structure, cx: &Types) -> Option<Vec<String>> {
    let (fields, true) = fields_of(cx, std::slice::from_ref(element)) else { return None };
    columns_of(schema, &fields)
}

/// When these fields' declarations are some table's columns', in order,
/// the table's column names.
pub fn columns_of(schema: &ark5::schema::Schema, fields: &[Structure]) -> Option<Vec<String>> {
    let said: Vec<String> = fields.iter().map(decl_name).collect();
    schema
        .tables
        .iter()
        .find(|t| t.columns.iter().map(|c| c.decl.clone()).collect::<Vec<_>>() == said)
        .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
}

/// A value of `ty` from canonical bytes of it.
pub fn decode(ty: &Ty, bytes: &[u8]) -> Result<Value, Refusal> {
    let mut r = Reader::new(bytes);
    let v = take(ty, &mut r)?;
    r.end()?;
    Ok(v)
}

/// The values of a run of types, read one after another: a row's columns.
pub fn decode_all(tys: &[Ty], bytes: &[u8]) -> Result<Vec<Value>, Refusal> {
    let mut r = Reader::new(bytes);
    let vs = tys.iter().map(|t| take(t, &mut r)).collect::<Result<Vec<_>, _>>()?;
    r.end()?;
    Ok(vs)
}

fn take(ty: &Ty, r: &mut Reader) -> Result<Value, Refusal> {
    Ok(match ty {
        Ty::Int => Value::Int(r.int()?),
        Ty::Text => Value::Text(r.text()?),
        Ty::Bytes => Value::Bytes(r.bytes()?),
        Ty::Id(_) => {
            let b = r.bytes()?;
            match <[u8; 16]>::try_from(b.as_slice()) {
                Ok(id) => Value::Id(id),
                Err(_) => Value::Bytes(b),
            }
        }
        Ty::Bool => Value::Bool(r.variant(2)? == 1),
        Ty::Option(t) => match r.variant(2)? {
            0 => Value::Null,
            _ => take(t, r)?,
        },
        Ty::List(t) => Value::List(r.list(|r| take(t, r))?),
        Ty::Struct(fs) => Value::Struct(fs.iter().map(|(_, t)| take(t, r)).collect::<Result<_, _>>()?),
        Ty::Enum(vs) => Value::Variant(r.variant(vs.len())?, vec![]),
        Ty::Sum(vs) => {
            let i = r.variant(vs.len())?;
            Value::Variant(i, vs[i as usize].iter().map(|t| take(t, r)).collect::<Result<_, _>>()?)
        }
    })
}

/// A value's bytes under `ty`, or why it is not one.
pub fn encode(ty: &Ty, v: &Value) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    put(ty, v, &mut out)?;
    Ok(out)
}

fn put(ty: &Ty, v: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match (ty, v) {
        (Ty::Int, Value::Int(n)) => put_int(out, *n),
        (Ty::Text, Value::Text(t)) => put_bytes(out, t.as_bytes()),
        (Ty::Bytes | Ty::Id(_), Value::Bytes(b)) => put_bytes(out, b),
        (Ty::Id(_), Value::Id(id)) => put_bytes(out, id),
        (Ty::Bool, Value::Bool(b)) => put_variant(out, *b as u8),
        (Ty::Option(_), Value::Null) => put_variant(out, 0),
        (Ty::Option(t), v) => {
            put_variant(out, 1);
            put(t, v, out)?;
        }
        (Ty::List(t), Value::List(xs)) => {
            for x in xs {
                out.push(1);
                put(t, x, out)?;
            }
            out.push(0);
        }
        (Ty::Struct(fs), Value::Struct(xs)) if fs.len() == xs.len() => {
            for ((_, t), x) in fs.iter().zip(xs) {
                put(t, x, out)?;
            }
        }
        (Ty::Enum(vs), Value::Variant(i, _)) if (*i as usize) < vs.len() => put_variant(out, *i),
        (Ty::Sum(vs), Value::Variant(i, xs)) if vs.get(*i as usize).is_some_and(|f| f.len() == xs.len()) => {
            put_variant(out, *i);
            for (t, x) in vs[*i as usize].iter().zip(xs) {
                put(t, x, out)?;
            }
        }
        (t, v) => return Err(format!("{} is not a value of {}", text_of(v), ty_name(t))),
    }
    Ok(())
}

/// A type as a person reads it: `text`, `id(playlist)`, `option(int)`,
/// `list(text)`, `{a: int, b: text}`.
pub fn ty_name(t: &Ty) -> String {
    match t {
        Ty::Int => "int".into(),
        Ty::Text => "text".into(),
        Ty::Bytes => "bytes".into(),
        Ty::Bool => "bool".into(),
        Ty::Id(table) => format!("id({table})"),
        Ty::Option(t) => format!("option({})", ty_name(t)),
        Ty::List(t) => format!("list({})", ty_name(t)),
        Ty::Struct(fs) => format!("{{{}}}", fs.iter().map(|(n, t)| format!("{n}: {}", ty_name(t))).collect::<Vec<_>>().join(", ")),
        Ty::Enum(vs) => format!("enum({})", vs.join(" | ")),
        Ty::Sum(vs) => format!(
            "sum({})",
            vs.iter()
                .map(|f| format!("({})", f.iter().map(ty_name).collect::<Vec<_>>().join(", ")))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    }
}

/// A refinement as it is said beside a field: `trimmed`, `1 to 120 chars`,
/// `non-empty`, and a predicate by its message.
pub fn rule_said(r: &ark5::ir::Refine) -> Option<String> {
    let why = r.why.as_ref().map_or(String::new(), |w| format!(" (\u{201c}{w}\u{201d})"));
    let chars = |n: i64| if n == 1 { "char" } else { "chars" };
    Some(match &r.rule {
        Rule::Utf8 => return None,
        Rule::Trim => "trimmed".into(),
        Rule::Len(n) => format!("exactly {n} {}{why}", chars(*n)),
        Rule::LenRange(lo, hi) if *hi == i64::MAX => format!("min {lo} {}{why}", chars(*lo)),
        Rule::LenRange(0, hi) => format!("max {hi} {}{why}", chars(*hi)),
        Rule::LenRange(lo, hi) => format!("{lo} to {hi} {}{why}", chars(*hi)),
        Rule::NonEmpty => format!("non-empty{why}"),
        Rule::Refine(_) => r.why.clone().unwrap_or_else(|| "refined".into()),
    })
}

/// What a declared type is checked against, said: its own refinements and
/// those of the declarations it names, outermost first.
pub fn checks_of(cx: &Types, t: &Structure) -> Vec<String> {
    let mut out = vec![];
    let mut t = t.clone();
    for _ in 0..16 {
        let Structure::Named(n) = &t else { break };
        let Some(d) = cx.decl(n) else { break };
        out.extend(d.refine.iter().filter_map(rule_said));
        t = d.ty.clone();
    }
    out
}

/// A value as one line of text: an id as `8-4-4-4-12` hex, bytes as hex,
/// `null` for none, a list in brackets, a record in parentheses, a variant
/// by its place.
pub fn text_of(v: &Value) -> String {
    let all = |xs: &[Value]| xs.iter().map(text_of).collect::<Vec<_>>().join(", ");
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Text(t) => t.clone(),
        Value::Bytes(b) => ark5::hash::hex(b),
        Value::Id(id) => {
            let h = ark5::hash::hex(id);
            format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
        }
        Value::List(xs) => format!("[{}]", all(xs)),
        Value::Struct(xs) => format!("({})", all(xs)),
        Value::Variant(i, xs) if xs.is_empty() => format!("#{i}"),
        Value::Variant(i, xs) => format!("#{i}({})", all(xs)),
    }
}
