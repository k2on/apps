//! An amount typed as arithmetic (`120+35.50`, `3 × 12.99`, `(80-12)/2`):
//! what a formula field takes (`ark_app::view::amount_entry`'s `.gives`),
//! on every platform with a keyboard to type one. Each figure in it is an
//! amount in the person's style ([`crate::parse_amount`]); `+` and `-` add
//! and take amounts, `*` (`×`) and `/` (`÷`) multiply and divide by a
//! plain number, in the usual order, with brackets. Worked in whole cents,
//! a product or a quotient rounded to the nearest cent, half away from
//! nothing.

use crate::numbers::{amount, parse_amount, NumberStyle};

/// What `typed` comes to in cents: an amount, or arithmetic of amounts;
/// none when it is neither (a figure missing, a bracket left open, a
/// division by nothing). Nothing typed is nothing, as for an amount.
pub fn parse_formula(typed: &str, s: &NumberStyle) -> Option<i64> {
    if typed.trim().is_empty() {
        return Some(0);
    }
    let (cents, _) = evaluate(typed, s)?;
    i64::try_from(cents).ok()
}

/// What a formula typed gives, as the field says it beside what is typed:
/// `= $155.50`. None while `typed` is a plain amount (there is nothing to
/// work out) or is no formula yet.
pub fn formula_gives(typed: &str, s: &NumberStyle) -> Option<String> {
    let (cents, worked) = evaluate(typed, s)?;
    let cents = i64::try_from(cents).ok().filter(|_| worked)?;
    Some(format!("= {}", amount(cents, s)))
}

/// The cents `typed` comes to, and whether any arithmetic was done.
fn evaluate(typed: &str, s: &NumberStyle) -> Option<(i128, bool)> {
    let tokens = tokens(typed)?;
    let mut p = Parser {
        tokens,
        at: 0,
        s,
        worked: false,
    };
    let v = p.sum()?;
    (p.at == p.tokens.len()).then_some((v, p.worked))
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Figure(String),
    Op(char),
}

/// `typed` split at its operators and brackets, each figure between them
/// as typed.
fn tokens(typed: &str) -> Option<Vec<Token>> {
    let mut out = vec![];
    let mut figure = String::new();
    for c in typed.chars() {
        let op = match c {
            '+' => Some('+'),
            '-' | '\u{2212}' => Some('-'),
            '*' | '×' => Some('*'),
            '/' | '÷' => Some('/'),
            '(' | ')' => Some(c),
            _ => None,
        };
        match op {
            Some(o) => {
                if !figure.trim().is_empty() {
                    out.push(Token::Figure(std::mem::take(&mut figure)));
                }
                figure.clear();
                out.push(Token::Op(o));
            }
            None => figure.push(c),
        }
    }
    if !figure.trim().is_empty() {
        out.push(Token::Figure(figure));
    }
    (!out.is_empty()).then_some(out)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    s: &'a NumberStyle,
    worked: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn op(&mut self, ops: &[char]) -> Option<char> {
        match self.peek() {
            Some(Token::Op(o)) if ops.contains(o) => {
                let o = *o;
                self.at += 1;
                Some(o)
            }
            _ => None,
        }
    }

    /// Terms added and taken away.
    fn sum(&mut self) -> Option<i128> {
        let mut v = self.product()?;
        while let Some(o) = self.op(&['+', '-']) {
            self.worked = true;
            let r = self.product()?;
            v = if o == '+' { v.checked_add(r)? } else { v.checked_sub(r)? };
        }
        Some(v)
    }

    /// Factors multiplied and divided: cents by a number, so the product
    /// of two figures is taken back down a hundredfold.
    fn product(&mut self) -> Option<i128> {
        let mut v = self.factor()?;
        while let Some(o) = self.op(&['*', '/']) {
            self.worked = true;
            let r = self.factor()?;
            v = match o {
                '*' => rounded(v.checked_mul(r)?, 100)?,
                _ => rounded(v.checked_mul(100)?, r)?,
            };
        }
        Some(v)
    }

    fn factor(&mut self) -> Option<i128> {
        if self.op(&['-']).is_some() {
            return self.factor()?.checked_neg();
        }
        if self.op(&['+']).is_some() {
            return self.factor();
        }
        if self.op(&['(']).is_some() {
            self.worked = true;
            let v = self.sum()?;
            self.op(&[')'])?;
            return Some(v);
        }
        match self.peek()? {
            Token::Figure(f) => {
                let cents = parse_amount(f.trim(), self.s)?;
                self.at += 1;
                Some(cents as i128)
            }
            Token::Op(_) => None,
        }
    }
}

/// `n / d`, to the nearest whole, half away from nothing; none by nothing.
fn rounded(n: i128, d: i128) -> Option<i128> {
    if d == 0 {
        return None;
    }
    let (q, r) = (n / d, n % d);
    Some(match (2 * r.abs() >= d.abs(), (n < 0) != (d < 0)) {
        (false, _) => q,
        (true, false) => q + 1,
        (true, true) => q - 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arithmetic of amounts in the usual order, each figure as the style
    /// writes it; a plain amount is as `parse_amount` reads it and says
    /// nothing worked out. Falsified by working left to right (`2+3*4`
    /// was 20.00), or by a product left a hundredfold too large.
    #[test]
    fn a_formula_is_worked_out_in_cents() {
        let us = NumberStyle::us();
        assert_eq!(parse_formula("120+35.50", &us), Some(15550));
        assert_eq!(parse_formula("$1,200 - 35.5", &us), Some(116450));
        assert_eq!(parse_formula("2+3*4", &us), Some(1400));
        assert_eq!(parse_formula("3 × 12.99", &us), Some(3897));
        assert_eq!(parse_formula("(80-12)/2", &us), Some(3400));
        assert_eq!(parse_formula("10/3", &us), Some(333));
        assert_eq!(parse_formula("-12.50", &us), Some(-1250));
        assert_eq!(parse_formula("", &us), Some(0));
        assert_eq!(formula_gives("120+35.50", &us).as_deref(), Some("= $155.50"));
        assert_eq!(formula_gives("35.50", &us), None);
        assert_eq!(formula_gives("-35.50", &us), None);
    }

    /// What is no formula yet is none: a figure missing, a bracket open,
    /// words, a division by nothing.
    #[test]
    fn what_is_not_yet_a_formula_is_none() {
        let us = NumberStyle::us();
        for typed in ["120+", "(1+2", "1+2)", "abc", "4/0", "1..2+3", "*3"] {
            assert_eq!(parse_formula(typed, &us), None, "{typed}");
            assert_eq!(formula_gives(typed, &us), None, "{typed}");
        }
    }

    /// In a style whose decimal mark is a comma, the figures are that
    /// style's. Falsified by splitting a figure at its decimal comma.
    #[test]
    fn the_figures_are_the_styles() {
        let mut eu = NumberStyle::us();
        eu.decimal = ",".into();
        eu.grouping = ".".into();
        assert_eq!(parse_formula("1.200,50 + 0,50", &eu), Some(120100));
    }
}
