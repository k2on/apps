//! Numbers as people say them: a length, a span of years, a count of things.

/// Seconds as `m:ss`, which is how long one piece is written down. Empty for
/// something that is not a length yet (a stream that has not reported one).
pub fn clock(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return String::new();
    }
    let secs = secs as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// How long a whole record runs, in the units people say it in.
///
/// [`clock`] is `m:ss` because that is how one piece is written down; a
/// two-and-a-half-hour oratorio in `m:ss` is `150:37`, which nobody reads as a
/// length. So a header says `2 hr 30 min` and a row still says `4:21`.
pub fn spell(ms: i64) -> String {
    let minutes = ms / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// `track` or `tracks`. English, and only for words that take an `s`.
pub fn plural(n: i64, word: &str) -> String {
    match n {
        1 => word.to_string(),
        _ => format!("{word}s"),
    }
}

/// `1685–1750`, or `1685–` for somebody still alive, or nothing at all.
///
/// Empty rather than a dash where a date should be: somebody nobody has
/// described yet gets whatever else is known in its place, which is a fact.
pub fn span(born: i64, died: i64) -> String {
    match (born, died) {
        (0, 0) => String::new(),
        (0, d) => format!("\u{2013}{d}"),
        (b, 0) => format!("{b}\u{2013}"),
        (b, d) => format!("{b}\u{2013}{d}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case each one exists for. Falsified by making `spell` print total
    /// minutes: Messiah comes out `150 min`.
    #[test]
    fn a_length_is_said_the_way_people_say_it() {
        assert_eq!(clock(261.9), "4:21");
        assert_eq!(clock(f64::NAN), "");
        assert_eq!(spell(150 * 60_000 + 37_000), "2 hr 30 min");
        assert_eq!(spell(60 * 60_000), "1 hr");
        assert_eq!(spell(42 * 60_000), "42 min");
        assert_eq!(plural(1, "track"), "track");
        assert_eq!(plural(0, "track"), "tracks");
        assert_eq!(span(1685, 1750), "1685\u{2013}1750");
        assert_eq!(span(0, 0), "");
    }
}
