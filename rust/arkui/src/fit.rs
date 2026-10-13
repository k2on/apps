//! Making a string fit a width.
//!
//! The failure it prevents is not subtle: `Wrapping::None` does not shorten a
//! string, it draws it at full length, and a long title runs straight under
//! the next column. Which end to take off depends on what the string is, so
//! there are two cuts, in characters ([`tail`], [`middle`]) — and one place
//! that knows the width in pixels ([`within`]), because a table cell does:
//! by the time iced lays it out, its column is a number and not an estimate.

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

/// The longest start of `body` that, with an ellipsis after it, `measure`s
/// no wider than `px` — or `body` itself when it already fits.
///
/// This is [`tail`] with the budget found rather than guessed: a table cell is
/// handed its column's width at layout, so the characters it can afford are
/// asked of the shaped text instead of an average advance that is wrong for
/// every string in a different direction. It never answers wider than `px`:
/// where not even an ellipsis fits, it answers nothing. A space left before
/// the ellipsis is dropped, so a name cut between two words reads `Johann…`
/// rather than `Johann …`.
///
/// `measure` is asked a handful of times: the whole, then a first guess in
/// proportion to how far over it is, then a character at a time from there.
pub fn within(body: &str, px: f32, measure: impl Fn(&str) -> f32) -> String {
    let full = measure(body);
    if full <= px {
        return body.to_string();
    }
    let n = body.chars().count();
    // `tail(body, k + 1)` is `k` characters and an ellipsis, for `k` in
    // `1..=n - 2`; zero characters is the ellipsis alone.
    let cut = |k: usize| -> String {
        if k == 0 {
            return '\u{2026}'.to_string();
        }
        let cut = tail(body, k + 1);
        let head = cut.strip_suffix('\u{2026}').unwrap_or(&cut).trim_end();
        format!("{head}\u{2026}")
    };
    let most = n.saturating_sub(2);
    let mut k = ((n as f32 * px / full).floor() as usize).min(most);
    while k > 0 && measure(&cut(k)) > px {
        k -= 1;
    }
    while k < most && measure(&cut(k + 1)) <= px {
        k += 1;
    }
    let out = cut(k);
    match measure(&out) <= px {
        true => out,
        false => String::new(),
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

    /// A width that is not every character's: capitals are wider, as they are
    /// in the face a cell draws.
    fn advance(s: &str) -> f32 {
        s.chars()
            .map(|c| match c {
                '\u{2026}' => 10.0,
                c if c.is_uppercase() => 9.0,
                ' ' => 3.0,
                _ => 6.5,
            })
            .sum()
    }

    /// What does not fit ends in an ellipsis and never measures wider than
    /// the column, at every width from nothing to more than it needs — and is
    /// the *longest* start that does, so a column with room for one more
    /// character draws it. What fits is drawn whole.
    ///
    /// Falsified by dropping the loop that steps the first guess down, which
    /// leaves only the last guard between an over-long guess and the column:
    /// `an ellipsis fits in 10px and nothing was drawn`. Dropping the guard as
    /// well, so the guess is trusted, draws past the edge: `"…" is 10 wide in
    /// a 0px column`.
    #[test]
    fn what_does_not_fit_ends_in_an_ellipsis_inside_its_width() {
        for body in [
            "Johann Sebastian Bach",
            "Goldberg Variations, BWV 988",
            "Frédéric Chopin",
            "Aria",
            "WWWWWWWW",
        ] {
            let whole = advance(body);
            assert_eq!(within(body, whole, advance), body, "{body} fits exactly");
            assert_eq!(within(body, whole + 40.0, advance), body, "{body} fits with room");
            for px in 0..whole.ceil() as usize {
                let px = px as f32;
                let out = within(body, px, advance);
                assert!(advance(&out) <= px, "{out:?} is {} wide in a {px}px column", advance(&out));
                if out.is_empty() {
                    assert!(advance("\u{2026}") > px, "an ellipsis fits in {px}px and nothing was drawn");
                    continue;
                }
                assert!(out.ends_with('\u{2026}'), "{out:?} was cut with no ellipsis");
                let head = out.trim_end_matches('\u{2026}');
                assert!(body.starts_with(head), "{out:?} is not the start of {body:?}");
                // One more character, past any space the cut dropped.
                let m = head.chars().count();
                let space = body.chars().skip(m).take_while(|c| c.is_whitespace()).count();
                let longer = format!("{}\u{2026}", body.chars().take(m + space + 1).collect::<String>());
                assert!(
                    m + space + 1 >= body.chars().count() - 1 || advance(&longer) > px,
                    "{longer:?} would have fit in {px}px, and {out:?} was drawn"
                );
            }
        }
        assert_eq!(within("Johann Sebastian Bach", 75.0, advance), "Johann Se\u{2026}");
        assert_eq!(
            within("Johann Sebastian Bach", 55.0, advance),
            "Johann\u{2026}",
            "no space before the ellipsis"
        );
    }
}
