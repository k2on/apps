//! Assignment by rule: a category given, each month, a share of what came
//! in — "10% of every paycheck from ACME into Checking or Savings goes to
//! Giving" — rather than an amount somebody typed.
//!
//! - **What a rule reads.** A month's inflows (positive, not transfers,
//!   dated in it) in the accounts it names — none named is every account
//!   on the budget; an account off the budget never counts — whose payee
//!   or the bank's own line for it (`imported_payee`) holds its text, any
//!   case. Their sum times its share (`times / per`, kept exactly) is the
//!   month's assignment, rounded to the cent half away from zero.
//! - **From a month on.** A rule is a version keyed by its category and
//!   the month it starts; a month follows the latest version starting at
//!   or before it. Changing a rule is a new version from the month it is
//!   done; removing one is a version that says `manual`. Every month
//!   before the first keeps what people assigned it, and a version set
//!   from a month replaces any that started after it.
//! - **A rule's amount is assigned as any other.** Ready to assign, the
//!   month's totals, overspending, what carries, what a card's payment
//!   category is moved — every view reads [`assignments`], so the rule's
//!   amount counts exactly where a typed one would. While a month follows
//!   a rule nobody assigns it by hand: assigning, moving money to or from
//!   it, and the month's quick buttons refuse or pass it by ([`refuse`]).
//! - **Not every category.** The budget's own (Ready to Assign) and a
//!   card's payment category — whose money is the card's — have none.
//!
//! As everywhere in the budget, nothing worked out is stored: a paycheck
//! that arrives late, or is renamed, moves the month it is in.

use std::collections::BTreeMap;

use ark::prelude::*;

use crate::budget::{budgetable, Plan};
use crate::dates::{day_of_ms, month_end, month_of, month_start};
use crate::schema::*;
use crate::types::*;

/// What a month following a rule says to a hand that would assign it.
pub const FOLLOWS: &str = "this category's assignment follows its rule";

/// The version of a category's rule a month follows, if one does: the
/// latest starting at or before it, unless that one says `manual`.
pub fn rule_in(db: &Db<Money>, id: Id<Category>, month: &str) -> Option<Rule> {
    db.rule
        .by(Rule::CATEGORY_ID_FROM)
        .between((id, String::new()), (id, format!("{month}~")))
        .last()
        .filter(|r| !r.manual)
}

/// Refused, when the month follows a rule: what assigning by hand checks.
pub fn refuse(db: &Db<Money>, id: Id<Category>, month: &str) -> Result<(), Refusal> {
    match rule_in(db, id, month) {
        Some(_) => Err(Refusal::new(FOLLOWS)),
        None => Ok(()),
    }
}

/// `amount × times / per`, to the cent, half away from zero.
pub fn share(amount: i64, times: i64, per: i64) -> i64 {
    if per == 0 {
        return 0;
    }
    let (n, d) = (amount as i128 * times as i128, per as i128);
    let (q, r) = (n / d, n % d);
    let up = 2 * r.abs() >= d.abs();
    let sign = if (n < 0) != (d < 0) { -1 } else { 1 };
    (q + if up { sign } else { 0 }).clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// What a rule reads: its text, its accounts, its share.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    pub matching: String,
    pub accounts: Vec<Id<Account>>,
    pub times: i64,
    pub per: i64,
}

impl Spec {
    pub fn of(r: &Rule) -> Spec {
        Spec {
            matching: r.matching.clone(),
            accounts: r.accounts.clone(),
            times: r.times,
            per: r.per,
        }
    }

    /// Whether a transaction is one it counts: money in, not a transfer,
    /// its payee or the bank's line holding the text.
    pub fn matches(&self, t: &Txn) -> bool {
        if t.amount <= 0 || t.transfer_account.is_some() || t.transfer_id.is_some() {
            return false;
        }
        let want = self.matching.trim().to_lowercase();
        !want.is_empty() && (t.payee.to_lowercase().contains(&want) || t.imported_payee.to_lowercase().contains(&want))
    }

    /// The accounts it reads: those named that are on the budget, or every
    /// one on the budget.
    pub fn accounts_in(&self, db: &Db<Money>) -> Vec<Account> {
        match self.accounts.is_empty() {
            true => db.account.scan().into_iter().filter(|a| a.on_budget).collect(),
            false => self.accounts.iter().filter_map(|a| db.account.get(*a)).filter(|a| a.on_budget).collect(),
        }
    }

    /// What it matched, month by month from `lo` through `hi` (months; none
    /// for no end): how many, and what they brought in.
    pub fn matched(&self, db: &Db<Money>, lo: &str, hi: Option<&str>) -> BTreeMap<String, (i64, i64)> {
        let mut out: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        let (from, to) = (month_start(lo), hi.map_or_else(|| "~".to_string(), month_end));
        for a in self.accounts_in(db) {
            for t in db.txn.by(Txn::ACCOUNT_ID_DATE).between((a.id, from.clone()), (a.id, to.clone())).all() {
                if self.matches(&t) {
                    let m = out.entry(month_of(&t.date)).or_default();
                    (m.0, m.1) = (m.0 + 1, m.1 + t.amount);
                }
            }
        }
        out
    }

    /// It, said shortly: "10% of income matching ACME PAYROLL in Checking,
    /// Savings".
    pub fn summary(&self, db: &Db<Money>) -> String {
        let mut named: Vec<Account> = self.accounts.iter().filter_map(|a| db.account.get(*a)).collect();
        named.sort_by_key(|a| (a.pos, a.id));
        let names: Vec<String> = named.into_iter().map(|a| a.name).collect();
        let r#in = match names.is_empty() {
            true => "any account".to_string(),
            false => names.join(", "),
        };
        format!(
            "{} of income matching {} in {}",
            percent(self.times, self.per),
            self.matching.trim(),
            r#in
        )
    }
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
    let g = gcd(times, per);
    Some((times / g, per / g))
}

fn gcd(a: i64, b: i64) -> i64 {
    match b {
        0 => a.max(1),
        _ => gcd(b, a % b),
    }
}

/// What a category was assigned, month by month: what people gave each
/// month that follows no rule, and each month that does, its rule's share
/// of what came in. Every view of the budget reads assignments from here.
pub fn assignments(db: &Db<Money>, id: Id<Category>) -> BTreeMap<String, i64> {
    let versions = db.rule.by(Rule::CATEGORY_ID_FROM).from(id).all();
    let mut out = BTreeMap::new();
    for a in db.assigned.by(Assigned::CATEGORY_ID_MONTH).from(id).all() {
        let follows = versions.iter().rev().find(|r| r.from <= a.month).is_some_and(|r| !r.manual);
        if !follows {
            *out.entry(a.month.clone()).or_default() += a.amount;
        }
    }
    for (i, r) in versions.iter().enumerate() {
        if r.manual {
            continue;
        }
        let until = versions.get(i + 1).map(|n| crate::dates::add_months(&n.from, -1));
        let spec = Spec::of(r);
        for (month, (_, sum)) in spec.matched(db, &r.from, until.as_deref()) {
            let amount = share(sum, r.times, r.per);
            if amount != 0 {
                *out.entry(month).or_default() += amount;
            }
        }
    }
    out
}

/// What a month assigned a category: its rule's share, when it follows
/// one, or what was typed.
pub fn assigned_of(db: &Db<Money>, month: &str, id: Id<Category>) -> i64 {
    match rule_in(db, id, month) {
        Some(r) => {
            let spec = Spec::of(&r);
            spec.matched(db, month, Some(month))
                .get(month)
                .map_or(0, |(_, sum)| share(*sum, r.times, r.per))
        }
        None => crate::budget::assigned_in(db, month, id),
    }
}

/// A category that may have a rule: one money is assigned to, not a card's.
fn ruleable(db: &Db<Money>, id: Id<Category>) -> Result<Category, Refusal> {
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
    for r in db.rule.by(Rule::CATEGORY_ID_FROM).from(id).all() {
        if r.from.as_str() >= from {
            db.rule.remove(&r)?;
        }
    }
    Ok(())
}

/// The version before a month: the one it would follow without its own.
fn before(db: &Db<Money>, id: Id<Category>, from: &str) -> Option<Rule> {
    db.rule
        .by(Rule::CATEGORY_ID_FROM)
        .from(id)
        .all()
        .into_iter()
        .rev()
        .find(|r| r.from.as_str() < from)
}

/// A category assigned by rule from a month on (none: this month), in
/// place of whatever it followed from then.
#[mutator("categories.set_rule")]
pub fn set_rule(ctx: &Ctx, db: &mut Db<Money>, input: SetRule) -> Result<(), Refusal> {
    ruleable(db, input.category_id)?;
    let mut accounts = input.accounts.clone();
    accounts.sort();
    accounts.dedup();
    for a in &accounts {
        match db.account.get(*a) {
            None => return Err(Refusal::new("no such account")),
            Some(a) if !a.on_budget => return Err(Refusal::new(format!("{} is not on the budget: its money is not assigned", a.name))),
            Some(_) => {}
        }
    }
    let from = from_of(ctx, &input.from);
    clear_from(db, input.category_id, &from)?;
    let rule = Rule {
        from,
        category_id: input.category_id,
        manual: false,
        matching: input.matching.trim().to_string(),
        accounts,
        times: input.times,
        per: input.per,
        at_ms: ctx.now,
        user_id: ctx.user().into(),
    };
    // The same rule as the month before already follows: nothing new.
    if before(db, rule.category_id, &rule.from).is_some_and(|b| !b.manual && Spec::of(&b) == Spec::of(&rule)) {
        return Ok(());
    }
    db.rule.insert(rule)
}

/// A category assigned by hand again from a month on (none: this month):
/// every month before keeps what its rule gave it.
#[mutator("categories.clear_rule")]
pub fn clear_rule(ctx: &Ctx, db: &mut Db<Money>, input: ClearRule) -> Result<(), Refusal> {
    ruleable(db, input.category_id)?;
    let from = from_of(ctx, &input.from);
    let later = db
        .rule
        .by(Rule::CATEGORY_ID_FROM)
        .from(input.category_id)
        .all()
        .into_iter()
        .any(|r| r.from >= from);
    let ruled_before = before(db, input.category_id, &from).is_some_and(|b| !b.manual);
    if !later && !ruled_before {
        return Err(Refusal::new("this category is assigned by hand already"));
    }
    clear_from(db, input.category_id, &from)?;
    if !ruled_before {
        return Ok(());
    }
    db.rule.insert(Rule {
        from,
        category_id: input.category_id,
        manual: true,
        at_ms: ctx.now,
        user_id: ctx.user().into(),
        ..Rule::default()
    })
}

/// A category's rule as a month follows it, with what it gives the month.
pub fn rule_line(db: &Db<Money>, r: &Rule, month: &str) -> RuleLine {
    let spec = Spec::of(r);
    let matched = spec.matched(db, month, Some(month)).get(month).map_or(0, |(_, sum)| *sum);
    RuleLine {
        category_id: r.category_id,
        from: r.from.clone(),
        matching: r.matching.clone(),
        accounts: r.accounts.clone(),
        times: r.times,
        per: r.per,
        summary: spec.summary(db),
        matched,
        amount: share(matched, r.times, r.per),
    }
}

/// Every category whose month follows a rule, and the rule.
#[view("categories.rules")]
pub fn rules(_: &Ctx, _: &Db<Money>, input: Month) -> Result<Plan<RuleLine>, Refusal> {
    let month = input.month;
    Ok(scan::<Money, Category>()
        .filter_with({
            let month = month.clone();
            move |db, c| rule_in(db, c.id, &month).is_some()
        })
        .map_with(move |db, c| rule_line(db, &rule_in(db, c.id, &month).unwrap_or_default(), &month))
        .sort(|l| l.category_id))
}

/// What a rule would assign in a month, before it is saved: one row, from
/// the accounts it would read.
#[view("categories.preview_rule")]
pub fn preview_rule(_: &Ctx, _: &Db<Money>, input: PreviewRule) -> Result<Plan<RulePreview>, Refusal> {
    let spec = Spec {
        matching: input.matching,
        accounts: input.accounts,
        times: input.times,
        per: input.per,
    };
    let month = input.month;
    Ok(scan::<Money, Account>().group(
        |_| 0i64,
        move |db, _, _| {
            let (count, matched) = spec.matched(db, &month, Some(&month)).get(&month).copied().unwrap_or_default();
            RulePreview {
                month: month.clone(),
                summary: spec.summary(db),
                count,
                matched,
                amount: share(matched, spec.times, spec.per),
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
