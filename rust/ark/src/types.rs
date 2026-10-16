//! §2.2 The type section, and a value held to a type (`docs/plan-live.md`).
//!
//! A live state is made of records and sums the module declares by name
//! ([`crate::ir::Module::types`]). Three things are asked of them, and all
//! three are here: what a named type stands for ([`resolve`]), whether a
//! value is one ([`conforms`]) — asked of an input, a snapshot and a
//! transition's result, wherever a value crosses a boundary — and which of
//! a state's fields are ephemeral, which the state's hash and the snapshot
//! a server keeps leave out ([`ephemeral_fields`], [`ordered_part`]).
//!
//! A value of each type, exactly:
//!
//! | type | its values |
//! |---|---|
//! | `Bool`, `Int`, `Text`, `Bytes`, `Id t` | the value of that kind |
//! | `Enum vs` | a `Text` that is one of `vs` |
//! | `Option t` | `Null`, or a value of `t` |
//! | `List t` | a list of values of `t` |
//! | `NonEmpty t` | a list of values of `t` with at least one |
//! | `Struct fs` | a struct of exactly the fields `fs`, each of its type |
//! | `Sum vs` | a struct whose `t` is the name of a variant of `vs`, and whose other fields are exactly that variant's, each of its type |
//! | `PerDevice t` | a struct keyed by device ids, each value of `t` |
//! | `Ephemeral t` | a value of `t` |
//! | `Named n` | a value of what `n` is declared as |

use std::collections::{BTreeMap, BTreeSet};

use crate::schema::Ty;
use crate::value::{FieldName, Value};

/// The tag a sum's value carries its variant's name under, as the IR's own
/// nodes do.
pub const TAG: &str = "t";

/// The most a type may nest named types: deeper is a cycle, which the
/// verifier refuses by name before anything resolves.
const DEPTH: usize = 64;

/// `t` with every named type replaced by its declaration, at any depth.
/// `Err` names a type the section does not declare, or one that refers to
/// itself.
pub fn resolve(types: &BTreeMap<String, Ty>, t: &Ty) -> Result<Ty, String> {
    resolve_at(types, t, &mut vec![])
}

fn resolve_at(types: &BTreeMap<String, Ty>, t: &Ty, seen: &mut Vec<String>) -> Result<Ty, String> {
    Ok(match t {
        Ty::Named(n) => {
            if seen.contains(n) || seen.len() > DEPTH {
                return Err(format!("the type {n} refers to itself"));
            }
            let Some(d) = types.get(n) else {
                return Err(format!("no type {n}"));
            };
            seen.push(n.clone());
            let r = resolve_at(types, d, seen)?;
            seen.pop();
            r
        }
        Ty::Option(x) => Ty::Option(Box::new(resolve_at(types, x, seen)?)),
        Ty::List(x) => Ty::List(Box::new(resolve_at(types, x, seen)?)),
        Ty::NonEmpty(x) => Ty::NonEmpty(Box::new(resolve_at(types, x, seen)?)),
        Ty::PerDevice(x) => Ty::PerDevice(Box::new(resolve_at(types, x, seen)?)),
        Ty::Ephemeral(x) => Ty::Ephemeral(Box::new(resolve_at(types, x, seen)?)),
        Ty::Struct(fs) => Ty::Struct(
            fs.iter()
                .map(|(k, v)| Ok((k.clone(), resolve_at(types, v, seen)?)))
                .collect::<Result<_, String>>()?,
        ),
        Ty::Sum(vs) => Ty::Sum(
            vs.iter()
                .map(|(n, fs)| {
                    let fs = fs
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), resolve_at(types, v, seen)?)))
                        .collect::<Result<_, String>>()?;
                    Ok((n.clone(), fs))
                })
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

/// A resolved type with every `Ephemeral` marker taken off: what the type
/// checker reads a state's fields as, since an ephemeral field is a value
/// of its type like any other and differs only in who may read it.
pub fn unmarked(t: &Ty) -> Ty {
    match t {
        Ty::Ephemeral(x) => unmarked(x),
        Ty::Option(x) => Ty::Option(Box::new(unmarked(x))),
        Ty::List(x) => Ty::List(Box::new(unmarked(x))),
        Ty::NonEmpty(x) => Ty::NonEmpty(Box::new(unmarked(x))),
        Ty::PerDevice(x) => Ty::PerDevice(Box::new(unmarked(x))),
        Ty::Struct(fs) => Ty::Struct(fs.iter().map(|(k, v)| (k.clone(), unmarked(v))).collect()),
        Ty::Sum(vs) => Ty::Sum(
            vs.iter()
                .map(|(n, fs)| (n.clone(), fs.iter().map(|(k, v)| (k.clone(), unmarked(v))).collect()))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The ephemeral fields of a state of this resolved type: the fields of
/// its record marked `Ephemeral`. None for a state that is not a record.
pub fn ephemeral_fields(state: &Ty) -> BTreeSet<FieldName> {
    match state {
        Ty::Struct(fs) => fs.iter().filter(|(_, t)| matches!(t, Ty::Ephemeral(_))).map(|(k, _)| k.clone()).collect(),
        _ => BTreeSet::new(),
    }
}

/// The ordered part of a state's value: the value with its ephemeral
/// fields taken out — what its hash is of, and what a server keeps.
pub fn ordered_part(ephemeral: &BTreeSet<FieldName>, v: &Value) -> Value {
    match v {
        Value::Struct(m) if !ephemeral.is_empty() => Value::Struct(Box::new(
            m.iter()
                .filter(|(k, _)| !ephemeral.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )),
        other => other.clone(),
    }
}

/// A value restored from its ordered part: every ephemeral field an empty
/// map, since nobody is standing in an instance a server has just opened.
pub fn with_empty_slots(ephemeral: &BTreeSet<FieldName>, v: Value) -> Value {
    match v {
        Value::Struct(mut m) if !ephemeral.is_empty() => {
            for f in ephemeral {
                m.insert(f.clone(), Value::Struct(Box::default()));
            }
            Value::Struct(m)
        }
        other => other,
    }
}

/// Whether `v` is a value of `t` (the module docs' table); `Err` says where
/// it is not, as a path and a sentence: `queue: an empty list where a
/// non-empty one is declared`.
pub fn conforms(types: &BTreeMap<String, Ty>, t: &Ty, v: &Value) -> Result<(), String> {
    conform_at(types, t, v, &mut vec![], 0)
}

fn at(path: &[String], what: String) -> String {
    if path.is_empty() {
        what
    } else {
        format!("{}: {what}", path.join("."))
    }
}

fn conform_at(types: &BTreeMap<String, Ty>, t: &Ty, v: &Value, path: &mut Vec<String>, depth: usize) -> Result<(), String> {
    let kind = |want: &str| Err(at(path, format!("expected {want}, found {}", short(v))));
    match (t, v) {
        (Ty::Named(n), _) => {
            if depth > DEPTH {
                return Err(at(path, format!("the type {n} refers to itself")));
            }
            let Some(d) = types.get(n) else {
                return Err(at(path, format!("no type {n}")));
            };
            conform_at(types, d, v, path, depth + 1)
        }
        (Ty::Ephemeral(x), _) => conform_at(types, x, v, path, depth),
        (Ty::Bool, Value::Bool(_))
        | (Ty::Int, Value::Int(_))
        | (Ty::Text, Value::Text(_))
        | (Ty::Bytes, Value::Bytes(_))
        | (Ty::Id(_), Value::Id(_)) => Ok(()),
        (Ty::Bool, _) => kind("a bool"),
        (Ty::Int, _) => kind("an int"),
        (Ty::Text, _) => kind("text"),
        (Ty::Bytes, _) => kind("bytes"),
        (Ty::Id(_), _) => kind("an id"),
        (Ty::Enum(vs), Value::Text(x)) if vs.iter().any(|c| **c == **x) => Ok(()),
        (Ty::Enum(vs), _) => Err(at(path, format!("expected one of {}, found {}", vs.join(", "), short(v)))),
        (Ty::Option(_), Value::Null) => Ok(()),
        (Ty::Option(x), _) => conform_at(types, x, v, path, depth),
        (Ty::List(x), Value::List(xs)) => elems(types, x, xs, path, depth),
        (Ty::NonEmpty(x), Value::List(xs)) => {
            if xs.is_empty() {
                return Err(at(path, "an empty list where a non-empty one is declared".into()));
            }
            elems(types, x, xs, path, depth)
        }
        (Ty::List(_), _) | (Ty::NonEmpty(_), _) => kind("a list"),
        (Ty::Struct(fs), Value::Struct(m)) => fields(types, fs, m, &BTreeSet::new(), path, depth),
        (Ty::Struct(_), _) => kind("a record"),
        (Ty::Sum(vs), Value::Struct(m)) => {
            let Some(Value::Text(tag)) = m.get(TAG) else {
                return Err(at(path, "a sum's value names its variant under t".into()));
            };
            let Some(fs) = vs.get(&**tag) else {
                return Err(at(path, format!("no variant {tag}")));
            };
            path.push(tag.to_string());
            let r = fields(types, fs, m, &[TAG.to_string()].into(), path, depth);
            path.pop();
            r
        }
        (Ty::Sum(_), _) => kind("a variant"),
        (Ty::PerDevice(x), Value::Struct(m)) => {
            for (k, e) in m.iter() {
                path.push(format!("[{k}]"));
                let r = conform_at(types, x, e, path, depth);
                path.pop();
                r?;
            }
            Ok(())
        }
        (Ty::PerDevice(_), _) => kind("a map of devices"),
    }
}

fn elems(types: &BTreeMap<String, Ty>, t: &Ty, xs: &[Value], path: &mut Vec<String>, depth: usize) -> Result<(), String> {
    for (i, x) in xs.iter().enumerate() {
        path.push(format!("[{i}]"));
        let r = conform_at(types, t, x, path, depth);
        path.pop();
        r?;
    }
    Ok(())
}

fn fields(
    types: &BTreeMap<String, Ty>,
    fs: &BTreeMap<FieldName, Ty>,
    m: &BTreeMap<FieldName, Value>,
    extra: &BTreeSet<String>,
    path: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    for k in m.keys() {
        if !fs.contains_key(k) && !extra.contains(k) {
            return Err(at(path, format!("no field {k}")));
        }
    }
    for (k, t) in fs {
        let Some(x) = m.get(k) else {
            return Err(at(path, format!("missing field {k}")));
        };
        path.push(k.clone());
        let r = conform_at(types, t, x, path, depth);
        path.pop();
        r?;
    }
    Ok(())
}

// A value named briefly, for a sentence.
fn short(v: &Value) -> &'static str {
    match v {
        Value::Null => "nothing",
        Value::Bool(_) => "a bool",
        Value::Int(_) => "an int",
        Value::Text(_) => "text",
        Value::Bytes(_) => "bytes",
        Value::Id(_) => "an id",
        Value::List(_) => "a list",
        Value::Struct(_) => "a struct",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> BTreeMap<String, Ty> {
        let track = Ty::Struct([("title".to_string(), Ty::Text)].into());
        let active = [
            ("queue".to_string(), Ty::NonEmpty(Box::new(Ty::Named("track".into())))),
            ("at".to_string(), Ty::Int),
        ]
        .into();
        [
            ("track".to_string(), track),
            (
                "session".to_string(),
                Ty::Sum([("idle".to_string(), BTreeMap::new()), ("active".to_string(), active)].into()),
            ),
        ]
        .into()
    }

    /// A sum's value is a struct tagged with its variant and nothing else;
    /// a non-empty list is held to its element. Falsified by letting an
    /// empty list through: `active` with no queue conformed.
    #[test]
    fn a_value_is_held_to_its_type() {
        let ty = session();
        let t = Ty::Named("session".into());
        let idle = Value::record(vec![("t", Value::text("idle"))]);
        assert_eq!(conforms(&ty, &t, &idle), Ok(()));
        let track = Value::record(vec![("title", Value::text("Air"))]);
        let active = |q: Vec<Value>| Value::record(vec![("t", Value::text("active")), ("queue", Value::list(q)), ("at", Value::int(0))]);
        assert_eq!(conforms(&ty, &t, &active(vec![track.clone()])), Ok(()));
        assert_eq!(
            conforms(&ty, &t, &active(vec![])),
            Err("active.queue: an empty list where a non-empty one is declared".into())
        );
        assert_eq!(
            conforms(&ty, &t, &Value::record(vec![("t", Value::text("gone"))])),
            Err("no variant gone".into())
        );
        let extra = Value::record(vec![("t", Value::text("idle")), ("at", Value::int(1))]);
        assert_eq!(conforms(&ty, &t, &extra), Err("idle: no field at".into()));
        assert!(resolve(&[("a".to_string(), Ty::Named("a".into()))].into(), &Ty::Named("a".into())).is_err());
    }
}
