//! Text without `core::fmt`, which costs kilobytes on the watch: a line
//! of bytes built from strings and numbers, for the log and the screen.

/// A line of at most 160 bytes; what does not fit is cut.
pub struct Line {
    buf: [u8; 160],
    len: usize,
}

/// A view's words are written into a line (`ark_kernel::Hole`).
impl ark_kernel::Put for Line {
    fn put(&mut self, bytes: &[u8]) {
        self.s(bytes);
    }
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

    /// Adds a number in decimal, as a view's holes are written
    /// (`ark_kernel::show`, in 32 bits).
    pub fn n(&mut self, v: u64) -> &mut Line {
        ark_kernel::show::decimal(self, v);
        self
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
        l.s(b"at ").n(83).s(b", ").n(0).s(b" ").hex(0xab);
        ark_kernel::Hole::hole(&ark_kernel::Clock(605_000), &mut l);
        assert_eq!(l.bytes(), b"at 83, 0 00000000000000ab10:05");
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
            ark_kernel::Hole::hole(&ark_kernel::Clock(ms), &mut l);
            assert_eq!(l.bytes(), text.as_bytes(), "{ms}");
        }
    }
}
