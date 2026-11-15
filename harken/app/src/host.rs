//! harken's part of its host (`ark_app::host`), once for every platform:
//! over a session it runs every screen's runtime — the shell, Home,
//! Library, Search, the page a route shows and the player — and carries
//! out what each asks of the app ([`HarkenRequest`]): a route is a page
//! pushed over the section it was opened from; a play, and every press of
//! the player, is this device's player's ([`listen`]): a live call while
//! it stands in the session, its own sound while it is alone, the sound
//! made to agree with the session whenever the store or the sound moved
//! (ui/docs/app.md, "harken's Port").

use ark_app::host::{Cx, Has, HostedApp, Link, Running, Screen, Session};
use harken_domain::player::Press;
use harken_domain::{HarkenChange, HarkenView};

use crate::home::{Home, HomeMsg};
use crate::library::{Library, LibraryMsg};
use crate::page::{Page, PageMsg};
use crate::player::{Player, PlayerMsg};
use crate::search::{Search, SearchMsg};
use crate::shell::{HarkenShell, ShellMsg};
use crate::{HarkenRequest, Route};

pub mod listen;

use listen::Listen;

/// harken's runtimes over one session.
pub struct Harken {
    pub shell: Screen<HarkenShell>,
    pub home: Screen<Home>,
    pub library: Screen<Library>,
    pub search: Screen<Search>,
    pub page: Screen<Page>,
    pub player: Screen<Player>,
    /// This device's player: the reconciler and its sound.
    pub listen: Listen,
    /// What was asked that no runtime takes yet (the sheets'), in order.
    pub waiting: Vec<HarkenRequest>,
    /// Why the last press was not taken, while it is the last.
    pub refused: Option<String>,
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
            listen: Listen::open(session, cx),
            waiting: vec![],
            refused: None,
        };
        let server = session.server.clone();
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
        ]
    }

    fn screens_ref(&self) -> Vec<&dyn Running<Harken>> {
        vec![&self.shell, &self.home, &self.library, &self.search, &self.page, &self.player]
    }

    /// The listening session, and every song: what a press and a play are
    /// read against.
    fn holds(&self) -> Vec<HarkenView> {
        vec![HarkenView::Listening, listen::songs()]
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
            r @ HarkenRequest::AddTo { .. } => return self.waiting.push(r),
        };
        self.refused = self.listen.press(press, cx);
        self.listen.step(cx);
        self.tell(cx);
    }

    /// The store moved, or the sound did: the sound made to agree.
    fn moved(&mut self, cx: &mut Cx<'_, Harken>) {
        self.listen.step(cx);
        self.tell(cx);
    }

    fn link(&mut self, link: Link, cx: &mut Cx<'_, Harken>) {
        if let Link::Address(address) = link {
            let route = <Route as ark_router::Routable>::route_of(&address);
            self.route(route, cx)
        }
    }

    fn launched(&mut self, _: Vec<String>, _: &mut Cx<'_, Harken>) {}
}

impl Harken {
    /// The player told what it draws from that the views do not hold: who
    /// and where this device is, and what plays here alone — each only
    /// when it moved.
    fn tell(&mut self, cx: &mut Cx<'_, Harken>) {
        let s = cx.place.session();
        let linked = s.linked == Some(true);
        let st = self.player.state();
        if (st.server.as_str(), st.device.as_str(), st.linked) != (s.server.as_str(), s.device.as_str(), linked) {
            let msg = PlayerMsg::Session {
                server: s.server.clone(),
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

has!(HarkenShell => shell, Home => home, Library => library, Search => search, Page => page, Player => player);

#[cfg(test)]
mod tests;

#[cfg(test)]
mod listen_tests;
