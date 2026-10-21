//! §1.1 Hash spaces, each its own type: [`StateHash`] (a store's rows
//! under its schema), [`EntryHash`] (the chain to a logged entry, §1.6)
//! and [`SchemaHash`] (a schema's JSON, which every state hash starts
//! from, §1.3). In JSON each is its hex. Each is SHA-256, thirty-two bytes, and
//! comparing two of different spaces does not compile.
//!
//! The state digests' arithmetic is here too: a table's digest is the sum
//! of its rows' leaves modulo 2^256, a leaf read as a big-endian unsigned
//! integer, so a write moves it by one leaf out and one in. The sum is not
//! a commitment against rows chosen to collide (`spec/README.md` §8.1 says
//! why); it is for replicas of one log to tell whether they agree.

use std::fmt;

use sha2::{Digest, Sha256};

/// SHA-256 over the parts, concatenated.
pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn hex(b: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(DIGITS[usize::from(x >> 4)] as char);
        s.push(DIGITS[usize::from(x & 15)] as char);
    }
    s
}

/// Lowercase hex's bytes; none for anything else.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    fn digit(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    if !s.len().is_multiple_of(2) {
        return None;
    }
    s.as_bytes().chunks(2).map(|p| Some(digit(p[0])? << 4 | digit(p[1])?)).collect()
}

macro_rules! spaces {
    ($($(#[$m:meta])* $n:ident),* $(,)?) => {$(
        $(#[$m])*
        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $n(pub [u8; 32]);
        impl $n {
            /// SHA-256 of the bytes, in this space.
            pub fn of(bytes: &[u8]) -> $n {
                $n(sha256(&[bytes]))
            }
            pub fn hex(&self) -> String {
                hex(&self.0)
            }
        }
        impl fmt::Debug for $n {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{}({})", stringify!($n), &self.hex()[..12])
            }
        }
        impl fmt::Display for $n {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(&self.hex())
            }
        }
        impl serde::Serialize for $n {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                crate::json::hex_bytes::serialize(&self.0, s)
            }
        }
        impl<'de> serde::Deserialize<'de> for $n {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<$n, D::Error> {
                crate::json::hex_bytes::deserialize(d).map($n)
            }
        }
    )*};
}

spaces! {
    /// `sha256(schema_hash ‖ the space's tables' digests in schema order)` (§1.3).
    StateHash,
    /// `sha256(prev ‖ json([seq, entry, facts, state]))` (§1.6); the
    /// chain's start is thirty-two zero bytes.
    EntryHash,
    /// `sha256(json(schema))` (§1.2).
    SchemaHash,
}

/// `a += b` modulo 2^256, both big-endian.
pub fn add256(a: &mut [u8; 32], b: &[u8; 32]) {
    let mut carry = 0u16;
    for i in (0..32).rev() {
        let s = a[i] as u16 + b[i] as u16 + carry;
        a[i] = s as u8;
        carry = s >> 8;
    }
}

/// `a -= b` modulo 2^256, both big-endian.
pub fn sub256(a: &mut [u8; 32], b: &[u8; 32]) {
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let d = a[i] as i16 - b[i] as i16 - borrow;
        a[i] = d as u8;
        borrow = (d < 0) as i16;
    }
}
