//! §1.1, §1.16 The value format: the bytes a value is stored, sent and
//! hashed as. Rows, inputs, entries, `Logged`, frames, snapshots and
//! journal records are serde values, written in one format, **postcard**:
//! a struct its fields in declaration order, integers as varints, an enum
//! its variant's index, bytes and text after their length. A value has one
//! encoding — no floats, maps as `BTreeMap` — and that encoding is what a
//! row's leaf of the state hash (§1.3) and an entry's link in the chain
//! (§1.6) are taken over.
//!
//! A **key** is the same value in the `memcomparable` encoding
//! ([`put_key`]): bytes whose `memcmp` order is the values' order,
//! no encoding a proper prefix of another's, so a prefix of a key's
//! columns is a prefix of its bytes and the store is ordered by `memcmp`.
//!
//! **JSON is the human form**: what a person reads and edits and the
//! vectors are written in ([`to_json`], [`from_json`]). In it hashes and
//! ids are hex, and bytes that are a value of the format — a row, an input
//! — are their hex (read as JSON under the schema by [`crate::dynamic`]).
//!
//! Bytes that are already a value — a row, an input — travel inside a
//! frame or a record as the bytes they are ([`raw`]).

use serde::de::{DeserializeOwned, DeserializeSeed, Error as _, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as J;

use crate::hash::{hex, unhex};
use crate::Refusal;

fn no(e: impl std::fmt::Display) -> Refusal {
    Refusal(e.to_string())
}

/// A value's bytes in the format.
pub fn encode<T: Serialize + ?Sized>(v: &T) -> Result<Vec<u8>, Refusal> {
    postcard::to_stdvec(v).map_err(no)
}

/// A value read by `seed` from bytes that are it whole: nothing after it.
pub fn decode_seed<'de, S: DeserializeSeed<'de>>(seed: S, b: &'de [u8]) -> Result<S::Value, Refusal> {
    let (v, rest) = take_seed(seed, b)?;
    match rest.is_empty() {
        true => Ok(v),
        false => Err(Refusal::new("bytes after the value")),
    }
}

/// A value read by `seed` from the front of `b`, and the bytes after it.
pub fn take_seed<'de, S: DeserializeSeed<'de>>(seed: S, b: &'de [u8]) -> Result<(S::Value, &'de [u8]), Refusal> {
    let mut d = postcard::Deserializer::from_bytes(b);
    let v = seed.deserialize(&mut d).map_err(no)?;
    Ok((v, d.finalize().map_err(no)?))
}

/// A value with its one encoding in the format, read back from it alone.
pub trait Encode: Serialize + DeserializeOwned {
    fn to_bytes(&self) -> Vec<u8> {
        encode(self).expect("a value serializes")
    }

    fn from_bytes(b: &[u8]) -> Result<Self, Refusal> {
        decode_seed(std::marker::PhantomData::<Self>, b)
    }
}

impl<T: Serialize + DeserializeOwned> Encode for T {}

/// A value's human form: JSON, as `serde_json` writes it.
pub fn to_json<T: Serialize + ?Sized>(v: &T) -> J {
    serde_json::to_value(v).expect("a value is JSON")
}

/// A value from its human form.
pub fn from_json<T: DeserializeOwned>(v: J) -> Result<T, Refusal> {
    serde_json::from_value(v).map_err(no)
}

/// `v`'s key bytes appended to `out`.
pub fn put_key<T: Serialize + ?Sized>(v: &T, out: &mut Vec<u8>) {
    let mut s = memcomparable::Serializer::new(std::mem::take(out));
    v.serialize(&mut s).expect("a key serializes");
    *out = s.into_inner();
}

// * Bytes

/// Bytes, from a format that writes them as bytes or as a list of them.
struct BytesV;

impl<'de> Visitor<'de> for BytesV {
    type Value = Vec<u8>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("bytes")
    }
    fn visit_bytes<E: serde::de::Error>(self, b: &[u8]) -> Result<Vec<u8>, E> {
        Ok(b.to_vec())
    }
    fn visit_byte_buf<E: serde::de::Error>(self, b: Vec<u8>) -> Result<Vec<u8>, E> {
        Ok(b)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Vec<u8>, A::Error> {
        let mut out = vec![];
        while let Some(x) = a.next_element::<u8>()? {
            out.push(x);
        }
        Ok(out)
    }
}

/// Bytes as a format writes bytes: their hex in JSON.
pub fn bytes<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    match d.is_human_readable() {
        true => {
            let s = <std::borrow::Cow<str>>::deserialize(d)?;
            unhex(&s).ok_or_else(|| D::Error::custom("not lowercase hex"))
        }
        false => d.deserialize_bytes(BytesV),
    }
}

/// Fixed bytes — a hash, an id — as lowercase hex in JSON, as bytes in
/// the binary format and in a key.
pub mod hex_bytes {
    use super::*;

    pub fn serialize<S: Serializer, B: AsRef<[u8]>>(b: &B, s: S) -> Result<S::Ok, S::Error> {
        match s.is_human_readable() {
            true => s.serialize_str(&hex(b.as_ref())),
            false => s.serialize_bytes(b.as_ref()),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, B: TryFrom<Vec<u8>>>(d: D) -> Result<B, D::Error> {
        B::try_from(bytes(d)?).map_err(|_| D::Error::custom("not as many bytes as it is"))
    }
}

// * Bytes that are a value

/// Bytes that are a value of the format, written as that value: inside a
/// JSON value their hex; inside a binary value, as bytes.
pub mod raw {
    use super::*;

    pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
        match s.is_human_readable() {
            true => s.serialize_str(&hex(b)),
            false => s.serialize_bytes(b),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        bytes(d)
    }
}

/// [`raw`]'s bytes as a serde value.
struct Raw<'a>(&'a [u8]);

impl Serialize for Raw<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        raw::serialize(self.0, s)
    }
}

struct RawIn(Vec<u8>);

impl<'de> Deserialize<'de> for RawIn {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<RawIn, D::Error> {
        raw::deserialize(d).map(RawIn)
    }
}

/// A list of values' bytes.
pub mod raw_list {
    use super::*;

    pub fn serialize<S: Serializer>(rows: &[Vec<u8>], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(rows.iter().map(|r| Raw(r)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<u8>>, D::Error> {
        Ok(<Vec<RawIn>>::deserialize(d)?.into_iter().map(|r| r.0).collect())
    }
}

/// Rows by table, each row's bytes a value.
pub mod raw_tables {
    use super::*;

    #[derive(Serialize)]
    struct Out<'a>(&'a str, #[serde(with = "raw_list")] &'a Vec<Vec<u8>>);

    #[derive(Deserialize)]
    struct In(String, #[serde(with = "raw_list")] Vec<Vec<u8>>);

    pub fn serialize<S: Serializer>(rows: &[(String, Vec<Vec<u8>>)], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(rows.iter().map(|(t, rs)| Out(t, rs)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<crate::log::Rows, D::Error> {
        Ok(<Vec<In>>::deserialize(d)?.into_iter().map(|In(t, rs)| (t, rs)).collect())
    }
}

/// A JSON value inside a value of the format — a column's default: itself
/// in JSON, its JSON text in the binary format (which cannot read a value
/// that does not say its type).
pub mod json_text {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Option<J>, s: S) -> Result<S::Ok, S::Error> {
        match s.is_human_readable() {
            true => v.serialize(s),
            false => v.as_ref().map(J::to_string).serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<J>, D::Error> {
        match d.is_human_readable() {
            true => Option::<J>::deserialize(d),
            false => Option::<String>::deserialize(d)?
                .map(|t| serde_json::from_str(&t).map_err(D::Error::custom))
                .transpose(),
        }
    }
}
