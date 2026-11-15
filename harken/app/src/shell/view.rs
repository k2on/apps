//! harken's shell laid out, once.

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{ShellMsg, ShellPage};

/// Home, Library and Search, each its toolbar over what it hosts, and a record's
/// page pushed over it.
#[view]
pub fn harken_shell(p: &ShellPage) -> View<ShellMsg> {
    adaptive_tabs(
        p.section,
        |tag| ShellMsg::Select { tag },
        (
            section_tab(
                "Home",
                "house",
                "home",
                toolbar(
                    "Home",
                    pushes(hosted("home"), toolbar(p.page_title, hosted("page"), ()), p.showing, ShellMsg::Back),
                    (),
                ),
            ),
            section_tab(
                "Library",
                "library",
                "library",
                toolbar(
                    "Library",
                    pushes(hosted("library"), toolbar(p.page_title, hosted("page"), ()), p.showing, ShellMsg::Back),
                    (),
                ),
            ),
            section_tab(
                "Search",
                "search",
                "search",
                toolbar(
                    "Search",
                    pushes(hosted("search"), toolbar(p.page_title, hosted("page"), ()), p.showing, ShellMsg::Back),
                    (),
                ),
            ),
        ),
    )
    .into()
}
