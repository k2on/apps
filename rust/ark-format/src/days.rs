//! Days and times as the apps say them, in English as every other word of
//! the apps is: how long ago something was, a register's day heading, a
//! month, the day an episode came out.
//!
//! A day here is the budget's `YYYY-MM-DD` and a month its `YYYY-MM`; an
//! instant is milliseconds since the epoch. Which day an instant falls on
//! depends on where the person is, so that is an argument too: the
//! platform's UTC offset, in minutes east of Greenwich, never read here.

const DAY_MS: i64 = 86_400_000;

const MONTHS: [&str; 12] = [
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

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// A month's name as a short date writes it: `Oct`, and `Sept` not.
fn short(m: usize) -> &'static str {
    &MONTHS[m - 1][..3]
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

/// The days from 1970-01-01 to a day of the calendar: Hinnant's
/// `days_from_civil`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `2026-10-26` as days since the epoch; nothing for what is not a day of
/// the calendar.
fn parse_day(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).filter(|p| p.bytes().all(|c| c.is_ascii_digit()))?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    if !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    (civil(days) == (y, m as usize, d)).then_some(days)
}

/// `2026-10` as its year and month.
fn parse_month(s: &str) -> Option<(i64, usize)> {
    let (y, m) = s.split_once('-')?;
    if y.len() != 4 || m.len() != 2 || !y.bytes().chain(m.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let m: usize = m.parse().ok()?;
    (1..=12).contains(&m).then_some((y.parse().ok()?, m))
}

fn weekday(days: i64) -> &'static str {
    // 1970-01-01 was a Thursday.
    WEEKDAYS[(days + 4).rem_euclid(7) as usize]
}

/// How long ago `ms` was at `now_ms`, as the middle of a sentence says it —
/// "Synced 5 minutes ago": `just now`, `1 minute ago`, `3 hours ago`,
/// `yesterday` (a day to two ago), `4 days ago`; `never` for 0. A time
/// after `now_ms` (another device's clock ahead) is `just now`.
pub fn ago(ms: i64, now_ms: i64) -> String {
    if ms <= 0 {
        return "never".into();
    }
    let s = now_ms.saturating_sub(ms).max(0) / 1000;
    let n = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..60 => "just now".into(),
        60..3_600 => n(s / 60, "minute"),
        3_600..86_400 => n(s / 3_600, "hour"),
        86_400..172_800 => "yesterday".into(),
        _ => n(s / 86_400, "day"),
    }
}

/// [`ago`] on its own, as a value beside its label: `Never`, `Yesterday`,
/// `Just now`, `3 hours ago`.
pub fn ago_alone(ms: i64, now_ms: i64) -> String {
    let mut a = ago(ms, now_ms);
    if let Some(first) = a.get(0..1) {
        let up = first.to_ascii_uppercase();
        a.replace_range(0..1, &up);
    }
    a
}

/// A register's day heading, as Wallet and Mail head one: `Today`,
/// `Yesterday`, `Monday, Oct 26`, and with its year from another year than
/// `today`'s: `Monday, Oct 26, 2025`. What is not a day is said as it is.
pub fn day_heading(day: &str, today: &str) -> String {
    let Some(d) = parse_day(day) else { return day.to_string() };
    let now = parse_day(today);
    if now == Some(d) {
        return "Today".into();
    }
    if now == Some(d + 1) {
        return "Yesterday".into();
    }
    let (y, m, dd) = civil(d);
    match now.map(|n| civil(n).0) == Some(y) {
        true => format!("{}, {} {dd}", weekday(d), short(m)),
        false => format!("{}, {} {dd}, {y}", weekday(d), short(m)),
    }
}

/// A day in a form: `Oct 26, 2026`. What is not a day is said as it is.
pub fn day_long(day: &str) -> String {
    match parse_day(day) {
        Some(d) => {
            let (y, m, dd) = civil(d);
            format!("{} {dd}, {y}", short(m))
        }
        None => day.to_string(),
    }
}

/// A month with its year: `October 2026`. What is not a month is said as
/// it is.
pub fn month(month: &str) -> String {
    match parse_month(month) {
        Some((y, m)) => format!("{} {y}", MONTHS[m - 1]),
        None => month.to_string(),
    }
}

/// A month alone: `October`.
pub fn month_alone(month: &str) -> String {
    match parse_month(month) {
        Some((_, m)) => MONTHS[m - 1].to_string(),
        None => month.to_string(),
    }
}

/// The day `ms` fell on where the person is (`offset_minutes` east of UTC,
/// the platform's at `now_ms`), as a list of episodes says it beside each:
/// `Today`, `Yesterday`, `Oct 21`, or `Oct 21, 2024` from another year than
/// `now_ms`'s. Nothing for 0, "nobody said".
pub fn published(ms: i64, now_ms: i64, offset_minutes: i32) -> String {
    if ms <= 0 {
        return String::new();
    }
    let shift = i64::from(offset_minutes) * 60_000;
    let (d, today) = ((ms + shift).div_euclid(DAY_MS), (now_ms + shift).div_euclid(DAY_MS));
    if d == today {
        return "Today".into();
    }
    if d + 1 == today {
        return "Yesterday".into();
    }
    let (y, m, dd) = civil(d);
    match y == civil(today).0 {
        true => format!("{} {dd}", short(m)),
        false => format!("{} {dd}, {y}", short(m)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;

    /// Falsified by counting hours past a day: two days ago is `48 hours ago`.
    #[test]
    fn how_long_ago_is_said_in_words() {
        let now = 1_792_324_800_000;
        assert_eq!(ago(0, now), "never");
        assert_eq!(ago(now - 30_000, now), "just now");
        assert_eq!(ago(now + 30_000, now), "just now", "a clock ahead");
        assert_eq!(ago(now - MIN, now), "1 minute ago");
        assert_eq!(ago(now - 5 * MIN, now), "5 minutes ago");
        assert_eq!(ago(now - HOUR, now), "1 hour ago");
        assert_eq!(ago(now - 3 * HOUR, now), "3 hours ago");
        assert_eq!(ago(now - 23 * HOUR, now), "23 hours ago");
        assert_eq!(ago(now - 25 * HOUR, now), "yesterday");
        assert_eq!(ago(now - 2 * DAY_MS, now), "2 days ago");
        assert_eq!(ago(now - 3 * DAY_MS, now), "3 days ago");
        assert_eq!(ago_alone(0, now), "Never");
        assert_eq!(ago_alone(now - 25 * HOUR, now), "Yesterday");
        assert_eq!(ago_alone(now - 3 * HOUR, now), "3 hours ago");
    }

    /// Falsified by an off-by-one in the weekday: the 26th of October 2026
    /// was a Monday.
    #[test]
    fn a_day_is_headed_as_wallet_heads_one() {
        assert_eq!(day_heading("2026-10-26", "2026-10-26"), "Today");
        assert_eq!(day_heading("2026-10-25", "2026-10-26"), "Yesterday");
        assert_eq!(day_heading("2026-10-26", "2026-10-30"), "Monday, Oct 26");
        assert_eq!(day_heading("2026-11-01", "2026-10-30"), "Sunday, Nov 1", "a day ahead");
        assert_eq!(day_heading("2025-12-31", "2026-01-01"), "Yesterday", "across a year");
        assert_eq!(day_heading("2025-12-30", "2026-01-01"), "Tuesday, Dec 30, 2025");
        assert_eq!(day_heading("2024-02-29", "2026-01-01"), "Thursday, Feb 29, 2024");
        assert_eq!(day_heading("someday", "2026-01-01"), "someday");
        assert_eq!(day_heading("2026-02-30", "2026-01-01"), "2026-02-30");
        assert_eq!(day_heading("2026-10-26", ""), "Monday, Oct 26, 2026");
    }

    #[test]
    fn days_and_months_are_named() {
        assert_eq!(day_long("2026-10-26"), "Oct 26, 2026");
        assert_eq!(day_long("2026-1-26"), "2026-1-26");
        assert_eq!(month("2026-10"), "October 2026");
        assert_eq!(month("2026-01"), "January 2026");
        assert_eq!(month("2026-13"), "2026-13");
        assert_eq!(month_alone("2026-09"), "September");
        assert_eq!(month_alone("Sept"), "Sept");
    }

    /// A day is said as a list of episodes says it. Falsified by an
    /// off-by-one in `civil`: the 15th was the 14th; and by taking the day
    /// in UTC: an episode out at 11 pm in New York came out "Tomorrow".
    #[test]
    fn a_day_is_said_the_way_a_list_of_episodes_says_it() {
        let now = 1_792_324_800_000; // 2026-10-18 12:00 UTC
        assert_eq!(published(now - 3_600_000, now, 0), "Today");
        assert_eq!(published(now - 86_400_000, now, 0), "Yesterday");
        assert_eq!(published(1_792_065_600_000, now, 0), "Oct 15");
        assert_eq!(published(1_729_512_000_000, now, 0), "Oct 21, 2024");
        assert_eq!(published(951_782_400_000, now, 0), "Feb 29, 2000");
        assert_eq!(published(0, now, 0), "");
        // 2026-10-18 03:30 UTC is still the 17th in New York (UTC-4), at
        // noon there on the 18th.
        let ny = -4 * 60;
        let noon_ny = now + 4 * HOUR;
        assert_eq!(published(now - 8 * HOUR - 30 * MIN, noon_ny, ny), "Yesterday");
        assert_eq!(published(now - 8 * HOUR - 30 * MIN, noon_ny, 0), "Today");
        // And Tokyo (UTC+9) is on the 19th by 15:00 UTC on the 18th.
        assert_eq!(published(now + 3 * HOUR, now + 3 * HOUR, 9 * 60), "Today");
        assert_eq!(published(now - 3 * HOUR, now + 3 * HOUR, 9 * 60), "Yesterday");
    }
}
