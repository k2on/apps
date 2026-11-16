//! Search laid out, once.

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use crate::parts::*;

use super::{SearchMsg, SearchPage};

/// The search field over what holds what is typed: albums and people as
/// shelves, songs as rows; nothing typed or nothing found said as such.
#[view]
pub fn search(p: &SearchPage) -> View<SearchMsg> {
    searchable(
        "Songs, albums and artists",
        p.needle,
        |text| SearchMsg::Typed { text },
        tab_page(
            "Search",
            (
                if p.idle {
                    card_section(empty_state("search", "Search Your Library", "Songs, albums and artists, as you type."))
                },
                if p.nothing {
                    card_section(empty_state("search", "No Results", p.nothing_text))
                },
                if p.has_albums {
                    card_section(artwork_shelf(for a in p.albums {
                        artwork_tile(
                            artwork(a.art, a.seed, a.title).size(ArtSize::Shelf),
                            a.title,
                            a.caption,
                            SearchMsg::OpenAlbum { key: a.key, title: a.title },
                        )
                    }))
                    .header("Albums")
                },
                if p.has_artists {
                    card_section(artwork_shelf(for a in p.artists {
                        artwork_tile(
                            artwork(a.art, a.seed, a.title).size(ArtSize::Shelf).round(),
                            a.title,
                            a.caption,
                            SearchMsg::OpenArtist { name: a.key },
                        )
                    }))
                    .header("Artists")
                },
                if p.has_songs {
                    card_section(for t in p.songs {
                        media_row(t.title, SearchMsg::Play { id: t.id })
                            .art(artwork(t.art, t.seed, t.title))
                            .under(t.under)
                            .trailing(text(t.length).style(TextStyle::Caption).ink(Ink::Quiet).lines(1))
                    })
                    .header("Songs")
                },
            ),
        ),
    )
}
