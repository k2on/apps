//! The full player laid out, once: what the now-playing bar grows into.

use ark_app::view;
use ark_app::view::*;

use super::{PlayerMsg, PlayerPage};

/// The cover as wide as it is given, what plays under it, the scrubber
/// counting on from the clock, the controls; where the sound is, a row
/// that puts up the devices to hand it to.
#[view]
pub fn player(p: &PlayerPage) -> View<PlayerMsg> {
    vstack((
        artwork(p.now.art, p.now.seed, p.now.title).size(ArtSize::Fill),
        title_stack(p.now.title).under(p.now.under),
        scrubber(p.now.at_ms, p.now.since_ms, p.now.playing, p.now.length_ms, |at_ms| PlayerMsg::Seek {
            at_ms,
        }),
        transport(p.play_glyph, p.play_label, PlayerMsg::Toggle, PlayerMsg::Previous, PlayerMsg::Next),
        if p.joined {
            card_section(list_row(p.output).glyph("speaker").pushes().on_tap(PlayerMsg::Devices))
        },
        if p.devices_open {
            form_sheet(
                "Devices",
                PlayerMsg::CloseDevices,
                card_section(for d in p.devices {
                    list_row(d.name).glyph("speaker").value(d.status).on_tap(PlayerMsg::Transfer { to: d.id })
                }),
            )
        },
    ))
    .spacing(Space::L)
    .into()
}
