//! The inspectors' records (ui/docs/apps/money.md): Category Details
//! beside the budget and Transaction Details beside a register, worked out
//! here from the views the shell holds, every figure and word, so the
//! three toolkits draw the same one (`view::category_inspector`,
//! `view::transaction_inspector`).
//!
//! The reference's Goal picker offers kinds of goal (a monthly amount, a
//! balance by a date); money's domain has one, a target a month, so the
//! picker's place says that kind and the target, and there is no date.

use ark_format::{amount, day_long, month, NumberStyle};
use money_core::words::Tone;

use crate::words::LineWords;
use money_core::{BudgetLine, Cleared, TxnLine};

/// One line of an inspector's list: a transaction, an item.
#[derive(Clone, Debug, PartialEq, Default, ark::twin::Twin)]
pub struct DetailLine {
    pub title: String,
    pub subtitle: String,
    pub cents: i64,
}

/// Category Details: the category's month and what to do about it.
#[derive(Clone, Debug, PartialEq, Default, ark::twin::Twin)]
pub struct CategoryInspector {
    /// Its id, hex.
    pub id: String,
    pub name: String,
    pub group: String,
    /// What it has, as the badge says it.
    pub figure: String,
    /// The badge's tint (ArkUI's names).
    pub tint: String,
    /// What needs attention, in words: "Overspent by $21.02".
    pub attention: String,
    /// "Assign $140.00", while assigning the target's rest would help.
    pub assign: String,
    pub can_assign: bool,
    /// Overspent: Cover Overspending is offered.
    pub cover: bool,
    /// The month, "November 2026".
    pub month: String,
    pub carried: i64,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    /// The goal's kind, "Monthly Amount", or "None".
    pub goal: String,
    pub has_goal: bool,
    pub target: i64,
    pub progress: f64,
    pub progress_tint: String,
    pub progress_note: String,
    /// The month's transactions in it, newest first.
    pub activity_lines: Vec<DetailLine>,
    pub quiet_month: bool,
}

/// Transaction Details: who, how much, where and when, what for.
#[derive(Clone, Debug, PartialEq, Default, ark::twin::Twin)]
pub struct TransactionInspector {
    pub id: String,
    pub payee: String,
    pub figure: String,
    pub tint: String,
    pub category: String,
    pub account: String,
    pub date: String,
    pub status: String,
    pub memo: String,
    pub has_memo: bool,
    /// Waiting for a look (the bank's import, not yet approved).
    pub review: bool,
}

/// ArkUI's tint for a category's tone.
pub fn tint_of(t: Tone) -> &'static str {
    match t {
        Tone::Positive => "positive",
        Tone::Short | Tone::OnCard => "caution",
        Tone::Overspent => "negative",
        Tone::Neutral => "neutral",
    }
}

fn us() -> NumberStyle {
    NumberStyle::us()
}

/// The category's inspector for `month`, its transactions among `register`.
pub fn category(l: &BudgetLine, month_key: &str, register: &[TxnLine]) -> CategoryInspector {
    let s = us();
    let a = |c: i64| amount(c, &s);
    let tone = l.tone();
    let meter = l.meter();
    let rest = (l.target - l.assigned).max(0);
    let lines: Vec<DetailLine> = register
        .iter()
        .filter(|t| t.category_id == Some(l.category_id) && t.date.starts_with(month_key))
        .map(|t| DetailLine {
            title: t.payee.clone(),
            subtitle: format!("{} \u{b7} {}", day_long(&t.date), t.account),
            cents: t.amount,
        })
        .collect();
    CategoryInspector {
        id: l.category_id.hex(),
        name: l.name.clone(),
        group: l.group.clone(),
        figure: a(l.available),
        tint: tint_of(tone).into(),
        attention: l.story(&s),
        assign: format!("Assign {}", a(rest)),
        can_assign: l.assignable() && l.target > 0 && rest > 0,
        cover: l.available < 0 && l.assignable(),
        month: month(month_key),
        carried: l.carried,
        assigned: l.assigned,
        activity: l.activity,
        available: l.available,
        goal: if l.target > 0 { "Monthly Amount".into() } else { "None".into() },
        has_goal: l.target > 0,
        target: l.target,
        progress: meter.map(|m| m.fraction()).unwrap_or(0.0),
        progress_tint: meter.map(|m| tint_of(m.tone)).unwrap_or("neutral").into(),
        progress_note: match meter {
            Some(m) => format!("{}% of {} assigned this month.", m.thousandths / 10, a(l.target)),
            None => String::new(),
        },
        quiet_month: lines.is_empty(),
        activity_lines: lines,
    }
}

/// A transaction's inspector.
pub fn transaction(t: &TxnLine) -> TransactionInspector {
    let s = us();
    TransactionInspector {
        id: t.id.hex(),
        payee: t.payee.clone(),
        figure: amount(t.amount, &s),
        tint: if t.amount > 0 { "positive".into() } else { "neutral".into() },
        category: t.category.clone(),
        account: t.account.clone(),
        date: day_long(&t.date),
        status: match (t.approved, t.cleared) {
            (false, _) => "Needs Review".into(),
            (true, Cleared::Reconciled) => "Reconciled".into(),
            (true, Cleared::Cleared) => "Cleared".into(),
            (true, Cleared::Uncleared) => "Uncleared".into(),
        },
        memo: t.memo.clone(),
        has_memo: !t.memo.is_empty(),
        review: !t.approved,
    }
}

/// The category a launch names among `lines`: its id, its name in any
/// case, or a part of its name.
pub fn category_named<'a>(lines: &'a [BudgetLine], key: &str) -> Option<&'a BudgetLine> {
    let k = key.to_lowercase();
    lines
        .iter()
        .find(|l| l.category_id.hex() == key || l.name.to_lowercase() == k)
        .or_else(|| lines.iter().find(|l| l.name.to_lowercase().contains(&k)))
}

/// The transaction a launch names among `all` (newest first), the
/// reference's keys: `receipt` the newest with a receipt, `review` the
/// newest waiting, `transfer`, a number (the n-th oldest), its id, else a
/// payee.
pub fn transaction_named<'a>(all: &'a [TxnLine], key: &str) -> Option<&'a TxnLine> {
    let k = key.to_lowercase();
    match k.as_str() {
        "receipt" => all.iter().find(|t| !t.receipt.is_empty() || t.photos > 0),
        "review" => all.iter().find(|t| !t.approved),
        "transfer" => all.iter().find(|t| t.transfer),
        _ => match k.parse::<usize>() {
            Ok(n) if n >= 1 && n <= all.len() => all.iter().rev().nth(n - 1),
            _ => all
                .iter()
                .find(|t| t.id.hex() == key)
                .or_else(|| all.iter().find(|t| t.payee.to_lowercase().contains(&k))),
        },
    }
}
