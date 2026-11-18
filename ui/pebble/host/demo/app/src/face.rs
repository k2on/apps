//! What the watch's screen shows: the record its `#[view]` reads
//! (`now-playing-view`, ui/docs/pebble.md), worked out by the watch from
//! the node's views, and what the screen's buttons send. Plain data, so the
//! view machinery reads it at build time and the watch fills it at run time.

use alloc::string::String;
use alloc::vec::Vec;

/// The now-playing screen's record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Face {
    /// The link and what is pending: "Online", "Offline, 1 Pending".
    pub status: String,
    /// The glyph before it: `wifi`, `wifi-off`, `refresh-cw` (syncing).
    pub link: String,
    pub title: String,
    pub artist: String,
    /// How far along the track is, 0 to 1.
    pub progress: f64,
    /// Where it is, and how long it is: `1:23`, `3:45`.
    pub at: String,
    pub length: String,
    /// The queue's heading: "Up Next", or "Queue Is Empty".
    pub queue: String,
    /// The next few in the queue.
    pub next: Vec<Next>,
    /// Select's glyph and what it is said as: `pause` while playing, else `play`.
    pub play_glyph: String,
    pub play_label: String,
}

/// One of the queue: its title, and its artist and length.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Next {
    pub title: String,
    pub line: String,
}

/// What the screen's buttons send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Play or pause.
    Toggle,
    /// The next in the queue, now.
    Skip,
}
