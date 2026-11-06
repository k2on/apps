//! How long something is: one piece as a row writes it down (`4:21`), a
//! whole record as people say it (`2 hr 30 min`).

/// Milliseconds as `m:ss`, which is how long one piece is written down, and
/// as `h:mm:ss` from an hour — an episode of two is `2:01:43`, not
/// `121:43`. Below nothing is `0:00`.
pub fn clock(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    match secs / 3600 {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
    }
}

/// [`clock`] of seconds as a player reports them, and nothing for what is
/// not a length yet (a stream that has not said one: not a number, or
/// below nothing).
pub fn clock_secs(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return String::new();
    }
    clock((secs * 1000.0) as i64)
}

/// How long a whole record runs, in the units people say it in, to the
/// nearest minute: `under a minute`, `48 min`, `2 hr`, `2 hr 30 min`.
///
/// [`clock`] is `m:ss` because that is how one piece is written down; a
/// two-and-a-half-hour oratorio in `m:ss` is `150:37`, which nobody reads as
/// a length. So a header says `2 hr 31 min` and a row still says `4:21`.
pub fn length(ms: i64) -> String {
    let minutes = ms.max(0).saturating_add(30_000) / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "under a minute".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Falsified by writing an hour as minutes: `121:43`.
    #[test]
    fn a_piece_is_written_down_by_the_clock() {
        assert_eq!(clock(261_900), "4:21");
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(-5_000), "0:00");
        assert_eq!(clock(5_000), "0:05");
        assert_eq!(clock(3_599_000), "59:59");
        assert_eq!(clock(3_600_000), "1:00:00");
        assert_eq!(clock(7_123_000), "1:58:43", "an episode of two hours");
        assert_eq!(clock(36_005_000), "10:00:05");
        assert_eq!(clock_secs(261.9), "4:21");
        assert_eq!(clock_secs(f64::NAN), "");
        assert_eq!(clock_secs(-1.0), "");
        assert_eq!(clock_secs(7123.0), "1:58:43");
    }

    /// The case each one exists for. Falsified by printing total minutes:
    /// Messiah comes out `151 min`; and by truncating: 59 min 50 s is `59 min`.
    #[test]
    fn a_length_is_said_the_way_people_say_it() {
        assert_eq!(length(150 * 60_000 + 37_000), "2 hr 31 min");
        assert_eq!(length(150 * 60_000 + 29_000), "2 hr 30 min");
        assert_eq!(length(60 * 60_000), "1 hr");
        assert_eq!(length(59 * 60_000 + 50_000), "1 hr");
        assert_eq!(length(42 * 60_000), "42 min");
        assert_eq!(length(48 * 60_000 + 10_000), "48 min");
        assert_eq!(length(29_000), "under a minute");
        assert_eq!(length(30_000), "1 min");
        assert_eq!(length(0), "under a minute");
        assert_eq!(length(-1), "under a minute");
        assert_eq!(length(i64::MAX), "2562047788015 hr 12 min", "no overflow");
    }
}
