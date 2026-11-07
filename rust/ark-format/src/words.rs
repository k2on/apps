//! Counts, names and facts in a line.

/// `track` or `tracks`. English, and only for words that take an `s`.
pub fn plural(n: i64, word: &str) -> String {
    match n {
        1 => word.to_string(),
        _ => format!("{word}s"),
    }
}

/// A count of things: `1 track`, `12 tracks`, `0 episodes`.
pub fn count(n: i64, word: &str) -> String {
    format!("{n} {}", plural(n, word))
}

/// Names as a sentence lists them, each once, in the order given: `Ada`,
/// `Ada and Grace`, `Ada, Grace and Edsger`. Empty names are left out.
pub fn names(all: &[String]) -> String {
    let mut once: Vec<&str> = Vec::with_capacity(all.len());
    for n in all.iter().map(|n| n.trim()) {
        if !n.is_empty() && !once.contains(&n) {
            once.push(n);
        }
    }
    match once.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The facts a header says, the empty ones left out, joined by a middle
/// dot: `12 songs · 48 min`.
pub fn facts(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| p.as_str())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// When somebody lived, as a catalogue heads a person: `1685–1750`,
/// `1950–` for the living, `–1750` for a birth nobody knows, or nothing at
/// all when neither year is known — rather than a dash where a date should
/// be, so whatever else is known stands in its place.
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

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn things_are_counted() {
        assert_eq!(plural(1, "track"), "track");
        assert_eq!(plural(0, "track"), "tracks");
        assert_eq!(count(1, "song"), "1 song");
        assert_eq!(count(12, "song"), "12 songs");
        assert_eq!(count(0, "episode"), "0 episodes");
    }

    /// Falsified by joining with commas alone (`Ada, Grace`), or by naming a
    /// guest twice whom the feed lists as host and guest.
    #[test]
    fn names_are_listed_as_a_sentence_lists_them() {
        assert_eq!(names(&[]), "");
        assert_eq!(names(&s(&["Ada"])), "Ada");
        assert_eq!(names(&s(&["Ada", "Grace"])), "Ada and Grace");
        assert_eq!(names(&s(&["Ada", "Grace", "Edsger"])), "Ada, Grace and Edsger");
        assert_eq!(names(&s(&["Ada", "Grace", "Ada", " ", "Edsger"])), "Ada, Grace and Edsger");
    }

    #[test]
    fn facts_and_lives_are_said_in_a_line() {
        assert_eq!(facts(&s(&["12 songs", "", "48 min"])), "12 songs \u{b7} 48 min");
        assert_eq!(facts(&s(&["", ""])), "");
        assert_eq!(span(1685, 1750), "1685\u{2013}1750");
        assert_eq!(span(1950, 0), "1950\u{2013}");
        assert_eq!(span(0, 1750), "\u{2013}1750");
        assert_eq!(span(0, 0), "");
    }
}
