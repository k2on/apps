//! Who an ArkDB peer is.
//!
//! The engine asks one question about identity: at every `Hello`, what does
//! this token prove? This crate signs people in for a server — with OpenID
//! Connect, or in dev mode by name — and mints the token; the session it
//! stands for is a row of the app's log ([`rows`]: the tables, the
//! functions over them and the scope an app's schema takes in), which the
//! hub writes and judges by ([`server::Auth`], plugged in by `ark-peer`'s
//! `Server::auth`). The other half is how a client gets a token to send,
//! for a desktop program and a page.
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
//! the session's id and the server's HMAC of it (`server::Auth::mint`):
//! the server keeps nothing but its key.
//!
//! # A session is a login on one device
//!
//! The session id is what every entry authored under it carries
//! (`ctx.session`), and what a live room names a device by. It is a row
//! ([`SignIn`]): who it is, the device, when it expires, whether it was
//! revoked — which its owner does through [`rows::revoke`], and the hub
//! closes its connections. Entries authored under it stay its
//! owner's.
//!
//! # Without a provider
//!
//! [`server::Mode::Dev`] signs anyone in as whatever name they give — a
//! laptop running two peers named alice and bob. A server has to be told
//! to run that way, and says so at startup ([`server::Mode::announce`]).

mod login;
mod util;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

#[cfg(not(target_arch = "wasm32"))]
pub mod client;
pub mod device_accounts;
pub mod first_run;
pub mod keep;
#[cfg(not(target_arch = "wasm32"))]
pub mod oidc;
pub mod onboarding;
pub mod remember;
/// The two tables and the functions over them are the kernel's
/// (`ark::auth`): a domain embeds them without depending on sign-in.
pub use ark::auth as rows;
pub use ark::auth_tables as install;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use login::{Account, Login};
pub use rows::{feed, register, AuthTables, Grant, Revoke, Role, SignIn, Welcome, ADMIN};
pub use util::{cookie_token, login_url, percent_decode, percent_encode, picture_url, query_value, socket_url, with_code, COOKIE, COOKIE_HELLO};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
