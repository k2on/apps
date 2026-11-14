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
    /// The overview, a tab at every width.
    pub top: Vec<PageCard>,
    /// The index of every page: in the sidebar, and the toolbar's.
    pub more: Vec<PageCard>,
    /// Each section's own page (one, as a list for `#[view]`), and its
    /// pages.
    pub foundations_page: Vec<PageCard>,
    pub foundations: Vec<PageCard>,
    pub components_page: Vec<PageCard>,
    pub components: Vec<PageCard>,
    pub overlays_page: Vec<PageCard>,
    pub overlays: Vec<PageCard>,
    pub motion_page: Vec<PageCard>,
    pub motion: Vec<PageCard>,
    pub behaviour_page: Vec<PageCard>,
    pub behaviour: Vec<PageCard>,
}

/// A page as its tab and its body read it.
#[derive(Clone, Debug, Default)]
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
/// pages under its heading — on a phone, a tab for each section, its own
/// page a row for each of its pages and each page pushed over it. Each
/// page its toolbar and the search field over it, the search's results in
/// its place while there is a search.
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
                "",
                for t in p.more {
                    page_tab(p, t)
                },
            ),
            tab_section(
                "Foundations",
                "layers",
                "foundations",
                for t in p.foundations_page {
                    page_bar(p, t)
                },
                for t in p.foundations {
                    page_tab(p, t)
                },
            ),
            tab_section(
                "Components",
                "list",
                "components",
                for t in p.components_page {
                    page_bar(p, t)
                },
                for t in p.components {
                    page_tab(p, t)
                },
            ),
            tab_section(
                "Overlays",
                "copy",
                "overlays",
                for t in p.overlays_page {
                    page_bar(p, t)
                },
                for t in p.overlays {
                    page_tab(p, t)
                },
            ),
            tab_section(
                "Motion",
                "zap",
                "motion",
                for t in p.motion_page {
                    page_bar(p, t)
                },
                for t in p.motion {
                    page_tab(p, t)
                },
            ),
            tab_section(
                "Behaviour",
                "wrench",
                "behaviour",
                for t in p.behaviour_page {
                    page_bar(p, t)
                },
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
    section_tab(t.title, t.glyph, t.tag, page_bar(p, t)).into()
}

/// A page under its toolbar: its title, the search over it.
#[allow(unused_braces)]
#[view]
pub fn page_bar(p: &ShellPage, t: &PageCard) -> View<Msg> {
    toolbar(
        t.title,
        searchable(
            "Search the Docs",
            p.search,
            |text| Msg::Search { text },
            if p.searching { results(p) } else { hosted("page") },
        ),
        bar_items(),
    )
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
        hosted("index"),
        hosted("live"),
        hosted("desktop"),
        writing(t.reference),
        hosted("mobile"),
        hosted("pager"),
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
