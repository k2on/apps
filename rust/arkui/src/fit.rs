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

    /// What does not fit is a start and an end of it around one ellipsis —
    /// only a start for [`Cut::End`] — with no space beside the ellipsis,
    /// never wider than the column, at every width from nothing to more than
    /// it needs; and it is the *widest* such cut, so a column with room for one
    /// more character draws it. What fits is drawn whole.
    ///
    /// Falsified by dropping the `trim_start` on the end's side of the cut:
    /// `"Johan… Bach" has a space beside its ellipsis`.
    #[test]
    fn what_does_not_fit_loses_its_middle_inside_its_width() {
        let bodies = [
            "Johann Sebastian Bach",
            "Prelude No. 14 in F-sharp minor, BWV 859",
            "Frédéric Chopin",
            "Aria",
            "WWWWWWWW",
        ];
        for cut in [Cut::Middle, Cut::End] {
            for body in bodies {
                let chars: Vec<char> = body.chars().collect();
                let whole = advance(body);
                assert_eq!(within(body, whole, cut, advance), body, "{body} fits exactly");
                assert_eq!(within(body, whole + 40.0, cut, advance), body, "{body} fits with room");
                for px in 0..whole.ceil() as usize {
                    let px = px as f32;
                    let out = within(body, px, cut, advance);
                    assert!(advance(&out) <= px, "{out:?} is {} wide in a {px}px column", advance(&out));
                    if out.is_empty() {
                        assert!(advance("\u{2026}") > px, "an ellipsis fits in {px}px and nothing was drawn");
                        continue;
                    }
                    let (start, end) = out.split_once('\u{2026}').unwrap_or_else(|| panic!("{out:?} has no ellipsis"));
                    assert!(!end.contains('\u{2026}'), "{out:?} has two");
                    assert!(body.starts_with(start) && body.ends_with(end), "{out:?} is not {body:?} around a cut");
                    assert!(!start.ends_with(' ') && !end.starts_with(' '), "{out:?} has a space beside its ellipsis");
                    if cut == Cut::End {
                        assert!(end.is_empty(), "{out:?} kept an end");
                    }
                    // No wider cut fits: every longer one is either this one
                    // again (a space dropped) or too wide.
                    for k in start.chars().count() + end.chars().count() + 1..chars.len() {
                        let wider = shorten(&chars, k, cut, true);
                        assert!(wider == out || advance(&wider) > px, "{wider:?} fits in {px}px, and {out:?} was drawn");
                    }
                }
            }
        }
        // A movement keeps its catalogue number, which is what tells it from
        // its twenty-three siblings.
        let prelude = "Prelude No. 14 in F-sharp minor, BWV 859";
        assert_eq!(within(prelude, 130.0, Cut::Middle, advance), "Prelude N\u{2026}, BWV 859");
        // ` Bach` would fit as well, and its space is dropped.
        assert_eq!(within("Johann Sebastian Bach", 75.0, Cut::Middle, advance), "Johan\u{2026}Bach");
        assert_eq!(within("Johann Sebastian Bach", 75.0, Cut::End, advance), "Johann Se\u{2026}");
        assert_eq!(within("Johann Sebastian Bach", 55.0, Cut::End, advance), "Johann\u{2026}");
    }
}
