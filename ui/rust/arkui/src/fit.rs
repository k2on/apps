//! Making a string fit a width.
//!
//! The failure it prevents is not subtle: `Wrapping::None` does not shorten a
//! string, it draws it at full length, and a long title runs straight under
//! the next column. Which part to take out depends on what the string is
//! ([`Cut`]), and there are two budgets: characters ([`tail`], [`middle`]),
//! for a width decided before layout, and pixels ([`within`]), because a
//! table cell is laid out — by then its column is a number and not an
//! estimate.

/// Which part of a string goes when it does not fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// The end, for a sentence or a label — a menu entry, a heading. `Go to
    /// Goldberg Variations, BWV…` still says what the row does, where taking
    /// the middle out spends most of the width on `Go to` and an ellipsis.
    /// It is also what AppKit does to a menu item that will not fit.
    End,
    /// The middle, for a name whose ends identify it — a table cell. A Bach
    /// movement is `Prelude No. 14 in F-sharp minor, BWV 859`, and the tail
    /// carries the catalogue number that tells it from the other twenty-three
    /// preludes; cutting the end leaves a column of `Prelude No. 14 in
    /// F-shar…`, the half that is the same in all of them. The bias is
    /// forward: the head is doing more work than the tail.
    Middle,
}

impl Cut {
    /// Of `keep` characters, how many are the start's; the rest are the end's.
    fn head(self, keep: usize) -> usize {
        match self {
            Cut::End => keep,
            Cut::Middle => keep.div_ceil(2),
        }
    }
}

/// `keep` characters of `chars` around one ellipsis, taken as `cut` says. With
/// `tidy`, a space beside the ellipsis goes: `Johann…`, not `Johann …`.
fn shorten(chars: &[char], keep: usize, cut: Cut, tidy: bool) -> String {
    let head = cut.head(keep);
    let start: String = chars[..head].iter().collect();
    let end: String = chars[chars.len() - (keep - head)..].iter().collect();
    match tidy {
        true => format!("{}\u{2026}{}", start.trim_end(), end.trim_start()),
        false => format!("{start}\u{2026}{end}"),
    }
}

/// Shorten to `max` characters, taking the *end* off ([`Cut::End`]). Anything
/// shorter than two characters of budget is left alone rather than reduced to
/// an ellipsis.
pub fn tail(body: &str, max: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() <= max || max < 2 {
        return body.to_string();
    }
    shorten(&chars, max - 1, Cut::End, false)
}

/// Shorten to `max` characters, taking the *middle* out ([`Cut::Middle`]).
pub fn middle(body: &str, max: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() <= max || max < 5 {
        return body.to_string();
    }
    shorten(&chars, max - 1, Cut::Middle, false)
}

/// The widest cut of `body` — as `cut` says, around one ellipsis — that
/// `measure`s no wider than `px`, or `body` itself when it already fits.
///
/// [`tail`] and [`middle`] with the budget found rather than guessed: a table
/// cell is handed its column's width at layout, so the characters it can
/// afford are asked of the shaped text instead of an average advance that is
/// wrong for every string in a different direction. It never answers wider
/// than `px`: where not even an ellipsis fits, it answers nothing. A space
/// beside the ellipsis is dropped, on either side, so a name cut between two
/// words reads `Johann…` rather than `Johann …`.
///
/// `measure` is asked a handful of times: the whole, then a first guess in
/// proportion to how far over it is, then a character at a time from there.
pub fn within(body: &str, px: f32, cut: Cut, measure: impl Fn(&str) -> f32) -> String {
    let full = measure(body);
    if full <= px {
        return body.to_string();
    }
    let chars: Vec<char> = body.chars().collect();
    // `k` characters kept around the ellipsis, from none (the ellipsis alone)
    // to all but one.
    let shown = |k: usize| shorten(&chars, k, cut, true);
    let most = chars.len().saturating_sub(1);
    let mut k = ((chars.len() as f32 * px / full).floor() as usize).min(most);
    while k > 0 && measure(&shown(k)) > px {
        k -= 1;
    }
    while k < most && measure(&shown(k + 1)) <= px {
        k += 1;
    }
    let out = shown(k);
    match measure(&out) <= px {
        true => out,
        false => String::new(),
    }
}

/// `body` with what the typeface cannot draw spelt in what it can: the
/// compatibility characters a title carries for their look — Roman numerals
/// (`NixCon 2026 Day Ⅰ`), full-width Latin, the Latin ligatures — as the
/// letters they stand for, which is their compatibility decomposition. Fira
/// Sans has none of them, and a browser has no system font to fall back on,
/// so each was a box. Anything else is left as it is.
pub fn drawable(body: &str) -> std::borrow::Cow<'_, str> {
    const ROMAN: [&str; 16] = [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "L", "C", "D", "M",
    ];
    let folds = |c: char| matches!(c as u32, 0x2160..=0x217f | 0xff01..=0xff5e | 0xfb00..=0xfb04);
    if !body.chars().any(folds) {
        return std::borrow::Cow::Borrowed(body);
    }
    let mut out = String::with_capacity(body.len());
    for c in body.chars() {
        match c as u32 {
            n @ 0x2160..=0x216f => out.push_str(ROMAN[(n - 0x2160) as usize]),
            n @ 0x2170..=0x217f => out.push_str(&ROMAN[(n - 0x2170) as usize].to_lowercase()),
            n @ 0xff01..=0xff5e => out.push(char::from_u32(n - 0xfee0).unwrap_or(c)),
            0xfb00 => out.push_str("ff"),
            0xfb01 => out.push_str("fi"),
            0xfb02 => out.push_str("fl"),
            0xfb03 => out.push_str("ffi"),
            0xfb04 => out.push_str("ffl"),
            _ => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}
