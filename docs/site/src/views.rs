//! The site laid out, once: ArkUI's shell over every page, and a page's
//! writing around what the app hosts (the live parts, the pictures, the
//! phones' captures). Drawn by arkui-web, written as HTML and taken over.
use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;
use ark_app::writing::writing;

use crate::app::Msg;

/// What the shell reads.
#[derive(Clone, Debug)]
pub struct ShellPage {
    /// The page shown, by its tag.
    pub page: String,
    pub search: String,
    pub searching: bool,
    pub results: Vec<Hit>,
    /// The overview and the index of pages, over the sections.
    pub top: Vec<PageCard>,
    pub foundations: Vec<PageCard>,
    pub components: Vec<PageCard>,
    pub overlays: Vec<PageCard>,
    pub motion: Vec<PageCard>,
    pub behaviour: Vec<PageCard>,
}

/// A page as its tab and its body read it.
#[derive(Clone, Debug)]
pub struct PageCard {
    pub title: String,
    pub glyph: String,
    pub tag: String,
    pub intro: String,
    pub reference: String,
}

/// A page the search found.
#[derive(Clone, Debug)]
pub struct Hit {
    pub title: String,
    pub said: String,
    pub tag: String,
}

/// The window: the overview and the index of pages, then each section's
/// pages under its heading; each page its toolbar and the search field
/// over it, the search's results in its place while there is a search.
#[view]
pub fn docs_shell(p: &ShellPage) -> View<Msg> {
    adaptive_tabs(
        p.page,
        |tag| Msg::Go { tag },
        (
            for t in p.top {
                page_tab(p, t)
            },
            tab_group(
                "Foundations",
                for t in p.foundations {
                    page_tab(p, t)
                },
            ),
            tab_group(
                "Components",
                for t in p.components {
                    page_tab(p, t)
                },
            ),
            tab_group(
                "Overlays",
                for t in p.overlays {
                    page_tab(p, t)
                },
            ),
            tab_group(
                "Motion",
                for t in p.motion {
                    page_tab(p, t)
                },
            ),
            tab_group(
                "Behaviour",
                for t in p.behaviour {
                    page_tab(p, t)
                },
            ),
        ),
    )
    .into()
}

/// One page's tab: its title, glyph and tag, its toolbar, the search.
// The `if`'s braces are `#[view]`'s to read, not a block.
#[allow(unused_braces)]
#[view]
pub fn page_tab(p: &ShellPage, t: &PageCard) -> View<Msg> {
    section_tab(
        t.title,
        t.glyph,
        t.tag,
        toolbar(
            t.title,
            searchable(
                "Search the Docs",
                p.search,
                |text| Msg::Search { text },
                if p.searching { results(p) } else { hosted("page") },
            ),
            bar_items(),
        ),
    )
    .into()
}

/// The toolbar's: every page (the search's way without script), and the
/// appearance, which needs script.
fn bar_items() -> View<Msg> {
    (
        bar_item(
            "All Pages",
            "list",
            Placement::Primary,
            Msg::Go {
                tag: crate::pages::ALL.into(),
            },
        ),
        bar_item("Appearance", "droplet", Placement::Primary, Msg::Appearance),
    )
        .into()
}

/// A page: its writing, the component's live parts and pictures, its
/// modules' reference, and the phones. The shell hosts it
/// (`hosted("page")`, `DocsWeb::page_view`), which draws its hosted parts
/// in their places.
#[view]
pub fn doc_page(t: &PageCard) -> View<Msg> {
    (
        writing(t.intro),
        hosted("live"),
        hosted("desktop"),
        writing(t.reference),
        hosted("mobile"),
    )
        .into()
}

/// What the search found, a row each.
#[view]
pub fn results(p: &ShellPage) -> View<Msg> {
    tab_page(
        "",
        card_section(for h in p.results {
            list_row(h.title).subtitle(h.said).pushes().on_tap(Msg::Go { tag: h.tag })
        })
        .header("Results"),
    )
    .into()
}
