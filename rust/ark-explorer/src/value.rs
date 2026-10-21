//! A value as the explorer shows and edits it (`docs/plan-v5.md` §1.16):
//! a value's JSON read under its type, the type said the way a person reads
//! it.
//!
//! [`of_type`] turns a domain's type description into a [`Ty`] a form and a
//! cell can draw — a record by its fields' names, an enum by its variants'
//! — and [`column_ty`] a table's column. A list of records whose fields are
//! a table's columns is labelled by the table's column names
//! ([`columns_of`]).
//!
//! [`Value`] is a value of a [`Ty`], read from its JSON by [`decode`] and
//! written back by [`encode`] — the store's own encoding (`ark::json`), so
//! what a form sends is what a function reads — and said as one line of
//! text by [`text_of`], which [`parse_cell`] reads back: what a cell shows,
//! its editor starts with, and a form's field is typed as.

use ark::json::typed_json;
use ark::row::Type;
use ark::schema::{Column, Schema, Table};
use ark::Refusal;
use serde_json::Value as J;

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
    /// An enum, its variants' names in order.
    Enum(Vec<String>),
    /// A sum with fields: each variant's name and fields.
    Sum(Vec<(String, Vec<(String, Ty)>)>),
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
        Type::Tuple(ts) => Ty::Struct(ts.iter().enumerate().map(|(i, t)| (i.to_string(), of_type(t))).collect()),
        Type::Record(_, fs) => Ty::Struct(fields(fs)),
        Type::Sum(_, vs) if vs.iter().all(|(_, fs)| fs.is_empty()) => Ty::Enum(vs.iter().map(|(n, _)| n.clone()).collect()),
        Type::Sum(_, vs) => Ty::Sum(vs.iter().map(|(n, fs)| (n.clone(), fields(fs))).collect()),
    }
}

/// The type a [`Ty`] draws: what its JSON is written under.
pub fn to_type(t: &Ty) -> Type {
    let fields = |fs: &[(String, Ty)]| fs.iter().map(|(n, t)| (n.clone(), to_type(t))).collect();
    match t {
        Ty::Int => Type::Int,
        Ty::Text => Type::Text,
        Ty::Bool => Type::Bool,
        Ty::Bytes => Type::Bytes,
        Ty::Id(t) => Type::Id(t.clone()),
        Ty::Option(t) => Type::Option(Box::new(to_type(t))),
        Ty::List(t) => Type::List(Box::new(to_type(t))),
        Ty::Struct(fs) if fs.is_empty() => Type::Unit,
        Ty::Struct(fs) if fs[0].0 == "0" => Type::Tuple(fs.iter().map(|(_, t)| to_type(t)).collect()),
        Ty::Struct(fs) => Type::Record(String::new(), fields(fs)),
        Ty::Enum(vs) => Type::Sum(String::new(), vs.iter().map(|n| (n.clone(), vec![])).collect()),
        Ty::Sum(vs) => Type::Sum(String::new(), vs.iter().map(|(n, fs)| (n.clone(), fields(fs))).collect()),
    }
}

/// The type of a table's row, its columns by their names.
pub fn row_ty(t: &Table) -> Ty {
    Ty::Struct(t.columns.iter().map(|c| (c.name.clone(), column_ty(c))).collect())
}

/// A column's type.
pub fn column_ty(c: &Column) -> Ty {
    of_type(&c.ty)
}

/// When these fields' types are some table's columns', in order, the
/// table's column names.
pub fn columns_of(schema: &Schema, fields: &[(String, Type)]) -> Option<Vec<String>> {
    let t = schema
        .tables
        .iter()
        .find(|t| t.columns.iter().map(|c| &c.ty).eq(fields.iter().map(|(_, t)| t)))?;
    Some(t.columns.iter().map(|c| c.name.clone()).collect())
}

/// A value of `ty` from its JSON.
pub fn decode(ty: &Ty, bytes: &[u8]) -> Result<Value, Refusal> {
    let j: J = serde_json::from_slice(bytes).map_err(|e| Refusal(e.to_string()))?;
    from_json(ty, &j).ok_or_else(|| Refusal(format!("{j} is not a value of {}", ty_name(ty))))
}

/// The values of a row's columns, from the row's JSON.
pub fn decode_all(t: &Table, bytes: &[u8]) -> Result<Vec<Value>, Refusal> {
    match decode(&row_ty(t), bytes)? {
        Value::Struct(vs) => Ok(vs),
        _ => unreachable!("a record reads as a record"),
    }
}

/// A row's key columns, as the record a `delete_<t>` takes: its JSON.
pub fn key_input(t: &Table, row: &[u8]) -> Result<Vec<u8>, String> {
    let vs = decode_all(t, row).map_err(|e| e.0)?;
    let fs = t.columns[..t.key].iter().map(|c| (c.name.clone(), column_ty(c))).collect();
    encode(&Ty::Struct(fs), &Value::Struct(vs[..t.key].to_vec()))
}

fn from_json(ty: &Ty, j: &J) -> Option<Value> {
    Some(match (ty, j) {
        (Ty::Int, J::Number(n)) => Value::Int(n.as_i64()?),
        (Ty::Text, J::String(t)) => Value::Text(t.clone()),
        (Ty::Bytes, J::Array(xs)) => Value::Bytes(xs.iter().map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok())).collect::<Option<_>>()?),
        (Ty::Id(_), J::String(h)) => Value::Id(parse_id(h)?),
        (Ty::Bool, J::Bool(b)) => Value::Bool(*b),
        (Ty::Option(_), J::Null) => Value::Null,
        (Ty::Option(t), j) => from_json(t, j)?,
        (Ty::List(t), J::Array(xs)) => Value::List(xs.iter().map(|x| from_json(t, x)).collect::<Option<_>>()?),
        (Ty::Struct(fs), J::Null) if fs.is_empty() => Value::Struct(vec![]),
        (Ty::Struct(fs), J::Array(xs)) if fs.len() == xs.len() => {
            Value::Struct(fs.iter().zip(xs).map(|((_, t), x)| from_json(t, x)).collect::<Option<_>>()?)
        }
        (Ty::Struct(fs), J::Object(m)) => Value::Struct(
            fs.iter()
                .map(|(n, t)| from_json(t, m.get(n).unwrap_or(&J::Null)))
                .collect::<Option<_>>()?,
        ),
        (Ty::Enum(vs), J::String(n)) => Value::Variant(vs.iter().position(|v| v == n)? as u8, vec![]),
        (Ty::Sum(vs), J::String(n)) => Value::Variant(vs.iter().position(|(v, fs)| v == n && fs.is_empty())? as u8, vec![]),
        (Ty::Sum(vs), J::Object(m)) if m.len() == 1 => {
            let (n, p) = m.iter().next()?;
            let i = vs.iter().position(|(v, _)| v == n)?;
            let fs = &vs[i].1;
            let xs = match (fs.len(), p) {
                (1, p) if fs[0].0 == "0" => vec![from_json(&fs[0].1, p)?],
                (_, J::Array(xs)) if fs.len() == xs.len() => fs.iter().zip(xs).map(|((_, t), x)| from_json(t, x)).collect::<Option<_>>()?,
                (_, J::Object(o)) => fs
                    .iter()
                    .map(|(f, t)| from_json(t, o.get(f).unwrap_or(&J::Null)))
                    .collect::<Option<_>>()?,
                _ => return None,
            };
            Value::Variant(i as u8, xs)
        }
        _ => return None,
    })
}

fn to_json(ty: &Ty, v: &Value) -> Option<J> {
    let fields = |fs: &[(String, Ty)], xs: &[Value]| -> Option<J> {
        if fs.len() != xs.len() {
            return None;
        }
        Some(match fs.first() {
            None => J::Null,
            Some((n, _)) if n == "0" && fs.len() == 1 => to_json(&fs[0].1, &xs[0])?,
            Some((n, _)) if n == "0" => J::Array(fs.iter().zip(xs).map(|((_, t), x)| to_json(t, x)).collect::<Option<_>>()?),
            _ => J::Object(
                fs.iter()
                    .zip(xs)
                    .map(|((n, t), x)| Some((n.clone(), to_json(t, x)?)))
                    .collect::<Option<_>>()?,
            ),
        })
    };
    Some(match (ty, v) {
        (Ty::Int, Value::Int(n)) => J::from(*n),
        (Ty::Text, Value::Text(t)) => J::from(t.clone()),
        (Ty::Bytes, Value::Bytes(b)) => J::from(b.clone()),
        (Ty::Id(_), Value::Id(id)) => J::from(ark::hash::hex(id)),
        (Ty::Bool, Value::Bool(b)) => J::from(*b),
        (Ty::Option(_), Value::Null) => J::Null,
        (Ty::Option(t), v) => to_json(t, v)?,
        (Ty::List(t), Value::List(xs)) => J::Array(xs.iter().map(|x| to_json(t, x)).collect::<Option<_>>()?),
        (Ty::Struct(fs), Value::Struct(xs)) if fs.first().is_some_and(|(n, _)| n == "0") => {
            J::Array(fs.iter().zip(xs).map(|((_, t), x)| to_json(t, x)).collect::<Option<_>>()?)
        }
        (Ty::Struct(fs), Value::Struct(xs)) => fields(fs, xs)?,
        (Ty::Enum(vs), Value::Variant(i, _)) => J::from(vs.get(*i as usize)?.clone()),
        (Ty::Sum(vs), Value::Variant(i, xs)) => {
            let (n, fs) = vs.get(*i as usize)?;
            match fs.is_empty() {
                true => J::from(n.clone()),
                false => J::Object([(n.clone(), fields(fs, xs)?)].into_iter().collect()),
            }
        }
        _ => return None,
    })
}

/// A value's JSON under `ty`, or why it is not one.
pub fn encode(ty: &Ty, v: &Value) -> Result<Vec<u8>, String> {
    let j = to_json(ty, v).ok_or_else(|| format!("{} is not a value of {}", text_of(v), ty_name(ty)))?;
    typed_json(&to_type(ty), &j).map_err(|e| e.0)
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
        Ty::Enum(vs) => format!("enum({})", vs.join(" | ")),
        Ty::Sum(vs) => format!(
            "sum({})",
            vs.iter()
                .map(|(n, f)| format!("{n}({})", join(&f.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>())))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
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
    ark::hash::unhex(&text.trim().replace('-', "").to_ascii_lowercase()).and_then(|b| <[u8; 16]>::try_from(b).ok())
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
        Ty::Bytes => ark::hash::unhex(&t.to_ascii_lowercase())
            .map(Value::Bytes)
            .ok_or_else(|| bad("hex digits")),
        Ty::Enum(vs) => match (vs.iter().position(|v| v == t), t.trim_start_matches('#').parse::<u8>()) {
            (Some(i), _) => Ok(Value::Variant(i as u8, vec![])),
            (None, Ok(i)) if (i as usize) < vs.len() => Ok(Value::Variant(i, vec![])),
            _ => Err(bad(&format!("#0 to #{}", vs.len().saturating_sub(1)))),
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
            each(
                &vs[i].1.iter().map(|(_, t)| t).collect::<Vec<_>>(),
                parts(&rest, '(', ')').unwrap_or_default(),
            )
            .map(|xs| Value::Variant(i as u8, xs))
        }
    }
}
