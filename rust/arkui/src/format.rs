//! Numbers as people say them: a length, a span of years, a count of things.

/// Seconds as `m:ss`, which is how long one piece is written down, and as
/// `h:mm:ss` from an hour — an episode of two is `2:01:43`, not `121:43`.
/// Empty for something that is not a length yet (a stream that has not
/// reported one).
pub fn clock(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return String::new();
    }
    let secs = secs as u64;
    match secs / 3600 {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
    }
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

/// The day `ms` (since the epoch) fell on, as a list of episodes says it
/// beside each: `Today`, `Yesterday`, `Oct 21`, or `Oct 21, 2024` from
/// another year than `now`'s. Empty for 0, "nobody said". In UTC: a page
/// has no time zone it can rely on in every place it runs.
pub fn day(ms: i64, now: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    const DAY: i64 = 86_400_000;
    let (d, today) = (ms.div_euclid(DAY), now.div_euclid(DAY));
    if d == today {
        return "Today".into();
    }
    if d + 1 == today {
        return "Yesterday".into();
    }
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, dd) = civil(d);
    match y == civil(today).0 {
        true => format!("{} {dd}", MONTHS[m - 1]),
        false => format!("{} {dd}, {y}", MONTHS[m - 1]),
    }
}

/// The year, month (1–12) and day of the month `days` after 1970-01-01:
/// Howard Hinnant's `civil_from_days`.
fn civil(days: i64) -> (i64, usize, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as usize, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A day is said as a list of episodes says it. Falsified by an
    /// off-by-one in `civil`: the 15th was the 14th.
    #[test]
    fn a_day_is_said_the_way_a_list_of_episodes_says_it() {
        let now = 1_792_324_800_000; // 2026-10-18 12:00 UTC
        assert_eq!(day(now - 3_600_000, now), "Today");
        assert_eq!(day(now - 86_400_000, now), "Yesterday");
        assert_eq!(day(1_792_065_600_000, now), "Oct 15");
        assert_eq!(day(1_729_512_000_000, now), "Oct 21, 2024");
        assert_eq!(day(951_782_400_000, now), "Feb 29, 2000");
        assert_eq!(day(0, now), "");
    }

    /// The case each one exists for. Falsified by making `spell` print total
    /// minutes: Messiah comes out `150 min`.
    #[test]
    fn a_length_is_said_the_way_people_say_it() {
        assert_eq!(clock(261.9), "4:21");
        assert_eq!(clock(f64::NAN), "");
        assert_eq!(clock(3599.0), "59:59");
        assert_eq!(clock(7123.0), "1:58:43", "an episode of two hours");
        assert_eq!(clock(3600.0 * 10.0 + 5.0), "10:00:05");
        assert_eq!(spell(150 * 60_000 + 37_000), "2 hr 30 min");
        assert_eq!(spell(60 * 60_000), "1 hr");
        assert_eq!(spell(42 * 60_000), "42 min");
        assert_eq!(plural(1, "track"), "track");
        assert_eq!(plural(0, "track"), "tracks");
        assert_eq!(span(1685, 1750), "1685\u{2013}1750");
        assert_eq!(span(0, 0), "");
    }
}
