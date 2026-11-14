//! Insights laid out, once: drawn by every toolkit from this function or
//! its recording.

use ark_app::view;
use ark_app::view::*;

use super::{InsightsMsg, InsightsPage};

/// The month's spending by group, the month as a whole, and each group's
/// plan beside what it spent.
#[view]
pub fn insights(p: &InsightsPage) -> View<InsightsMsg> {
    tab_page(
        "Insights",
        (
            if p.nothing_spent {
                card_section(empty_state("chart-column", "Nothing Spent Yet", p.empty_text)).header("Spending")
            },
            if p.spent_any {
                card_section((
                    bar_chart(p.labels, p.values, p.said),
                    for g in p.groups {
                        list_row(g.title).value(g.share).amount(g.cents)
                    },
                ))
                .header("Spending")
            },
            card_section((
                list_row("Income").amount(p.income),
                list_row("Spent").amount(p.spent),
                list_row("Left Over").amount(p.left).subtitle(p.kept),
                if p.has_cards {
                    list_row("Card Debt Not Covered").amount(p.uncovered).quiet()
                },
            ))
            .header(p.month),
            if p.has_plans {
                card_section(for g in p.plans {
                    meter_row(g.title, g.status, g.tint).meter(g.fraction, g.tint).note(g.note, g.tint)
                })
                .header("Assigned and Spent")
                .footer(Some("Each group's plan for the month beside what it spent.".to_string()))
            },
        ),
    )
    .refresh(InsightsMsg::Refresh)
    .into()
}
