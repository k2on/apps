//! Cents as people write them, and back.

/// `$1,234.56`, `-$12.00`.
pub fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let c = cents.unsigned_abs();
    let dollars = (c / 100).to_string();
    let mut grouped = String::new();
    for (i, d) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(d);
    }
    format!("{sign}${grouped}.{:02}", c % 100)
}

/// [`money`], or nothing for nothing: a column of mostly empty cells reads
/// faster than a column of `$0.00`.
pub fn money_or_blank(cents: i64) -> String {
    match cents {
        0 => String::new(),
        c => money(c),
    }
}

/// What somebody typed as an amount, in cents: `12`, `12.5`, `$1,200.00`,
/// `-3.10`. Nothing typed is nothing; anything else is not an amount.
pub fn parse(typed: &str) -> Option<i64> {
    let t: String = typed.chars().filter(|c| !matches!(c, '$' | ',' | ' ')).collect();
    if t.is_empty() {
        return Some(0);
    }
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, t.trim_start_matches('+').to_string()),
    };
    let (whole, frac) = t.split_once('.').unwrap_or((&t, ""));
    if whole.is_empty() && frac.is_empty() || frac.len() > 2 || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let frac: i64 = format!("{frac:0<2}").parse().ok()?;
    let cents = whole.checked_mul(100)?.checked_add(frac)?;
    Some(if negative { -cents } else { cents })
}

/// Cents as an amount field shows them for editing: `1234.5` is `1234.50`,
/// nothing is empty.
pub fn editable(cents: i64) -> String {
    match cents {
        0 => String::new(),
        c => format!("{}{}.{:02}", if c < 0 { "-" } else { "" }, c.abs() / 100, c.abs() % 100),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_read_and_write_the_way_people_do() {
        assert_eq!(money(123_456), "$1,234.56");
        assert_eq!(money(-1_200), "-$12.00");
        assert_eq!(money(5), "$0.05");
        assert_eq!(money(100_000_000), "$1,000,000.00");
        assert_eq!(parse("12"), Some(1_200));
        assert_eq!(parse("12.5"), Some(1_250));
        assert_eq!(parse("$1,200.00"), Some(120_000));
        assert_eq!(parse("-3.1"), Some(-310));
        assert_eq!(parse(".75"), Some(75));
        assert_eq!(parse(""), Some(0));
        assert_eq!(parse("1.234"), None);
        assert_eq!(parse("ten"), None);
        assert_eq!(editable(123_450), "1234.50");
        assert_eq!(money_or_blank(0), "");
    }
}
