//! harken as an Ark app (ui/docs/app.md, "harken's Port"): a screen's
//! state, the messages that change it, its update and its `#[view]`, in
//! Rust, run by `ark_app::Runtime` on every platform; where harken can be
//! ([`routes`]), which every shell spells the same way; and what it says
//! on its first run ([`welcome`]).
//!
//! The screens moved here so far are the library's: Home ([`home`]),
//! Library ([`library`]) and a record's page ([`page`]). The player, the
//! search, the sheets, the shell and the host's part follow
//! (ui/docs/app.md, "Where It Stands").

pub mod library;
pub mod routes;
pub mod welcome;
pub mod words;

#[cfg(test)]
pub(crate) mod testing;

pub use harken_domain::{HarkenChange, HarkenView};
pub use routes::Route;

/// What harken's screens ask of their host that is neither a change, a
/// route nor a device module (`ark_app::App::Request`): the player's, until
/// it is the host's own runtime.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum HarkenRequest {
    /// Play a list from one of its tracks (`at`, 0 the first), said as
    /// where it was played from (a route's URL), shuffled or not.
    Play {
        tracks: Vec<String>,
        at: i64,
        from: String,
        shuffle: bool,
    },
    /// One track next, or at the end of what is queued.
    Queue { track: String, next: bool },
    /// A track's playlists, to put it on one or take it off (a sheet).
    AddTo { track: String },
}

/// A row id as harken's views take it: its sixteen bytes from its hex;
/// none for what is not one.
pub fn key_of(hex: &str) -> Option<[u8; 16]> {
    ark::hash::unhex(hex).and_then(|b| b.try_into().ok())
}
