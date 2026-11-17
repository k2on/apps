//! A record's page laid out, once.

use ark_app::view;
use ark_app::view::*;

use crate::components::*;

use super::{PageMsg, RecordPage};

/// Its header — the cover beside what it is, its title, who it is by and
/// its small print, Play and Shuffle under them — then its tracks, each a
/// media row with its menu; none said as such. A person's cover is round,
/// as their tile is in the library and in search.
#[view]
pub fn record(p: &RecordPage) -> View<PageMsg> {
    detail_page((
        if p.round {
            media_header(
                artwork(p.art, p.seed, p.title).round(),
                title_stack(p.title).over(p.kicker).under(p.by).under_ink(Ink::Accent).caption(p.caption),
                play_shuffle(PageMsg::PlayAll { shuffle: false }, PageMsg::PlayAll { shuffle: true }),
            )
        } else {
            media_header(
                artwork(p.art, p.seed, p.title),
                title_stack(p.title).over(p.kicker).under(p.by).under_ink(Ink::Accent).caption(p.caption),
                play_shuffle(PageMsg::PlayAll { shuffle: false }, PageMsg::PlayAll { shuffle: true }),
            )
        },
        card_section(for t in p.tracks {
            media_row(t.title, PageMsg::Play { id: t.id })
                .art(artwork(t.art, t.seed, t.title))
                .under(t.under)
                .trailing(text(t.length).style(TextStyle::Caption).ink(Ink::Quiet).lines(1))
                .menu("Play Next", "list-start", PageMsg::Queue { id: t.id, next: true })
                .menu("Play Last", "list-end", PageMsg::Queue { id: t.id, next: false })
                .menu("Add to Playlist", "list-plus", PageMsg::AddTo { id: t.id })
        }),
        if p.empty {
            card_section(empty_state("music", "Nothing Here Yet"))
        },
    ))
}
