//! money's library for its phones (`docs/plan-v5.md` §1.16, N3): the
//! bridge (`ark-ffi`) with money's domain linked in. The apps load this one
//! shared library, open their replica with [`money_open`] — or the demo's
//! household with [`money_demo`] — and call every function of the domain by
//! name through it, typed by what `arkdb gen` writes from the domain. The
//! views a budget is drawn from (accounts, the month's lines and summary, a
//! register, what needs a look, payees, banks, spending) hand over UniFFI's
//! records, no JSON per row (`money_domain`'s `ffi` feature).
//!
//! Signing in is the bridge's, as harken's phones do it: the server's page
//! (`login_url`, redirecting to `money://auth`), the code exchanged
//! (`exchange`), the peer told (`Peer::login`) and dialled (`socket_url`,
//! `Peer::connect`). A refusal is a value wherever the app can ask before
//! it acts (`Peer::check`) or after (`Peer::standing`, `take_rejections`);
//! [`money_send`] is a mutation whose own refusal is one too. A [`Hall`] is
//! a money server in the same process, over memory, that devices dial
//! without a socket: what the apps' tests sync through.

use std::sync::Arc;

use ark_ffi::{Login, Peer, Refusal};
use ark_peer::Config;

uniffi::setup_scaffolding!();

mod demo;
mod hall;
pub use demo::{money_demo, DemoBank, DemoSync};
pub use hall::Hall;

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
    let d = money_domain::shared();
    format!("{} {}", d.schema_hash(), d.version)
}

/// What became of a mutation as the device sent it: its intent, whose
/// standing the app then follows, or the domain's own sentence for why not.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Sent {
    Intent { id: String },
    Refused { why: String },
}

/// The function `name` on its input's JSON, as `Peer::mutate` does it, its
/// refusal a value: what a form's button calls when it says the sentence
/// under the field rather than catching.
#[uniffi::export]
pub fn money_send(peer: Arc<Peer>, name: String, input: String) -> Sent {
    match peer.mutate(name, input) {
        Ok(id) => Sent::Intent { id },
        Err(Refusal::Refused { why }) => Sent::Refused { why },
    }
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
