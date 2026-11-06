//! harken's domain in plain Rust (`docs/plan-v5.md` §1.16): the schema
//! ([`schema`]), the types ([`types`]), the library's keys ([`keys`]), the
//! library ([`library`], [`browse`]), the playlists ([`playlists`]), the
//! plays ([`plays`]), the shows and the feeds followed ([`shows`]), what
//! is still to be downloaded ([`downloads`]), how far each person has
//! listened to each episode ([`progress`]) and the listening session
//! ([`listening`], [`queue`], [`devices`], [`groups`]) — one file per part
//! — and what a host knows of
//! it beside its functions ([`host`]). Who is signed in is `ark-auth`'s
//! two tables and functions ([`ark_auth::rows`]), and [`feed`]. [`domain`] registers every function
//! by its name; the apps link this crate and call its types and functions
//! as they are.

use std::sync::{Arc, OnceLock};

use ark::domain::{Domain, Scope};
use ark::peer::Ctx;
use ark::prelude::scope;

pub mod browse;
pub mod catalogue;
pub mod devices;
pub mod downloads;
pub mod groups;
pub mod host;
pub mod keys;
pub mod library;
pub mod listening;
pub mod migrations;
pub mod playlists;
pub mod plays;
pub mod progress;
pub mod queue;
pub mod schema;
pub mod shows;
pub mod types;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub use ark::row::Id;
pub use schema::*;
pub use types::*;

/// The library's role: the scanner's account holds it, and nobody else
/// unless the server's configuration grants it. Every library mutation is
/// guarded by it ([`library::is_library`]).
pub const LIBRARY: &str = "library";

/// This domain's version: what a build says it runs, beside its schema's hash.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every function this crate declares, registered by its name.
pub fn domain() -> Domain {
    ark::functions!(Domain::new::<Harken>("harken", VERSION))
        .with(ark_auth::register::<Harken>)
        .with(<Look as ark::effect::Effect>::register::<Harken>)
}

/// What a connection fed by scope holds (§1.8): every durable table whole;
/// of the sign-ins and the roles, its person's; and of the live space
/// (§1.9), which is fed by the same scope, its person's listening session,
/// devices and scrubbers.
#[scope("session.feed")]
pub fn feed() -> Scope<Harken> {
    fn all<R>(_: &Ctx, _: &R) -> bool {
        true
    }
    let theirs = Scope::new()
        .on::<Listening>(|ctx, l| l.user == ctx.user())
        .on::<ListeningDevice>(|ctx, d| d.user == ctx.user())
        .on::<ListeningScrub>(|ctx, s| s.user == ctx.user());
    ark_auth::feed(
        theirs
            .on::<Media>(all)
            .on::<Album>(all)
            .on::<Person>(all)
            .on::<Work>(all)
            .on::<Movement>(all)
            .on::<Recording>(all)
            .on::<Credit>(all)
            .on::<Song>(all)
            .on::<Playlist>(all)
            .on::<PlaylistItem>(all)
            .on::<Play>(all)
            .on::<Show>(all)
            .on::<Episode>(all)
            .on::<Subscription>(all)
            .on::<Download>(all)
            .on::<Progress>(all)
            .on::<Chapter>(all)
            .on::<Appearance>(all)
            .on::<Cue>(all)
            .on::<Voice>(all)
            .on::<VoiceName>(all),
    )
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}
