//! money's domain in plain Rust (`docs/plan-v5.md` §1.16): a household's
//! budget. The schema ([`schema`]), the types ([`types`]), days and months
//! ([`dates`]); the accounts ([`accounts`]), the budget's shape
//! ([`categories`]), the budget itself ([`budget`]), the transactions
//! ([`transactions`]) and the banks, through Plaid ([`plaid`]) — one file
//! per part. Who is signed in is `ark-auth`'s two tables and functions, and
//! [`feed`]. [`domain`] registers every function by its name. Behind
//! features: the demo's household (`seed`), and the rows the
//! phones read typed as UniFFI records (`ffi`, for `money/ffi`).
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
pub mod budget;
pub mod categories;
pub mod dates;
pub mod plaid;
pub mod schema;
#[cfg(feature = "seed")]
pub mod seed;
pub mod transactions;
pub mod types;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub use ark::row::Id;
pub use schema::*;
pub use types::*;

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
        categories::add_category, categories::rename_category, categories::hide_category, categories::move_category,
        categories::set_target, categories::delete_category,
        accounts::open_account, accounts::rename_account, accounts::close_account, accounts::accounts,
        transactions::add_transaction, transactions::edit_transaction, transactions::delete_transactions,
        transactions::approve, transactions::categorize, transactions::set_cleared, transactions::reconcile,
        transactions::register, transactions::inbox, transactions::payees,
        plaid::import, plaid::link, plaid::ignore, plaid::banks, feed)
    .with(ark_auth::register::<Money>)
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
            .on::<Group>(all)
            .on::<Category>(all)
            .on::<Assigned>(all)
            .on::<Txn>(all)
            .on::<Bank>(all)
            .on::<Linked>(all),
    )
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}
