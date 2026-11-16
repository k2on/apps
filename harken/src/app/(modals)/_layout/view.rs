//! harken's sheets laid out, once.

use ark_app::view;
use ark_app::view::*;

use super::{SheetsMsg, SheetsPage};

/// A track's playlists, each pressed to put it on or take it off, "Added"
/// beside those it is on; following a show, its feed's address typed.
#[view]
pub fn sheets(p: &SheetsPage) -> View<SheetsMsg> {
    (
        if p.adding {
            form_sheet(
                "Add to Playlist",
                SheetsMsg::Dismiss,
                card_section(for l in p.playlists {
                    list_row(l.name)
                        .glyph("list-music")
                        .value(l.mark)
                        .on_tap(SheetsMsg::Toggle { playlist: l.id })
                }),
            )
        },
        if let Some(form) = p.following {
            form_sheet(
                "Follow",
                SheetsMsg::Dismiss,
                (
                    text(form.title).style(TextStyle::Headline),
                    name_field("Feed Address", form.url, form.problem, |url| SheetsMsg::Url { url }),
                    form_button("Follow", form.can_save, SheetsMsg::Subscribe),
                ),
            )
        },
    )
        .into()
}
