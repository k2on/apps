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
//! The view reads sources, never a record: the host's status and time
//! (cells), what is playing (a one-row plan) and the queue's first few (a
//! plan), each a `let`; what it shows of them is worked out in `let`s too,
//! by `now_playing::show`'s plain functions, and each `text!` keeps its
//! holes apart. The watch's build writes each layer's binding from those
//! `let`s (`watch/build.rs`); iced and the browser draw it from a node's
//! reader (`screen::view(db)`). The phones are written from the same
//! recording, reading a record whose fields are its `let`s, until their
//! bridges read sources too (step 10 of docs/north-star.md).

use ark_app::text;
use ark_app::view;
use ark_app::view::*;
use ark_kernel::{Clock, NOW, STATUS};
use ark_ui_gen::pebble;
use now_playing::{playback, show, up_next, Act, SHOWN};

/// The screen: the link and the track over the queue, the action bar
/// beside them.
#[view]
pub fn screen() -> View<Act> {
    let link = cell(STATUS);
    let now = cell(NOW);
    let p = row(playback());
    let next = list(up_next(SHOWN));
    let status = show::status_line(link);
    let wifi = show::link_glyph(link);
    let title = show::title(p);
    let artist = show::artist(p);
    let progress = show::progress(p, now);
    let at = show::at(p, now);
    let length = show::length(p);
    let heading = show::heading(next);
    let play = show::play_glyph(p);
    let say = show::play_label(p);
    action_bar(
        vstack((
            vstack((
                hstack((
                    glyph(wifi).style(TextStyle::Caption).ink(Ink::Quiet),
                    text(status).style(TextStyle::Caption).ink(Ink::Quiet).lines(1),
                ))
                .spacing(Space::S),
                text(title).style(TextStyle::Title).lines(1),
                text(artist).style(TextStyle::Subheadline).ink(Ink::Quiet).lines(1),
                vstack((
                    meter(progress, "accent"),
                    hstack((
                        text(at).style(TextStyle::Body).lines(1),
                        spacer(),
                        text(length).style(TextStyle::Body).ink(Ink::Quiet).lines(1),
                    )),
                ))
                .spacing(Space::S)
                .padding(Edges::Top, Space::S),
            ))
            .align(Align::Start)
            .padding(Edges::Horizontal, Space::Page),
            card_section(for t in next {
                let name = text!("{}", t.title);
                let line = text!("{}  {}", t.artist, Clock(u64::from(t.ms)));
                list_row(name).subtitle(line)
            })
            .header(heading),
        ))
        .spacing(Space::S),
    )
    .select(play, say, Act::Toggle)
    .down("skip-forward", "Skip", Act::Skip)
    .into()
}

/// What the watch's build writes the screen out as.
pub const WATCH: pebble::Screen = pebble::Screen {
    source: "ui/pebble/host/demo/view/src/lib.rs",
    uses: &["now_playing::{playback, show, up_next, Act, SHOWN}", "ark_kernel::{Clock, NOW, STATUS}"],
    // harken's.
    accent: "gold",
    // What `wifi` and `play` may name.
    glyphs: &["wifi", "wifi-off", "refresh-cw", "play", "pause"],
};

/// The screen as Pebble's layers: the module `watch/build.rs` writes.
pub fn watch() -> String {
    pebble::emit(&screen::recording(), &WATCH)
}
