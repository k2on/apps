//! money's domain in plain Rust (`docs/plan-v5.md` §1.16): a household's
//! budget. The schema ([`schema`]), the types ([`types`]), days and months
//! ([`dates`]); the accounts ([`accounts`]), the budget's shape
//! ([`categories`]), the budget itself ([`budget`]), its assignments by
//! formula ([`formulas`]) and its credit cards
//! ([`cards`]), the transactions ([`transactions`]), the filters that say
//! which ([`filters`]), and transfers made of
//! them ([`transfers`]), and the banks, through Plaid ([`plaid`]), connected
//! from the app ([`banks`]) — one file
//! per part. Who is signed in is `ark-auth`'s two tables and functions, and
//! its scope. [`domain`] registers every function by its name. The demo's
//! household is beside the app, not in it: `money/seed`.
//!
//! **One budget, the household's.** Everybody a server lets in — the
//! people its OpenID provider knows as the household — reads and writes the
//! same accounts, categories and transactions: no table is anybody's, and
//! every connection fed by scope (`session.feed`) is fed every row but the
//! sign-ins and roles of others, the only rows that say their owner
//! (`#[ark(owner = user)]`). A row's `user_id` says who
//! wrote it, for the register to show, and decides nothing.

use std::sync::{Arc, OnceLock};

use ark::domain::Domain;

pub mod accounts;
pub mod banks;
pub mod budget;
pub mod cards;
pub mod categories;
pub mod dates;
pub mod filters;
pub mod formulas;
pub mod migrations;
pub mod plaid;
pub mod receipts;
pub mod schema;
pub mod setup;
pub mod stores;
pub mod transactions;
pub mod transfers;
pub mod types;

pub use ark::row::Id;
pub use schema::*;
pub use types::*;

/// The bank importer's role: a server's Plaid importer holds it, and
/// nobody else unless the server's configuration grants it. Only
/// [`plaid::import`] asks for it.
pub const BANK: &str = "bank";

/// This domain's version: what a build says it runs, beside its schema's hash.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every function this crate declares, registered by its name.
pub fn domain() -> Domain {
    ark::functions!(Domain::new::<Money>("money", VERSION))
        .with(ark_auth::register::<Money>)
        .with(<Photo as ark::effect::Effect>::register::<Money>)
        .with(<Asking as ark::effect::Effect>::register::<Money>)
        .with(<Syncing as ark::effect::Effect>::register::<Money>)
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}
