//! §2 The canonical encoding, as `Ark.Canon` defines it.
//!
//! RFC 8949 §4.2.1 deterministic CBOR plus the mapping of [`Value`] onto
//! CBOR's types: shortest heads, definite lengths, map keys sorted by the
//! bytes they encode to, tag 37 over sixteen bytes for an id. [`decode`]
//! accepts canonical bytes and nothing else, so that `decode(encode(v)) ==
//! v` and `encode(decode(b)) == b` hold at once.

use std::collections::BTreeMap;

use crate::value::{Id, Value};

// * Encoding

/// The canonical bytes of a value. Total: every value has an encoding.
pub fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    build(v, &mut out);
    out
}

/// The head of a data item: a major type in the top three bits and an
/// argument in the shortest form that holds it (§4.2.1).
fn header(major: u8, n: u64, out: &mut Vec<u8>) {
    let mt = major << 5;
    if n < 24 {
        out.push(mt | n as u8);
    } else if n < 0x100 {
        out.push(mt | 24);
        out.push(n as u8);
    } else if n < 0x1_0000 {
        out.push(mt | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n < 0x1_0000_0000 {
        out.push(mt | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push(mt | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn string(major: u8, b: &[u8], out: &mut Vec<u8>) {
    header(major, b.len() as u64, out);
    out.extend_from_slice(b);
}

/// How each constructor is spelled in CBOR (`Ark.Canon.build`).
fn build(v: &Value, out: &mut Vec<u8>) {
    match v {
        // The simple value null: major type 7, argument 22.
        Value::Null => out.push(0xf6),
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        // Major type 0 when not negative, major type 1 carrying -1 - n
        // otherwise; the subtraction cannot overflow because n is negative.
        Value::Int(n) => {
            if *n >= 0 {
                header(0, *n as u64, out);
            } else {
                header(1, ((-1) - n) as u64, out);
            }
        }
        // Major type 3: the UTF-8 bytes behind a definite byte length.
        Value::Text(t) => string(3, t.as_bytes(), out),
        // Major type 2.
        Value::Bytes(b) => string(2, b, out),
        // Tag 37 over a sixteen-byte string: always 0xd8 0x25 0x50 then the id.
        Value::Id(i) => {
            header(6, 37, out);
            string(2, i, out);
        }
        // Major type 4: the elements in order.
        Value::List(xs) => {
            header(4, xs.len() as u64, out);
            for x in xs {
                build(x, out);
            }
        }
        // Major type 5: pairs sorted by the bytes the key encodes to, which
        // is length first and then bytewise — not the map's own order.
        Value::Struct(m) => {
            header(5, m.len() as u64, out);
            let mut pairs: Vec<(Vec<u8>, &Value)> = m.iter().map(|(k, v)| (encode(&Value::Text(k.clone())), v)).collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in pairs {
                out.extend_from_slice(&k);
                build(v, out);
            }
        }
    }
}

// * Decoding

/// Why some bytes are not the canonical encoding of any value; the first
/// rule the input broke, reading left to right (`Ark.Canon.DecodeError`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A head longer than the shortest that holds its argument, or the
    /// reserved additional information 28–30.
    NonCanonicalHead,
    /// Additional information 31: an indefinite length or a stray break.
    IndefiniteLength,
    /// A map key that sorts before the key preceding it.
    UnsortedKeys,
    /// A map key equal to the key preceding it.
    DuplicateKey,
    /// A map key that is not a text string.
    NonTextKey,
    /// A tag other than 37.
    BadTag,
    /// Tag 37 over something other than a byte string of sixteen bytes.
    BadId,
    /// A half, single or double float.
    Float,
    /// A simple value other than false, true and null.
    BadSimple,
    /// A text string whose bytes are not UTF-8.
    BadUtf8,
    /// An integer outside `i64`.
    IntOutOfRange,
    /// Bytes after the value.
    Trailing,
    /// The input ended inside a value.
    Truncated,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecodeError {}

/// A value from its canonical bytes, and only from those.
pub fn decode(input: &[u8]) -> Result<Value, DecodeError> {
    let mut p = Parser { s: input };
    let v = p.value()?;
    if p.s.is_empty() {
        Ok(v)
    } else {
        Err(DecodeError::Trailing)
    }
}

/// Whether some bytes are a canonical encoding: they decode, and what they
/// decode to encodes back to exactly them.
pub fn round_trip(b: &[u8]) -> bool {
    match decode(b) {
        Ok(v) => encode(&v) == b,
        Err(_) => false,
    }
}

struct Parser<'a> {
    s: &'a [u8],
}

const INT64_LIMIT: u64 = i64::MAX as u64;

impl<'a> Parser<'a> {
    fn byte(&mut self) -> Result<u8, DecodeError> {
        match self.s.split_first() {
            Some((b, rest)) => {
                self.s = rest;
                Ok(*b)
            }
            None => Err(DecodeError::Truncated),
        }
    }

    fn peek(&self) -> Result<u8, DecodeError> {
        self.s.first().copied().ok_or(DecodeError::Truncated)
    }

    fn chunk(&mut self, n: u64) -> Result<&'a [u8], DecodeError> {
        if n > self.s.len() as u64 {
            return Err(DecodeError::Truncated);
        }
        let (a, rest) = self.s.split_at(n as usize);
        self.s = rest;
        Ok(a)
    }

    fn big_endian(&mut self, k: usize) -> Result<u64, DecodeError> {
        let mut acc: u64 = 0;
        for _ in 0..k {
            acc = (acc << 8) | self.byte()? as u64;
        }
        Ok(acc)
    }

    /// The argument of a head, insisting on the shortest form.
    fn argument(&mut self, ai: u8) -> Result<u64, DecodeError> {
        let (k, least) = match ai {
            0..=23 => return Ok(ai as u64),
            24 => (1, 24),
            25 => (2, 0x100),
            26 => (4, 0x1_0000),
            27 => (8, 0x1_0000_0000),
            31 => return Err(DecodeError::IndefiniteLength),
            _ => return Err(DecodeError::NonCanonicalHead),
        };
        let n = self.big_endian(k)?;
        if n < least {
            Err(DecodeError::NonCanonicalHead)
        } else {
            Ok(n)
        }
    }

    fn text(&mut self, n: u64) -> Result<String, DecodeError> {
        let b = self.chunk(n)?;
        std::str::from_utf8(b).map(|s| s.to_string()).map_err(|_| DecodeError::BadUtf8)
    }

    /// One data item, as a value: the inverse of `build`, case for case.
    fn value(&mut self) -> Result<Value, DecodeError> {
        let ib = self.byte()?;
        let ai = ib & 0x1f;
        match ib >> 5 {
            0 => {
                let n = self.argument(ai)?;
                if n > INT64_LIMIT {
                    Err(DecodeError::IntOutOfRange)
                } else {
                    Ok(Value::Int(n as i64))
                }
            }
            1 => {
                let n = self.argument(ai)?;
                if n > INT64_LIMIT {
                    Err(DecodeError::IntOutOfRange)
                } else {
                    Ok(Value::Int(-1 - (n as i64)))
                }
            }
            2 => {
                let n = self.argument(ai)?;
                Ok(Value::Bytes(self.chunk(n)?.to_vec()))
            }
            3 => {
                let n = self.argument(ai)?;
                Ok(Value::Text(self.text(n)?))
            }
            4 => {
                let n = self.argument(ai)?;
                let mut xs = Vec::new();
                for _ in 0..n {
                    xs.push(self.value()?);
                }
                Ok(Value::List(xs))
            }
            5 => {
                let n = self.argument(ai)?;
                Ok(Value::Struct(self.pairs(n)?))
            }
            6 => {
                let t = self.argument(ai)?;
                if t != 37 {
                    Err(DecodeError::BadTag)
                } else {
                    Ok(Value::Id(self.identifier()?))
                }
            }
            _ => match ai {
                20 => Ok(Value::Bool(false)),
                21 => Ok(Value::Bool(true)),
                22 => Ok(Value::Null),
                25..=27 => Err(DecodeError::Float),
                31 => Err(DecodeError::IndefiniteLength),
                _ => Err(DecodeError::BadSimple),
            },
        }
    }

    /// `n` pairs whose keys are text strings in strictly increasing order of
    /// their encoded bytes, head included.
    fn pairs(&mut self, n: u64) -> Result<BTreeMap<String, Value>, DecodeError> {
        let mut m = BTreeMap::new();
        let mut prev: Option<&'a [u8]> = None;
        for _ in 0..n {
            let before = self.s;
            let k = self.key()?;
            let raw = &before[..before.len() - self.s.len()];
            if let Some(p) = prev {
                if raw == p {
                    return Err(DecodeError::DuplicateKey);
                }
                if raw < p {
                    return Err(DecodeError::UnsortedKeys);
                }
            }
            let v = self.value()?;
            m.insert(k, v);
            prev = Some(raw);
        }
        Ok(m)
    }

    /// A map key: a text string, and nothing else.
    fn key(&mut self) -> Result<String, DecodeError> {
        let ib = self.peek()?;
        if ib >> 5 != 3 {
            return Err(DecodeError::NonTextKey);
        }
        self.byte()?;
        let n = self.argument(ib & 0x1f)?;
        self.text(n)
    }

    /// What follows tag 37: a byte string of exactly sixteen bytes.
    fn identifier(&mut self) -> Result<Id, DecodeError> {
        let ib = self.byte()?;
        if ib >> 5 != 2 {
            return Err(DecodeError::BadId);
        }
        let n = self.argument(ib & 0x1f)?;
        if n != 16 {
            return Err(DecodeError::BadId);
        }
        let b = self.chunk(n)?;
        b.try_into().map_err(|_| DecodeError::BadId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_are_shortest_and_checked() {
        assert_eq!(encode(&Value::int(24)), vec![0x18, 24]);
        assert_eq!(encode(&Value::int(-1)), vec![0x20]);
        assert_eq!(encode(&Value::int(i64::MIN)), vec![0x3b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(decode(&[0x18, 23]), Err(DecodeError::NonCanonicalHead));
        assert_eq!(decode(&[0x1b, 0x80, 0, 0, 0, 0, 0, 0, 0]), Err(DecodeError::IntOutOfRange));
        assert_eq!(decode(&[0xd9, 0x00, 0x25, 0x50]), Err(DecodeError::NonCanonicalHead));
        assert_eq!(decode(&[0xf9, 0, 0]), Err(DecodeError::Float));
        assert_eq!(decode(&[0x9f]), Err(DecodeError::IndefiniteLength));
        assert_eq!(decode(&[0x01, 0x02]), Err(DecodeError::Trailing));
        assert_eq!(decode(&[0x62, 0xff, 0xfe]), Err(DecodeError::BadUtf8));
    }

    #[test]
    fn maps_are_length_first() {
        let v = Value::record(vec![("aa", Value::int(1)), ("b", Value::int(2))]);
        let b = encode(&v);
        assert_eq!(b, vec![0xa2, 0x61, 0x62, 0x02, 0x62, 0x61, 0x61, 0x01]);
        assert_eq!(decode(&b), Ok(v));
        assert_eq!(decode(&[0xa2, 0x62, 0x61, 0x61, 0x01, 0x61, 0x62, 0x02]), Err(DecodeError::UnsortedKeys));
        assert_eq!(decode(&[0xa2, 0x61, 0x62, 0x01, 0x61, 0x62, 0x02]), Err(DecodeError::DuplicateKey));
        assert_eq!(decode(&[0xa1, 0x01, 0x01]), Err(DecodeError::NonTextKey));
    }

    #[test]
    fn ids_are_tagged() {
        let v = Value::Id([0; 16]);
        let b = encode(&v);
        assert_eq!(&b[..3], &[0xd8, 0x25, 0x50]);
        assert!(round_trip(&b));
        assert_eq!(decode(&[0xd8, 0x25, 0x41, 0x00]), Err(DecodeError::BadId));
        assert_eq!(decode(&[0xd8, 0x26, 0x00]), Err(DecodeError::BadTag));
    }
}
