//! Assignment by formula: a category given, each month, what a formula
//! over that month's transactions comes to — "10% × SUM(ACME paychecks)"
//! — rather than an amount somebody typed, the way a spreadsheet's cell is.
//!
//! - **What a formula is.** A sum of terms, each `factor × SUM(filter)`:
//!   a saved filter ([`crate::filters`]), referred to by its id, over the
//!   month's days, and a factor kept exactly as a ratio (`times / per`;
//!   typed as "10%", "0.1" or "1.5x", [`parse_share`]). The terms are
//!   added exactly and the month's total rounded once, to the cent, half
//!   away from zero ([`combine`]).
//! - **From a month on.** A formula is a version keyed by its category and
//!   the month it starts; a month follows the latest version starting at
//!   or before it. Changing one is a new version from the month it is
//!   done; removing one is a version that says `manual`. Every month
//!   before the first keeps what people assigned it, and a version set
//!   from a month replaces any that started after it.
//! - **A formula's amount is assigned as any other.** Ready to assign, the
//!   month's totals, overspending, what carries, what a card's payment
//!   category is moved — every view reads [`assignments`], so the amount
//!   counts exactly where a typed one would. While a month follows a
//!   formula nobody assigns it by hand: assigning and moving money to or
//!   from it refuse ([`refuse`]), and the month's quick buttons pass it by.
//! - **Not every category.** The budget's own (Ready to Assign) and a
//!   card's payment category — whose money is the card's — have none.
//! - **Its filters stay.** A saved filter a formula sums is not deleted
//!   (the deletion is refused); were one gone anyway, its term would count
//!   nothing. Changing a filter's conditions moves every month the formula
//!   covers, as editing a range moves a spreadsheet's sum.
//!
//! This replaced assignment by rule, whose `Rule` rows are no longer read.
//! What the phones set through the old `categories.set_rule` is kept as a
//! formula of one term ([`set_rule`]).
//!
//! As everywhere in the budget, nothing worked out is stored: a paycheck
//! that arrives late, or is renamed, moves the month it is in.

use std::collections::BTreeMap;

use ark::prelude::*;

use crate::budget::{budgetable, Plan};
use crate::dates::{add_months, day_of_ms, first_day, last_day, month_of};
use crate::schema::*;
use crate::types::*;

/// What a month following a formula says to a hand that would assign it.
/// ("Its rule" is the word the phones were built with.)
pub const FOLLOWS: &str = "this category's assignment follows its rule, a formula";

/// The most terms a formula has.
pub const TERMS: usize = 12;

/// The version of a category's formula a month follows, if one does: the
/// latest starting at or before it, unless that one says `manual`.
pub fn formula_in(db: &Db<Money>, id: Id<Category>, month: &str) -> Option<Formula> {
    db.formula
        .by(Formula::CATEGORY_ID_FROM)
        .between((id, String::new()), (id, format!("{month}~")))
        .last()
        .filter(|f| !f.manual)
}

/// Refused, when the month follows a formula: what assigning by hand checks.
pub fn refuse(db: &Db<Money>, id: Id<Category>, month: &str) -> Result<(), Refusal> {
    match formula_in(db, id, month) {
        Some(_) => Err(Refusal::new(FOLLOWS)),
        None => Ok(()),
    }
}

// * The arithmetic

fn gcd(a: i128, b: i128) -> i128 {
    match b {
        0 => a.abs().max(1),
        _ => gcd(b, a % b),
    }
}

/// `n / d`, to the nearest whole, half away from zero.
fn round(n: i128, d: i128) -> i128 {
    if d == 0 {
        return 0;
    }
    let (q, r) = (n / d, n % d);
    let up = 2 * r.abs() >= d.abs();
    let sign = if (n < 0) != (d < 0) { -1 } else { 1 };
    q + if up { sign } else { 0 }
}

fn clamp(n: i128) -> i64 {
    n.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// `amount × times / per`, to the cent, half away from zero.
pub fn share(amount: i64, times: i64, per: i64) -> i64 {
    combine(&[(amount, times, per)])
}

/// `Σ amount × times / per`, added exactly and rounded once, to the cent,
/// half away from zero: what a month's terms come to.
pub fn combine(terms: &[(i64, i64, i64)]) -> i64 {
    // The sum kept as a fraction n / d in lowest terms.
    let (mut n, mut d): (i128, i128) = (0, 1);
    for &(amount, times, per) in terms {
        if per <= 0 {
            continue;
        }
        let (tn, td) = (amount as i128 * times as i128, per as i128);
        let g = gcd(d, td);
        let Some(lcm) = (d / g).checked_mul(td) else {
            // Beyond what fractions this size hold: the nearest cent of
            // each, added.
            n = round(n, d) + round(tn, td);
            d = 1;
            continue;
        };
        let (a, b) = ((lcm / d).checked_mul(n), (lcm / td).checked_mul(tn));
        match (a, b) {
            (Some(a), Some(b)) => {
                let sum = a.saturating_add(b);
                let g = gcd(sum, lcm);
                (n, d) = (sum / g, lcm / g);
            }
            _ => {
                n = round(n, d).saturating_add(round(tn, td));
                d = 1;
            }
        }
    }
    clamp(round(n, d))
}

/// A share as a percentage, as exactly as four places say it: "10%",
/// "150%", "12.5%".
pub fn percent(times: i64, per: i64) -> String {
    if per <= 0 {
        return "?%".into();
    }
    // Ten-thousandths of a percent.
    let n = share(times, 100 * 10_000, per);
    let (whole, part) = (n / 10_000, n % 10_000);
    match part {
        0 => format!("{whole}%"),
        _ => format!("{whole}.{}%", format!("{part:04}").trim_end_matches('0')),
    }
}

/// A share as somebody types it: "10%", "0.1", "1.5", "1.5x", "150 %" —
/// a percentage when it ends in `%`, else a multiplier — as `times / per`,
/// exactly. Nothing for what is not one, or is below nothing.
pub fn parse_share(s: &str) -> Option<(i64, i64)> {
    let s = s.trim();
    let (num, pct) = match s.strip_suffix('%') {
        Some(n) => (n.trim(), true),
        None => (s.strip_suffix(['x', 'X', '\u{d7}']).unwrap_or(s).trim(), false),
    };
    if num.is_empty() || num.starts_with('-') {
        return None;
    }
    let (whole, frac) = num.split_once('.').unwrap_or((num, ""));
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) || (whole.is_empty() && frac.is_empty()) {
        return None;
    }
    let frac = frac.trim_end_matches('0');
    if frac.len() > 6 || whole.len() > 9 {
        return None;
    }
    let mut per: i64 = 10i64.pow(frac.len() as u32);
    let times: i64 = format!("{whole}{frac}").parse().ok()?;
    if pct {
        per *= 100;
    }
    let g = gcd(times as i128, per as i128) as i64;
    Some((times / g, per / g))
}

// * What a formula reads

/// What one term's filter matched, month by month from `lo` through `hi`
/// (months; none for no end): how many, and their sum. A filter that is
/// gone matches nothing.
pub fn by_month(db: &Db<Money>, filter: Id<SavedFilter>, lo: &str, hi: Option<&str>) -> BTreeMap<String, (i64, i64)> {
    let mut out: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let Some(saved) = db.saved_filter.get(filter) else { return out };
    let mut f = saved.filter;
    let lo = first_day(lo);
    f.from = Some(f.from.map_or(lo.clone(), |d| d.max(lo)));
    if let Some(hi) = hi {
        let hi = last_day(hi);
        f.through = Some(f.through.map_or(hi.clone(), |d| d.min(hi)));
    }
    for t in f.txns(db) {
        let m = out.entry(month_of(&t.date)).or_default();
        (m.0, m.1) = (m.0 + 1, m.1 + t.amount);
    }
    out
}

/// Each term in one month — its filter's name, what it matched — and
/// what they come to together.
pub fn in_month(db: &Db<Money>, terms: &[Term], month: &str) -> (Vec<TermLine>, i64) {
    let lines: Vec<TermLine> = terms
        .iter()
        .map(|t| {
            let (count, sum) = by_month(db, t.filter_id, month, Some(month)).get(month).copied().unwrap_or_default();
            TermLine {
                filter_id: t.filter_id,
                name: db.saved_filter.get(t.filter_id).map(|f| f.name).unwrap_or_default(),
                times: t.times,
                per: t.per,
                count,
                sum,
            }
        })
        .collect();
    let amount = combine(&lines.iter().map(|l| (l.sum, l.times, l.per)).collect::<Vec<_>>());
    (lines, amount)
}

/// Terms said as a spreadsheet would: "10% × SUM(ACME paychecks) +
/// SUM(Side gigs)". A factor of one is left out.
pub fn text_of(db: &Db<Money>, terms: &[Term]) -> String {
    let said: Vec<String> = terms
        .iter()
        .map(|t| {
            let name = db.saved_filter.get(t.filter_id).map_or_else(|| "a filter gone".to_string(), |f| f.name);
            match t.times == t.per {
                true => format!("SUM({name})"),
                false => format!("{} \u{d7} SUM({name})", percent(t.times, t.per)),
            }
        })
        .collect();
    said.join(" + ")
}

/// A formula, said: as the phones set it, or by its terms.
pub fn said_of(db: &Db<Money>, f: &Formula) -> String {
    match f.said.is_empty() {
        true => text_of(db, &f.terms),
        false => f.said.clone(),
    }
}

/// What a category was assigned, month by month: what people gave each
/// month that follows no formula, and each month that does, what its
/// formula came to. Every view of the budget reads assignments from here.
pub fn assignments(db: &Db<Money>, id: Id<Category>) -> BTreeMap<String, i64> {
    let versions = db.formula.by(Formula::CATEGORY_ID_FROM).from(id).all();
    let mut out = BTreeMap::new();
    for a in db.assigned.by(Assigned::CATEGORY_ID_MONTH).from(id).all() {
        let follows = versions.iter().rev().find(|f| f.from <= a.month).is_some_and(|f| !f.manual);
        if !follows {
            *out.entry(a.month.clone()).or_default() += a.amount;
        }
    }
    for (i, f) in versions.iter().enumerate() {
        if f.manual {
            continue;
        }
        let until = versions.get(i + 1).map(|n| add_months(&n.from, -1));
        // Each month any term matched something in: what each term summed.
        let mut months: BTreeMap<String, Vec<(i64, i64, i64)>> = BTreeMap::new();
        for t in &f.terms {
            for (month, (_, sum)) in by_month(db, t.filter_id, &f.from, until.as_deref()) {
                months.entry(month).or_default().push((sum, t.times, t.per));
            }
        }
        for (month, terms) in months {
            let amount = combine(&terms);
            if amount != 0 {
                *out.entry(month).or_default() += amount;
            }
        }
    }
    out
}

/// What a month assigned a category: its formula's amount, when it
/// follows one, or what was typed.
pub fn assigned_of(db: &Db<Money>, month: &str, id: Id<Category>) -> i64 {
    match formula_in(db, id, month) {
        Some(f) => in_month(db, &f.terms, month).1,
        None => crate::budget::assigned_in(db, month, id),
    }
}

// * Setting one

/// A category that may have a formula: one money is assigned to, not a card's.
fn formulable(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
    let c = budgetable(db, id)?;
    if crate::cards::is_payment(db, id) {
        return Err(Refusal::new("a card's payment category is assigned by hand: its money is the card's"));
    }
    Ok(c)
}

/// The month a change is from: the one said, or this one.
fn from_of(ctx: &Ctx, from: &Option<String>) -> String {
    from.clone().unwrap_or_else(|| month_of(&day_of_ms(ctx.now)))
}

/// A category's versions from a month on, removed: what a version set from
/// that month replaces.
fn clear_from(db: &mut Db<Money>, id: Id<Category>, from: &str) -> Result<(), Refusal> {
    for f in db.formula.by(Formula::CATEGORY_ID_FROM).from(id).all() {
        if f.from.as_str() >= from {
            db.formula.remove(&f)?;
        }
    }
    Ok(())
}

/// The version before a month: the one it would follow without its own.
fn before(db: &Db<Money>, id: Id<Category>, from: &str) -> Option<Formula> {
    db.formula
        .by(Formula::CATEGORY_ID_FROM)
        .from(id)
        .all()
        .into_iter()
        .rev()
        .find(|f| f.from.as_str() < from)
}

/// Terms a formula may have: one at least, each a saved filter that is
/// here and a factor that is a ratio of something.
fn checked(db: &Db<Money>, terms: &[Term]) -> Result<(), Refusal> {
    if terms.is_empty() {
        return Err(Refusal::new("a formula sums at least one filter"));
    }
    if terms.len() > TERMS {
        return Err(Refusal::new(format!("a formula has at most {TERMS} terms")));
    }
    for t in terms {
        if !db.saved_filter.has(t.filter_id) {
            return Err(Refusal::new("no such filter"));
        }
        if t.times < 0 || t.per <= 0 {
            return Err(Refusal::new("a factor is a share of something, never below nothing"));
        }
    }
    Ok(())
}

/// A version from `from` on, in place of whatever followed from then —
/// unless the month before already follows the same.
fn put(ctx: &Ctx, db: &mut Db<Money>, id: Id<Category>, from: String, terms: Vec<Term>, said: String) -> Result<(), Refusal> {
    clear_from(db, id, &from)?;
    let f = Formula {
        from,
        category_id: id,
        manual: false,
        terms,
        said,
        at_ms: ctx.now,
        user_id: ctx.user().into(),
    };
    if before(db, id, &f.from).is_some_and(|b| !b.manual && b.terms == f.terms && b.said == f.said) {
        return Ok(());
    }
    db.formula.insert(f)
}

/// A category assigned by formula from a month on (none: this month), in
/// place of whatever it followed from then.
#[mutator("categories.set_formula")]
pub fn set_formula(ctx: &Ctx, db: &mut Db<Money>, input: SetFormula) -> Result<(), Refusal> {
    formulable(db, input.category_id)?;
    checked(db, &input.terms)?;
    let from = from_of(ctx, &input.from);
    put(ctx, db, input.category_id, from, input.terms, String::new())
}

/// A category assigned by hand again from a month on (none: this month):
/// every month before keeps what its formula gave it.
#[mutator("categories.clear_formula")]
pub fn clear_formula(ctx: &Ctx, db: &mut Db<Money>, input: ClearFormula) -> Result<(), Refusal> {
    formulable(db, input.category_id)?;
    let from = from_of(ctx, &input.from);
    let later = db
        .formula
        .by(Formula::CATEGORY_ID_FROM)
        .from(input.category_id)
        .all()
        .into_iter()
        .any(|f| f.from >= from);
    let followed_before = before(db, input.category_id, &from).is_some_and(|b| !b.manual);
    if !later && !followed_before {
        return Err(Refusal::new("this category is assigned by hand already"));
    }
    clear_from(db, input.category_id, &from)?;
    if !followed_before {
        return Ok(());
    }
    db.formula.insert(Formula {
        from,
        category_id: input.category_id,
        manual: true,
        at_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Formula::default()
    })
}

/// What the phones set before formulas, kept so a phone built then still
/// can: `times / per` of the month's inflows, not transfers, in the
/// accounts named (none: every account on the budget) whose payee or bank
/// line holds `matching`. It is a formula of one term, over a saved filter
/// of exactly those conditions — the one there is, or a new one, "Income
/// matching acme in Checking" — said as the rule was ("10% of income
/// matching acme in Checking").
#[mutator("categories.set_rule")]
pub fn set_rule(ctx: &Ctx, db: &mut Db<Money>, input: SetRule) -> Result<(), Refusal> {
    formulable(db, input.category_id)?;
    let mut accounts = input.accounts.clone();
    accounts.sort();
    accounts.dedup();
    let mut names = Vec::new();
    for a in &accounts {
        match db.account.get(*a) {
            None => return Err(Refusal::new("no such account")),
            Some(a) if !a.on_budget => return Err(Refusal::new(format!("{} is not on the budget: its money is not assigned", a.name))),
            Some(a) => names.push(a),
        }
    }
    names.sort_by_key(|a| (a.pos, a.id));
    let names: Vec<String> = names.into_iter().map(|a| a.name).collect();
    let matching = input.matching.trim().to_string();
    let filter = Filter {
        accounts,
        on_budget: true,
        text: matching.clone(),
        direction: Direction::In,
        no_transfers: true,
        ..Filter::default()
    }
    .tidy();
    let r#in = match names.is_empty() {
        true => "any account".to_string(),
        false => names.join(", "),
    };
    let saved = db.saved_filter.scan().into_iter().find(|f| f.filter == filter);
    let filter_id = match saved {
        Some(f) => f.id,
        None => {
            let base = match names.is_empty() {
                true => format!("Income matching {matching}"),
                false => format!("Income matching {matching} in {}", names.join(", ")),
            };
            let taken = |n: &str| db.saved_filter.scan().into_iter().any(|f| f.name.eq_ignore_ascii_case(n));
            let name = (1..)
                .map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") })
                .find(|n| !taken(n))
                .unwrap_or(base);
            let id = ctx.new_id();
            let pos = db.saved_filter.by(SavedFilter::POS).whole().all().last().map_or(0, |f| f.pos + 1);
            db.saved_filter.insert(SavedFilter {
                id,
                name,
                pos,
                filter,
                created_ms: ctx.now,
                user_id: ctx.user().into(),
            })?;
            id
        }
    };
    let said = format!("{} of income matching {matching} in {}", percent(input.times, input.per), r#in);
    let term = Term {
        filter_id,
        times: input.times,
        per: input.per,
    };
    put(ctx, db, input.category_id, from_of(ctx, &input.from), vec![term], said)
}

// * The views

/// A category's formula as a month follows it, with what it gives the month.
pub fn formula_line(db: &Db<Money>, f: &Formula, month: &str) -> FormulaLine {
    let (terms, amount) = in_month(db, &f.terms, month);
    FormulaLine {
        category_id: f.category_id,
        from: f.from.clone(),
        terms,
        text: said_of(db, f),
        amount,
    }
}

/// Every category whose month follows a formula, and the formula.
#[view("categories.formulas")]
pub fn formulas(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<FormulaLine>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>()
        .filter_with({
            let month = month.clone();
            move |db, c| formula_in(db, c.id, &month).is_some()
        })
        .map_with(move |db, c| formula_line(db, &formula_in(db, c.id, &month).unwrap_or_default(), &month))
        .sort(|l| l.category_id))
}

/// What a formula would assign in a month, before it is saved: one row.
#[view("categories.preview_formula")]
pub fn preview_formula(_: &Ctx, _: &Db<Money>, input: PreviewFormula) -> Result<Plan<FormulaPreview>, Refusal> {
    let (month, terms) = (input.month, input.terms);
    Ok(scan::<Money, Category>().group(
        |_| 0i64,
        move |db, _, _| {
            let (lines, amount) = in_month(db, &terms, &month);
            FormulaPreview {
                month: month.clone(),
                terms: lines,
                text: text_of(db, &terms),
                amount,
            }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_rounds_half_away_from_nothing() {
        assert_eq!(share(100_000, 1, 10), 10_000);
        assert_eq!(share(12_345, 1, 10), 1_235);
        assert_eq!(share(12_344, 1, 10), 1_234);
        assert_eq!(share(-12_345, 1, 10), -1_235);
        assert_eq!(share(10_001, 3, 2), 15_002);
        assert_eq!(share(5, 1, 2), 3);
        assert_eq!(share(-5, 1, 2), -3);
        assert_eq!(share(i64::MAX, 2, 1), i64::MAX);
    }

    /// The terms are added exactly and rounded once: two halves of a cent
    /// are a cent, not two.
    #[test]
    fn terms_are_rounded_once_together() {
        // 10% of $0.05 is half a cent, twice: one cent, where rounding
        // each would say two.
        assert_eq!(combine(&[(5, 1, 10), (5, 1, 10)]), 1);
        assert_eq!(share(5, 1, 10) + share(5, 1, 10), 2);
        // A third and two thirds of a dollar: a dollar.
        assert_eq!(combine(&[(100, 1, 3), (100, 2, 3)]), 100);
        // Money in and money out cancel before rounding.
        assert_eq!(combine(&[(1_005, 1, 2), (-1_005, 1, 2)]), 0);
        assert_eq!(combine(&[(-3, 1, 2)]), -2);
        assert_eq!(combine(&[]), 0);
        // Fractions too fine to keep together still come out near.
        let fine = [(i64::MAX / 4, 1, 999_999_937), (i64::MAX / 4, 1, 999_999_929), (1, 1, 999_999_893)];
        assert!(combine(&fine) > 0);
    }

    #[test]
    fn a_share_reads_as_it_is_typed() {
        assert_eq!(parse_share("10%"), Some((1, 10)));
        assert_eq!(parse_share("0.1"), Some((1, 10)));
        assert_eq!(parse_share(".1"), Some((1, 10)));
        assert_eq!(parse_share("1.5"), Some((3, 2)));
        assert_eq!(parse_share("1.5x"), Some((3, 2)));
        assert_eq!(parse_share(" 150 % "), Some((3, 2)));
        assert_eq!(parse_share("12.5%"), Some((1, 8)));
        assert_eq!(parse_share("2"), Some((2, 1)));
        assert_eq!(parse_share("0"), Some((0, 1)));
        for bad in ["", "%", "-10%", "ten", "1.2.3", "1e3", "x"] {
            assert_eq!(parse_share(bad), None, "{bad:?}");
        }
        assert_eq!(percent(1, 10), "10%");
        assert_eq!(percent(3, 2), "150%");
        assert_eq!(percent(1, 8), "12.5%");
        assert_eq!(percent(1, 3), "33.3333%");
    }
}
