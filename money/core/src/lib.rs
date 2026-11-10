//! money's core in plain Rust (`docs/plan-v5.md` §1.16): the domain of a
//! household's budget. The schema ([`schema`]), the types ([`types`]), days and months
//! ([`dates`]); the accounts ([`accounts`]), the budget's shape
//! ([`categories`]), the budget itself ([`budget`]), its assignments by
//! formula ([`formulas`]) and its credit cards
//! ([`cards`]), the transactions ([`transactions`]), the filters that say
//! which ([`filters`]), and transfers made of
//! them ([`transfers`]), and the banks, through Plaid ([`plaid`]), connected
//! from the app ([`banks`]) — one file
//! per part; every view an app opens, and what a page is made of
//! ([`views`]). Who is signed in is `ark-auth`'s two tables and functions, and
//! [`feed`]. [`domain`] registers every function by its name. Behind a
//! feature: the rows the phones read typed as UniFFI records (`ffi`, for
//! the phones' library), and the demo's household ([`demo`], `demo`; and
//! `money-seed`, which writes it into a server's data directory, `cli`),
//! which an app or a server carries only when it asks for it.
//!
//! **One budget, the household's.** Everybody a server lets in — the
//! people its OpenID provider knows as the household — reads and writes the
//! same accounts, categories and transactions: no table is anybody's, and
//! every connection is fed every row ([`feed`]). A row's `user_id` says who
//! wrote it, for the register to show, and decides nothing.

use std::sync::{Arc, OnceLock};

use ark::domain::{Domain, Scope};
use ark::peer::Ctx;
use ark::prelude::scope;

pub mod accounts;
pub mod banks;
pub mod budget;
pub mod cards;
pub mod categories;
pub mod changes;
pub mod dates;
#[cfg(feature = "demo")]
pub mod demo;
#[cfg(feature = "ffi")]
mod ffi;
pub mod filters;
pub mod formulas;
pub mod migrations;
pub mod plaid;
pub mod receipts;
pub mod routes;
pub mod schema;
pub mod setup;
pub mod stores;
pub mod transactions;
pub mod transfers;
pub mod types;
pub mod views;
pub mod words;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub use ark::row::Id;
pub use changes::MoneyChange;
pub use schema::*;
pub use types::*;
pub use views::MoneyView;

/// The bank importer's role: a server's Plaid importer holds it, and
/// nobody else unless the server's configuration grants it. Only
/// [`plaid::import`] asks for it.
pub const BANK: &str = "bank";

/// This domain's version, which a `Hello` carries beside its schema's hash.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every function, registered by its name.
pub fn domain() -> Domain {
    ark::register!(Domain::new::<Money>("money", VERSION);
        budget::setup, budget::assign, budget::move_money, budget::auto_assign,
        budget::groups, budget::month, budget::summary, budget::spending,
        categories::add_group, categories::rename_group, categories::hide_group, categories::delete_group,
        categories::add_category, categories::rename_category, categories::hide_category, categories::move_category, categories::move_group,
        categories::set_target, categories::delete_category, categories::add_subcategory, categories::tree,
        formulas::set_formula, formulas::clear_formula, formulas::formulas, formulas::preview_formula, formulas::set_rule,
        filters::save_filter, filters::rename_filter, filters::change_filter, filters::delete_filter, filters::move_filter,
        filters::saved, filters::filtered, filters::filtered_total,
        accounts::open_account, accounts::rename_account, accounts::close_account, accounts::accounts,
        transactions::add_transaction, transactions::edit_transaction, transactions::delete_transactions,
        transactions::approve, transactions::categorize, transactions::set_cleared, transactions::reconcile,
        transactions::register, transactions::inbox, transactions::payees, transactions::split, transactions::add_split, transactions::splits,
        receipts::add_receipts, receipts::file_items, receipts::pay, receipts::receipts, receipts::items, receipts::receipt_items,
        receipts::add_charges, receipts::pay_charge, receipts::charges, receipts::candidates, receipts::add_photo, receipts::photos, receipts::write_receipt, receipts::for_txn,
        plaid::import, plaid::link, plaid::open, plaid::ignore, plaid::sync, plaid::banks, plaid::linked, plaid::retired,
        banks::connect, banks::reconnect, banks::add_accounts, banks::disconnect, banks::answer, banks::failed, banks::asks, feed)
    .with(ark_auth::register::<Money>)
    .with(<Photo as ark::effect::Effect>::register::<Money>)
    .with(|d| migrations::all().into_iter().fold(d, Domain::migration))
}

/// What a connection fed by scope holds (§1.8): the budget whole — it is
/// the household's — and, of the sign-ins and the roles, its person's.
#[scope("session.feed")]
pub fn feed() -> Scope<Money> {
    fn all<R>(_: &Ctx, _: &R) -> bool {
        true
    }
    ark_auth::feed(
        Scope::new()
            .on::<Account>(all)
            .on::<Category>(all)
            .on::<Assigned>(all)
            .on::<Txn>(all)
            .on::<Bank>(all)
            .on::<Linked>(all)
            .on::<Asked>(all)
            .on::<BankAsk>(all)
            .on::<Rule>(all)
            .on::<SavedFilter>(all)
            .on::<Formula>(all)
            .on::<Split>(all)
            .on::<Receipt>(all)
            .on::<Item>(all)
            .on::<Charge>(all)
            .on::<Photo>(all),
    )
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}
