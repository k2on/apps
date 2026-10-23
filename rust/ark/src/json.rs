//! §1.1, §1.16 A value without a Rust type: JSON under a [`Type`].
//!
//! A row as the store and a replica see it, a row a person edits, an
//! input a phone sends as JSON, is a [`serde_json::Value`] read under the
//! schema's or the function's [`Type`]. [`Typed`] serializes it exactly as
//! the Rust type it describes would be serialized, so its bytes in the
//! value format ([`crate::format`]) are that type's bytes and its key that
//! type's key, whichever side made it; [`Seed`] reads it back from those
//! bytes, as the JSON the Rust type would be written as.

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::de::{DeserializeSeed, EnumAccess, Error as _, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde::ser::{Error as _, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as J;

use crate::hash::{hex, unhex};
use crate::row::Type;
use crate::Refusal;

/// A name kept for the life of the process: what serde's `&'static str`
/// names a schema's field or variant by. One per distinct name.
pub fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    match names.get(name) {
        Some(n) => n,
        None => {
            let n: &'static str = Box::leak(name.to_owned().into_boxed_str());
            names.insert(n);
            n
        }
    }
}

/// Names kept for the life of the process, as a list: a struct's fields or
/// a sum's variants, as serde's deserializers are told them.
fn interned(names: Vec<&str>) -> &'static [&'static str] {
    static LISTS: Mutex<BTreeSet<&'static [&'static str]>> = Mutex::new(BTreeSet::new());
    let names: Vec<&'static str> = names.into_iter().map(intern).collect();
    let mut lists = LISTS.lock().unwrap_or_else(|e| e.into_inner());
    match lists.get(&names[..]) {
        Some(l) => l,
        None => {
            let l: &'static [&'static str] = Box::leak(names.into_boxed_slice());
            lists.insert(l);
            l
        }
    }
}

// * A value under a type

/// A JSON value under a [`Type`], serialized as the Rust type the type
/// describes is: its JSON the same bytes, its key the same key.
pub struct Typed<'a>(pub &'a Type, pub &'a J);

/// A record's fields named `0`, `1`, … are a tuple struct's.
fn positional(fs: &[(String, Type)]) -> bool {
    fs.first().is_some_and(|(n, _)| n == "0")
}

impl Serialize for Typed<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (ty, v) = (self.0, self.1);
        let bad = || S::Error::custom(format!("{v} is not a {}", ty.decl()));
        let null = J::Null;
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
            (Type::Id(_), J::String(h)) => {
                let b = unhex(h).filter(|b| b.len() == 16).ok_or_else(bad)?;
                match s.is_human_readable() {
                    true => s.serialize_str(h),
                    false => s.serialize_bytes(&b),
                }
            }
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
            (Type::Record(n, fs), v) if fs.len() == 1 && positional(fs) => s.serialize_newtype_struct(intern(n), &Typed(&fs[0].1, v)),
            (Type::Record(n, fs), J::Array(xs)) if positional(fs) && fs.len() == xs.len() => {
                let mut tup = s.serialize_tuple_struct(intern(n), fs.len())?;
                for ((_, t), x) in fs.iter().zip(xs) {
                    tup.serialize_field(&Typed(t, x))?;
                }
                tup.end()
            }
            (Type::Record(n, fs), J::Object(m)) if !positional(fs) => {
                let mut st = s.serialize_struct(intern(n), fs.len())?;
                for (f, t) in fs {
                    st.serialize_field(intern(f), &Typed(t, m.get(f).unwrap_or(&null)))?;
                }
                st.end()
            }
            (Type::Sum(n, vs), v) => {
                let (name, payload) = match v {
                    J::String(x) => (x.as_str(), None),
                    J::Object(m) if m.len() == 1 => m.iter().next().map(|(k, p)| (k.as_str(), Some(p))).unwrap(),
                    _ => return Err(bad()),
                };
                let i = vs.iter().position(|(x, _)| x == name).ok_or_else(bad)?;
                let fs = &vs[i].1;
                let (sum, var) = (intern(n), intern(name));
                match (fs.len(), payload) {
                    (0, None) => s.serialize_unit_variant(sum, i as u32, var),
                    (1, Some(p)) if positional(fs) => s.serialize_newtype_variant(sum, i as u32, var, &Typed(&fs[0].1, p)),
                    (k, Some(J::Array(xs))) if positional(fs) && k == xs.len() => {
                        let mut tv = s.serialize_tuple_variant(sum, i as u32, var, k)?;
                        for ((_, t), x) in fs.iter().zip(xs) {
                            tv.serialize_field(&Typed(t, x))?;
                        }
                        tv.end()
                    }
                    (k, Some(J::Object(m))) if !positional(fs) => {
                        let mut sv = s.serialize_struct_variant(sum, i as u32, var, k)?;
                        for (f, t) in fs {
                            sv.serialize_field(intern(f), &Typed(t, m.get(f).unwrap_or(&null)))?;
                        }
                        sv.end()
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
pub fn typed_bytes(t: &Type, v: &J) -> Result<Vec<u8>, Refusal> {
    crate::format::encode(&Typed(t, v))
}

/// A value of a type from its bytes in the value format, whole: the JSON
/// its Rust type is written as.
pub fn typed_value(t: &Type, b: &[u8]) -> Result<J, Refusal> {
    crate::format::decode_seed(Seed(t), b)
}

/// A value's key under its type, appended to `out`, or why it is not one.
pub fn typed_key(t: &Type, v: &J, out: &mut Vec<u8>) -> Result<(), Refusal> {
    let mut s = memcomparable::Serializer::new(std::mem::take(out));
    let r = Typed(t, v).serialize(&mut s).map_err(|e| Refusal(e.to_string()));
    *out = s.into_inner();
    r
}

/// A value's bytes in the value format from its JSON text under its type:
/// how a value crosses from a host that speaks JSON (a phone, a command
/// line). No text is `null`.
pub fn from_text(t: &Type, text: &str) -> Result<Vec<u8>, Refusal> {
    let v: J = match text.trim() {
        "" => J::Null,
        t => serde_json::from_str(t).map_err(|e| Refusal(e.to_string()))?,
    };
    typed_bytes(t, &v)
}

/// A value's JSON text under its type, from its bytes in the value format.
pub fn to_text(t: &Type, b: &[u8]) -> Result<String, Refusal> {
    let v = typed_value(t, b)?;
    serde_json::to_string(&Typed(t, &v)).map_err(|e| Refusal(e.to_string()))
}

// * A value of a type, read

/// Reads a value of a type as the JSON its Rust type is written as:
/// [`Typed`]'s inverse, for a format that does not say what it holds.
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
            Type::Record(n, fs) if fs.len() == 1 && positional(fs) => d.deserialize_newtype_struct(intern(n), Of(&fs[0].1)),
            Type::Record(n, fs) if positional(fs) => d.deserialize_tuple_struct(intern(n), fs.len(), fields(fs, false)),
            Type::Record(n, fs) => d.deserialize_struct(intern(n), interned(fs.iter().map(|f| &f.0[..]).collect()), fields(fs, true)),
            Type::Sum(n, vs) => d.deserialize_enum(intern(n), interned(vs.iter().map(|v| &v.0[..]).collect()), Variants(vs)),
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
    fn visit_unit<E: serde::de::Error>(self) -> Result<J, E> {
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
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
        let mut m = serde_json::Map::new();
        while let Some(k) = a.next_key::<String>()? {
            let (_, t) = self
                .0
                .iter()
                .find(|(n, _)| *n == k)
                .ok_or_else(|| A::Error::custom(format!("no field {k}")))?;
            m.insert(k, a.next_value_seed(Seed(t))?);
        }
        Ok(J::Object(m))
    }
}

/// A sum's variants: the one read, by its index, as serde's JSON writes
/// it — its name, or an object of one entry, its name to its payload.
struct Variants<'a>(&'a [(String, Vec<(String, Type)>)]);

/// A variant's index, as a binary format writes it, or its name.
struct Which<'a>(&'a [(String, Vec<(String, Type)>)]);

impl<'de> DeserializeSeed<'de> for Which<'_> {
    type Value = usize;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<usize, D::Error> {
        d.deserialize_identifier(self)
    }
}

impl<'de> Visitor<'de> for Which<'_> {
    type Value = usize;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a variant")
    }
    fn visit_u64<E: serde::de::Error>(self, i: u64) -> Result<usize, E> {
        (i as usize)
            .lt(&self.0.len())
            .then_some(i as usize)
            .ok_or_else(|| E::custom("no such variant"))
    }
    fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<usize, E> {
        self.0
            .iter()
            .position(|(n, _)| n == s)
            .ok_or_else(|| E::custom(format!("no variant {s}")))
    }
}

impl<'de> Visitor<'de> for Variants<'_> {
    type Value = J;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a variant")
    }
    fn visit_enum<A: EnumAccess<'de>>(self, a: A) -> Result<J, A::Error> {
        let (i, v) = a.variant_seed(Which(self.0))?;
        let (name, fs) = &self.0[i];
        let one = |p: J| J::Object([(name.clone(), p)].into_iter().collect());
        match fs.len() {
            0 => v.unit_variant().map(|_| J::String(name.clone())),
            1 if positional(fs) => v.newtype_variant_seed(Seed(&fs[0].1)).map(one),
            k if positional(fs) => v.tuple_variant(k, fields(fs, false)).map(one),
            _ => v
                .struct_variant(interned(fs.iter().map(|f| &f.0[..]).collect()), fields(fs, true))
                .map(one),
        }
    }
}
