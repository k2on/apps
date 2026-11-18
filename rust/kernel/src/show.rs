//! What a view shows, as values every device agrees on: words and
//! fractions, without `core::fmt` (which costs kilobytes on a watch).
//!
//! - [`Text`] is a view's words: a template whose holes are filled apart
//!   ([`text!`](crate::text)), never a `String` worked out elsewhere and
//!   handed in. A literal converts; there is no `From<String>`; logging
//!   keeps `format!`. The rule is every view's, on every toolkit.
//! - A [`Hole`] is anything that writes itself as words into a [`Put`]: a
//!   string, bytes, a number, a [`Clock`], a [`Text`]. A toolkit's line (a
//!   watch's layer, a phone's string) is a `Put`.
//! - A [`Fraction`] is how far along, in thousandths, as a meter shows it:
//!   a watch divides no floats.

use alloc::string::String;
use alloc::vec::Vec;

/// Where words are written: a toolkit's line, or bytes.
pub trait Put {
    fn put(&mut self, bytes: &[u8]);
}

impl Put for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// What writes itself as words: what fills a [`Text`]'s hole.
pub trait Hole {
    fn hole(&self, out: &mut dyn Put);
}

impl Hole for str {
    fn hole(&self, out: &mut dyn Put) {
        out.put(self.as_bytes());
    }
}

impl Hole for [u8] {
    fn hole(&self, out: &mut dyn Put) {
        out.put(self);
    }
}

impl Hole for Vec<u8> {
    fn hole(&self, out: &mut dyn Put) {
        out.put(self);
    }
}

impl Hole for String {
    fn hole(&self, out: &mut dyn Put) {
        out.put(self.as_bytes());
    }
}

impl<T: Hole + ?Sized> Hole for &T {
    fn hole(&self, out: &mut dyn Put) {
        (**self).hole(out);
    }
}

/// Nothing while there is none.
impl<T: Hole> Hole for Option<T> {
    fn hole(&self, out: &mut dyn Put) {
        if let Some(t) = self {
            t.hole(out);
        }
    }
}

macro_rules! number {
    ($($t:ty),*) => {$(
        impl Hole for $t {
            fn hole(&self, out: &mut dyn Put) {
                decimal(out, u64::try_from(*self).unwrap_or(0));
            }
        }
    )*};
}

number!(u8, u16, u32, u64, usize);

/// A number in decimal. In 32 bits, which a Cortex-M3 divides (64-bit
/// division links a kilobyte of compiler builtins): a number past
/// `u32::MAX` is written as it.
pub fn decimal(out: &mut dyn Put, v: u64) {
    let mut v = v.min(u64::from(u32::MAX)) as u32;
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.put(&digits[at..]);
}

/// Milliseconds as a clock reads them, `m:ss`, or `h:mm:ss` from an hour:
/// the text `ark_format::clock` gives every other client.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clock(pub u64);

impl Hole for Clock {
    fn hole(&self, out: &mut dyn Put) {
        fn two(out: &mut dyn Put, v: u32) {
            if v < 10 {
                out.put(b"0");
            }
            decimal(out, u64::from(v));
        }
        let secs = self.0.min(u64::from(u32::MAX)) as u32 / 1000;
        if secs >= 3600 {
            decimal(out, u64::from(secs / 3600));
            out.put(b":");
            two(out, secs / 60 % 60);
        } else {
            decimal(out, u64::from(secs / 60));
        }
        out.put(b":");
        two(out, secs % 60);
    }
}

/// How far along, in thousandths: what a meter shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fraction(pub u16);

impl Fraction {
    /// `part` of `whole`, in whole seconds as a clock under it reads them,
    /// in 32 bits.
    pub fn of_seconds(part_ms: u64, whole_ms: u64) -> Fraction {
        let whole = (whole_ms.min(u64::from(u32::MAX)) as u32 / 1000).max(1);
        let part = (part_ms.min(whole_ms).min(u64::from(u32::MAX)) as u32 / 1000).min(whole);
        Fraction((part * 1000 / whole) as u16)
    }
}

/// A view's words: a template, each hole `{}`, and what fills each,
/// written as words when the text is made (each its length, then it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    pub template: &'static str,
    pub holes: Vec<u8>,
}

impl Text {
    /// A template, its holes to come ([`Text::fill`]): what
    /// [`text!`](crate::text) makes.
    pub fn template(template: &'static str) -> Text {
        Text { template, holes: Vec::new() }
    }

    /// The next hole filled with `h`'s words.
    #[inline(never)]
    pub fn fill(mut self, h: &dyn Hole) -> Text {
        let at = self.holes.len();
        self.holes.extend_from_slice(&[0, 0]);
        h.hole(&mut self.holes);
        let n = (self.holes.len() - at - 2).min(usize::from(u16::MAX)) as u16;
        self.holes[at..at + 2].copy_from_slice(&n.to_le_bytes());
        self
    }

    /// What fills each hole, as words.
    pub fn holes(&self) -> impl Iterator<Item = &[u8]> {
        let mut rest = &self.holes[..];
        core::iter::from_fn(move || {
            let n = usize::from(u16::from_le_bytes([*rest.first()?, *rest.get(1)?]));
            let (h, r) = rest[2..].split_at(n.min(rest.len() - 2));
            rest = r;
            Some(h)
        })
    }

    /// The words, the holes filled, as a string (bytes that are not UTF-8
    /// replaced).
    pub fn words(&self) -> String {
        let mut out = Vec::new();
        self.hole(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }
}

impl From<&'static str> for Text {
    fn from(s: &'static str) -> Text {
        Text::template(s)
    }
}

impl Hole for Text {
    fn hole(&self, out: &mut dyn Put) {
        // By bytes, not `str::split`: a watch links no substring search.
        let (t, mut holes) = (self.template.as_bytes(), self.holes());
        let (mut from, mut i) = (0, 0);
        while i + 1 < t.len() {
            if t[i] == b'{' && t[i + 1] == b'}' {
                out.put(&t[from..i]);
                if let Some(h) = holes.next() {
                    out.put(h);
                }
                i += 2;
                from = i;
            } else {
                i += 1;
            }
        }
        out.put(&t[from..]);
    }
}

/// A [`Text`] template, its holes filled by what follows it, each a
/// [`Hole`]: `text!("{}, {} Pending", word, pending)`. Holes are `{}`
/// alone.
#[macro_export]
macro_rules! text {
    ($t:literal) => {
        $crate::show::Text::from($t)
    };
    ($t:literal, $($hole:expr),+ $(,)?) => {
        $crate::show::Text::template($t)$(.fill(&$hole))+
    };
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;

    crate::listed!(holes_are_written_as_words);

    fn words(h: &dyn Hole) -> Vec<u8> {
        let mut out = Vec::new();
        h.hole(&mut out);
        out
    }

    #[cfg_attr(test, test)]
    fn holes_are_written_as_words() {
        assert_eq!(words(&crate::text!("{}, {} Pending", "Online", 2u32)), b"Online, 2 Pending");
        assert_eq!(words(&crate::text!("Up Next")), b"Up Next");
        assert_eq!(words(&Clock(83_000)), b"1:23");
        assert_eq!(words(&Clock(3_723_000)), b"1:02:03");
        assert_eq!(words(&None::<u32>), b"");
        assert_eq!(Fraction::of_seconds(15_500, 60_000), Fraction(250));
        assert_eq!(Fraction::of_seconds(5, 0), Fraction(0));
    }
}
