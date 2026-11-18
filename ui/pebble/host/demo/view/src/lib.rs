//! The demo's screen, standing in for harken's now playing on the watch:
//! a `#[view]` of arkui's primitives and native parts (ui/docs/pebble.md),
//! the same view code every toolkit draws. The watch runs none of this:
//! its build (`watch/build.rs`) records the view and writes it out as
//! Pebble's layers ([`watch`]), as the phones' SwiftUI and Compose are
//! written from a recording.
//!
//! What playing, where it is, and the next few in the queue, with the link
//! at its head; Select plays or pauses, Down skips (the action bar).
//!
//! The view reads [`Sources`]: what the screen shows, by name and type.
//! The watch never makes one: each field is a source function of the
//! watch's over its node (`watch/src/sources.rs`): the playback's one-row
//! answer, the queue's watched list, the link's cell, and functions over
//! them (the clock's words from the anchor and the time). The emitter binds
//! each layer to the function its path names, as data. The phones are
//! written from the same recording, their record this struct, until their
//! bridges read sources too (step 10 of docs/north-star.md).

use ark_app::view;
use ark_app::view::*;
use ark_ui_gen::pebble;
use now_playing::Act;

/// What the screen reads, each a source on the watch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    /// The link and what is pending: "Online", "Offline, 1 Pending".
    pub status: Text,
    /// The glyph before it: `wifi`, `wifi-off`, `refresh-cw` (syncing).
    pub link: Text,
    pub title: Text,
    pub artist: Text,
    /// How far along the track is, 0 to 1.
    pub progress: f64,
    /// Where it is, and how long it is: `1:23`, `3:45`.
    pub at: Text,
    pub length: Text,
    /// The queue's heading: "Up Next", or "Queue Is Empty".
    pub queue: Text,
    /// The next few in the queue.
    pub next: Vec<Next>,
    /// Select's glyph and what it is said as: `pause` while playing, else `play`.
    pub play_glyph: Text,
    pub play_label: Text,
}

/// One of the queue: its title, and its artist and length.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Next {
    pub title: Text,
    pub line: Text,
}

/// The screen: the link and the track over the queue, the action bar
/// beside them.
#[view]
pub fn screen(p: &Sources) -> View<Act> {
    action_bar(
        vstack((
            vstack((
                hstack((
                    glyph(p.link).style(TextStyle::Caption).ink(Ink::Quiet),
                    text(p.status).style(TextStyle::Caption).ink(Ink::Quiet).lines(1),
                ))
                .spacing(Space::S),
                text(p.title).style(TextStyle::Title).lines(1),
                text(p.artist).style(TextStyle::Subheadline).ink(Ink::Quiet).lines(1),
                vstack((
                    meter(p.progress, "accent"),
                    hstack((
                        text(p.at).style(TextStyle::Body).lines(1),
                        spacer(),
                        text(p.length).style(TextStyle::Body).ink(Ink::Quiet).lines(1),
                    )),
                ))
                .spacing(Space::S)
                .padding(Edges::Top, Space::S),
            ))
            .align(Align::Start)
            .padding(Edges::Horizontal, Space::Page),
            card_section(for t in p.next {
                list_row(t.title).subtitle(t.line)
            })
            .header(p.queue),
        ))
        .spacing(Space::S),
    )
    .select(p.play_glyph, p.play_label, Act::Toggle)
    .down("skip-forward", "Skip", Act::Skip)
    .into()
}

/// What the watch's build writes the screen out as.
pub const WATCH: pebble::Screen = pebble::Screen {
    source: "ui/pebble/host/demo/view/src/lib.rs",
    root: "p",
    sources: "crate::sources",
    uses: &["now_playing::Act"],
    // harken's.
    accent: "gold",
    // What `p.link` and `p.play_glyph` may name.
    glyphs: &["wifi", "wifi-off", "refresh-cw", "play", "pause"],
};

/// The screen as Pebble's layers: the module `watch/build.rs` writes.
pub fn watch() -> String {
    pebble::emit(&screen::record(), &WATCH)
}
