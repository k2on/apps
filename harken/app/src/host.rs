//! harken's part of its host (`ark_app::host`), once for every platform:
//! over a session it runs every screen's runtime — the shell, Home,
//! Library, Search, the page a route shows and the player — and carries
//! out what each asks of the app ([`HarkenRequest`]): a route is a page
//! pushed over the section it was opened from; a play, and every press of
//! the player, is this device's player's ([`listen`]): a live call while
//! it stands in the session, its own sound while it is alone, the sound
//! made to agree with the session whenever the store or the sound moved
//! (ui/docs/app.md, "harken's Port").

use ark_app::host::{Cx, Form, Has, HostedApp, Link, LiveSpace, Running, Screen, Session, ShellAsk, ThisDevice};
use harken_domain::host::Kind;
use harken_domain::player::{Change, Press};
use harken_domain::{HarkenChange, HarkenView};

use crate::home::{Home, HomeMsg};
use crate::library::{Library, LibraryMsg};
use crate::page::{Page, PageMsg};
use crate::player::{Player, PlayerMsg};
use crate::search::{Search, SearchMsg};
use crate::sheets::{Sheets, SheetsMsg};
use crate::shell::{HarkenShell, ShellMsg};
use crate::{HarkenRequest, Route};

pub mod car;
pub mod listen;

use car::Car;
use listen::Listen;

/// harken's runtimes over one session.
pub struct Harken {
    pub shell: Screen<HarkenShell>,
    pub home: Screen<Home>,
    pub library: Screen<Library>,
    pub search: Screen<Search>,
    pub page: Screen<Page>,
    pub player: Screen<Player>,
    pub sheets: Screen<Sheets>,
    /// This device's player: the reconciler and its sound.
    pub listen: Listen,
    /// What a car browses.
    pub car: Car,
    /// Why the last press was not taken, while it is the last.
    pub refused: Option<String>,
    /// Settings shows, in a window its host draws itself
    /// (`ark_app::window`); a phone's shell puts up its own.
    pub settings: bool,
}

/// `n` tracks' order shuffled from `seed`: a permutation, the same for one
/// seed (the moment it was pressed).
fn shuffled(n: usize, seed: i64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut x = seed as u64 | 1;
    for i in (1..n).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        order.swap(i, (x % (i as u64 + 1)) as usize);
    }
    order
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

/// The path an address names, its scheme, host and query gone: `search`
/// for `/search/`, `harken://search` and `harken:///search?x=1`.
fn section_of(address: &str) -> &str {
    let path = address.split(['?', '#']).next().unwrap_or_default();
    let path = match path.split_once("://") {
        Some((scheme, rest)) if scheme.starts_with("http") => rest.find('/').map_or("", |i| &rest[i..]),
        Some((_, rest)) => rest,
        None => path,
    };
    path.trim_matches('/')
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
            player: Screen::new(),
            sheets: Screen::new(),
            listen: Listen::open(session, cx),
            car: Car::default(),
            refused: None,
            settings: false,
        };
        let server = session.url.clone();
        h.home.send(HomeMsg::Server { server: server.clone() }, cx);
        h.library.send(LibraryMsg::Server { server: server.clone() }, cx);
        h.search.send(SearchMsg::Server { server: server.clone() }, cx);
        h.page.send(PageMsg::Server { server }, cx);
        h.tell(cx);
        h
    }

    fn screens(&mut self) -> Vec<&mut dyn Running<Harken>> {
        vec![
            &mut self.shell,
            &mut self.home,
            &mut self.library,
            &mut self.search,
            &mut self.page,
            &mut self.player,
            &mut self.sheets,
        ]
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Harken>> {
        vec![
            &self.shell,
            &self.home,
            &self.library,
            &self.search,
            &self.page,
            &self.player,
            &self.sheets,
        ]
    }

    /// The listening session, and every song: what a press and a play are
    /// read against; and what a car has browsed.
    fn holds(&self) -> Vec<HarkenView> {
        let mut all = vec![HarkenView::Listening, listen::songs()];
        all.extend(crate::words::Covers::VIEWS);
        all.extend(self.car.held.iter().cloned());
        all
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

    fn request(&mut self, r: HarkenRequest, cx: &mut Cx<'_, Harken>) {
        let press = match r {
            HarkenRequest::Play { tracks, at, from, shuffle } => {
                let mut queue = Listen::tracks(&tracks, cx);
                let mut at = at.clamp(0, queue.len().saturating_sub(1) as i64);
                if shuffle {
                    let order = shuffled(queue.len(), cx.now_ms);
                    queue = order.into_iter().map(|i| queue[i].clone()).collect();
                    at = 0;
                }
                if queue.is_empty() {
                    return;
                }
                Press::Start {
                    queue,
                    at,
                    position_ms: 0,
                    from,
                    shuffled: shuffle,
                }
            }
            HarkenRequest::Queue { track, next } => {
                let tracks = Listen::tracks(&[track], cx);
                match next {
                    true => Press::Next { tracks },
                    false => Press::Append { tracks },
                }
            }
            HarkenRequest::Toggle => Press::Toggle,
            HarkenRequest::Skip { by } => Press::Skip { by },
            HarkenRequest::Seek { at_ms } => Press::Seek { ms: at_ms },
            HarkenRequest::Scrub { at_ms } => Press::Scrub { at_ms: Some(at_ms) },
            HarkenRequest::Transfer { to } => Press::Transfer { to },
            HarkenRequest::Volume { level } => self.listen.loud(Change::To { level }, cx),
            HarkenRequest::Mute { muted } => self.listen.loud(Change::Mute { muted }, cx),
            HarkenRequest::AddTo { track } => return self.sheets.send(SheetsMsg::AddTo { track }, cx),
            HarkenRequest::Follow { kind } => return self.sheets.send(SheetsMsg::Follow { kind }, cx),
            HarkenRequest::Settings { pane } => return cx.native(ShellAsk::Settings { pane }),
        };
        self.refused = self.listen.press(press, cx);
        self.listen.step(cx);
        self.tell(cx);
    }

    /// The store moved, or the sound did: the sound made to agree, and
    /// what a car asked answered.
    fn moved(&mut self, cx: &mut Cx<'_, Harken>) {
        self.listen.step(cx);
        let server = cx.place.session().url;
        for (parent, items) in self.car.step(self.listen.asks(), &server, cx) {
            self.listen.children(parent, items);
        }
        self.tell(cx);
    }

    /// A link: what shows of the shell's own put away (a sheet, the full
    /// player, Settings), then a launch's arguments, or an address — a
    /// section's own (`/`, `/search`), Settings', else a route's page.
    fn link(&mut self, link: Link, cx: &mut Cx<'_, Harken>) {
        self.sheets.send(SheetsMsg::Dismiss, cx);
        self.shell.send(ShellMsg::ClosePlayer, cx);
        cx.native(ShellAsk::Reset);
        let address = match link {
            Link::Launch(args) => return self.shell.send(ShellMsg::Launch { args }, cx),
            Link::Address(address) => address,
        };
        match section_of(&address) {
            "" | "home" => self.shell.send(ShellMsg::Select { tag: "home".into() }, cx),
            "search" => self.shell.send(ShellMsg::Select { tag: "search".into() }, cx),
            "library/playlists" => {
                self.shell.send(ShellMsg::Select { tag: "library".into() }, cx);
                self.library.send(LibraryMsg::Show { kind: "playlists".into() }, cx)
            }
            // Settings, on the pane its query names (`?pane=diagnostics`).
            "settings" => {
                let pane = address.split_once("pane=").map(|(_, p)| p.split('&').next().unwrap_or_default());
                let mut args = vec!["-screen".to_string(), "settings".into()];
                args.extend(pane.into_iter().flat_map(|p| ["-pane".to_string(), p.to_string()]));
                self.shell.send(ShellMsg::Launch { args }, cx)
            }
            _ => {
                let route = <Route as ark_router::Routable>::route_of(&address);
                self.route(route, cx)
            }
        }
    }

    fn launched(&mut self, args: Vec<String>, cx: &mut Cx<'_, Harken>) {
        self.shell.send(ShellMsg::Launch { args }, cx)
    }

    /// The listening session, as this device stands in it: a phone (a
    /// tablet too) or a computer (a browser too), by its name, able to play
    /// (every host has the media player); the scrubber relayed as it is
    /// dragged.
    fn live(device: &ThisDevice) -> Option<LiveSpace> {
        let kind = match device.form {
            Form::Phone | Form::Tablet => Kind::Phone,
            Form::Computer | Form::Browser => Kind::Computer,
        };
        Some(LiveSpace {
            hello: harken_domain::host::hello(true, kind, &device.name),
            ephemeral: harken_domain::host::EPHEMERAL.iter().map(|e| e.to_string()).collect(),
            every_ms: harken_domain::host::RELAY_MS,
        })
    }
}

impl Harken {
    /// The player told what it draws from that the views do not hold: who
    /// and where this device is, and what plays here alone; and the shell
    /// whether anything plays, for its accessory — each only when it
    /// moved.
    fn tell(&mut self, cx: &mut Cx<'_, Harken>) {
        let s = cx.place.session();
        let linked = s.linked == Some(true);
        let st = self.player.state();
        if (st.server.as_str(), st.device.as_str(), st.linked) != (s.url.as_str(), s.device.as_str(), linked) {
            let msg = PlayerMsg::Session {
                server: s.url.clone(),
                device: s.device.clone(),
                linked,
            };
            self.player.send(msg, cx);
        }
        if self.player.state().own != self.listen.own {
            self.player.send(
                PlayerMsg::Own {
                    own: self.listen.own.clone(),
                },
                cx,
            );
        }
        let facts = crate::shell::Facts::of(&s);
        if self.shell.state().facts != facts {
            self.shell.send(ShellMsg::Facts { facts }, cx);
        }
        let playing = self.player.page(&*cx.place, cx.now_ms).has;
        if self.shell.state().playing != playing {
            self.shell.send(ShellMsg::Playing { on: playing }, cx);
        }
    }
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

has!(HarkenShell => shell, Home => home, Library => library, Search => search, Page => page, Player => player, Sheets => sheets);

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod listen_tests;
