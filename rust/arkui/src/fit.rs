//! Making a string fit a width that is only known roughly.
//!
//! iced lays text out in pixels, after `view`; everything here runs before
//! that, so it works in characters against an estimate. The failure it
//! prevents is not subtle: `Wrapping::None` does not shorten a string, it
//! draws it at full length, and a long title runs straight under the next
//! column. Which end to take off depends on what the string is, so there are
//! two.
use iced::Length;

/// Roughly how many characters fit in one `FillPortion` of a table at size 13.
///
/// An estimate on purpose, and it errs short: erring short costs an ellipsis
/// nobody needed, erring long costs two columns of text on top of each other.
pub const PER_PORTION: usize = 13;

/// Shorten to `max` characters, taking the *end* off.
///
/// The right one for a sentence — a menu entry. `Go to Goldberg Variations,
/// BWV…` still says what the row does, where taking the middle out spends
/// most of the width on `Go to` and an ellipsis. It is also what AppKit does
/// to a menu item that will not fit. Anything shorter than two characters of
/// budget is left alone rather than reduced to an ellipsis.
pub fn tail(body: &str, max: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() <= max || max < 2 {
        return body.to_string();
    }
    let mut out: String = chars[..max - 1].iter().collect();
    out.push('\u{2026}');
    out
}

/// Shorten to `max` characters, taking the *middle* out.
///
/// The right one for a name whose ends identify it: a Bach movement is
/// `Prelude No. 14 in F-sharp minor, BWV 859`, and the tail carries the
/// catalogue number that tells it from the other twenty-three preludes.
/// Clipping the end leaves a column of `Prelude No. 14 in F-shar…`, which is
/// the half that is the same in all of them. The bias is forward: the head is
/// doing more work than the tail.
pub fn middle(body: &str, max: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() <= max || max < 5 {
        return body.to_string();
    }
    let keep = max - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('\u{2026}');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

/// What a table cell of `width` can draw of `body`: the middle taken out to
/// [`PER_PORTION`] a portion, and anything that is not a portion left alone.
pub fn to_width(body: &str, width: Length) -> String {
    match width {
        Length::FillPortion(n) => middle(body, n as usize * PER_PORTION),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ends, taken off the two ways, and nothing touched that fits.
    ///
    /// Falsified by making `middle` keep only its head (`let head = keep`):
    /// the first assertion names the `Prelude No. 14 in F-sh…` it drew.
    #[test]
    fn a_sentence_loses_its_end_and_a_name_its_middle() {
        let name = "Prelude No. 14 in F-sharp minor, BWV 859";
        assert_eq!(middle(name, 21), "Prelude No\u{2026}r, BWV 859");
        assert_eq!(tail("Go to Goldberg Variations, BWV 988", 20), "Go to Goldberg Vari\u{2026}");
        assert_eq!(middle(name, 200), name, "what fits is left alone");
        assert_eq!(tail(name, 200), name);
        assert_eq!(middle(name, 21).chars().count(), 21, "and what is cut is exactly the budget");
    }
}
