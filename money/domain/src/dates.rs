//! Days and months as text: `YYYY-MM-DD` and `YYYY-MM`, which sort as time
//! does. A function reads no clock, so "today" is always `ctx.now` said as a
//! day ([`day_of_ms`]), in UTC, the same on every peer.

/// Days since 1970-01-01 of a civil date (Howard Hinnant's
/// `days_from_civil`).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `days` after 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

/// A day as text.
pub fn day(y: i64, m: i64, d: i64) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// The day a moment falls on, in UTC.
pub fn day_of_ms(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(86_400_000));
    day(y, m, d)
}

/// A day's parts, when it is one.
pub fn parse_day(s: &str) -> Option<(i64, i64, i64)> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit()))?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let ok = (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m);
    ok.then_some((y, m, d))
}

/// Whether `s` is a day.
pub fn is_day(s: &str) -> bool {
    parse_day(s).is_some()
}

/// A month's parts, when it is one.
pub fn parse_month(s: &str) -> Option<(i64, i64)> {
    let b = s.as_bytes();
    if b.len() != 7 || b[4] != b'-' {
        return None;
    }
    let y = s.get(0..4).filter(|p| p.bytes().all(|c| c.is_ascii_digit()))?.parse::<i64>().ok()?;
    let m = s.get(5..7).filter(|p| p.bytes().all(|c| c.is_ascii_digit()))?.parse::<i64>().ok()?;
    (1..=12).contains(&m).then_some((y, m))
}

/// Whether `s` is a month.
pub fn is_month(s: &str) -> bool {
    parse_month(s).is_some()
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// The month a day is in: its first seven characters.
pub fn month_of(day: &str) -> String {
    day.get(..7).unwrap_or_default().to_string()
}

/// The month `n` months after `month` (before, for a negative `n`).
pub fn add_months(month: &str, n: i64) -> String {
    let (y, m) = parse_month(month).unwrap_or((1970, 1));
    let k = y * 12 + (m - 1) + n;
    format!("{:04}-{:02}", k.div_euclid(12), k.rem_euclid(12) + 1)
}

/// The day `n` days after `day`.
pub fn add_days(day_: &str, n: i64) -> String {
    let (y, m, d) = parse_day(day_).unwrap_or((1970, 1, 1));
    let (y, m, d) = civil_from_days(days_from_civil(y, m, d) + n);
    day(y, m, d)
}

/// Days from `a` to `b`.
pub fn days_between(a: &str, b: &str) -> i64 {
    let at = |s: &str| parse_day(s).map(|(y, m, d)| days_from_civil(y, m, d)).unwrap_or(0);
    at(b) - at(a)
}

/// Where a month's days start, as a key: before its first.
pub fn month_start(month: &str) -> String {
    format!("{month}-")
}

/// Where a month's days end, as a key: after its last.
pub fn month_end(month: &str) -> String {
    format!("{month}-~")
}

/// `October 2026`.
pub fn month_name(month: &str) -> String {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    match parse_month(month) {
        Some((y, m)) => format!("{} {y}", NAMES[(m - 1) as usize]),
        None => month.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_months_count_as_a_calendar_does() {
        assert_eq!(day_of_ms(0), "1970-01-01");
        assert_eq!(day_of_ms(1_700_000_000_000), "2023-11-14");
        assert_eq!(add_months("2026-11", 2), "2027-01");
        assert_eq!(add_months("2026-01", -1), "2025-12");
        assert_eq!(add_days("2024-02-28", 1), "2024-02-29");
        assert_eq!(add_days("2023-02-28", 1), "2023-03-01");
        assert_eq!(days_between("2026-10-30", "2026-11-02"), 3);
        assert!(parse_day("2026-02-30").is_none());
        assert!(parse_day("2026-1-01").is_none());
        assert!(parse_month("2026-13").is_none());
        assert_eq!(month_name("2026-10"), "October 2026");
        assert!(month_start("2026-10").as_str() < "2026-10-01" && "2026-10-31" < month_end("2026-10").as_str());
    }
}
