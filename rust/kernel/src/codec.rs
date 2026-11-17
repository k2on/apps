//! The kernel's own codec: compact, binary, and the same on every device.
//!
//! ```text
//! unsigned   varint: seven bits a byte, low first, the high bit "more"
//! signed     zigzag, then varint
//! bool, u8   one byte
//! bytes      varint length, then the bytes
//! Option<T>  0, or 1 then T
//! Vec<T>     varint count, then each T
//! a struct   its fields in order, nothing between
//! an enum    a varint tag, then its fields
//! a frame    varint length, then the value
//! ```
//!
//! Every value has one encoding: a varint written longer than it needs, a
//! bool other than 0 or 1, or a value with bytes left over is refused. So
//! an entry decoded and encoded again is the same bytes, and a hash of them
//! means one thing.
//!
//! [`Encode`] and [`Decode`] are written by hand for now; a derive writes
//! them later. [`Frames`] reads frames from bytes delivered in any parts,
//! as a socket or a watch's messages deliver them.

use alloc::vec::Vec;

/// Why bytes are not a value. A code, worded by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes end before the value does.
    Short,
    /// A varint longer than its value needs, or too large for its type.
    Overlong,
    /// A tag, or a bool, that names nothing.
    Tag,
    /// Bytes left after the value.
    Trailing,
    /// A frame longer than the reader takes.
    TooLong,
}

/// A value written in the codec.
pub trait Encode {
    fn encode(&self, out: &mut Vec<u8>);
}

/// A value read from the codec.
pub trait Decode: Sized {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error>;
}

/// The value's bytes.
pub fn to_vec<T: Encode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode(&mut out);
    out
}

/// The value these bytes are, all of them.
pub fn from_slice<T: Decode>(bytes: &[u8]) -> Result<T, Error> {
    let mut r = Reader::new(bytes);
    let v = T::decode(&mut r)?;
    if r.is_empty() {
        Ok(v)
    } else {
        Err(Error::Trailing)
    }
}

/// Writes `v` as a varint.
pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Writes `b` as length-prefixed bytes.
pub fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// Writes `v` as a frame: its length, then its bytes.
pub fn put_frame<T: Encode + ?Sized>(out: &mut Vec<u8>, v: &T) {
    let body = to_vec(v);
    put_bytes(out, &body);
}

/// Bytes being read, from the front.
pub struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes }
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// How many bytes are left.
    pub fn remaining(&self) -> usize {
        self.bytes.len()
    }

    pub fn byte(&mut self) -> Result<u8, Error> {
        let (&b, rest) = self.bytes.split_first().ok_or(Error::Short)?;
        self.bytes = rest;
        Ok(b)
    }

    /// The next `n` bytes.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.bytes.len() {
            return Err(Error::Short);
        }
        let (b, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(b)
    }

    pub fn varint(&mut self) -> Result<u64, Error> {
        let mut v = 0u64;
        for i in 0..10 {
            let b = self.byte()?;
            // The tenth byte holds the 64th bit only.
            if i == 9 && b > 1 {
                return Err(Error::Overlong);
            }
            v |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                // A last byte of 0 after others wrote the value too long.
                if b == 0 && i > 0 {
                    return Err(Error::Overlong);
                }
                return Ok(v);
            }
        }
        Err(Error::Overlong)
    }

    /// Length-prefixed bytes.
    pub fn bytes(&mut self) -> Result<&'a [u8], Error> {
        let n = self.varint()?;
        self.take(usize::try_from(n).map_err(|_| Error::Short)?)
    }

    /// A count of things to come, bounded by the bytes left so that a
    /// count no frame could hold allocates nothing.
    pub fn count(&mut self) -> Result<usize, Error> {
        let n = self.varint()?;
        usize::try_from(n).ok().filter(|&n| n <= self.bytes.len()).ok_or(Error::Short)
    }
}

impl Encode for u8 {
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(*self);
    }
}

impl Decode for u8 {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        r.byte()
    }
}

impl Encode for bool {
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(u8::from(*self));
    }
}

impl Decode for bool {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match r.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Tag),
        }
    }
}

macro_rules! unsigned {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, out: &mut Vec<u8>) {
                put_varint(out, *self as u64);
            }
        }

        impl Decode for $t {
            fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
                <$t>::try_from(r.varint()?).map_err(|_| Error::Overlong)
            }
        }
    )*};
}

unsigned!(u16, u32, u64);

impl Encode for i64 {
    fn encode(&self, out: &mut Vec<u8>) {
        put_varint(out, ((*self << 1) ^ (*self >> 63)) as u64);
    }
}

impl Decode for i64 {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        let v = r.varint()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            None => out.push(0),
            Some(v) => {
                out.push(1);
                v.encode(out);
            }
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match r.byte()? {
            0 => Ok(None),
            1 => Ok(Some(T::decode(r)?)),
            _ => Err(Error::Tag),
        }
    }
}

/// A list: its count, then each. A `Vec<u8>` is so length-prefixed bytes.
impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, out: &mut Vec<u8>) {
        put_varint(out, self.len() as u64);
        for v in self {
            v.encode(out);
        }
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        let n = r.count()?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(T::decode(r)?);
        }
        Ok(v)
    }
}

/// Frames read from bytes that arrive in parts: each part is pushed as it
/// comes, and [`Frames::read`] gives each whole frame's value once all its
/// bytes are in.
///
/// A frame that is whole but not a value is an error and is skipped, so
/// the frames after it still read. A frame longer than `most` is an error
/// that leaves the stream unread: the connection is to be closed.
pub struct Frames {
    buf: Vec<u8>,
    at: usize,
    most: usize,
}

impl Frames {
    /// A reader of frames of at most `most` bytes each.
    pub fn new(most: usize) -> Frames {
        Frames {
            buf: Vec::new(),
            at: 0,
            most,
        }
    }

    /// Bytes as they arrived.
    pub fn push(&mut self, part: &[u8]) {
        // Read bytes are let go of once they are half of what is held.
        if self.at > 0 && self.at * 2 >= self.buf.len() {
            self.buf.drain(..self.at);
            self.at = 0;
        }
        self.buf.extend_from_slice(part);
    }

    /// How many bytes are held and not yet read.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.at
    }

    /// The next whole frame's value, or `None` until its bytes are in.
    pub fn read<T: Decode>(&mut self) -> Result<Option<T>, Error> {
        let held = &self.buf[self.at..];
        let mut r = Reader::new(held);
        let len = match r.varint() {
            Ok(len) => len,
            Err(Error::Short) => return Ok(None),
            Err(e) => return Err(e),
        };
        let len = usize::try_from(len).ok().filter(|&n| n <= self.most).ok_or(Error::TooLong)?;
        let head = held.len() - r.remaining();
        if r.remaining() < len {
            return Ok(None);
        }
        let value = from_slice(&held[head..head + len]);
        self.at += head + len;
        if self.at == self.buf.len() {
            self.buf.clear();
            self.at = 0;
        }
        value.map(Some)
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::Rng;
    use alloc::vec;

    fn back<T: Encode + Decode + PartialEq + core::fmt::Debug>(v: T) {
        let bytes = to_vec(&v);
        assert_eq!(from_slice::<T>(&bytes), Ok(v));
    }

    #[cfg_attr(test, test)]
    fn varints_are_shortest() {
        for (v, bytes) in [
            (0u64, &[0u8][..]),
            (1, &[1]),
            (127, &[0x7f]),
            (128, &[0x80, 1]),
            (300, &[0xac, 2]),
            (u64::MAX, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1]),
        ] {
            assert_eq!(to_vec(&v), bytes);
            assert_eq!(from_slice::<u64>(bytes), Ok(v));
        }
        // Longer than needed, past 64 bits, and too large for the type.
        assert_eq!(from_slice::<u64>(&[0x80, 0]), Err(Error::Overlong));
        assert_eq!(
            from_slice::<u64>(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2]),
            Err(Error::Overlong)
        );
        assert_eq!(from_slice::<u16>(&[0x80, 0x80, 4]), Err(Error::Overlong));
        assert_eq!(from_slice::<u64>(&[0x80]), Err(Error::Short));
    }

    #[cfg_attr(test, test)]
    fn primitives_round_trip() {
        for v in [0i64, 1, -1, 63, -64, 64, i64::MIN, i64::MAX] {
            back(v);
        }
        assert_eq!(to_vec(&-1i64), [1]);
        back(true);
        back(false);
        back(7u8);
        back(u32::MAX);
        back(Some(5u16));
        back(None::<u16>);
        back(vec![1u8, 2, 3]);
        back(vec![Some(vec![9u64]), None]);
        assert_eq!(to_vec(&vec![1u8, 2]), [2, 1, 2]);
    }

    #[cfg_attr(test, test)]
    fn malformed_is_refused() {
        assert_eq!(from_slice::<bool>(&[2]), Err(Error::Tag));
        assert_eq!(from_slice::<Option<u8>>(&[3, 0]), Err(Error::Tag));
        assert_eq!(from_slice::<u8>(&[1, 2]), Err(Error::Trailing));
        assert_eq!(from_slice::<Vec<u8>>(&[3, 1, 2]), Err(Error::Short));
        // A count no bytes could hold allocates nothing.
        assert_eq!(from_slice::<Vec<u64>>(&[0xff, 0xff, 0xff, 0xff, 0x0f]), Err(Error::Short));
    }

    #[cfg_attr(test, test)]
    fn frames_read_in_parts() {
        let mut wire = Vec::new();
        put_frame(&mut wire, &300u64);
        put_frame(&mut wire, &vec![7u8; 200]);
        put_frame(&mut wire, &true);
        let mut f = Frames::new(1024);
        let mut read = Vec::new();
        for b in &wire {
            f.push(core::slice::from_ref(b));
            if let Some(v) = f.read::<Vec<u8>>().ok().flatten() {
                read.push(v);
            }
        }
        // 300 and `true` are not bytes and are skipped; the list reads.
        assert_eq!(read, [vec![7u8; 200]]);
        assert_eq!(f.buffered(), 0);

        let mut f = Frames::new(1024);
        f.push(&wire[..1]);
        assert_eq!(f.read::<u64>(), Ok(None));
        f.push(&wire[1..3]);
        assert_eq!(f.read::<u64>(), Ok(Some(300)));
        assert_eq!(f.read::<u64>(), Ok(None));
        f.push(&wire[3..]);
        assert_eq!(f.read::<u64>(), Err(Error::Trailing));
        assert_eq!(f.read::<bool>(), Ok(Some(true)));
        assert_eq!(f.read::<bool>(), Ok(None));
    }

    #[cfg_attr(test, test)]
    fn frames_past_the_limit_are_refused() {
        let mut wire = Vec::new();
        put_frame(&mut wire, &vec![0u8; 100]);
        let mut f = Frames::new(64);
        f.push(&wire[..2]);
        assert_eq!(f.read::<Vec<u8>>(), Err(Error::TooLong));
    }

    /// Random values of every primitive, each read back as itself from
    /// frames split at random places.
    #[cfg_attr(test, test)]
    fn random_frames_round_trip() {
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            let values: Vec<(u64, i64, Option<Vec<u8>>, bool)> = (0..rng.below(8))
                .map(|_| {
                    let b = rng.bytes(40);
                    (rng.any(), rng.any() as i64, (rng.below(2) == 0).then_some(b), rng.below(2) == 0)
                })
                .collect();
            let mut wire = Vec::new();
            for (a, b, c, d) in &values {
                let mut v = Vec::new();
                a.encode(&mut v);
                b.encode(&mut v);
                c.encode(&mut v);
                d.encode(&mut v);
                put_bytes(&mut wire, &v);
            }
            let mut f = Frames::new(4096);
            let mut read = Vec::new();
            let mut at = 0;
            while at < wire.len() {
                let n = (1 + rng.below(16) as usize).min(wire.len() - at);
                f.push(&wire[at..at + n]);
                at += n;
                while let Some(v) = f.read::<Quad>().unwrap() {
                    read.push((v.0, v.1, v.2, v.3));
                }
            }
            assert_eq!(read, values);
        }
    }

    #[derive(Debug, PartialEq)]
    struct Quad(u64, i64, Option<Vec<u8>>, bool);

    impl Decode for Quad {
        fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
            Ok(Quad(u64::decode(r)?, i64::decode(r)?, Option::decode(r)?, bool::decode(r)?))
        }
    }

    crate::listed!(
        varints_are_shortest,
        primitives_round_trip,
        malformed_is_refused,
        frames_read_in_parts,
        frames_past_the_limit_are_refused,
        random_frames_round_trip,
    );
}
