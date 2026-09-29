#![deny(unsafe_code)]
//! A Rust peer on ArkDB, for any app's module.
//!
//! ```text
//! Peer ─ the Replica of the log, persisted (a directory, localStorage, memory)
//!      ─ ark::protocol::Client ── Link ── Transport (tungstenite thread | browser WebSocket)
//!      ─ an Authority when alone (no server)
//! ```
//!
//! A peer is opened over a [`Domain`] — an app's authored module and its
//! procedures, run natively — and a [`storage::Storage`]. A screen calls
//! [`Peer::mutate`] and [`Peer::query`] by name, holds lists as
//! [`View`]s, and calls [`Peer::pump`] on a tick; the pump dials, moves
//! frames, reconnects with a backoff, and writes whatever moved. Live rooms
//! ride the same socket: [`Peer::say`] and [`Peer::heard`].
//!
//! ```no_run
//! use ark_client::{args, demo, Options, Peer};
//! use ark::value::Value;
//!
//! let mut peer = Peer::open_memory(demo::domain(), Options::dev("alice")).unwrap();
//! peer.connect("ws://127.0.0.1:8787/sync");
//! peer.mutate("create_playlist", args([("name", Value::text("Road trip"))])).unwrap();
//! loop {
//!     peer.pump();
//!     std::thread::sleep(std::time::Duration::from_millis(50));
//! }
//! ```
//!
//! The README maps every call harken's iced client made on petros to this.

pub mod autos;
pub mod demo;
mod domain;
pub mod link;
mod peer;
pub mod storage;
mod view;

pub use autos::Autos;
pub use domain::Domain;
pub use link::Timing;
pub use peer::{refusal_text, Options, Peer, Pumped, Rejection, Standing, Status};
pub use view::{splice, Update, View};

pub use ark;
pub use ark::eval::{Args, Checked};
pub use ark::peer::Changes;
pub use ark::value::{Id, Value};
pub use ark::view::Patch;

/// Why a call did not do what it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownFunction(String),
    /// The function exists and is not this kind.
    NotA(String, &'static str),
    /// A verdict: a check, a guard, a constraint, a `refuse`. Nothing changed.
    Refused(ark::store::Refusal),
    /// A fault in the domain or the engine, never a verdict.
    Bug(String),
    Storage(String),
    Corrupt(String),
    /// The storage was last opened the other way — alone, or with a server —
    /// and its sequences mean something else under this one.
    ModeMismatch {
        was: String,
        now: String,
    },
    /// Signing in on a peer that is its own authority: there is no server
    /// to sign in to.
    Alone,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnknownFunction(n) => write!(f, "the module has no function {n}"),
            Error::NotA(n, k) => write!(f, "{n} is not a {k}"),
            Error::Refused(r) => write!(f, "{}", refusal_text(r)),
            Error::Bug(s) => write!(f, "bug: {s}"),
            Error::Storage(s) => write!(f, "storage: {s}"),
            Error::Corrupt(s) => write!(f, "corrupt: {s}"),
            Error::ModeMismatch { was, now } => write!(f, "the storage was opened {was} before and {now} now"),
            Error::Alone => write!(f, "a peer alone has no server to sign in to"),
        }
    }
}

impl std::error::Error for Error {}

/// The arguments of a call, from pairs: `args([("name", Value::text("x"))])`.
pub fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
