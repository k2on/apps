//! What a window draws of money's own for the names its shell hosts,
//! laid out once (`#[view]`): the month in the budget's inspector, who is
//! using the budget at the sidebar's foot.

use ark_app::view;
use ark_app::view::*;

use super::MonthInspector;
use crate::budget::BudgetMsg;
use crate::shell::{Household, ShellMsg};

/// The budget's inspector with no category chosen: the month's figures,
/// and Auto-Assign for every category at once.
#[view]
pub fn month_inspector(p: &MonthInspector) -> View<BudgetMsg> {
    detail_page((
        card_section((
            list_row("Left Over From Last Month").amount(p.carried),
            list_row("Assigned This Month").amount(p.assigned),
            list_row("Activity").amount(p.activity),
            list_row("Available").amount(p.available),
        ))
        .header(p.title),
        card_section((
            list_row("Income This Month").amount(p.income),
            list_row("Overspent Last Month").amount(p.overspent_last_month),
        )),
        card_section((
            list_row("Underfunded").glyph("target").accent().on_tap(BudgetMsg::AutoUnderfunded),
            list_row("Assigned Last Month").glyph("clock").accent().on_tap(BudgetMsg::AutoLastMonth),
            list_row("Reset Assigned Amounts").glyph("undo-2").accent().on_tap(BudgetMsg::AutoReset),
        ))
        .header("Auto-Assign")
        .footer(Some("Every category at once, for this month.".to_string())),
    ))
}

/// Who is using the budget, at the sidebar's foot: Settings, on General.
#[view]
pub fn household_foot(h: &Household) -> View<ShellMsg> {
    list_row(h.who).glyph("users").said("Settings").on_tap(ShellMsg::Account).into()
}
