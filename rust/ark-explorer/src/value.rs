//! A value as the explorer shows and edits it (`docs/plan-v5.md` §1.16):
//! bytes read under a type, the type said the way a person reads it.
//!
//! [`of_type`] turns a domain's type description into a [`Ty`] a form and a
//! cell can draw — a record by its fields' names, an enum by its variants'
//! places — and [`column_ty`] a table's column, from its declared name
//! (`Text`, `Id<playlist>`, `Option<Text>`) or else its structure. A list
//! of records whose fields are a table's columns is labelled by the table's
//! column names ([`columns_of`]).
//!
//! [`Value`] is a value of a [`Ty`], read from bytes by [`decode`] and
//! written back by [`encode`] — the codec's own encoding, so what a form
//! sends is what `parse` reads — and said as one line of text by
//! [`text_of`], which [`parse_cell`] reads back: what a cell shows, its
//! editor starts with, and a form's field is typed as.

use ark::codec::{put_bytes, put_int, put_variant, Reader};
use ark::row::Type;
use ark::schema::{Column, Schema, Table};
use ark::ty::Ty as Structure;
use ark::Refusal;

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
    Enum(usize),
    Sum(Vec<Vec<Ty>>),
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
}

fn is_bool(vs: &[Vec<Structure>]) -> bool {
    vs.len() == 2 && vs.iter().all(Vec::is_empty)
}

/// A domain's type as the explorer draws it. An enum of no fields is an
/// enum, whatever its size.
pub fn of_type(t: &Type) -> Ty {
    let fields = |fs: &[(String, Type)]| fs.iter().map(|(n, t)| (n.clone(), of_type(t))).collect();
    match t {
        Type::Int => Ty::Int,
        Type::Text => Ty::Text,
        Type::Bool => Ty::Bool,
        Type::Bytes => Ty::Bytes,
        Type::Unit => Ty::Struct(vec![]),
        Type::Id(t) => Ty::Id(t.clone()),
        Type::Option(t) => Ty::Option(Box::new(of_type(t))),
        Type::List(t) => Ty::List(Box::new(of_type(t))),
        Type::Tuple(ts) => Ty::Struct(ts.iter().enumerate().map(|(i, t)| ((i + 1).to_string(), of_type(t))).collect()),
        Type::Record(_, fs) => Ty::Struct(fields(fs)),
        Type::Sum(_, vs) if vs.iter().all(|(_, fs)| fs.is_empty()) => Ty::Enum(vs.len()),
        Type::Sum(_, vs) => Ty::Sum(vs.iter().map(|(_, fs)| fs.iter().map(|(_, t)| of_type(t)).collect()).collect()),
    }
}

/// A structure as the explorer draws it, where nothing names its parts.
pub fn of_structure(t: &Structure) -> Ty {
    match t {
        Structure::Int => Ty::Int,
        Structure::Bytes | Structure::Named(_) => Ty::Bytes,
        Structure::List(t) => Ty::List(Box::new(of_structure(t))),
        Structure::Tuple(ts) => Ty::Struct(ts.iter().enumerate().map(|(i, t)| ((i + 1).to_string(), of_structure(t))).collect()),
        Structure::Sum(vs) if is_bool(vs) => Ty::Bool,
        Structure::Sum(vs) if vs.len() == 2 && vs[0].is_empty() && vs[1].len() == 1 => Ty::Option(Box::new(of_structure(&vs[1][0]))),
        Structure::Sum(vs) if vs.iter().all(Vec::is_empty) => Ty::Enum(vs.len()),
        Structure::Sum(vs) => Ty::Sum(vs.iter().map(|f| f.iter().map(of_structure).collect()).collect()),
    }
}

/// A type by its declared name and its structure: `Text`, `Int`, `Bool`,
/// `Id<t>`, `Option<…>` by name, anything else by its structure.
fn of_decl(decl: &str, ty: &Structure) -> Ty {
    match (decl, ty) {
        ("Int", _) => Ty::Int,
        ("Text", _) => Ty::Text,
        ("Bool", _) => Ty::Bool,
        (d, _) if d.starts_with("Id<") && d.ends_with('>') => Ty::Id(d[3..d.len() - 1].to_string()),
        (d, Structure::Sum(vs)) if d.starts_with("Option<") && d.ends_with('>') && vs.len() == 2 && vs[1].len() == 1 => {
            Ty::Option(Box::new(of_decl(&d[7..d.len() - 1], &vs[1][0])))
        }
        _ => of_structure(ty),
    }
}

/// The type of a table's row, its columns by their names.
pub fn row_ty(t: &Table) -> Ty {
    Ty::Struct(t.columns.iter().map(|c| (c.name.clone(), column_ty(c))).collect())
}

/// A column's type: its declaration, else its structure.
pub fn column_ty(c: &Column) -> Ty {
    of_decl(&c.decl, &c.ty)
}

/// When these fields' declared types are some table's columns', in order,
/// the table's column names.
pub fn columns_of(schema: &Schema, fields: &[(String, Type)]) -> Option<Vec<String>> {
    let said: Vec<String> = fields.iter().map(|(_, t)| t.decl()).collect();
    let t = schema.tables.iter().find(|t| t.columns.iter().map(|c| &c.decl).eq(said.iter()))?;
    Some(t.columns.iter().map(|c| c.name.clone()).collect())
}

/// A value of `ty` from canonical bytes of it.
pub fn decode(ty: &Ty, bytes: &[u8]) -> Result<Value, Refusal> {
    decode_all(std::slice::from_ref(ty), bytes).map(|mut vs| vs.remove(0))
}

/// The values of a run of types, read one after another: a row's columns.
pub fn decode_all(tys: &[Ty], bytes: &[u8]) -> Result<Vec<Value>, Refusal> {
    fn take(ty: &Ty, r: &mut Reader) -> Result<Value, Refusal> {
        let all = |ts: &[Ty], r: &mut Reader| ts.iter().map(|t| take(t, r)).collect::<Result<Vec<_>, _>>();
        Ok(match ty {
            Ty::Int => Value::Int(r.int()?),
            Ty::Text => Value::Text(r.text()?),
            Ty::Bytes => Value::Bytes(r.bytes()?),
            Ty::Id(_) => r.bytes().map(|b| <[u8; 16]>::try_from(b.as_slice()).map_or(Value::Bytes(b), Value::Id))?,
            Ty::Bool => Value::Bool(r.variant(2)? == 1),
            Ty::Option(t) => match r.variant(2)? {
                0 => Value::Null,
                _ => take(t, r)?,
            },
            Ty::List(t) => Value::List(r.list(|r| take(t, r))?),
            Ty::Struct(fs) => Value::Struct(fs.iter().map(|(_, t)| take(t, r)).collect::<Result<_, _>>()?),
            Ty::Enum(n) => Value::Variant(r.variant(*n)?, vec![]),
            Ty::Sum(vs) => {
                let i = r.variant(vs.len())?;
                Value::Variant(i, all(&vs[i as usize], r)?)
            }
        })
    }
    let mut r = Reader::new(bytes);
    let vs = tys.iter().map(|t| take(t, &mut r)).collect::<Result<Vec<_>, _>>()?;
    r.end()?;
    Ok(vs)
}

/// A value's bytes under `ty`, or why it is not one.
pub fn encode(ty: &Ty, v: &Value) -> Result<Vec<u8>, String> {
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
            (Ty::Enum(n), Value::Variant(i, _)) if (*i as usize) < *n => put_variant(out, *i),
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
    let mut out = vec![];
    put(ty, v, &mut out)?;
    Ok(out)
}

/// A type as a person reads it: `text`, `id(playlist)`, `option(int)`,
/// `list(text)`, `{a: int, b: text}`, `enum(3)`.
pub fn ty_name(t: &Ty) -> String {
    let join = |ts: &[Ty]| ts.iter().map(ty_name).collect::<Vec<_>>().join(", ");
    match t {
        Ty::Id(table) => format!("id({table})"),
        Ty::Option(t) => format!("option({})", ty_name(t)),
        Ty::List(t) => format!("list({})", ty_name(t)),
        Ty::Struct(fs) => format!(
            "{{{}}}",
            fs.iter().map(|(n, t)| format!("{n}: {}", ty_name(t))).collect::<Vec<_>>().join(", ")
        ),
        Ty::Enum(n) => format!("enum({n})"),
        Ty::Sum(vs) => format!("sum({})", vs.iter().map(|f| format!("({})", join(f))).collect::<Vec<_>>().join(" | ")),
        t => format!("{t:?}").to_lowercase(),
    }
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
        Value::Bytes(b) => ark::hash::hex(b),
        Value::Id(id) => {
            let h = ark::hash::hex(id);
            format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
        }
        Value::List(xs) => format!("[{}]", all(xs)),
        Value::Struct(xs) => format!("({})", all(xs)),
        Value::Variant(i, xs) if xs.is_empty() => format!("#{i}"),
        Value::Variant(i, xs) => format!("#{i}({})", all(xs)),
    }
}

/// An id as text — 32 hex digits, dashes anywhere, as a cell shows one and a
/// person pastes one.
pub fn parse_id(text: &str) -> Option<[u8; 16]> {
    ark::hash::unhex(&text.trim().replace('-', "")).and_then(|b| <[u8; 16]>::try_from(b).ok())
}

/// The parts of `[a, b]` or `(a, b)` — what is inside the brackets, split
/// at the commas outside any nested ones — or `None` when it is not
/// bracketed so. Each part is trimmed: a text inside one keeps no space at
/// its ends, and cannot hold a comma or a bracket.
fn parts(t: &str, open: char, close: char) -> Option<Vec<&str>> {
    let inner = t.strip_prefix(open)?.strip_suffix(close)?;
    let (mut out, mut depth, mut from) = (vec![], 0i32, 0);
    for (i, ch) in inner.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(inner[from..].trim());
    Some(if inner.trim().is_empty() { vec![] } else { out })
}

/// Text read as a value of `ty`, the way [`text_of`] writes it: an option
/// takes `null` (or nothing) as none; a text is the text; an int in
/// decimal; `true` or `false` (nothing is false); an id as 32 hex digits,
/// dashes anywhere; bytes as hex; an enum `#n` or `n`; a list `[a, b]`
/// (nothing is empty); a record `(a, b)`; a variant with fields `#n(a, b)`.
/// The reason, with `name`, where it is not one.
pub fn parse_cell(name: &str, ty: &Ty, text: &str) -> Result<Value, String> {
    let t = text.trim();
    let bad = |what: &str| format!("{name}: {what}");
    let each = |ts: &[&Ty], ps: Vec<&str>| -> Result<Vec<Value>, String> {
        if ps.len() != ts.len() {
            return Err(bad(&format!("{} values, in parentheses", ts.len())));
        }
        ts.iter()
            .zip(ps)
            .enumerate()
            .map(|(k, (t, p))| parse_cell(&format!("{name}.{}", k + 1), t, p))
            .collect()
    };
    match ty {
        Ty::Option(_) if t.is_empty() || t == "null" => Ok(Value::Null),
        Ty::Option(inner) => parse_cell(name, inner, text),
        Ty::Text => Ok(Value::text(text)),
        Ty::Int => t.parse().map(Value::Int).map_err(|_| bad("a whole number")),
        Ty::Bool if matches!(t, "" | "true" | "false") => Ok(Value::Bool(t == "true")),
        Ty::Bool => Err(bad("true or false")),
        Ty::Id(_) => parse_id(t).map(Value::Id).ok_or_else(|| bad("an id, 32 hex digits")),
        Ty::Bytes => ark::hash::unhex(t).map(Value::Bytes).ok_or_else(|| bad("hex digits")),
        Ty::Enum(n) => match t.trim_start_matches('#').parse::<u8>() {
            Ok(i) if (i as usize) < *n => Ok(Value::Variant(i, vec![])),
            _ => Err(bad(&format!("#0 to #{}", n.saturating_sub(1)))),
        },
        Ty::List(_) if t.is_empty() => Ok(Value::List(vec![])),
        Ty::List(e) => {
            let ps = parts(t, '[', ']').ok_or_else(|| bad("a list, [a, b]"))?;
            ps.into_iter()
                .enumerate()
                .map(|(k, p)| parse_cell(&format!("{name}.{}", k + 1), e, p))
                .collect::<Result<_, _>>()
                .map(Value::List)
        }
        Ty::Struct(fs) => {
            let ps = parts(t, '(', ')').ok_or_else(|| bad(&format!("{} values, in parentheses", fs.len())))?;
            each(&fs.iter().map(|(_, t)| t).collect::<Vec<_>>(), ps).map(Value::Struct)
        }
        Ty::Sum(vs) => {
            let t = t.trim_start_matches('#');
            let (i, rest) = t.split_once('(').map_or((t, String::new()), |(i, r)| (i, format!("({r}")));
            let i = i.trim().parse::<usize>().ok().filter(|i| *i < vs.len());
            let i = i.ok_or_else(|| bad(&format!("#0 to #{}, its values in parentheses", vs.len().saturating_sub(1))))?;
            each(&vs[i].iter().collect::<Vec<_>>(), parts(&rest, '(', ')').unwrap_or_default()).map(|xs| Value::Variant(i as u8, xs))
        }
    }
}
