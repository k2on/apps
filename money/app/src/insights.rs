//! Insights: where the month's money went. Its spending by group, the most
//! first — a bar each, then each with its share and its amount; the month
//! as a whole (what came in, what was spent, what was left and the share
//! kept, and what of the cards' debt nothing covers); and each group's plan
//! beside what it spent. Paying a card is not spending (what it bought
//! was), so the cards' group is in none of it. The month is the budget's,
//! paged by the shell's toolbar.

use ark_app::{App, Command, Data};
use ark_format::{amount, percent, NumberStyle};
use money_core::*;

use crate::accounts::Style;
use crate::routes::Route;
use crate::MoneyRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// Insights, as `ark_app` runs it.
pub struct Insights;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The month shown (`2026-11`), the budget's.
    pub month: String,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum InsightsMsg {
    /// The month the budget is on.
    Month {
        month: String,
    },
    Refresh,
}

/// A group's spending.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct SpentGroup {
    pub title: String,
    /// "34%" of the month's spending.
    pub share: String,
    /// Spent, as an outflow (below nothing).
    pub cents: i64,
}

/// A group's plan beside what it spent.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct PlanRow {
    pub title: String,
    /// "$120.00 of $200.00".
    pub status: String,
    pub tint: String,
    pub fraction: Option<f64>,
    pub note: Option<String>,
}

/// Everything Insights shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct InsightsPage {
    /// "November 2026".
    pub month: String,
    pub spent_any: bool,
    pub nothing_spent: bool,
    /// "What November 2026 spends shows here."
    pub empty_text: String,
    /// The bars: a group each, the most first.
    pub labels: Vec<String>,
    pub values: Vec<i64>,
    pub said: Vec<String>,
    pub groups: Vec<SpentGroup>,
    pub income: i64,
    pub spent: i64,
    pub left: i64,
    /// "40% of income kept", or "—" with no income.
    pub kept: String,
    pub has_cards: bool,
    pub uncovered: i64,
    pub has_plans: bool,
    pub plans: Vec<PlanRow>,
}

impl App for Insights {
    type Key = MoneyView;
    type State = State;
    type Msg = InsightsMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = MoneyRequest;
    type Screen = InsightsPage;

    fn holds(s: &State) -> Vec<MoneyView> {
        match s.month.is_empty() {
            true => vec![],
            false => vec![
                MoneyView::Spending { month: s.month.clone() },
                MoneyView::Summary { month: s.month.clone() },
                MoneyView::Lines { month: s.month.clone() },
            ],
        }
    }

    fn update(s: &mut State, msg: InsightsMsg, _: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        match msg {
            InsightsMsg::Month { month } => {
                s.month = month;
                vec![]
            }
            InsightsMsg::Refresh => vec![Command::Request(MoneyRequest::Refresh)],
        }
    }

    fn screen(s: &State, data: &Data<'_, MoneyView>) -> InsightsPage {
        let style: &NumberStyle = &s.style.0;
        let all: &[SpendingLine] = data.rows(&MoneyView::Spending { month: s.month.clone() });
        let mut spending: Vec<&SpendingLine> = all.iter().filter(|g| g.spent > 0).collect();
        spending.sort_by_key(|g| (std::cmp::Reverse(g.spent), g.pos));
        let total: i64 = spending.iter().map(|g| g.spent).sum();
        let summary = data
            .rows::<Summary>(&MoneyView::Summary { month: s.month.clone() })
            .first()
            .cloned()
            .unwrap_or_default();
        let lines: &[BudgetLine] = data.rows(&MoneyView::Lines { month: s.month.clone() });
        let month = ark_format::month(&s.month);
        // Each group's plan: its lines' assigned and spent, the cards' left out.
        let mut plans: Vec<(String, i64, i64, i64)> = vec![];
        let mut shown: Vec<&BudgetLine> = lines.iter().filter(|l| !l.hidden && !l.is_payment()).collect();
        shown.sort_by_key(|l| (l.group_pos, l.pos));
        for l in shown {
            match plans.last_mut() {
                Some(p) if p.0 == l.group => {
                    p.1 += l.assigned;
                    p.2 += (-l.activity).max(0);
                }
                _ => plans.push((l.group.clone(), l.assigned, (-l.activity).max(0), l.group_pos)),
            }
        }
        plans.retain(|p| p.1 != 0 || p.2 != 0);
        let left = summary.income - total;
        InsightsPage {
            spent_any: !spending.is_empty(),
            nothing_spent: spending.is_empty(),
            empty_text: format!("What {month} spends shows here."),
            labels: spending.iter().map(|g| g.group.clone()).collect(),
            values: spending.iter().map(|g| g.spent).collect(),
            said: spending.iter().map(|g| amount(g.spent, style)).collect(),
            groups: spending
                .iter()
                .map(|g| SpentGroup {
                    title: g.group.clone(),
                    share: percent(g.spent, total),
                    cents: -g.spent,
                })
                .collect(),
            income: summary.income,
            spent: -total,
            left,
            kept: match summary.income {
                0 => "\u{2014}".into(),
                i => format!("{} of income kept", percent(left.max(0), i)),
            },
            has_cards: lines.iter().any(|l| l.is_payment()),
            uncovered: money_core::words::uncovered(lines),
            has_plans: !plans.is_empty(),
            plans: plans
                .into_iter()
                .map(|(title, assigned, spent, _)| {
                    let over = spent > assigned;
                    PlanRow {
                        status: format!("{} of {}", amount(spent, style), amount(assigned, style)),
                        tint: if over { "negative" } else { "positive" }.into(),
                        fraction: (assigned > 0).then(|| (spent as f64 / assigned as f64).min(1.0)),
                        note: over.then(|| format!("{} over", amount(spent - assigned, style))),
                        title,
                    }
                })
                .collect(),
            month,
        }
    }
}
