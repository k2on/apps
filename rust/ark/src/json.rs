//! §1.1 Values and keys. A value is JSON: `serde_json`, compact, a
//! struct's fields in declaration order, a map's keys in order (`BTreeMap`)
//! and no floats, so one value has one encoding, which is what a row's leaf
//! of the state hash (§1.3) and an entry's link in the chain (§1.6) are
//! taken over. A key is the same value in the `memcomparable` encoding:
//! bytes whose `memcmp` order is the values' order, no encoding a proper
//! prefix of another's, so a prefix of a key's columns is a prefix of its
//! bytes and the store is ordered by `memcmp`.
//!
//! A value without a Rust type — a row as the store and a replica see it,
//! a row the explorer edits — is a [`serde_json::Value`] read under the
//! schema's [`Type`]: [`Typed`] serializes it exactly as the Rust type it
//! describes would be serialized, so its JSON is that type's bytes and its
//! key that type's key, whichever side made it.
//!
//! Bytes that are already JSON — a row, an input — travel inside a frame
//! or a record as the JSON they are ([`raw`]); hashes and ids are hex.

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::de::{DeserializeOwned, Error as _};
use serde::ser::{Error as _, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use serde_json::Value as J;

use crate::hash::{hex, unhex};
use crate::row::Type;
use crate::Refusal;

/// A value with its one JSON encoding, read back from it alone.
pub trait Json: Serialize + DeserializeOwned {
    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a value serializes")
    }

    fn from_bytes(b: &[u8]) -> Result<Self, Refusal> {
        serde_json::from_slice(b).map_err(|e| Refusal(e.to_string()))
    }
}

impl<T: Serialize + DeserializeOwned> Json for T {}

/// `v`'s key bytes appended to `out`.
pub fn put_key<T: Serialize + ?Sized>(v: &T, out: &mut Vec<u8>) {
    let mut s = memcomparable::Serializer::new(std::mem::take(out));
    v.serialize(&mut s).expect("a key serializes");
    *out = s.into_inner();
}

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

// * Fixed bytes as hex

/// Bytes as lowercase hex in JSON, as bytes in a key.
pub mod hex_bytes {
    use super::*;

    pub fn serialize<S: Serializer, B: AsRef<[u8]>>(b: &B, s: S) -> Result<S::Ok, S::Error> {
        match s.is_human_readable() {
            true => s.serialize_str(&hex(b.as_ref())),
            false => s.serialize_bytes(b.as_ref()),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, B: TryFrom<Vec<u8>>>(d: D) -> Result<B, D::Error> {
        let s = <std::borrow::Cow<str>>::deserialize(d)?;
        let b = unhex(&s).ok_or_else(|| D::Error::custom("not lowercase hex"))?;
        B::try_from(b).map_err(|_| D::Error::custom("not as many bytes as it is"))
    }
}

// * Bytes that are JSON

/// Bytes that are JSON, written as they are: the shape serde_json's own
/// `RawValue` serializes as, which its serializer writes without reading (a
/// row or an input is read once, where it enters).
struct AsRaw<'a>(&'a str);

const RAW_TOKEN: &str = "$serde_json::private::RawValue";

impl Serialize for AsRaw<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct(RAW_TOKEN, 1)?;
        st.serialize_field(RAW_TOKEN, self.0)?;
        st.end()
    }
}

fn as_raw<E: serde::ser::Error>(b: &[u8]) -> Result<AsRaw<'_>, E> {
    std::str::from_utf8(b).map(AsRaw).map_err(E::custom)
}

/// Bytes that are a JSON value, written as that value.
pub mod raw {
    use super::*;

    pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
        as_raw::<S::Error>(b)?.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Ok(<Box<RawValue>>::deserialize(d)?.get().as_bytes().to_vec())
    }
}

/// A list of JSON values' bytes.
pub mod raw_list {
    use super::*;

    pub fn serialize<S: Serializer>(rows: &[Vec<u8>], s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(rows.len()))?;
        for r in rows {
            seq.serialize_element(&as_raw::<S::Error>(r)?)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<u8>>, D::Error> {
        Ok(<Vec<Box<RawValue>>>::deserialize(d)?
            .iter()
            .map(|r| r.get().as_bytes().to_vec())
            .collect())
    }
}

/// Rows by table, each row's bytes JSON.
pub mod raw_tables {
    use super::*;

    #[derive(Serialize)]
    struct Out<'a>(&'a str, #[serde(with = "raw_list")] &'a Vec<Vec<u8>>);

    #[derive(Deserialize)]
    struct In(String, #[serde(with = "raw_list")] Vec<Vec<u8>>);

    /// Rows by table, as a store's snapshot holds them.
    pub type Tables = Vec<(String, Vec<Vec<u8>>)>;

    pub fn serialize<S: Serializer>(rows: &[(String, Vec<Vec<u8>>)], s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(rows.len()))?;
        for (t, rs) in rows {
            seq.serialize_element(&Out(t, rs))?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Tables, D::Error> {
        Ok(<Vec<In>>::deserialize(d)?.into_iter().map(|In(t, rs)| (t, rs)).collect())
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
                let (sum, var, fs) = (intern(n), intern(name), &vs[i].1);
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

/// A value's JSON under its type, or why it is not one of it.
pub fn typed_json(t: &Type, v: &J) -> Result<Vec<u8>, Refusal> {
    serde_json::to_vec(&Typed(t, v)).map_err(|e| Refusal(e.to_string()))
}

/// A value's key under its type, appended to `out`, or why it is not one.
pub fn typed_key(t: &Type, v: &J, out: &mut Vec<u8>) -> Result<(), Refusal> {
    let mut s = memcomparable::Serializer::new(std::mem::take(out));
    let r = Typed(t, v).serialize(&mut s).map_err(|e| Refusal(e.to_string()));
    *out = s.into_inner();
    r
}
