//! harken's part of its host (`ark_app::host`), once for every platform:
//! over a session it runs every screen's runtime — the shell, Home,
//! Library, Search and the page a route shows — and carries out what each asks of
//! the app ([`HarkenRequest`]): a route is a page pushed over the section
//! it was opened from; a play is the player's, which keeps what it was
//! asked until it is a runtime of the host's own (ui/docs/app.md,
//! "harken's Port").

use ark_app::host::{Cx, Has, HostedApp, Link, Running, Screen, Session};
use harken_domain::{HarkenChange, HarkenView};

use crate::home::{Home, HomeMsg};
use crate::library::{Library, LibraryMsg};
use crate::page::{Page, PageMsg};
use crate::search::{Search, SearchMsg};
use crate::shell::{HarkenShell, ShellMsg};
use crate::{HarkenRequest, Route};

/// harken's runtimes over one session.
pub struct Harken {
    pub shell: Screen<HarkenShell>,
    pub home: Screen<Home>,
    pub library: Screen<Library>,
    pub search: Screen<Search>,
    pub page: Screen<Page>,
    /// What was asked of the player, in order, until it is the host's.
    pub played: Vec<HarkenRequest>,
}

/// The title a route's page is pushed under.
fn title(r: &Route) -> String {
    match r {
        Route::Album { title, .. } | Route::Work { title, .. } | Route::Recording { title, .. } | Route::Show { title, .. } => title.clone(),
        Route::Artist { name } => name.clone(),
        Route::Playlist { name, .. } => name.clone(),
        r => r.title(),
    }
}

impl HostedApp for Harken {
    type Key = HarkenView;
    type Change = HarkenChange;
    type Route = Route;
    type Request = HarkenRequest;
    const KEEPS: &'static str = "your library";

    fn welcome(scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy {
        crate::welcome::copy(scan, demo)
    }

    fn encode(change: &HarkenChange) -> (&'static str, Vec<u8>) {
        (change.name(), change.input())
    }

    fn open(session: &Session, cx: &mut Cx<'_, Harken>) -> Harken {
        let mut h = Harken {
            shell: Screen::new(),
            home: Screen::new(),
            library: Screen::new(),
            search: Screen::new(),
            page: Screen::new(),
            played: vec![],
        };
        let server = session.server.clone();
        h.home.send(HomeMsg::Server { server: server.clone() }, cx);
        h.library.send(LibraryMsg::Server { server: server.clone() }, cx);
        h.search.send(SearchMsg::Server { server: server.clone() }, cx);
        h.page.send(PageMsg::Server { server }, cx);
        h
    }

    fn screens(&mut self) -> Vec<&mut dyn Running<Harken>> {
        vec![&mut self.shell, &mut self.home, &mut self.library, &mut self.search, &mut self.page]
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Harken>> {
        vec![&self.shell, &self.home, &self.library, &self.search, &self.page]
    }

    /// A route a screen opened: a section's own index is that section (an
    /// index of the library its way); a record's page is pushed over the
    /// section shown.
    fn route(&mut self, r: Route, cx: &mut Cx<'_, Harken>) {
        let way = match &r {
            Route::Library => Some("songs"),
            Route::Albums => Some("albums"),
            Route::Artists | Route::Composers => Some("artists"),
            Route::Podcasts => Some("podcasts"),
            Route::Sermons => Some("sermons"),
            _ => None,
        };
        if let Some(kind) = way {
            self.shell.send(ShellMsg::Select { tag: "library".into() }, cx);
            return self.library.send(LibraryMsg::Show { kind: kind.into() }, cx);
        }
        match r {
            Route::Album { .. } | Route::Artist { .. } | Route::Playlist { .. } | Route::Show { .. } => {
                let t = title(&r);
                self.page.send(PageMsg::Show { route: r }, cx);
                self.shell.send(ShellMsg::Push { title: t }, cx);
            }
            // Settings, the transcript and the diagnostics are the shell's
            // own and the player's: not here yet.
            _ => {}
        }
    }

    fn request(&mut self, r: HarkenRequest, _: &mut Cx<'_, Harken>) {
        self.played.push(r)
    }

    fn link(&mut self, link: Link, cx: &mut Cx<'_, Harken>) {
        if let Link::Address(address) = link {
            let route = <Route as ark_router::Routable>::route_of(&address);
            self.route(route, cx)
        }
    }

    fn launched(&mut self, _: Vec<String>, _: &mut Cx<'_, Harken>) {}
}

macro_rules! has {
    ($($app:ty => $field:ident),*) => {$(
        impl Has<$app> for Harken {
            fn screen(&self) -> &Screen<$app> {
                &self.$field
            }
            fn screen_mut(&mut self) -> &mut Screen<$app> {
                &mut self.$field
            }
        }
    )*};
}

has!(HarkenShell => shell, Home => home, Library => library, Search => search, Page => page);

#[cfg(test)]
mod tests;
