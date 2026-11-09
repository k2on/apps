//! The Accounts tab laid out, once: drawn by iced and the browser from
//! this function, written out as SwiftUI and Compose from its recording
//! (`nix run .#update-ui`).

use ark_app::view;
use ark_app::view::*;

use super::{AccountItem, AccountsMsg, AccountsPage};

/// The tab: what the household is worth, All Accounts, the shelves of
/// accounts, Bank Connections and Add Account; Add Account's sheet and
/// the rename alert over it while they are up.
#[view]
pub fn accounts(p: &AccountsPage) -> View<AccountsMsg> {
    tab_page(
        "Accounts",
        (
            card_section(
                list_row("All Accounts")
                    .tile("layers")
                    .amount(p.all_total)
                    .quiet()
                    .on_tap(AccountsMsg::OpenAll),
            ),
            for shelf in p.shelves {
                card_section(for a in shelf.accounts {
                    account_row(a)
                })
                .header(shelf.title)
                .amount(shelf.total)
                .shows_amount(shelf.shows_total)
            },
            card_section((
                list_row(p.banks_title).glyph("link").on_tap(AccountsMsg::OpenBanks),
                list_row("Add Account").glyph("plus").accent().on_tap(AccountsMsg::Add),
            ))
            .footer(p.synced),
            if let Some(form) = p.adding {
                sheet(
                    "Add Account",
                    AccountsMsg::DismissDraft,
                    (
                        name_field("Name", form.name, form.name_problem, |name| AccountsMsg::DraftName { name }),
                        chip_row(for k in form.kinds {
                            chip(k.title, k.glyph, k.selected, AccountsMsg::DraftKindKey { key: k.key })
                        }),
                        amount_field(form.balance_label, form.typed, form.balance_problem, |typed| AccountsMsg::DraftBalance {
                            typed,
                        }),
                        switch_row("In the Budget", form.on_budget_note, form.on_budget, |on_budget| {
                            AccountsMsg::DraftOnBudget { on_budget }
                        }),
                        form_button("Add Account", form.can_save, AccountsMsg::SaveDraft),
                    ),
                )
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
    .top(hero_figure("Net Worth", p.net_worth, "Every open account, less what's owed"))
    .refresh(AccountsMsg::Refresh)
    .into()
}

/// One account: its kind's mark, its name and what feeds it, how many wait
/// for a look, and its balance. Held, it is renamed, closed or reopened,
/// or its balance copied.
#[view]
pub fn account_row(a: &AccountItem) -> View<AccountsMsg> {
    list_row(a.name)
        .subtitle(a.subtitle)
        .mark(a.glyph, a.kind_key)
        .badge(a.reviews)
        .amount(a.balance)
        .said(a.said)
        .on_tap(AccountsMsg::OpenAccount { id: a.id })
        .menu_fixed("Rename", "pencil", AccountsMsg::Rename { id: a.id })
        .menu_fixed("Copy Balance", "copy", AccountsMsg::CopyBalance { id: a.id })
        .menu(a.close_title, "archive", AccountsMsg::ToggleClosed { id: a.id }, a.is_open)
        .into()
}
