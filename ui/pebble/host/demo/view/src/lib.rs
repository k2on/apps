//! The demo's screen, standing in for harken's now playing on the watch:
//! a `#[view]` of arkui's primitives and native parts (ui/docs/pebble.md),
//! the same view code every toolkit draws. The watch runs none of this:
//! its build (`watch/build.rs`) records the view and writes it out as
//! Pebble's layers ([`watch`]), as the phones' SwiftUI and Compose are
//! written from a recording.
//!
//! What playing, where it is, and the next few in the queue, with the link
//! at its head; Select plays or pauses, Down skips (the action bar).

use ark_app::view;
use ark_app::view::*;
use ark_ui_gen::pebble;
use now_playing::face::{Act, Face};

/// The screen: the link and the track over the queue, the action bar
/// beside them.
#[view]
pub fn screen(p: &Face) -> View<Act> {
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
    record: "Face",
    uses: &["now_playing::face::{Act, Face}"],
    // harken's.
    accent: "gold",
    // What `p.link` and `p.play_glyph` may name.
    glyphs: &["wifi", "wifi-off", "refresh-cw", "play", "pause"],
};

/// The screen as Pebble's layers: the module `watch/build.rs` writes.
pub fn watch() -> String {
    pebble::emit(&screen::record(), &WATCH)
}
