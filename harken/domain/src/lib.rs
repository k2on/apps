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

/// This domain's version, which a `Hello` carries beside its schema's hash.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every function, registered by its name.
pub fn domain() -> Domain {
    ark::register!(Domain::new::<Harken>("harken", VERSION);
        library::add_song, library::describe_work, library::describe_recording, library::describe_person,
        library::credit_recording, library::remove_media, library::look,
        browse::library, browse::search, browse::albums, browse::artists, browse::track_details, browse::album,
        browse::artist, browse::composers, browse::works, browse::work, browse::recordings, browse::credits, browse::album_credits,
        browse::recording,
        playlists::create_playlist, playlists::welcome, playlists::add_to_playlist, playlists::add_all_to_playlist,
        playlists::remove_from_playlist, playlists::playlists, playlists::playlists_of, playlists::playlist,
        plays::record_play, plays::last_played, plays::recently_played,
        listening::start, listening::play, listening::pause, listening::seek, listening::skip, listening::jump,
        listening::transfer, listening::stop,
        queue::enqueue, queue::remove, queue::move_track, queue::clear_upcoming, queue::started, queue::advance,
        queue::stalled, queue::report_position, queue::release,
        devices::equip, devices::set_volume, devices::nudge_volume, devices::set_mute, devices::report_volume,
        devices::scrub, devices::on_join, devices::on_leave, devices::on_restore,
        groups::join, groups::unjoin, groups::report_group, groups::set_group_volume, groups::nudge_group_volume,
        groups::set_group_mute, shows::add_show, shows::add_episode, shows::shows, shows::show,
        shows::episode_details, shows::subscribe, shows::unsubscribe, shows::fetch_older, shows::feed_read,
        shows::subscriptions, downloads::queue_download, downloads::download_progress, downloads::downloaded,
        downloads::downloads, progress::report_progress, progress::mark_played, progress::my_progress,
        progress::show_progress, shows::set_chapters, shows::chapters, shows::set_people, shows::people,
        shows::appearances, shows::set_transcript, shows::transcript, feed)
    .with(ark_auth::register::<Harken>)
    .with(<Look as ark::effect::Effect>::register::<Harken>)
}

/// What a connection fed by scope holds (§1.8): every durable table whole
/// and, of the sign-ins and the roles, its person's.
#[scope("session.feed")]
pub fn feed() -> Scope<Harken> {
    fn all<R>(_: &Ctx, _: &R) -> bool {
        true
    }
    ark_auth::feed(
        Scope::new()
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
            .on::<Cue>(all),
    )
}

/// [`domain`], once.
pub fn shared() -> Arc<Domain> {
    static D: OnceLock<Arc<Domain>> = OnceLock::new();
    D.get_or_init(|| Arc::new(domain())).clone()
}
