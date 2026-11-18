//! Text without `core::fmt`, which costs kilobytes on the watch: a line
//! of bytes built from strings and numbers, for the log and the screen.

/// A line of at most 160 bytes; what does not fit is cut.
pub struct Line {
    buf: [u8; 160],
    len: usize,
}

impl Default for Line {
    fn default() -> Line {
        Line::new()
    }
}

impl Line {
    pub const fn new() -> Line {
        Line { buf: [0; 160], len: 0 }
    }

    /// A line on the heap.
    #[inline(never)]
    pub fn boxed() -> alloc::boxed::Box<Line> {
        alloc::boxed::Box::new(Line::new())
    }

    /// Adds bytes.
    pub fn s(&mut self, s: &[u8]) -> &mut Line {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s[..n]);
        self.len += n;
        self
    }

    /// Adds a number in decimal. In 32 bits, which a Cortex-M3 divides
    /// (64-bit division links a kilobyte of compiler builtins): a number
    /// past `u32::MAX` is shown as it.
    pub fn n(&mut self, v: u64) -> &mut Line {
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
        self.s(&digits[at..])
    }

    /// Adds a number in hexadecimal, sixteen digits.
    pub fn hex(&mut self, v: u64) -> &mut Line {
        let mut digits = [0u8; 16];
        for (i, d) in digits.iter_mut().enumerate() {
            let nib = (v >> (60 - 4 * i)) as u8 & 15;
            *d = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
        }
        self.s(&digits)
    }

    /// Adds milliseconds as a clock reads them, `m:ss`, or `h:mm:ss` from
    /// an hour: the text `ark_format::clock` gives every other client.
    pub fn clock(&mut self, ms: u64) -> &mut Line {
        let secs = ms.min(u64::from(u32::MAX)) as u32 / 1000;
        if secs >= 3600 {
            self.n(u64::from(secs / 3600)).s(b":").two(secs / 60 % 60)
        } else {
            self.n(u64::from(secs / 60))
        }
        .s(b":")
        .two(secs % 60)
    }

    /// Adds a number below a hundred in two digits.
    fn two(&mut self, v: u32) -> &mut Line {
        if v < 10 {
            self.s(b"0");
        }
        self.n(u64::from(v))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Writes the line to the app's log.
    pub fn log(&self) {
        crate::sys::log(self.bytes());
    }
}

/// Logs the parts as one line.
#[macro_export]
macro_rules! log {
    ($($part:expr),* $(,)?) => {{
        // On the heap: the watch's stack is small, and a line is 168 bytes.
        let mut line = $crate::text::Line::boxed();
        $( $crate::text::Part::put(&$part, &mut *line); )*
        line.log();
    }};
}

/// What [`log!`] takes: strings and numbers.
pub trait Part {
    fn put(&self, line: &mut Line);
}

impl Part for &str {
    fn put(&self, line: &mut Line) {
        line.s(self.as_bytes());
    }
}

impl Part for &[u8] {
    fn put(&self, line: &mut Line) {
        line.s(self);
    }
}

macro_rules! number_part {
    ($($t:ty),*) => {$(
        impl Part for $t {
            fn put(&self, line: &mut Line) {
                line.n(*self as u64);
            }
        }
    )*};
}

number_part!(u8, u16, u32, u64, usize);

impl Part for i64 {
    fn put(&self, line: &mut Line) {
        if *self < 0 {
            line.s(b"-");
        }
        line.n(self.unsigned_abs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let mut l = Line::new();
        l.s(b"at ").clock(83_400).s(b" of ").clock(605_000).s(b", ").n(0).s(b" ").hex(0xab);
        assert_eq!(l.bytes(), b"at 1:23 of 10:05, 0 00000000000000ab");
    }

    /// `ark_format::clock`'s cases, which the phones, the desktop and the
    /// browser show. Falsified by writing an hour as minutes: `121:43`.
    #[test]
    fn a_clock_reads_as_every_client_writes_it() {
        for (ms, text) in [
            (261_900, "4:21"),
            (0, "0:00"),
            (5_000, "0:05"),
            (3_599_000, "59:59"),
            (3_600_000, "1:00:00"),
            (7_123_000, "1:58:43"),
            (36_005_000, "10:00:05"),
        ] {
            let mut l = Line::new();
            l.clock(ms);
            assert_eq!(l.bytes(), text.as_bytes(), "{ms}");
        }
    }
}
