//! money for its phones (`docs/plan-v5.md` §1.16, N3), under the crate's
//! `ffi` feature: the bridge (`ark-ffi`) with money's domain and its
//! screens' runtime, exported through UniFFI. The phones' library is no
//! crate of money's: nix generates it (nix/ark-app.nix, `money-ffi`), a
//! shim that links this crate as a static library for the iPhone and the
//! Mac and a shared one for Android. The apps open their replica with
//! [`money_open`] — or the demo's household with [`money_demo`] — and
//! send every change of the domain's through it as a `MoneyChange`
//! ([`Session::send`]). Every view hands over UniFFI's records, and the
//! records carry the domain's rules as methods (`money_core`'s `ffi`
//! feature): nothing crosses as JSON.
//!
//! Signing in is `ark_auth::onboarding`'s machine, through the bridge: the server's page
//! (`Effect::Browse`, redirecting to `money://auth`), the code exchanged
//! (`exchange`), the peer told (`Peer::login`) and dialled (`socket_url`,
//! `Peer::connect`). A refusal is a value wherever the app can ask before
//! it acts (`Peer::check`) or after (`Peer::standing`, `take_rejections`);
//! [`money_send`] is a mutation whose own refusal is one too. A [`Hall`] is
//! a money server in the same process, over memory, that devices dial
//! without a socket: what the apps' tests sync through.
//!
//! The session's views ([`MoneyView`], [`Budget`], [`Holding`]) are what
//! the screens still hand-written in money/native read; they go as those
//! screens move into this crate's views.

use std::sync::Arc;

use ark_ffi::{Login, Peer, Refusal};
use ark_peer::Config;

/// Numbers, days and lengths as every client writes them: in this library,
/// so its bindings (`uniffi.ark_format`; `formatAmount`, `formatLength` …)
/// come with the app's, and the phones print what the desktop prints.
pub use ark_format;

mod app;
mod demo;
mod hall;
#[cfg(feature = "page")]
mod page;
mod session;
pub use app::{AccountsApp, AppShell, ShellApp};
pub use demo::{money_demo, DemoBank, DemoSync};
pub use hall::Hall;
#[cfg(feature = "page")]
pub use page::money_page_corners;
pub use session::{Budget, Changed, Holding, MoneyView, Session};

/// money's peer as a phone is made of it: the domain for the log, and no
/// live space — a budget has nothing but what was said.
pub fn config() -> Config {
    Config::of(money_core::shared())
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
    let d = money_core::shared();
    format!("{} {}", d.schema_hash(), d.version)
}

/// What became of a mutation as the device sent it: its intent, whose
/// standing the app then follows, or the domain's own sentence for why not.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Sent {
    Intent { id: String },
    Refused { why: String },
}

/// A change sent over `peer` with no session, its refusal a value: the
/// tests', and anything that sends before it holds a view.
#[uniffi::export]
pub fn money_send(peer: Arc<Peer>, change: <money_core::MoneyChange as ark_ffi::Twin<money_core::UniFfiTag>>::Ffi) -> Sent {
    let c = <money_core::MoneyChange as ark_ffi::Twin<money_core::UniFfiTag>>::from_ffi(change);
    match peer.mutate_encoded(c.name(), c.input()) {
        Ok(id) => Sent::Intent { id },
        Err(Refusal::Refused { why }) => Sent::Refused { why },
    }
}

/// Today, as the budget writes a day (`2026-10-26`), in UTC as the domain
/// reads one.
#[uniffi::export]
pub fn money_today() -> String {
    money_core::dates::day_of_ms(ark_peer::now_ms())
}

/// The emoji the phones draw before a category's name, by the words in
/// it ([`crate::look::category_emoji`]); empty when none says.
#[uniffi::export]
pub fn money_category_emoji(name: String) -> String {
    crate::look::category_emoji(&name).into()
}

/// The month `n` months from `month` (`2026-10`), either way.
#[uniffi::export]
pub fn money_add_months(month: String, n: i64) -> String {
    money_core::dates::add_months(&month, n)
}
