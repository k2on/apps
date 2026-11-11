//! The sheets laid out, once: drawn by every toolkit from these functions
//! or their recordings.

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{ReconcileSheet, SheetsMsg, SheetsPage, TxnSheet};
use crate::receipts::view::receipt_section;

/// Whichever sheet is up: the transaction's form, a category chosen for
/// transactions, an account reconciled.
#[view]
pub fn sheets(p: &SheetsPage) -> View<SheetsMsg> {
    (
        if let Some(t) = p.transaction {
            if t.adding {
                form_sheet("New Transaction", SheetsMsg::Dismiss, txn_fields(t)).confirm("Add Transaction", t.can_save, SheetsMsg::Save)
            } else {
                form_sheet("Transaction", SheetsMsg::Dismiss, txn_fields(t)).confirm("Save", t.can_save, SheetsMsg::Save)
            }
        },
        if let Some(c) = p.categorizing {
            form_sheet(
                "Categorize",
                SheetsMsg::Dismiss,
                form_section(option_picker(
                    "Category",
                    c.chosen,
                    |key| SheetsMsg::ChoseCategory { key },
                    for o in c.choices {
                        picker_choice(o.title, o.glyph, o.key)
                    },
                )),
            )
            .confirm("Save", c.can_save, SheetsMsg::SaveCategory)
        },
        if let Some(r) = p.reconciling {
            form_sheet("Reconcile", SheetsMsg::Dismiss, reconcile_fields(r)).confirm("Reconcile", r.differs, SheetsMsg::SaveReconcile)
        },
    )
        .into()
}

/// The transaction's form: which way and how much, on the keypad; who it
/// was paid (the payees offered under it while it is typed, what the bank
/// called it under that), what for (a category, or the account a transfer
/// goes to); in which account, on which day, cleared or not; a memo; and,
/// for one already there, Delete, asked first.
#[view]
pub fn txn_fields(t: &TxnSheet) -> View<SheetsMsg> {
    (
        form_section((
            option_picker(
                "Way",
                t.way,
                |way| SheetsMsg::Way { way },
                for w in t.ways {
                    picker_choice(w.title, w.glyph, w.key)
                },
            ),
            amount_entry(
                "Amount",
                t.prompt,
                t.typed,
                t.cents,
                t.amount_problem,
                |typed| SheetsMsg::Typed { typed },
                |cents| SheetsMsg::Cents { cents },
            ),
        )),
        form_section((
            if t.has_payee {
                name_field("Payee", t.payee, None, |payee| SheetsMsg::Payee { payee })
            },
            if t.has_payees {
                for o in t.payees {
                    list_row(o.name)
                        .subtitle(o.category)
                        .glyph("clock")
                        .on_tap(SheetsMsg::ChosePayee { name: o.name })
                }
            },
            if t.has_bank {
                list_row("Bank Description").subtitle(t.bank).quiet()
            },
            option_picker(
                "Category",
                t.category,
                |key| SheetsMsg::Category { key },
                for o in t.categories {
                    picker_choice(o.title, o.glyph, o.key)
                },
            ),
        ))
        .footer(t.category_note),
        form_section((
            if t.editing {
                list_row("Account").value(t.account_name)
            } else {
                option_picker(
                    "Account",
                    t.account,
                    |key| SheetsMsg::Account { key },
                    for a in t.accounts {
                        picker_choice(a.title, a.glyph, a.key)
                    },
                )
            },
            date_field("Date", t.day, |day| SheetsMsg::Day { day }),
            if t.adding {
                switch("Cleared", t.cleared, |cleared| SheetsMsg::Cleared { cleared })
            },
        )),
        form_section(name_field("Memo", t.memo, None, |memo| SheetsMsg::Memo { memo })).footer(t.note),
        // Its receipt as the paper is, or the ways to give it one.
        if let Some(r) = t.receipt {
            receipt_section(r)
        },
        if t.editing {
            form_section(list_row("Delete Transaction").glyph("trash").on_tap(SheetsMsg::Delete))
        },
        if t.deleting {
            confirmation_dialog("Delete Transaction?", t.delete_message, SheetsMsg::CancelDelete).action("Delete", true, SheetsMsg::ConfirmDelete)
        },
    )
        .into()
}

/// Reconciling: is the cleared balance the bank's? Yes locks what was
/// cleared; no asks what the bank says, and the difference is entered as
/// an adjustment.
#[view]
pub fn reconcile_fields(r: &ReconcileSheet) -> View<SheetsMsg> {
    (
        if r.asking {
            form_section((
                list_row(r.question).quiet(),
                if r.has_bank {
                    list_row(r.bank).glyph("landmark").quiet()
                },
                form_button("Yes", true, SheetsMsg::Agree),
                form_button("No", true, SheetsMsg::Differs),
            ))
        },
        if r.differs {
            form_section(amount_entry(
                "Balance",
                r.prompt,
                r.typed,
                r.cents,
                r.problem,
                |typed| SheetsMsg::ReconcileTyped { typed },
                |cents| SheetsMsg::ReconcileCents { cents },
            ))
            .footer("The difference is entered as an adjustment, and everything cleared is locked.")
        },
    )
        .into()
}
