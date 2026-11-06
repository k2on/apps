//! Amounts, shares and quantities.
//!
//! An amount is whole cents (hundredths of the currency) in an `i64`, as
//! every Ark app keeps money, and it is written the way the place the
//! person is in writes money: its symbol before or after the figure, its
//! decimal mark, its grouping, where its minus goes. That is a
//! [`NumberStyle`], which each platform reads once from its own formatter
//! (`dev.arkui.LocaleNumbers` on Android, ArkUI's `LocaleNumbers` on the
//! iPhone) and the desktop and the web take as [`NumberStyle::us`]. Every
//! group is three digits: a lakh is written as a thousand hundreds.

/// How a place writes money, read once from the platform's formatter.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct NumberStyle {
    /// The currency's ISO code, `USD`: what a field also accepts typed.
    pub currency: String,
    /// What is written beside the figure: `$`, `€`, `US$`.
    pub symbol: String,
    /// The symbol before the figure (`$1.00`) or after it (`1,00 €`).
    pub symbol_first: bool,
    /// A (no-break) space between the symbol and the figure: `1,00 €`.
    pub symbol_spaced: bool,
    /// The decimal mark: `.` or `,`.
    pub decimal: String,
    /// What groups thousands: `,`, `.`, a no-break space, or nothing.
    pub grouping: String,
    /// The minus sign: `-`, or `−` where the place writes that.
    pub minus: String,
    /// With the symbol first, whether the minus goes before it (`-$12.00`)
    /// or between it and the figure (`$-12.00`). With the symbol after the
    /// figure the minus is always the figure's own (`-12,00 €`).
    pub minus_first: bool,
}

impl NumberStyle {
    /// The United States' dollar, as the desktop and the web write money:
    /// `$1,234.56`, `-$12.00`.
    pub fn us() -> Self {
        NumberStyle {
            currency: "USD".into(),
            symbol: "$".into(),
            symbol_first: true,
            symbol_spaced: false,
            decimal: ".".into(),
            grouping: ",".into(),
            minus: "-".into(),
            minus_first: true,
        }
    }
}

impl Default for NumberStyle {
    fn default() -> Self {
        Self::us()
    }
}

/// The space between a symbol and its figure, which never breaks a line.
const SPACE: &str = "\u{a0}";

/// `n` with its thousands grouped by `by`.
fn grouped(n: u64, by: &str) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * by.len());
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(by);
        }
        out.push(d);
    }
    out
}

/// A figure without its sign, its symbol put where the style puts it, and
/// the minus where it goes for one below nothing.
fn written(figure: &str, negative: bool, s: &NumberStyle) -> String {
    let gap = if s.symbol_spaced { SPACE } else { "" };
    let minus = if negative { s.minus.as_str() } else { "" };
    match (s.symbol_first, s.minus_first) {
        (true, true) => format!("{minus}{}{gap}{figure}", s.symbol),
        (true, false) => format!("{}{gap}{minus}{figure}", s.symbol),
        (false, _) => format!("{minus}{figure}{gap}{}", s.symbol),
    }
}

/// Cents as money: `$1,234.56`, `-$12.00`, `1.234,56 €`.
pub fn amount(cents: i64, s: &NumberStyle) -> String {
    let c = cents.unsigned_abs();
    let figure = format!("{}{}{:02}", grouped(c / 100, &s.grouping), s.decimal, c % 100);
    written(&figure, cents < 0, s)
}

/// [`amount`] with a plus on what is above nothing, as money that came in
/// reads: `+$25.00`. Nothing is `$0.00`, with no sign.
pub fn amount_signed(cents: i64, s: &NumberStyle) -> String {
    match cents > 0 {
        true => format!("+{}", amount(cents, s)),
        false => amount(cents, s),
    }
}

/// [`amount`], or nothing for nothing: a column of mostly empty cells reads
/// faster than a column of `$0.00`.
pub fn amount_or_blank(cents: i64, s: &NumberStyle) -> String {
    match cents {
        0 => String::new(),
        c => amount(c, s),
    }
}

/// Cents to the nearest whole unit, as a chart's axis says them: `$1,235`.
/// A half rounds away from nothing.
pub fn amount_whole(cents: i64, s: &NumberStyle) -> String {
    let c = cents.unsigned_abs();
    let whole = c / 100 + u64::from(c % 100 >= 50);
    written(&grouped(whole, &s.grouping), cents < 0 && whole > 0, s)
}

/// Cents as an amount field shows them for editing: `1234.50`, or
/// `1234,50` where the comma is the decimal mark; no grouping, no symbol,
/// a plain `-`; nothing for nothing. [`parse_amount`] reads it back.
pub fn amount_editable(cents: i64, s: &NumberStyle) -> String {
    match cents {
        0 => String::new(),
        c => {
            let m = c.unsigned_abs();
            format!("{}{}{}{:02}", if c < 0 { "-" } else { "" }, m / 100, s.decimal, m % 100)
        }
    }
}

/// What somebody typed as an amount, in cents: `12`, `12.5`, `$1,200.00`,
/// `-3.10` — or `12,50` and `1.234,56 €` where the comma is the decimal
/// mark. Nothing typed is nothing (0); anything else that is not an amount
/// (`ten`, `1.234` with three places, `1.2.3`) is `None`.
pub fn parse_amount(typed: &str, s: &NumberStyle) -> Option<i64> {
    // The mark that is not the decimal one is only ever grouping.
    let other = if s.decimal == "," { '.' } else { ',' };
    let mut t = typed.to_string();
    for word in [s.symbol.as_str(), s.currency.as_str()] {
        if !word.is_empty() {
            t = t.replace(word, "");
        }
    }
    if !s.grouping.is_empty() && s.grouping != s.decimal && !s.grouping.chars().all(char::is_whitespace) {
        t = t.replace(&s.grouping, "");
    }
    if !s.minus.is_empty() && s.minus != "-" {
        t = t.replace(&s.minus, "-");
    }
    let t: String = t
        .chars()
        .filter(|&c| !(c.is_whitespace() || c == other || matches!(c, '$' | '€' | '£')))
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .collect();
    let t = if s.decimal != "." { t.replace(&s.decimal, ".") } else { t };
    if t.is_empty() {
        return Some(0);
    }
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
    if whole.is_empty() && frac.is_empty() || frac.len() > 2 || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let frac: i64 = format!("{frac:0<2}").parse().ok()?;
    let cents = whole.checked_mul(100)?.checked_add(frac)?;
    Some(if negative { -cents } else { cents })
}

/// A part's share of the whole as a whole percent, a half rounded away
/// from nothing: `12%`. Nothing for a whole of nothing.
pub fn percent(part: i64, whole: i64) -> String {
    if whole <= 0 {
        return String::new();
    }
    let (p, w) = (i128::from(part), i128::from(whole));
    let n = (p.abs() * 200 + w) / (2 * w);
    format!("{}{n}%", if p < 0 && n > 0 { "-" } else { "" })
}

/// How many, kept in thousandths, as a receipt's line says it: `2`,
/// `11.482`, `2.5`. A point whatever the place, as the receipt printed it.
pub fn quantity(thousandths: i64) -> String {
    let sign = if thousandths < 0 { "-" } else { "" };
    let q = thousandths.unsigned_abs();
    match q % 1000 {
        0 => format!("{sign}{}", q / 1000),
        f => format!("{sign}{}.{}", q / 1000, format!("{f:03}").trim_end_matches('0')),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Germany's euro, as java.text and NumberFormatter write it.
    fn de() -> NumberStyle {
        NumberStyle {
            currency: "EUR".into(),
            symbol: "€".into(),
            symbol_first: false,
            symbol_spaced: true,
            decimal: ",".into(),
            grouping: ".".into(),
            minus: "-".into(),
            minus_first: true,
        }
    }

    /// Falsified by grouping from the left: `$1,234.56` comes out `$123,456`.
    #[test]
    fn amounts_are_written_as_the_place_writes_money() {
        let us = NumberStyle::us();
        assert_eq!(amount(123_456, &us), "$1,234.56");
        assert_eq!(amount(-1_200, &us), "-$12.00");
        assert_eq!(amount(5, &us), "$0.05");
        assert_eq!(amount(0, &us), "$0.00");
        assert_eq!(amount(100_000_000, &us), "$1,000,000.00");
        assert_eq!(amount(i64::MIN, &us), "-$92,233,720,368,547,758.08");
        assert_eq!(amount_signed(2_500, &us), "+$25.00");
        assert_eq!(amount_signed(0, &us), "$0.00");
        assert_eq!(amount_signed(-2_500, &us), "-$25.00");
        assert_eq!(amount_or_blank(0, &us), "");
        assert_eq!(amount_or_blank(1, &us), "$0.01");
        // The euro, with the German grouping and decimal mark, after the figure.
        assert_eq!(amount(123_456, &de()), "1.234,56\u{a0}€");
        assert_eq!(amount(-1_200, &de()), "-12,00\u{a0}€");
        assert_eq!(amount_signed(2_500, &de()), "+25,00\u{a0}€");
        let between = NumberStyle {
            minus_first: false,
            ..NumberStyle::us()
        };
        assert_eq!(amount(-1_200, &between), "$-12.00");
        let swiss = NumberStyle {
            symbol: "CHF".into(),
            symbol_spaced: true,
            grouping: "\u{2019}".into(),
            ..NumberStyle::us()
        };
        assert_eq!(amount(12_345_678, &swiss), "CHF\u{a0}123\u{2019}456.78");
        let none = NumberStyle {
            grouping: String::new(),
            ..NumberStyle::us()
        };
        assert_eq!(amount(123_456, &none), "$1234.56");
    }

    /// Falsified by truncating: `$1,234.50` comes out `$1,234`.
    #[test]
    fn a_chart_says_whole_units() {
        let us = NumberStyle::us();
        assert_eq!(amount_whole(123_450, &us), "$1,235");
        assert_eq!(amount_whole(123_449, &us), "$1,234");
        assert_eq!(amount_whole(-150, &us), "-$2");
        assert_eq!(amount_whole(-49, &us), "$0");
        assert_eq!(amount_whole(0, &us), "$0");
        assert_eq!(amount_whole(250_000, &de()), "2.500\u{a0}€");
    }

    /// Falsified by reading `,` as grouping in Germany: `12,50` is 1250 €.
    #[test]
    fn what_is_typed_is_read_in_the_place() {
        let us = NumberStyle::us();
        assert_eq!(parse_amount("12", &us), Some(1_200));
        assert_eq!(parse_amount("12.5", &us), Some(1_250));
        assert_eq!(parse_amount("$1,200.00", &us), Some(120_000));
        assert_eq!(parse_amount("-3.1", &us), Some(-310));
        assert_eq!(parse_amount("-3.10", &us), Some(-310));
        assert_eq!(parse_amount("+3", &us), Some(300));
        assert_eq!(parse_amount("\u{2212}3", &us), Some(-300));
        assert_eq!(parse_amount(".75", &us), Some(75));
        assert_eq!(parse_amount("  ", &us), Some(0));
        assert_eq!(parse_amount("", &us), Some(0));
        assert_eq!(parse_amount("USD 4", &us), Some(400));
        assert_eq!(parse_amount("1.234", &us), None);
        assert_eq!(parse_amount("12.345", &us), None);
        assert_eq!(parse_amount("1.2.3", &us), None);
        assert_eq!(parse_amount("ten", &us), None);
        assert_eq!(parse_amount("twelve", &us), None);
        assert_eq!(parse_amount(".", &us), None);
        assert_eq!(parse_amount("99999999999999999999", &us), None);
        assert_eq!(parse_amount("12,50", &de()), Some(1_250));
        assert_eq!(parse_amount("1.234,56", &de()), Some(123_456));
        assert_eq!(parse_amount("1.234,56\u{a0}€", &de()), Some(123_456));
        assert_eq!(parse_amount("-12,00 EUR", &de()), Some(-1_200));
    }

    /// Falsified by writing grouping into a field: `1,234.50` is not read
    /// back the same in every place.
    #[test]
    fn an_amount_is_edited_as_it_is_typed() {
        let us = NumberStyle::us();
        assert_eq!(amount_editable(123_450, &us), "1234.50");
        assert_eq!(amount_editable(-5, &us), "-0.05");
        assert_eq!(amount_editable(0, &us), "");
        assert_eq!(amount_editable(1_250, &de()), "12,50");
        for c in [1, -1, 1_250, 123_456_789, -99] {
            assert_eq!(parse_amount(&amount_editable(c, &us), &us), Some(c));
            assert_eq!(parse_amount(&amount_editable(c, &de()), &de()), Some(c));
            assert_eq!(parse_amount(&amount(c, &us), &us), Some(c));
            assert_eq!(parse_amount(&amount(c, &de()), &de()), Some(c));
        }
    }

    /// Falsified by truncating: a sixth is `16%`.
    #[test]
    fn a_share_is_a_whole_percent() {
        assert_eq!(percent(1, 6), "17%");
        assert_eq!(percent(1, 8), "13%", "a half rounds up");
        assert_eq!(percent(1, 3), "33%");
        assert_eq!(percent(3, 2), "150%");
        assert_eq!(percent(0, 5), "0%");
        assert_eq!(percent(5, 5), "100%");
        assert_eq!(percent(-1, 8), "-13%");
        assert_eq!(percent(1, 1000), "0%");
        assert_eq!(percent(1, 0), "");
        assert_eq!(percent(i64::MAX, i64::MAX), "100%");
    }

    /// Falsified by the phone's `%.3f`, which wrote `2,5` in Germany.
    #[test]
    fn a_quantity_is_written_as_the_receipt_printed_it() {
        assert_eq!(quantity(2_000), "2");
        assert_eq!(quantity(11_482), "11.482");
        assert_eq!(quantity(2_500), "2.5");
        assert_eq!(quantity(10), "0.01");
        assert_eq!(quantity(0), "0");
        assert_eq!(quantity(-1_500), "-1.5");
    }
}
