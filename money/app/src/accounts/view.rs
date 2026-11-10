//! The Accounts tab laid out, once: drawn by iced and the browser from
//! this function, written out as SwiftUI and Compose from its recording
//! (`money_app::screens`, written while nix builds the native projects:
//! nix/ark-app.nix).

use ark_app::view;
use ark_app::view::*;

use super::{AccountItem, AccountsMsg, AccountsPage};

/// The tab: what the household is worth in its card (what it owns, what it
/// owes, how it went), the shelves of accounts (Budget Accounts, Tracking
/// Accounts, Closed) each with its total and a note, Bank Connections and
/// Add Account; Add Account's sheet and the rename alert over it while
/// they are up.
#[view]
pub fn accounts(p: &AccountsPage) -> View<AccountsMsg> {
    tab_page(
        "Accounts",
        (
            summary_card("Net Worth", p.net_worth, "Assets", p.assets, "Liabilities", p.liabilities, p.trend),
            for shelf in p.shelves {
                card_section(for a in shelf.accounts {
                    account_row(a)
                })
                .header(shelf.title)
                .amount(shelf.total)
                .shows_amount(shelf.shows_total)
                .footer(shelf.note)
            },
            card_section((
                list_row(p.banks_title).glyph("link").pushes().on_tap(AccountsMsg::OpenBanks),
                list_row("Add Account").glyph("circle-plus").accent().on_tap(AccountsMsg::Add),
            ))
            .footer(p.synced),
            if let Some(form) = p.adding {
                form_sheet(
                    "Add Account",
                    AccountsMsg::DismissDraft,
                    (
                        form_section((
                            name_field("Name", form.name, form.name_problem, |name| AccountsMsg::DraftName { name }),
                            option_picker(
                                "Type",
                                form.kind_key,
                                |key| AccountsMsg::DraftKindKey { key },
                                for k in form.kinds {
                                    picker_choice(k.title, k.glyph, k.key)
                                },
                            ),
                        )),
                        form_section(amount_entry(
                            form.balance_label,
                            form.balance_prompt,
                            form.typed,
                            form.cents,
                            form.balance_problem,
                            |typed| AccountsMsg::DraftBalance { typed },
                            |cents| AccountsMsg::DraftCents { cents },
                        ))
                        .header(form.balance_header),
                        form_section(switch("In the Budget", form.on_budget, |on_budget| AccountsMsg::DraftOnBudget {
                            on_budget,
                        }))
                        .footer(form.on_budget_note),
                    ),
                )
                .confirm("Add Account", form.can_save, AccountsMsg::SaveDraft)
            },
            if let Some(r) = p.renaming {
                prompt(
                    "Rename Account",
                    r.name,
                    |name| AccountsMsg::RenameTyped { name },
                    "Rename",
                    AccountsMsg::SaveRename,
                    AccountsMsg::CancelRename,
                )
            },
        ),
    )
    .refresh(AccountsMsg::Refresh)
    .into()
}

/// One account: its kind's mark, its name and what feeds it, how many wait
/// for a look, and its balance; tapped, its register. Held (right-clicked
/// on the desktop), it is renamed, reconciled while open, its balance
/// copied, or it is closed or reopened.
#[view]
pub fn account_row(a: &AccountItem) -> View<AccountsMsg> {
    list_row(a.name)
        .subtitle(a.subtitle)
        .mark(a.glyph, a.kind_key)
        .badge(a.reviews)
        .amount(a.balance)
        .said(a.said)
        .pushes()
        .on_tap(AccountsMsg::OpenAccount { id: a.id })
        .menu_fixed("Rename", "pencil", AccountsMsg::Rename { id: a.id })
        .menu_when("Reconcile…", "badge-check", AccountsMsg::Reconcile { id: a.id }, a.is_open)
        .menu_fixed("Copy Balance", "copy", AccountsMsg::CopyBalance { id: a.id })
        .menu_glyph(a.close_title, a.close_glyph, AccountsMsg::ToggleClosed { id: a.id }, a.is_open)
        .into()
}
