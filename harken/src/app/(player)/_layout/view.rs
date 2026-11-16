//! The player laid out, once: the now-playing bar, and the full player it
//! grows into (the shell's accessory carries the one into the other).

use ark_app::view;
use ark_app::view::*;

use crate::components::*;

use super::{PlayerMsg, PlayerPage};

/// What plays, on the bar over the sections: its cover, its title over
/// what it is by, play or pause and next, how far along it is at its foot.
/// Pressed, the shell grows it into the full player.
#[view]
pub fn now_playing(p: &PlayerPage) -> View<PlayerMsg> {
    vstack((
        hstack((
            artwork(p.now.art, p.now.seed, p.now.title),
            title_stack(p.now.title).under(p.now.under).plain().one_line(),
            spacer_min(Space::Xs),
            glyph_button(p.play_glyph, p.play_label, TextStyle::Title, Ink::Ink, PlayerMsg::Toggle),
            glyph_button("skip-forward", "Next", TextStyle::Title, Ink::Ink, PlayerMsg::Next),
        ))
        .spacing(Space::M)
        .padding(Edges::Horizontal, Space::M)
        .fill_width_at(Align::Start),
        vstack(hairline(p.now.at_ms, p.now.since_ms, p.now.playing, p.now.length_ms)).padding(Edges::Horizontal, Space::L),
    ))
    .spacing(Space::Xs)
    .padding(Edges::Vertical, Space::S)
    .into()
}

/// The cover as wide as it is given, what plays under it, the scrubber
/// counting on from the clock, the controls; then where the sound is (a
/// row that puts up the devices to hand it to, and the group's picker),
/// the queue and, for what has one, the transcript, each a sheet.
#[view]
pub fn player(p: &PlayerPage) -> View<PlayerMsg> {
    (
        vstack((
            artwork(p.now.art, p.now.seed, p.now.title).size(ArtSize::Fill),
            title_stack(p.now.title).under(p.now.under),
            scrubber(p.now.at_ms, p.now.since_ms, p.now.playing, p.now.length_ms, |at_ms| PlayerMsg::Seek {
                at_ms,
            }),
            transport(p.play_glyph, p.play_label, PlayerMsg::Toggle, PlayerMsg::Previous, PlayerMsg::Next),
            if p.has_volume {
                volume("Volume", p.now.volume, p.now.muted, |level| PlayerMsg::Volume { level }, PlayerMsg::Mute)
            },
            card_section((
                if p.joined {
                    list_row(p.output).glyph("speaker").pushes().on_tap(PlayerMsg::Devices)
                },
                list_row("Queue").glyph("list-music").value(p.next).pushes().on_tap(PlayerMsg::Queue),
                if p.has_transcript {
                    list_row("Transcript").glyph("captions").pushes().on_tap(PlayerMsg::Transcript)
                },
            )),
        ))
        .spacing(Space::L),
        if p.devices_open {
            form_sheet(
                "Devices",
                PlayerMsg::CloseDevices,
                (
                    card_section(for d in p.devices {
                        list_row(d.name).glyph("speaker").value(d.status).on_tap(PlayerMsg::Transfer { to: d.id })
                    }),
                    if p.has_group {
                        card_section(for g in p.group {
                            list_row(g.name).glyph("speaker").value(g.mark).on_tap(PlayerMsg::Group { device: g.id })
                        })
                        .header("Group")
                    },
                ),
            )
        },
        if p.queue_open {
            form_sheet(
                "Queue",
                PlayerMsg::CloseQueue,
                card_section(for q in p.queue {
                    media_row(q.title, PlayerMsg::Jump { at: q.at })
                        .art(artwork(q.art, q.seed, q.title).size(ArtSize::Row))
                        .under(q.under)
                        .menu("Move Up", "arrow-up", PlayerMsg::MoveUp { at: q.at })
                        .menu("Move Down", "arrow-down", PlayerMsg::MoveDown { at: q.at })
                        .menu("Remove", "trash", PlayerMsg::Remove { at: q.at })
                })
                .header(p.next),
            )
        },
        if p.transcript_open {
            form_sheet(
                "Transcript",
                PlayerMsg::CloseTranscript,
                card_section(for l in p.lines {
                    vstack((
                        text(l.speaker).style(TextStyle::Caption).ink(Ink::Quiet).if_any(),
                        clock_words(l.text, l.at_ms, l.since_ms, l.playing, l.from_ms, l.until_ms),
                    ))
                    .spacing(Space::Xs)
                    .padding(Edges::Horizontal, Space::L)
                    .padding(Edges::Vertical, Space::S)
                    .fill_width_at(Align::Start)
                }),
            )
        },
    )
        .into()
}
