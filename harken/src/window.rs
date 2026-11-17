//! harken in a window its host draws itself (`ark_app::window`): the
//! browser's page and the desktop's window. What the window shows is every
//! platform's views — the shell's, each screen's, the player's bar and the
//! full player it grows into, Settings' panes — so nothing here is drawn of
//! harken's own beyond what the phones draw.

use ark_app::host::{Cx, Place, ShellAsk};
use ark_app::shell::{CommandItem, CommandMenu};
use ark_app::view::{empty_state, View};
use ark_app::window::Windowed;
use harken_domain::{HarkenChange, HarkenView};

use crate::home::HomeMsg;
use crate::host::Harken;
use crate::library::LibraryMsg;
use crate::page::PageMsg;
use crate::player::PlayerMsg;
use crate::search::SearchMsg;
use crate::sheets::SheetsMsg;
use crate::shell::view::{commands, harken_settings, harken_shell};
use crate::shell::{Appearance, ShellMsg};
use crate::Route;

/// A message for one of harken's screens, or for its window.
#[derive(Clone, Debug, PartialEq)]
pub enum HarkenMsg {
    Shell(ShellMsg),
    Home(HomeMsg),
    Library(LibraryMsg),
    Search(SearchMsg),
    /// The record's page pushed over a section.
    Page(PageMsg),
    /// The now-playing bar and the full player.
    Player(PlayerMsg),
    Sheets(SheetsMsg),
    /// Settings' Done: put away.
    CloseSettings,
}

type Seen<'a> = &'a dyn Place<HarkenView, HarkenChange>;

impl Windowed for Harken {
    type Msg = HarkenMsg;

    fn take(&mut self, msg: HarkenMsg, cx: &mut Cx<'_, Harken>) {
        match msg {
            HarkenMsg::Shell(m) => self.shell.send(m, cx),
            HarkenMsg::Home(m) => self.home.send(m, cx),
            HarkenMsg::Library(m) => self.library.send(m, cx),
            HarkenMsg::Search(m) => self.search.send(m, cx),
            HarkenMsg::Page(m) => self.page.send(m, cx),
            HarkenMsg::Player(m) => self.player.send(m, cx),
            HarkenMsg::Sheets(m) => self.sheets.send(m, cx),
            HarkenMsg::CloseSettings => self.settings = false,
        }
    }

    fn window(&self, place: Seen<'_>, now_ms: i64) -> View<HarkenMsg> {
        harken_shell::view(&self.shell.page(place, now_ms)).map(HarkenMsg::Shell)
    }

    fn hosted(&self, name: &str, place: Seen<'_>, now_ms: i64) -> View<HarkenMsg> {
        match name {
            "home" => crate::home::view::home::view(&self.home.page(place, now_ms)).map(HarkenMsg::Home),
            "library" => crate::library::view::library::view(&self.library.page(place, now_ms)).map(HarkenMsg::Library),
            "search" => crate::search::view::search::view(&self.search.page(place, now_ms)).map(HarkenMsg::Search),
            "page" => crate::page::view::record::view(&self.page.page(place, now_ms)).map(HarkenMsg::Page),
            "player.bar" => crate::player::view::now_playing::view(&self.player.page(place, now_ms)).map(HarkenMsg::Player),
            "player" => crate::player::view::player::view(&self.player.page(place, now_ms)).map(HarkenMsg::Player),
            // What a platform adds to Settings' General: the desktop's alone.
            "settings.more" => View::Empty,
            _ => empty_state("music", "Nothing Selected"),
        }
    }

    fn over(&self, place: Seen<'_>, now_ms: i64) -> View<HarkenMsg> {
        crate::sheets::view::sheets::view(&self.sheets.page(place, now_ms)).map(HarkenMsg::Sheets)
    }

    fn settings(&self, place: Seen<'_>, now_ms: i64) -> Option<(View<HarkenMsg>, HarkenMsg)> {
        self.settings.then(|| {
            (
                harken_settings::view(&self.shell.page(place, now_ms)).map(HarkenMsg::Shell),
                HarkenMsg::CloseSettings,
            )
        })
    }

    fn commands(&self) -> Vec<CommandMenu<HarkenMsg>> {
        commands()
            .into_iter()
            .map(|m| CommandMenu {
                place: m.place,
                items: m
                    .items
                    .into_iter()
                    .map(|c| CommandItem {
                        title: c.title,
                        shortcut: c.shortcut,
                        run: HarkenMsg::Shell(c.run),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Where the window is: Settings' pane while it shows, the page pushed
    /// over a section, else the section (the library's by its kind); with
    /// `player=1` while the player has grown over it, so opening it is a
    /// step of its own that back undoes, as a phone's back closes it.
    fn address(&self, place: Seen<'_>, now_ms: i64) -> String {
        let at = self.place_address(place, now_ms);
        match self.shell.state().player_open && !self.settings {
            true => ark_router::Address::of_url(&at, "/").with(PLAYER, "1").url(),
            false => at,
        }
    }

    fn appearance(&self) -> Option<&'static str> {
        self.shell.state().appearance.map(|a| match a {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        })
    }

    fn native(&mut self, native: ShellAsk, _cx: &mut Cx<'_, Harken>) {
        match native {
            ShellAsk::Settings { .. } => self.settings = true,
            ShellAsk::Reset | ShellAsk::SignOut => self.settings = false,
            // Add a Device's page is a phone's.
            ShellAsk::AddDevice => {}
        }
    }
}

/// The query key the grown player is said by (`?player=1`).
pub const PLAYER: &str = "player";

impl Harken {
    /// Where the window is, under what has grown over it: Settings' pane
    /// while it shows, the page pushed over a section, else the section
    /// (the library's by its kind).
    fn place_address(&self, place: Seen<'_>, now_ms: i64) -> String {
        if self.settings {
            let pane = self.shell.page(place, now_ms).pane.to_lowercase();
            return Route::Settings.address().with("pane", pane).url();
        }
        let shell = self.shell.state();
        if shell.pushed.is_some() {
            if let Some(route) = &self.page.state().route {
                return route.url();
            }
        }
        match self.shell.page(place, now_ms).section.as_str() {
            "library" => match self.library.state().kind.as_str() {
                "albums" => Route::Albums.url(),
                "artists" => Route::Artists.url(),
                "songs" => Route::Library.url(),
                "podcasts" => Route::Podcasts.url(),
                "sermons" => Route::Sermons.url(),
                _ => "/library/playlists/".into(),
            },
            "search" => "/search/".into(),
            _ => "/".into(),
        }
    }
}

#[cfg(test)]
mod tests;
