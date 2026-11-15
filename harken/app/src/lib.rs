//! harken as an Ark app (ui/docs/app.md, "harken's Port"): a screen's
//! state, the messages that change it, its update and its `#[view]`, in
//! Rust, run by `ark_app::Runtime` on every platform; where harken can be
//! ([`routes`]), which every shell spells the same way; and what it says
//! on its first run ([`welcome`]).
//!
//! The screens moved here so far are the library's: Home ([`home`]),
//! Library ([`library`]), Search ([`search`]) and a record's page
//! ([`page`]), under the shell ([`shell`]), the player ([`player`]) and
//! the sheets any of them raises ([`sheets`]),
//! run by the host's part ([`host`]), which drives this device's own sound
//! ([`sound`], the media player module). The sheets and Settings follow
//! (ui/docs/app.md, "Where It Stands").

pub mod home;
pub mod host;
pub mod library;
pub mod page;
pub mod player;
pub mod routes;
pub mod search;
pub mod sheets;
pub mod shell;
pub mod sound;
pub mod welcome;
pub mod words;

#[cfg(test)]
pub(crate) mod testing;

pub use harken_domain::{HarkenChange, HarkenView};
pub use routes::Route;

/// What harken's screens ask of their host that is neither a change, a
/// route nor a device module (`ark_app::App::Request`): the player's, each
/// a press the host makes — of the session while this device stands in it,
/// of its own sound while it is alone — and the sheets'.
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
    Queue {
        track: String,
        next: bool,
    },
    /// A track's playlists, to put it on one or take it off (a sheet).
    AddTo {
        track: String,
    },
    /// Follow a show (`podcast`, `sermon`) by its feed (a sheet).
    Follow {
        kind: String,
    },
    /// Pause what sounds, else play.
    Toggle,
    /// On (1) or back (-1) a track.
    Skip {
        by: i64,
    },
    Seek {
        at_ms: i64,
    },
    /// The scrubber held at `at_ms`, for the other devices to show.
    Scrub {
        at_ms: i64,
    },
    /// The sound handed to another device.
    Transfer {
        to: String,
    },
    /// How loud where it plays: the group's while joined, this device's
    /// alone.
    Volume {
        level: i64,
    },
    Mute {
        muted: bool,
    },
}

/// A row id as harken's views take it: its sixteen bytes from its hex;
/// none for what is not one.
pub fn key_of(hex: &str) -> Option<[u8; 16]> {
    ark::hash::unhex(hex).and_then(|b| b.try_into().ok())
}
