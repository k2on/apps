//! harken as an Ark app (ui/docs/app.md, "harken's Port"): a screen's
//! state, the messages that change it, its update and its `#[view]`, in
//! Rust, run by `ark_app::Runtime` on every platform; where harken can be
//! ([`routes`]), which every shell spells the same way; and what it says
//! on its first run ([`welcome`]).
//!
//! Its screens are Home ([`home`]), Library ([`library`]), Search
//! ([`search`]) and a record's page ([`page`]), under the shell
//! ([`shell`], with Settings), the player ([`player`]) and the sheets any
//! of them raises ([`sheets`]), run by the host's part ([`host`]), which
//! drives this device's own sound ([`sound`], the media player module);
//! the window the desktop and the browser draw is [`window`]'s.

pub mod home;
pub mod host;
pub mod library;
pub mod page;
pub mod parts;
pub mod player;
pub mod routes;
pub mod search;
pub mod sheets;
pub mod shell;
pub mod sound;
pub mod welcome;
pub mod window;
pub mod words;

#[cfg(test)]
pub(crate) mod testing;

// harken's declaration (ui/docs/app.md, "The App's Declaration"): its part of
// its host and each screen it runs, with the `#[view]` each is drawn by.
// Every generated target is written from this (its manifest); nothing of
// it is said in app.toml.
ark_app::app! {
    /// harken as every generated target runs it: its part of its host.
    pub type App = host::Harken {
        home: home::Home = home::view::home,
        library: library::Library = library::view::library,
        search: search::Search = search::view::search,
        // A record's page (an album, an artist, a playlist, a show), pushed
        // over the section it was opened from.
        page: page::Page = page::view::record,
        // The full player, and its now-playing bar (`player.bar`), which the
        // shell's accessory grows into it.
        player: player::Player = player::view::player {
            bar: player::view::now_playing,
        },
        // Over every section: its sheets and dialogs.
        #[over]
        sheets: sheets::Sheets = sheets::view::sheets,
        // The shell's main window, its Settings and the menu bar's commands.
        #[shell]
        shell: shell::HarkenShell = shell::view::harken_shell {
            settings: shell::view::harken_settings,
            commands: shell::view::commands,
        },
    }
}

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
    /// A track of the queue, from its start; taken out; moved.
    Jump {
        index: i64,
    },
    Remove {
        index: i64,
    },
    Move {
        from: i64,
        to: i64,
    },
    /// A device put in the group that plays, or taken out.
    Join {
        device: String,
    },
    Unjoin {
        device: String,
    },
    /// Settings, on the pane of this title: the shell's own on a phone, the
    /// window's place on the desktop and in the browser.
    Settings {
        pane: String,
    },
}

/// A row id as harken's views take it: its sixteen bytes from its hex;
/// none for what is not one.
pub fn key_of(hex: &str) -> Option<[u8; 16]> {
    ark::hash::unhex(hex).and_then(|b| b.try_into().ok())
}
