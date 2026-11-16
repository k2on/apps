//! A register laid out, once: drawn by every toolkit from this function or
//! its recording. A table on a regular width and the desktop, a list's
//! rows when compact (ArkUI's `data_table` decides).

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{RegisterDay, RegisterMsg, RegisterPage, RegisterRow};

/// The account's balances over its days of transactions; every account's
/// days alone, with the account as a column.
#[view]
pub fn register(p: &RegisterPage) -> View<RegisterMsg> {
    tab_page(
        "Register",
        (
            if p.owed {
                summary_card("Working Balance", p.balance, "Cleared", p.cleared, "Uncleared", p.uncleared, p.trend)
            },
            if p.owes {
                summary_card("Balance Owed", p.balance, "Cleared", p.cleared, "Uncleared", p.uncleared, p.trend)
            },
            if p.one {
                card_section((
                    if p.has_bank {
                        list_row(p.bank).glyph("landmark").quiet()
                    },
                    if p.can_reconcile {
                        list_row("Reconcile…").glyph("badge-check").accent().on_tap(RegisterMsg::Reconcile)
                    },
                    if p.can_approve_all {
                        list_row(p.approve_all).glyph("check").accent().on_tap(RegisterMsg::ApproveAll)
                    },
                    if p.closed {
                        list_row(p.close_title)
                            .glyph("undo-2")
                            .accent()
                            .on_tap(RegisterMsg::Close { closed: false })
                    } else {
                        list_row(p.close_title)
                            .glyph("archive")
                            .accent()
                            .on_tap(RegisterMsg::Close { closed: true })
                    },
                ))
            },
            if p.all {
                if p.can_approve_all {
                    card_section(list_row(p.approve_all).glyph("check").accent().on_tap(RegisterMsg::ApproveAll))
                }
            },
            if p.empty {
                card_section(empty_state("list", p.empty_title, p.empty_text))
            },
            if p.all {
                data_table(
                    [
                        table_column("Payee", ColumnWidth::Share(3)).slot(ColumnSlot::Title),
                        table_column("Category", ColumnWidth::Share(3)).slot(ColumnSlot::Subtitle),
                        table_column("Account", ColumnWidth::Share(2)).slot(ColumnSlot::Subtitle),
                        table_column("Memo", ColumnWidth::Share(2)),
                        table_column("Amount", ColumnWidth::Figure).end().slot(ColumnSlot::Trailing),
                        table_column("", ColumnWidth::Mark).center().slot(ColumnSlot::TrailingSub),
                    ],
                    for d in p.days {
                        register_day(d)
                    },
                )
            } else {
                data_table(
                    [
                        table_column("Payee", ColumnWidth::Share(3)).slot(ColumnSlot::Title),
                        table_column("Category", ColumnWidth::Share(3)).slot(ColumnSlot::Subtitle),
                        table_column("Memo", ColumnWidth::Share(2)).slot(ColumnSlot::Subtitle),
                        table_column("Amount", ColumnWidth::Figure).end().slot(ColumnSlot::Trailing),
                        table_column("", ColumnWidth::Mark).center().slot(ColumnSlot::TrailingSub),
                    ],
                    for d in p.days {
                        register_day(d)
                    },
                )
            },
            if p.deleting {
                confirmation_dialog("Delete Transaction?", p.delete_message, RegisterMsg::CancelDelete).action(
                    "Delete",
                    true,
                    RegisterMsg::ConfirmDelete,
                )
            },
        ),
    )
    .refresh(RegisterMsg::Refresh)
    .into()
}

/// A day: its heading, then its transactions.
#[view]
pub fn register_day(d: &RegisterDay) -> View<RegisterMsg> {
    (
        table_heading(d.title),
        for t in d.rows {
            register_row(t)
        },
    )
        .into()
}

/// A transaction's row: tapped, its details; held, Edit, Categorize,
/// Approve, Mark Cleared or Uncleared, and Delete.
#[view]
pub fn register_row(t: &RegisterRow) -> View<RegisterMsg> {
    table_row(t.cells)
        .tints(t.tints)
        .glyphs(t.glyphs)
        .marked(t.waiting)
        .selected(t.selected)
        .said(t.said)
        .on_tap(RegisterMsg::Open { id: t.id })
        .menu("Edit", "pencil", RegisterMsg::Edit { id: t.id }, false)
        .menu_if("Categorize…", "folder", RegisterMsg::Categorize { id: t.id }, false, t.can_categorize)
        .menu_if("Approve", "check", RegisterMsg::Approve { id: t.id }, false, t.can_approve)
        .menu_if(
            "Mark Cleared",
            "circle-check",
            RegisterMsg::Clear { id: t.id, cleared: true },
            false,
            t.can_mark_cleared,
        )
        .menu_if(
            "Mark Uncleared",
            "circle",
            RegisterMsg::Clear { id: t.id, cleared: false },
            false,
            t.can_mark_uncleared,
        )
        .menu("Delete", "trash", RegisterMsg::Delete { id: t.id }, true)
        .into()
}
