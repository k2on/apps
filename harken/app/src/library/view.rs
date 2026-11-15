//! Library laid out, once: drawn by iced and the browser from these
//! functions, written out as SwiftUI and Compose from their recordings.

use ark_app::view;
use ark_app::view::*;

use super::{LibraryMsg, LibraryPage};

/// The chips, then what they choose: the playlists (New Playlist first),
/// the albums and the people in grids of covers, every song a row, the
/// shows in a grid; an empty way said as such.
#[view]
pub fn library(p: &LibraryPage) -> View<LibraryMsg> {
    tab_page(
        "Library",
        (
            chip_row(for c in p.chips {
                chip(c.label, c.glyph, c.selected, LibraryMsg::Show { kind: c.tag })
            }),
            if p.on_playlists {
                card_section((
                    list_row("New Playlist").glyph("circle-plus").accent().on_tap(LibraryMsg::NewPlaylist),
                    for l in p.playlists {
                        list_row(l.name)
                            .glyph("list-music")
                            .pushes()
                            .on_tap(LibraryMsg::OpenPlaylist { id: l.id, name: l.name })
                    },
                ))
            },
            if p.on_albums {
                artwork_grid(for a in p.albums {
                    artwork_tile(
                        artwork(a.art, a.seed, a.title).size(ArtSize::Fill),
                        a.title,
                        a.caption,
                        LibraryMsg::OpenAlbum { key: a.key, title: a.title },
                    )
                })
            },
            if p.on_artists {
                artwork_grid(for a in p.artists {
                    artwork_tile(
                        artwork(a.art, a.seed, a.title).size(ArtSize::Fill).round(),
                        a.title,
                        a.caption,
                        LibraryMsg::OpenArtist { name: a.key },
                    )
                })
            },
            if p.on_songs {
                card_section(for t in p.songs {
                    media_row(t.title, LibraryMsg::Play { id: t.id })
                        .art(artwork(t.art, t.seed, t.title))
                        .under(t.under)
                        .trailing(text(t.length).style(TextStyle::Caption).ink(Ink::Quiet))
                        .menu("Play Next", "list-start", LibraryMsg::Queue { id: t.id, next: true })
                        .menu("Play Last", "list-end", LibraryMsg::Queue { id: t.id, next: false })
                        .menu("Add to Playlist", "list-plus", LibraryMsg::AddTo { id: t.id })
                })
            },
            if p.on_shows {
                (
                    card_section(list_row(p.follow).glyph("circle-plus").accent().on_tap(LibraryMsg::Follow)),
                    artwork_grid(for s in p.shows {
                        artwork_tile(
                            artwork(s.art, s.seed, s.title).size(ArtSize::Fill),
                            s.title,
                            s.caption,
                            LibraryMsg::OpenShow { key: s.key, title: s.title },
                        )
                    }),
                )
            },
            if p.empty {
                card_section(empty_state(p.empty_glyph, p.empty_title, p.empty_text))
            },
            if let Some(form) = p.naming {
                form_sheet(
                    "New Playlist",
                    LibraryMsg::Dismiss,
                    (
                        name_field("Name", form.name, form.problem, |name| LibraryMsg::Name { name }),
                        form_button("Make Playlist", form.can_save, LibraryMsg::SavePlaylist),
                    ),
                )
            },
        ),
    )
    .into()
}
