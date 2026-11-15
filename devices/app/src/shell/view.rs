//! devices' shell laid out, once: drawn natively from these recordings
//! (ui/docs/app.md, "The App Library").

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{ShellMsg, ShellPage};

/// The main window: the status screen, who is signed in at its foot.
#[view]
pub fn devices_shell(p: &ShellPage) -> View<ShellMsg> {
    adaptive_tabs(
        p.section,
        |tag| ShellMsg::Select { tag },
        section_tab("Status", "settings", "status", toolbar("This Phone", hosted("status"), ())),
    )
    .footer(hosted("household"))
    .into()
}

/// Settings: who is signed in, the server, the version.
#[view]
pub fn devices_settings(p: &ShellPage) -> View<ShellMsg> {
    settings_panes(
        p.pane,
        |title| ShellMsg::Pane { title },
        settings_pane(
            "General",
            "settings",
            detail_page((
                card_section(list_row(p.who).subtitle(p.detail).tile("users")),
                card_section((
                    list_row("Server").value(p.server).glyph("cloud"),
                    list_row("Version").value(p.version).glyph("settings"),
                )),
            )),
        ),
    )
}

/// The menu bar's and the toolbars' commands: none of devices' own.
pub fn commands() -> Vec<CommandMenu<ShellMsg>> {
    vec![]
}
