//! harken's shell laid out, once.

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{ShellMsg, ShellPage};

/// Home, Library and Search, each its toolbar over what it hosts, and a record's
/// page pushed over it; the player over them all, its bar while something
/// plays, swiped to skip, grown into the full player.
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
    .account(account_item(p.household.who, p.household.picture, ShellMsg::Account))
    .accessory(hosted("player.bar"), hosted("player"), ShellMsg::OpenPlayer, ShellMsg::ClosePlayer)
    .showing(p.playing)
    .expanded(p.player_open)
    .skips(ShellMsg::Skip { by: 1 }, ShellMsg::Skip { by: -1 })
    .into()
}

/// Settings: a window of panes on the Mac and the desktop, a list of
/// screens elsewhere. General is the server and whether it can be reached,
/// and what the app is; Diagnostics, the session's numbers.
#[view]
pub fn harken_settings(p: &ShellPage) -> View<ShellMsg> {
    settings_panes(
        p.pane,
        |title| ShellMsg::Pane { title },
        (
            settings_pane("General", "settings", settings_general(p)),
            settings_pane("Diagnostics", "list", settings_diagnostics(p)),
        ),
    )
}

/// General: the server, its link, the version.
#[view]
pub fn settings_general(p: &ShellPage) -> View<ShellMsg> {
    detail_page((
        if p.facts.on_server {
            card_section(list_row(p.facts.server).value(p.facts.link).glyph("cloud")).header("Server")
        } else {
            card_section(list_row("Demo Library").value(p.facts.link).glyph("music")).header("Library")
        },
        card_section(list_row("Version").value(p.facts.version).glyph("badge-check")).header("About"),
    ))
}

/// Diagnostics: what waits to sync, this device as the session names it,
/// the connections opened, each a figure under its label.
#[view]
pub fn settings_diagnostics(p: &ShellPage) -> View<ShellMsg> {
    detail_page(card_section(
        vstack((
            figure_label("Waiting to Sync", p.facts.pending),
            figure_label("This Device", p.facts.device),
            figure_label("Connections", p.facts.connections),
        ))
        .spacing(Space::L)
        .align(Align::Start)
        .padding(Edges::All, Space::L)
        .fill_width_at(Align::Start),
    ))
}

/// The menu bar's commands: View, Go. On the Mac, the menu bar; on an iPad
/// and Android, the hardware keyboard's shortcuts and the app bar's menu;
/// on the desktop and in the browser, the keys and the command box.
pub fn commands() -> Vec<CommandMenu<ShellMsg>> {
    let go = |tag: &str| ShellMsg::Select { tag: tag.into() };
    vec![
        command_menu(
            MenuPlace::View,
            vec![
                command("Play or Pause", "p", ShellMsg::Toggle),
                command("Next Track", "]", ShellMsg::Skip { by: 1 }),
                command("Previous Track", "[", ShellMsg::Skip { by: -1 }),
            ],
        ),
        command_menu(
            MenuPlace::Go,
            vec![
                command("Home", "1", go("home")),
                command("Library", "2", go("library")),
                command("Search", "3", go("search")),
            ],
        ),
    ]
}
