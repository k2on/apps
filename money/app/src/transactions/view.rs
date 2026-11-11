//! The Transactions tab laid out, once: drawn by every toolkit from this
//! function or its recording.

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{TransactionsMsg, TransactionsPage, TxnItem};

/// The tab: To Review and All as two chips; To Review's rows under how
/// many there are, Approve All under them, or that all is caught up; All's
/// by day; over it the category's sheet and the question before a delete.
#[view]
pub fn transactions(p: &TransactionsPage) -> View<TransactionsMsg> {
    tab_page(
        "Transactions",
        (
            chip_row((
                chip(p.review_title, "inbox", p.reviewing, TransactionsMsg::Show { review: true }),
                chip("All", "list", p.all_shown, TransactionsMsg::Show { review: false }),
            )),
            if p.reviewing {
                (
                    if p.caught_up {
                        card_section(list_row(p.empty_title).subtitle(p.empty_text).glyph("circle-check").quiet())
                    },
                    if p.has_review {
                        card_section((
                            for t in p.review {
                                txn_row(t)
                            },
                            if p.can_approve_all {
                                list_row(p.approve_all).glyph("check").accent().on_tap(TransactionsMsg::ApproveAll)
                            },
                        ))
                        .header(p.review_count)
                        .footer(p.review_note)
                    },
                )
            },
            if p.all_shown {
                for d in p.days {
                    card_section(for t in d.rows {
                        txn_row(t)
                    })
                    .header(d.title)
                }
            },
            (
                if let Some(c) = p.categorizing {
                    form_sheet(
                        "Categorize",
                        TransactionsMsg::Dismiss,
                        form_section(option_picker(
                            "Category",
                            c.chosen,
                            |key| TransactionsMsg::Chose { key },
                            for o in c.choices {
                                picker_choice(o.title, o.glyph, o.key)
                            },
                        )),
                    )
                    .confirm("Save", c.can_save, TransactionsMsg::SaveCategory)
                },
                if let Some(d) = p.deleting {
                    confirmation_dialog("Delete Transaction?", d.message, TransactionsMsg::CancelDelete).action(
                        "Delete",
                        true,
                        TransactionsMsg::ConfirmDelete,
                    )
                },
            ),
        ),
    )
    .refresh(TransactionsMsg::Refresh)
    .into()
}

/// A transaction: its payee, its category and account (New while it waits
/// to be approved), its amount; tapped, a category asked for or its
/// details; held, Approve, Categorize, Edit and Delete.
#[view]
pub fn txn_row(t: &TxnItem) -> View<TransactionsMsg> {
    list_row(t.payee)
        .subtitle(t.detail)
        .amount(t.amount)
        .said(t.said)
        .on_tap(TransactionsMsg::Open { id: t.id })
        .menu_when("Approve", "check", TransactionsMsg::Approve { id: t.id }, t.can_approve)
        .menu("Categorize…", "folder", TransactionsMsg::Categorize { id: t.id }, false)
        .menu("Edit", "pencil", TransactionsMsg::Edit { id: t.id }, false)
        .menu("Delete", "trash", TransactionsMsg::Delete { id: t.id }, true)
        .into()
}
