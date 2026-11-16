//! Home laid out, once.

use ark_app::view;
use ark_app::view::*;

use crate::components::*;

use super::{HomeMsg, HomePage};

/// What was played last on a card, with its way back; then the newest
/// songs, the playlists and the albums, each a shelf under its heading.
#[view]
pub fn home(p: &HomePage) -> View<HomeMsg> {
    tab_page(
        "Home",
        (
            if let Some(l) = p.last {
                card_section(
                    media_row(l.title, HomeMsg::Resume)
                        .art(artwork(l.art, l.seed, l.title))
                        .under(l.under)
                        .trailing(glyph_button("play", "Play", TextStyle::Title, Ink::Accent, HomeMsg::Resume)),
                )
                .header("Last Played")
            },
            artwork_shelf(
                "Recently Added",
                for t in p.newest {
                    artwork_tile(
                        artwork(t.art, t.seed, t.title).size(ArtSize::Shelf),
                        t.title,
                        t.caption,
                        HomeMsg::OpenNew { key: t.key },
                    )
                },
            ),
            if p.has_playlists {
                card_section(for l in p.playlists {
                    list_row(l.name)
                        .glyph("list-music")
                        .pushes()
                        .on_tap(HomeMsg::OpenPlaylist { id: l.id, name: l.name })
                })
                .header("Playlists")
            },
            if p.has_albums {
                artwork_shelf(
                    "Albums",
                    for a in p.albums {
                        artwork_tile(
                            artwork(a.art, a.seed, a.title).size(ArtSize::Shelf),
                            a.title,
                            a.caption,
                            HomeMsg::OpenAlbum { key: a.key, title: a.title },
                        )
                    },
                )
            },
            if p.empty {
                card_section(empty_state("music", "Nothing Here Yet", "What your server finds shows here."))
            },
        ),
    )
    .into()
}
