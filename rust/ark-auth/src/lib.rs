//! Who an ArkDB peer is.
//!
//! The engine asks one question about identity: at every `Hello`, what does
//! this token prove? (`ark::protocol::Authenticate`.) This crate is the
//! answer for a server that signs people in with OpenID Connect, and the
//! other half — how a client gets a token to send — for a desktop program
//! and a page. It is `petros-auth` on ArkDB, with the session store in a
//! file instead of SQLite.
//!
//! # The shape
//!
//! The server is the only OpenID Connect client. It holds the secret, talks
//! to the provider, and hands each signed-in peer a session token of its
//! own; no client speaks OpenID Connect or holds a secret, and all of them
//! sign in by opening one URL and receiving one code.
//!
//! ```text
//! client                          server                       provider
//!   |-- GET /auth/login?redirect=R -->|                            |
//!   |<-- 302 ------------------------|-- authorize?state&nonce -->|
//!   |            (the person signs in at the provider)           |
//!   |                                |<-- callback?code&state ----|
//!   |                                |-- token(code, secret) ---->|
//!   |<-- 302 R?code=C ---------------|<-- id_token ---------------|
//!   |-- POST /auth/exchange {C} ---->|                            |
//!   |<-- {token, user, session} -----|                            |
//!   |-- Hello { token } ------------>|  (the engine, from here)   |
//! ```
//!
//! `R` is where the code goes back to: a loopback port the desktop listens
//! on, the page's own origin, or an app's URL scheme; the server sends a
//! code nowhere else. `C` is single-use and lives a minute. The token is
//! what the client keeps; the server keeps its hash.
//!
//! # A session is a login on one device
//!
//! The session id is what every entry authored under it carries
//! (`ctx.session`), and what a live room names a device by. Revoking one
//! (`/auth/logout`) ends its token; entries authored under it stay its
//! owner's.
//!
//! # Without a provider
//!
//! [`server::Mode::Dev`] signs anyone in as whatever name they give — a
//! laptop running two peers named alice and bob. A server has to be told
//! to run that way, and says so at startup ([`server::Mode::announce`]).

mod login;
mod util;

#[cfg(feature = "server")]
pub mod oidc;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod session;

#[cfg(all(feature = "client", not(target_arch = "wasm32")))]
pub mod client;
#[cfg(feature = "client")]
pub mod remember;
#[cfg(all(feature = "client", target_arch = "wasm32"))]
pub mod web;

pub use login::{Account, Login};
pub use util::{login_url, percent_decode, percent_encode, query_value, socket_url, with_code};
