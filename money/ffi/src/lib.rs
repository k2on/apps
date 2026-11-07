//! money's library for its phones (`docs/plan-v5.md` §1.16, N3): the
//! bridge (`ark-ffi`) with money's domain linked in. The apps load this one
//! shared library, open their replica with [`money_open`] — or the demo's
//! household with [`money_demo`] — and call every function of the domain by
//! name through it, typed by what `arkdb gen` writes from the domain: a
//! view's rows cross as their bytes, which the generated types read.
//!
//! Signing in is the bridge's, as harken's phones do it: the server's page
//! (`login_url`, redirecting to `money://auth`), the code exchanged
//! (`exchange`), the peer told (`Peer::login`) and dialled
//! (`Peer::connect`). A refusal is a value wherever the app can ask before
//! it acts (`Peer::check`) or after (`Peer::outcome`, `take_refusals`),
//! and a mutation's own is the `Refusal` it throws. A [`Hall`] is
//! a money server in the same process, over memory, that devices dial
//! without a socket: what the apps' tests sync through.

use std::sync::Arc;

use ark_ffi::{Login, Peer, Refusal};
use ark_peer::Config;

uniffi::setup_scaffolding!();

mod demo;
mod hall;
#[cfg(feature = "page")]
mod page;
pub use demo::{money_demo, DemoBank, DemoSync};
pub use hall::Hall;
#[cfg(feature = "page")]
pub use page::money_page_corners;

/// money's peer as a phone is made of it: the domain for the log, and no
/// live space — a budget has nothing but what was said.
pub fn config() -> Config {
    Config::of(money_domain::shared())
}

/// money's peer on the disk at `dir`, as whoever last signed in there or
/// as `login`.
#[uniffi::export]
pub fn money_open(dir: String, login: Option<Login>) -> Result<Arc<Peer>, Refusal> {
    Peer::open(config(), &dir, login)
}

/// The domain this library carries as a server's `/healthz` names it: its
/// schema's hash and its version (the server's `domain` line).
#[uniffi::export]
pub fn money_domain() -> String {
    ark_peer::version(&money_domain::shared())
}

/// Today, as the budget writes a day (`2026-10-26`), in UTC as the domain
/// reads one.
#[uniffi::export]
pub fn money_today() -> String {
    money_domain::dates::day_of_ms(ark_peer::now_ms())
}

/// The month `n` months from `month` (`2026-10`), either way.
#[uniffi::export]
pub fn money_add_months(month: String, n: i64) -> String {
    money_domain::dates::add_months(&month, n)
}

/// A month as a heading says it: `October 2026`.
#[uniffi::export]
pub fn money_month_name(month: String) -> String {
    money_domain::dates::month_name(&month)
}
