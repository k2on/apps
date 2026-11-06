//! §1.1, §1.16 A value without a Rust type: JSON under a [`Type`].
//!
//! A row as the store and a replica see it, a row a person edits, an
//! input a phone sends as JSON, is a [`serde_json::Value`] read under the
//! schema's or the function's [`Type`]. [`Typed`] serializes it exactly as
//! the Rust type it describes would be serialized in a binary format, so
//! its bytes in the value format ([`crate::format`]) are that type's bytes
//! and its key that type's key, whichever side made it; [`Seed`] reads it
//! back from those bytes, as the JSON the Rust type would be written as.
//! Neither format writes a name (a record is a tuple, a variant its index),
//! so neither is told one.

use serde::de::{DeserializeSeed, EnumAccess, Error as _, SeqAccess, VariantAccess, Visitor};
use serde::ser::{Error as _, SerializeSeq, SerializeTuple, SerializeTupleVariant};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as J;

use crate::hash::{hex, unhex};
use crate::row::Type;

// * A value under a type

/// A JSON value under a [`Type`], serialized as the Rust type the type
/// describes is in a binary format: the same bytes, the same key.
pub struct Typed<'a>(pub &'a Type, pub &'a J);

/// A record's fields named `0`, `1`, … are a tuple struct's.
fn positional(fs: &[(String, Type)]) -> bool {
    fs.first().is_some_and(|(n, _)| n == "0")
}

/// A record's fields, each its type and its value, in order: from an array
/// of them, a tuple struct's, or an object of them by name (one missing is
/// null).
fn fields_of<'a>(fs: &'a [(String, Type)], v: &'a J) -> Option<Vec<(&'a Type, &'a J)>> {
    static NULL: J = J::Null;
    match v {
        J::Array(xs) if positional(fs) && xs.len() == fs.len() => Some(fs.iter().map(|f| &f.1).zip(xs).collect()),
        J::Object(m) if !positional(fs) => Some(fs.iter().map(|(f, t)| (t, m.get(f).unwrap_or(&NULL))).collect()),
        _ => None,
    }
}

impl Serialize for Typed<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (ty, v) = (self.0, self.1);
        let bad = || S::Error::custom(format!("{v} is not a {}", ty.decl()));
        match (ty, v) {
            (Type::Int, J::Number(n)) => s.serialize_i64(n.as_i64().ok_or_else(bad)?),
            (Type::Text, J::String(x)) => s.serialize_str(x),
            (Type::Bool, J::Bool(b)) => s.serialize_bool(*b),
            (Type::Bytes, J::Array(xs)) => {
                let mut seq = s.serialize_seq(Some(xs.len()))?;
                for x in xs {
                    seq.serialize_element(&x.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or_else(bad)?)?;
                }
                seq.end()
            }
            (Type::Unit, J::Null) => s.serialize_unit(),
            (Type::Id(_), J::String(h)) => s.serialize_bytes(&unhex(h).filter(|b| b.len() == 16).ok_or_else(bad)?),
            (Type::Option(_), J::Null) => s.serialize_none(),
            (Type::Option(t), v) => s.serialize_some(&Typed(t, v)),
            (Type::List(t), J::Array(xs)) => {
                let mut seq = s.serialize_seq(Some(xs.len()))?;
                for x in xs {
                    seq.serialize_element(&Typed(t, x))?;
                }
                seq.end()
            }
            (Type::Tuple(ts), J::Array(xs)) if ts.len() == xs.len() => {
                let mut tup = s.serialize_tuple(ts.len())?;
                for (t, x) in ts.iter().zip(xs) {
                    tup.serialize_element(&Typed(t, x))?;
                }
                tup.end()
            }
            (Type::Record(_, fs), v) if fs.len() == 1 && positional(fs) => s.serialize_newtype_struct("", &Typed(&fs[0].1, v)),
            (Type::Record(_, fs), v) => {
                let mut tup = s.serialize_tuple(fs.len())?;
                for (t, x) in fields_of(fs, v).ok_or_else(bad)? {
                    tup.serialize_element(&Typed(t, x))?;
                }
                tup.end()
            }
            (Type::Sum(_, vs), v) => {
                let (name, payload) = match v {
                    J::String(x) => (x.as_str(), None),
                    J::Object(m) if m.len() == 1 => m.iter().next().map(|(k, p)| (k.as_str(), Some(p))).unwrap(),
                    _ => return Err(bad()),
                };
                let i = vs.iter().position(|(x, _)| x == name).ok_or_else(bad)?;
                let fs = &vs[i].1;
                match (fs.len(), payload) {
                    (0, None) => s.serialize_unit_variant("", i as u32, ""),
                    (1, Some(p)) if positional(fs) => s.serialize_newtype_variant("", i as u32, "", &Typed(&fs[0].1, p)),
                    (k, Some(p)) => {
                        let mut tv = s.serialize_tuple_variant("", i as u32, "", k)?;
                        for (t, x) in fields_of(fs, p).ok_or_else(bad)? {
                            tv.serialize_field(&Typed(t, x))?;
                        }
                        tv.end()
                    }
                    _ => Err(bad()),
                }
            }
            _ => Err(bad()),
        }
    }
}

/// A value's bytes in the value format under its type, or why it is not
/// one of it.
pub fn typed_bytes(t: &Type, v: &J) -> Result<Vec<u8>, String> {
    crate::format::encode(&Typed(t, v))
}

/// A value of a type from its bytes in the value format, whole: the JSON
/// its Rust type is written as.
pub fn typed_value(t: &Type, b: &[u8]) -> Result<J, String> {
    crate::format::decode_seed(Seed(t), b)
}

/// A record's bytes as an older or a newer build of its type wrote them,
/// read as this one's (§1.2, as a row is, [`crate::schema::Table::fit`]):
/// a field past where they end takes its type's empty value — none, 0, "",
/// false, nothing — and what follows this one's fields is dropped. None
/// for a type that is no record, bytes of none of its fields (a record
/// grows from one of its own, not from nothing), or a field missing with no
/// empty value.
pub fn fit(t: &Type, b: &[u8]) -> Option<Vec<u8>> {
    let Type::Record(_, fs) = t else { return None };
    if b.is_empty() {
        return None;
    }
    let (mut rest, mut vs) = (b, vec![]);
    for (_, t) in fs {
        let v = match (rest.is_empty(), t) {
            (true, Type::Option(_) | Type::Unit) => J::Null,
            (true, Type::Int) => 0.into(),
            (true, Type::Text) => "".into(),
            (true, Type::Bool) => false.into(),
            (true, Type::Bytes | Type::List(_)) => J::Array(vec![]),
            (true, _) => return None,
            (false, _) => {
                let (v, r) = crate::format::take_seed(Seed(t), rest).ok()?;
                rest = r;
                v
            }
        };
        vs.push(v);
    }
    let v = match positional(fs) {
        true if vs.len() == 1 => vs.remove(0),
        true => J::Array(vs),
        false => J::Object(fs.iter().map(|f| f.0.clone()).zip(vs).collect()),
    };
    typed_bytes(t, &v).ok()
}

/// A value's key under its type, appended to `out`, or why it is not one.
pub fn typed_key(t: &Type, v: &J, out: &mut Vec<u8>) -> Result<(), String> {
    let mut s = memcomparable::Serializer::new(std::mem::take(out));
    let r = Typed(t, v).serialize(&mut s).map_err(|e| e.to_string());
    *out = s.into_inner();
    r
}

/// A value's bytes in the value format from its JSON text under its type:
/// how a value crosses from a host that speaks JSON (a phone, a command
/// line). No text is `null`.
pub fn from_text(t: &Type, text: &str) -> Result<Vec<u8>, String> {
    let v: J = match text.trim() {
        "" => J::Null,
        t => serde_json::from_str(t).map_err(|e| e.to_string())?,
    };
    typed_bytes(t, &v)
}

/// A value's JSON text under its type, from its bytes in the value format.
pub fn to_text(t: &Type, b: &[u8]) -> Result<String, String> {
    typed_value(t, b).map(|v| v.to_string())
}

// * A value of a type, read

/// Reads a value of a type as the JSON its Rust type is written as:
/// [`Typed`]'s inverse, for the value format, which does not say what it
/// holds: a record read as the tuple it writes, a variant by its index.
#[derive(Clone, Copy)]
pub struct Seed<'a>(pub &'a Type);

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = J;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<J, D::Error> {
        match self.0 {
            Type::Int => i64::deserialize(d).map(J::from),
            Type::Text => String::deserialize(d).map(J::String),
            Type::Bool => bool::deserialize(d).map(J::Bool),
            Type::Bytes => Vec::<u8>::deserialize(d).map(|b| J::Array(b.into_iter().map(J::from).collect())),
            Type::Unit => <()>::deserialize(d).map(|_| J::Null),
            Type::Id(_) => match crate::format::bytes(d)? {
                b if b.len() == 16 => Ok(J::String(hex(&b))),
                _ => Err(D::Error::custom("an id is 16 bytes")),
            },
            Type::Option(t) => d.deserialize_option(Of(t)),
            Type::List(t) => d.deserialize_seq(Of(t)),
            Type::Tuple(ts) => d.deserialize_tuple(ts.len(), Fields(ts.iter().map(|t| ("", t)).collect(), false)),
            Type::Record(_, fs) if fs.len() == 1 && positional(fs) => d.deserialize_newtype_struct("", Of(&fs[0].1)),
            // The value format writes a record as a tuple, and a variant by its
            // index: neither is read by a name.
            Type::Record(_, fs) => d.deserialize_tuple(fs.len(), fields(fs, !positional(fs))),
            Type::Sum(_, vs) => d.deserialize_enum("", &[], Variants(vs)),
        }
    }
}

fn fields(fs: &[(String, Type)], named: bool) -> Fields<'_> {
    Fields(fs.iter().map(|(n, t)| (&n[..], t)).collect(), named)
}

/// An option's, a list's or a newtype's one type.
struct Of<'a>(&'a Type);

impl<'de> Visitor<'de> for Of<'_> {
    type Value = J;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "a {}", self.0.decl())
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<J, E> {
        Ok(J::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<J, D::Error> {
        Seed(self.0).deserialize(d)
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<J, D::Error> {
        Seed(self.0).deserialize(d)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
        let mut out = vec![];
        while let Some(x) = a.next_element_seed(Seed(self.0))? {
            out.push(x);
        }
        Ok(J::Array(out))
    }
}

/// A tuple's or a record's fields, in order: an array, or an object of
/// them by name.
struct Fields<'a>(Vec<(&'a str, &'a Type)>, bool);

impl<'de> Visitor<'de> for Fields<'_> {
    type Value = J;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} fields", self.0.len())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
        let mut xs = Vec::with_capacity(self.0.len());
        for (_, t) in &self.0 {
            xs.push(a.next_element_seed(Seed(t))?.ok_or_else(|| A::Error::custom("a field missing"))?);
        }
        Ok(match self.1 {
            true => J::Object(self.0.iter().map(|(n, _)| n.to_string()).zip(xs).collect()),
            false => J::Array(xs),
        })
    }
}

/// A sum's variants: the one read, by its index, as serde's JSON writes
/// it — its name, or an object of one entry, its name to its payload.
struct Variants<'a>(&'a [(String, Vec<(String, Type)>)]);

impl<'de> Visitor<'de> for Variants<'_> {
    type Value = J;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a variant")
    }
    fn visit_enum<A: EnumAccess<'de>>(self, a: A) -> Result<J, A::Error> {
        let (i, v) = a.variant::<u32>()?;
        let (name, fs) = self.0.get(i as usize).ok_or_else(|| A::Error::custom("no such variant"))?;
        let one = |p: J| J::Object([(name.clone(), p)].into_iter().collect());
        match fs.len() {
            0 => v.unit_variant().map(|_| J::String(name.clone())),
            1 if positional(fs) => v.newtype_variant_seed(Seed(&fs[0].1)).map(one),
            k => v.tuple_variant(k, fields(fs, !positional(fs))).map(one),
        }
    }
}
