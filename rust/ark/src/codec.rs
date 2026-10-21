//! §1.1 The encoding: one byte string per value, and the order of values
//! of one type is the bytewise order of their encodings.
//!
//! - `Int`: eight bytes, the `i64` with its sign bit flipped, big-endian.
//! - `Bytes`: each `00` byte written `00 FF`; then `00 00`.
//! - `List T`: each element preceded by `01`; then `00`.
//! - `Tuple [T]`: the fields' encodings, concatenated.
//! - `Sum [Tuple [T]]`: one byte, the variant's index; then its tuple.
//!
//! Canonical, prefix-free and order-preserving (`tests/codec.rs` holds
//! all three over generated values). So comparing two values of a type is
//! `memcmp`, an index key is column bytes concatenated, and a tuple orders
//! by its first field, then its second.
//!
//! [`parse`] is the one door bytes enter by: it takes canonical bytes of
//! the type and nothing else — an unescaped `00`, a missing terminator, a
//! variant index out of range, a short `Int` or a trailing byte is a
//! [`Refusal`]. It checks structure; a refinement's predicate is the
//! language's to add at this same door.

use std::cell::RefCell;

use crate::hash::{EntryHash, SchemaHash, StateHash, TypeHash};
use crate::log::{EntryId, Origin};
use crate::ty::Ty;
use crate::Refusal;

// * Writing

pub fn put_int(out: &mut Vec<u8>, n: i64) {
    out.extend_from_slice(&((n as u64) ^ (1 << 63)).to_be_bytes());
}

pub fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    for &x in b {
        out.push(x);
        if x == 0 {
            out.push(0xFF);
        }
    }
    out.extend_from_slice(&[0, 0]);
}

pub fn put_variant(out: &mut Vec<u8>, i: u8) {
    out.push(i);
}

/// A list: each element by `each`, preceded by `01`; then `00`.
pub fn put_list<T>(out: &mut Vec<u8>, items: impl IntoIterator<Item = T>, mut each: impl FnMut(&mut Vec<u8>, T)) {
    for x in items {
        out.push(1);
        each(out, x);
    }
    out.push(0);
}

/// `Option<T>`: `00` for none, `01` then the value.
pub fn put_option<T>(out: &mut Vec<u8>, x: Option<T>, each: impl FnOnce(&mut Vec<u8>, T)) {
    match x {
        None => out.push(0),
        Some(x) => {
            out.push(1);
            each(out, x);
        }
    }
}

/// The encoding of one `Bytes` value, alone.
pub fn bytes_enc(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + 2);
    put_bytes(&mut out, b);
    out
}

/// The encoding of one `Int` value, alone.
pub fn int_enc(n: i64) -> [u8; 8] {
    ((n as u64) ^ (1 << 63)).to_be_bytes()
}

// * Reading

/// A cursor over canonical bytes. Every read refuses what is not canonical
/// for the type read.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}

fn fail<T>(why: &str, at: usize) -> Result<T, Refusal> {
    Err(Refusal(format!("{why} at byte {at}")))
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    fn byte(&mut self, what: &str) -> Result<u8, Refusal> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| Refusal(format!("{what}: the bytes end at byte {}", self.at)))?;
        self.at += 1;
        Ok(b)
    }

    pub fn int(&mut self) -> Result<i64, Refusal> {
        let Some(b) = self.bytes.get(self.at..self.at + 8) else {
            return fail("an int needs eight bytes", self.at);
        };
        self.at += 8;
        Ok((u64::from_be_bytes(b.try_into().unwrap()) ^ (1 << 63)) as i64)
    }

    /// The span of an escaped `Bytes` value, terminator excluded, and moves
    /// past it: what [`Reader::bytes`] unescapes and [`skip`] only checks.
    fn escaped(&mut self) -> Result<(usize, usize), Refusal> {
        let start = self.at;
        loop {
            match self.bytes.get(self.at) {
                None => return fail("bytes without their 00 00 terminator", start),
                Some(0) => match self.bytes.get(self.at + 1) {
                    Some(0) => {
                        self.at += 2;
                        return Ok((start, self.at - 2));
                    }
                    Some(0xFF) => self.at += 2,
                    Some(_) => return fail("an unescaped 00", self.at),
                    None => return fail("bytes without their 00 00 terminator", start),
                },
                Some(_) => self.at += 1,
            }
        }
    }

    pub fn bytes(&mut self) -> Result<Vec<u8>, Refusal> {
        let (s, e) = self.escaped()?;
        let mut out = Vec::with_capacity(e - s);
        let mut i = s;
        while i < e {
            out.push(self.bytes[i]);
            i += if self.bytes[i] == 0 { 2 } else { 1 };
        }
        Ok(out)
    }

    /// `Bytes` read as UTF-8 text: what every name in a frame is.
    pub fn text(&mut self) -> Result<String, Refusal> {
        let at = self.at;
        String::from_utf8(self.bytes()?).or_else(|_| fail("text that is not UTF-8", at))
    }

    /// Exactly `N` bytes in a `Bytes` value: a hash, an id.
    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N], Refusal> {
        let at = self.at;
        self.bytes()?.try_into().or_else(|_| fail(&format!("not {N} bytes"), at))
    }

    /// Whether a list has another element: `01` yes, `00` no.
    pub fn more(&mut self) -> Result<bool, Refusal> {
        match self.byte("a list")? {
            0 => Ok(false),
            1 => Ok(true),
            b => fail(&format!("a list marker {b:02x}"), self.at - 1),
        }
    }

    /// A list read element by element.
    pub fn list<T>(&mut self, mut each: impl FnMut(&mut Self) -> Result<T, Refusal>) -> Result<Vec<T>, Refusal> {
        let mut out = vec![];
        while self.more()? {
            out.push(each(self)?);
        }
        Ok(out)
    }

    /// A variant index below `n`.
    pub fn variant(&mut self, n: usize) -> Result<u8, Refusal> {
        let b = self.byte("a variant")?;
        if (b as usize) < n {
            Ok(b)
        } else {
            fail(&format!("variant {b} of a sum of {n}"), self.at - 1)
        }
    }

    pub fn option<T>(&mut self, each: impl FnOnce(&mut Self) -> Result<T, Refusal>) -> Result<Option<T>, Refusal> {
        match self.variant(2)? {
            0 => Ok(None),
            _ => each(self).map(Some),
        }
    }

    /// Nothing may follow a value.
    pub fn end(&self) -> Result<(), Refusal> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            fail("trailing bytes", self.at)
        }
    }
}

/// Moves past one value of `ty`, checking that it is canonical.
pub fn skip(ty: &Ty, r: &mut Reader) -> Result<(), Refusal> {
    match ty {
        Ty::Int => r.int().map(|_| ()),
        Ty::Bytes => r.escaped().map(|_| ()),
        Ty::List(t) => {
            while r.more()? {
                skip(t, r)?;
            }
            Ok(())
        }
        Ty::Tuple(ts) => ts.iter().try_for_each(|t| skip(t, r)),
        Ty::Sum(vs) => {
            let i = r.variant(vs.len())?;
            vs[i as usize].iter().try_for_each(|t| skip(t, r))
        }
        Ty::Named(n) => Err(Refusal(format!("{n}: a declared type, which the language reads"))),
    }
}

/// Bytes [`parse`] accepted: canonical bytes of the type it was given. The
/// field is private, so one that was not checked cannot be made.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Typed(pub(crate) Vec<u8>);

impl Typed {
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// §1.1 The boundary: canonical bytes of `ty`, whole, or why not.
pub fn parse(ty: &Ty, bytes: &[u8]) -> Result<Typed, Refusal> {
    let mut r = Reader::new(bytes);
    skip(ty, &mut r)?;
    r.end()?;
    Ok(Typed(bytes.to_vec()))
}

// * Rust types as values

/// A Rust type as a value of §1.1: how to read and write its bytes.
pub trait Node: Sized {
    fn ty() -> Ty;
    fn put(&self, out: &mut Vec<u8>);
    fn take(r: &mut Reader) -> Result<Self, Refusal>;

    /// `List Self`; `u8` makes it `Bytes`, so `Vec<u8>` is bytes.
    fn list_ty() -> Ty {
        Ty::list(Self::ty())
    }
    fn put_list(xs: &[Self], out: &mut Vec<u8>) {
        put_list(out, xs, |o, x| x.put(o));
    }
    fn take_list(r: &mut Reader) -> Result<Vec<Self>, Refusal> {
        r.list(Self::take)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![];
        self.put(&mut out);
        out
    }

    /// The value, from canonical bytes of it and nothing after them.
    fn from_bytes(b: &[u8]) -> Result<Self, Refusal> {
        let mut r = Reader::new(b);
        let x = Self::take(&mut r)?;
        r.end()?;
        Ok(x)
    }
}

thread_local!(static OPEN: RefCell<Vec<&'static str>> = const { RefCell::new(vec![]) });

/// The type a derived `Node` is: `f()`, or `Named(name)` where the type
/// recurs inside itself.
pub fn named(name: &'static str, f: impl FnOnce() -> Ty) -> Ty {
    if OPEN.with(|o| o.borrow().contains(&name)) {
        return Ty::Named(name.into());
    }
    OPEN.with(|o| o.borrow_mut().push(name));
    let t = f();
    OPEN.with(|o| o.borrow_mut().pop());
    t
}

impl Node for i64 {
    fn ty() -> Ty {
        Ty::Int
    }
    fn put(&self, out: &mut Vec<u8>) {
        put_int(out, *self);
    }
    fn take(r: &mut Reader) -> Result<i64, Refusal> {
        r.int()
    }
}

macro_rules! narrow {
    ($($t:ty),*) => {$(
        impl Node for $t {
            fn ty() -> Ty {
                Ty::Int
            }
            fn put(&self, out: &mut Vec<u8>) {
                put_int(out, *self as i64);
            }
            fn take(r: &mut Reader) -> Result<$t, Refusal> {
                let at = r.at;
                <$t>::try_from(r.int()?).map_err(|_| Refusal(format!("an int out of range at byte {at}")))
            }
        }
    )*};
}
narrow!(usize, u32);

impl Node for u8 {
    fn ty() -> Ty {
        Ty::Int
    }
    fn put(&self, out: &mut Vec<u8>) {
        put_int(out, *self as i64);
    }
    fn take(r: &mut Reader) -> Result<u8, Refusal> {
        u8::try_from(r.int()?).map_err(|_| Refusal::new("a byte out of range"))
    }
    fn list_ty() -> Ty {
        Ty::Bytes
    }
    fn put_list(xs: &[u8], out: &mut Vec<u8>) {
        put_bytes(out, xs);
    }
    fn take_list(r: &mut Reader) -> Result<Vec<u8>, Refusal> {
        r.bytes()
    }
}

impl Node for bool {
    fn ty() -> Ty {
        Ty::bool()
    }
    fn put(&self, out: &mut Vec<u8>) {
        out.push(*self as u8);
    }
    fn take(r: &mut Reader) -> Result<bool, Refusal> {
        Ok(r.variant(2)? == 1)
    }
}

impl Node for String {
    fn ty() -> Ty {
        Ty::Bytes
    }
    fn put(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.as_bytes());
    }
    fn take(r: &mut Reader) -> Result<String, Refusal> {
        r.text()
    }
}

impl Node for () {
    fn ty() -> Ty {
        Ty::unit()
    }
    fn put(&self, _: &mut Vec<u8>) {}
    fn take(_: &mut Reader) -> Result<(), Refusal> {
        Ok(())
    }
}

impl<T: Node> Node for Vec<T> {
    fn ty() -> Ty {
        T::list_ty()
    }
    fn put(&self, out: &mut Vec<u8>) {
        T::put_list(self, out);
    }
    fn take(r: &mut Reader) -> Result<Vec<T>, Refusal> {
        T::take_list(r)
    }
}

impl<T: Node> Node for Option<T> {
    fn ty() -> Ty {
        Ty::option(T::ty())
    }
    fn put(&self, out: &mut Vec<u8>) {
        crate::codec::put_option(out, self.as_ref(), |o, x| x.put(o));
    }
    fn take(r: &mut Reader) -> Result<Option<T>, Refusal> {
        r.option(T::take)
    }
}

impl<T: Node> Node for Box<T> {
    fn ty() -> Ty {
        T::ty()
    }
    fn put(&self, out: &mut Vec<u8>) {
        (**self).put(out);
    }
    fn take(r: &mut Reader) -> Result<Box<T>, Refusal> {
        T::take(r).map(Box::new)
    }
}

macro_rules! tuples {
    ($(($($t:ident $i:tt),+))*) => {$(
        impl<$($t: Node),+> Node for ($($t,)+) {
            fn ty() -> Ty {
                Ty::Tuple(vec![$($t::ty()),+])
            }
            fn put(&self, out: &mut Vec<u8>) {
                $(self.$i.put(out);)+
            }
            fn take(r: &mut Reader) -> Result<Self, Refusal> {
                Ok(($($t::take(r)?,)+))
            }
        }
    )*};
}
tuples!((A 0) (A 0, B 1) (A 0, B 1, C 2) (A 0, B 1, C 2, D 3));

/// A fixed number of bytes — a hash, an id — is `Bytes` of that length.
macro_rules! fixed {
    ($($t:ty: $n:literal),*) => {$(
        impl Node for $t {
            fn ty() -> Ty {
                Ty::Bytes
            }
            fn put(&self, out: &mut Vec<u8>) {
                put_bytes(out, &self.0);
            }
            fn take(r: &mut Reader) -> Result<$t, Refusal> {
                r.fixed::<$n>().map(Self)
            }
        }
    )*};
}
fixed!(StateHash: 32, TypeHash: 32, EntryHash: 32, SchemaHash: 32, EntryId: 16, Origin: 16);
