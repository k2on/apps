//! harken's domain in plain Rust (`docs/plan-v5.md` §1.16): the schema
//! ([`schema`]), the types ([`types`]), the library's keys ([`keys`]), the
//! library ([`library`], [`browse`]), the playlists ([`playlists`]), the
//! plays ([`plays`]) and the listening session ([`listening`], [`queue`],
//! [`devices`], [`groups`]), who is signed in ([`session`]) — one file per part — and what a host knows of
//! it beside its functions ([`host`]). [`domain`] registers every function
//! by its name; the apps link this crate and call its types and functions
//! as they are.

use std::sync::{Arc, OnceLock};

use ark::domain::Domain;

pub mod browse;
pub mod devices;
pub mod groups;
pub mod host;
pub mod keys;
pub mod library;
pub mod listening;
pub mod playlists;
pub mod plays;
pub mod queue;
pub mod schema;
pub mod session;
pub mod types;

pub use ark::row::Id;
pub use schema::*;
pub use types::*;

/// The library's role: the scanner's account holds it, and nobody else
/// unless the server's configuration grants it. Every library mutation is
/// guarded by it ([`library::is_library`]).
pub const LIBRARY: &str = "library";

/// This domain's version, which a `Hello` carries beside its schema's hash.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every function, registered by its name.
pub fn domain() -> Domain {
    ark::register!(Domain::new::<Harken>("harken", VERSION);
        library::add_song, library::describe_work, library::describe_recording, library::describe_person,
        library::credit_recording, library::remove_media,
        browse::library, browse::search, browse::albums, browse::artists, browse::track_details, browse::album,
        browse::artist, browse::composers, browse::works, browse::work, browse::recordings, browse::credits,
        browse::recording,
        playlists::create_playlist, playlists::add_to_playlist, playlists::add_all_to_playlist,
        playlists::remove_from_playlist, playlists::playlists, playlists::playlists_of, playlists::playlist,
        plays::record_play, plays::last_played, plays::recently_played,
        listening::start, listening::play, listening::pause, listening::seek, listening::skip, listening::jump,
        listening::transfer, listening::stop,
        queue::enqueue, queue::remove, queue::move_track, queue::clear_upcoming, queue::started, queue::advance,
        queue::stalled, queue::report_position, queue::release,
        devices::equip, devices::set_volume, devices::nudge_volume, devices::set_mute, devices::report_volume,
        devices::scrub, devices::on_join, devices::on_leave, devices::on_restore,
        groups::join, groups::unjoin, groups::report_group, groups::set_group_volume, groups::nudge_group_volume,
        groups::set_group_mute,
        session::revoke, session::grant, session::revoke_role, session::current, session::mine, session::feed)
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}
