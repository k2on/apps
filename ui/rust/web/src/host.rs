//! An Ark app in the browser, hosted once for every app (ui/docs/app.md,
//! "The App's Part of Its Host"): the page is the place its part runs over
//! ([`WebPlace`]: a replica of its domain in the page's memory and the
//! views it holds, the time, its device modules, who is signed in where),
//! and [`WebHost`] is the page itself: it drives `ark_app::host::Hosting`
//! over that place and draws the window the part says
//! (`ark_app::window::Windowed`) — its shell, the names it hosts, its sheets
//! and its Settings. Every page is a `WebHost`, whatever made it: drawn in
//! the browser alone ([`crate::run`]), written ahead by the export and
//! taken over ([`crate::export`], [`crate::hydrate`]), or written by a
//! server (arkui-serve).
//!
//! The household is the server's that served the page ([`crate::live`]):
//! the page signs in through it as the phones do, holds a replica and syncs
//! over its socket; the first run is every app's (`ark_app::welcome`). A
//! page published as the demo (`<html data-ark-demo>`, [`crate::page_demo`])
//! is the app's demo, in the page's memory, its own authority, with no
//! server, seeded to `?today=`: what the screenshots and the public demo
//! run. An app with no data of its own (the docs: no domain in its
//! app.toml) is the page alone, with no replica at all.
//!
//! An app names nothing of its own here: its entry point is generated from
//! its app.toml (ui/prebuild.py's `web`, the phones' library's twin), a
//! [`WebHosted`] of its name, scheme, accent, look, domain and demo, and
//! [`start`]. The address opens any screen with no steps: the launch
//! arguments every shell reads (`?screen=budget`, `?inspector=1`,
//! `?appearance=dark`, `?pane=household`), or the window's own address
//! (`Windowed::address`, read back as a link), and arkui-web's own
//! (`?context=`, `?menu=`, `?popover=`, `?command=`, `?keys=`,
//! `?theme=`).

use std::marker::PhantomData;
use std::sync::Arc;

use ark_app::host::{HostedApp, Hosting, Link, Place, Session};
use ark_app::window::Windowed;
use ark_app::Modules;
use ark_peer::store::Store;
use ark_peer::Peer;
use leptos::prelude::*;

use crate::live::Live;
use crate::{Address, AnyView};

/// What an app is in the browser, beside its part of its host: what its
/// generated entry point says of its app.toml. Never written by hand.
pub trait WebHosted: 'static {
    /// Its part of its host, drawn in a window (money's `money_app::host::Money`).
    type App: Windowed;
    /// Its id (`money`), whose module the page loads (`<id>-web`).
    const ID: &'static str;
    /// Its name as a person reads it (`Money`), and the scheme its links
    /// come back on (`money`).
    const NAME: &'static str;
    const SCHEME: &'static str;
    /// Its accent (an `[accent.<name>]` of ui/theme.toml) and version.
    const VERSION: &'static str;
    const ACCENT: &'static str;
    /// Its icon (icon.svg): the mark its first run's pages show, and the
    /// page's.
    const MARK: &'static str;
    /// Its own styles after ArkUI's (app.toml's `[web] css`).
    const CSS: &'static str = "";
    /// What every page it writes says in its `<head>` besides (app.toml's
    /// `[web] head`), `{base}` the site's base and `{version}` the build's.
    const HEAD: &'static str = "";
    /// Files its pages load, written beside them as they are (app.toml's
    /// `[web] files`): a script, a picture.
    const FILES: &'static [(&'static str, &'static [u8])] = &[];
    /// Its pages are written ahead (app.toml's `[web] modes` has `ssg`):
    /// the export writes each of its paths (`Windowed::paths`), whole
    /// without script, which its module takes over.
    const STATIC: bool = false;
    /// The domain its replica's log is of; none for an app with no data
    /// of its own, which is the page alone.
    fn domain() -> Option<Arc<ark::domain::Domain>> {
        None
    }
    /// Its demo's household on a peer of its own, seeded up to `today` at
    /// `now_ms`; none for an app with no demo.
    fn demo(_today: &str, _now_ms: i64) -> Option<Peer> {
        None
    }
    /// What it works out natively before its pages are written (app.toml's
    /// `[web] prepare`): files written into the export's directory (the
    /// docs' pictures), and what it is opened with (`Session::prepared`).
    #[cfg(not(target_arch = "wasm32"))]
    fn prepare(_out: &std::path::Path) -> std::io::Result<String> {
        Ok(String::new())
    }
    /// The browser's devices of the modules its crate depends on
    /// (module.toml's `[web]`).
    fn modules(_modules: &mut Modules) {}
}

type App<W> = <W as WebHosted>::App;
type Key<W> = <App<W> as HostedApp>::Key;
type Change<W> = <App<W> as HostedApp>::Change;
pub(crate) type Msg<W> = <App<W> as Windowed>::Msg;

/// Everything the page can be told.
#[derive(Clone, Debug)]
pub enum HostMsg<M> {
    /// For one of the app's screens, or its window.
    App(M),
    /// The socket has something to say, or the peer's deadline came
    /// ([`crate::live`]): it is pumped.
    Wake,
    /// A device's operation answered after its message (a file chosen):
    /// the screens take it.
    Answered,
    /// What came of one of the sign-in machine's effects.
    Onboard(Box<ark_auth::onboarding::Event>),
    /// A press on the first run's page.
    Welcome(ark_app::welcome::WelcomeMsg),
    /// Where the page's address asks the app to be: a link it takes.
    Arrive(Link),
}

/// Where a page is: its day, the site's base, and what the export worked
/// out for it — what the app's session says of the browser beside who is
/// signed in.
#[derive(Clone, Debug, Default)]
pub struct Here {
    /// The session's day (`2026-11-10`).
    pub today: String,
    /// Where the site is served from: `/`, `/apps/docs/`.
    pub base: String,
    /// What `WebHosted::prepare` returned (`Session::prepared`).
    pub prepared: String,
}

impl Here {
    /// The page the browser loaded, on `today`: its base and what was
    /// prepared for it as its export wrote them (`data-ark-base`,
    /// `#ark-prepared`).
    pub fn of_page(today: String) -> Here {
        let prepared = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("ark-prepared"))
            .and_then(|e| e.text_content())
            .unwrap_or_default();
        Here {
            today,
            base: crate::page_base(),
            prepared,
        }
    }
}

/// The page: its replica and the views it holds (none for an app with no
/// data), and the app's part running every screen over it.
pub struct WebHost<W: WebHosted> {
    pub peer: Option<Peer>,
    pub store: Store<Key<W>>,
    pub here: Here,
    pub hosting: Hosting<App<W>>,
    /// How many changes this page has sent its data.
    pub changes: u64,
    /// Changes counted and not sent ([`WebHost::dry_run`]).
    pub dry: bool,
    /// The page on its server, signed in there; none for the demo.
    pub live: Option<Live>,
    /// Who a server wrote the page for (arkui-serve's visitor): their
    /// login's account.
    pub visitor: Option<String>,
    /// Why the last change the page sent was refused there, in the
    /// domain's words, until taken ([`WebHost::take_refused`]).
    refused: Option<String>,
    w: PhantomData<W>,
}

/// Noon on `today`, in milliseconds: the time a written page is worked out
/// at, and a demo's when its day is not the clock's.
pub fn noon(today: &str) -> i64 {
    ark_format::noon_of(today).unwrap_or(0)
}

/// The page as the place the app's part runs over (`ark_app::host::Place`).
pub struct WebPlace<'a, W: WebHosted> {
    peer: Option<&'a mut Peer>,
    store: &'a mut Store<Key<W>>,
    live: Option<&'a Live>,
    visitor: Option<&'a str>,
    here: &'a Here,
    changes: &'a mut u64,
    refused: &'a mut Option<String>,
    dry: bool,
}

/// Who is signed in where, as the page knows it: its server and login, a
/// server's visitor, the demo in the page alone (which has no way out), or
/// nobody, on a page with no data.
fn session_of<W: WebHosted>(peer: Option<&Peer>, live: Option<&Live>, visitor: Option<&str>, here: &Here) -> Session {
    let page = Session {
        today: here.today.clone(),
        version: W::VERSION.to_string(),
        base: here.base.clone(),
        prepared: here.prepared.clone(),
        ..Default::default()
    };
    let (Some(peer), Some(live)) = (peer, live) else {
        return match (peer, visitor) {
            (Some(_), Some(user)) => Session {
                user: user.into(),
                linked: Some(true),
                ..page
            },
            (Some(_), None) => Session { demo: true, ..page },
            (None, _) => page,
        };
    };
    let status = peer.status();
    let user = live.login.as_ref().map(|l| l.user.clone()).unwrap_or_default();
    Session {
        name: user.name.clone(),
        user: user.id.clone(),
        email: user.email.clone(),
        picture: ark_auth::picture_url(&live.server, &user.id, &user.picture).unwrap_or_default(),
        server: ark_auth::onboarding::display(&live.server),
        url: live.server.clone(),
        linked: Some(status.linked),
        pending: status.pending as i64,
        device: peer.me().session.clone(),
        epoch: status.epoch as i64,
        leaves: live.login.is_some(),
        // The browser makes no code for another device.
        adds_devices: false,
        ..page
    }
}

/// The views as a screen reads them: a file a row names read from the
/// replica's memory (`data:`); none at all on a page with no data.
fn read_files<K: ark_app::ViewKey>(peer: Option<&Peer>, store: &Store<K>, f: &mut dyn FnMut(&dyn ark::views::Views<K>)) {
    match peer {
        Some(peer) => f(&ark::views::WithFiles {
            views: store,
            url: |file: &ark::file::FileRef| peer.file_url(file),
        }),
        None => f(store),
    }
}

/// The browser's devices: the app's modules' (module.toml's `[web]`).
fn modules<W: WebHosted>() -> Modules {
    let mut m = Modules::new();
    W::modules(&mut m);
    m
}

impl<W: WebHosted> Place<Key<W>, Change<W>> for WebPlace<'_, W> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<Key<W>>)) {
        read_files(self.peer.as_deref(), self.store, f)
    }
    fn now_ms(&self) -> i64 {
        clock(&self.here.today)
    }
    /// The page's own, demo or not: what a sound is timed by.
    fn clock(&self) -> ark_app::host::Clock {
        std::sync::Arc::new(ark_peer::now_ms)
    }
    fn send(&mut self, change: Change<W>) -> Result<(), String> {
        *self.changes += 1;
        if self.dry {
            return Ok(());
        }
        let Some(peer) = self.peer.as_deref_mut() else {
            return Err("this page has no data to change".into());
        };
        let (name, input) = App::<W>::encode(&change);
        let r = peer.change(name, input);
        self.store.settle(peer);
        if let Err(why) = &r {
            *self.refused = Some(why.clone());
        }
        r
    }
    fn hold(&mut self, key: Key<W>) {
        if let Some(peer) = self.peer.as_deref_mut() {
            self.store.hold(peer, key);
            self.store.settle(peer);
        }
    }
    fn release(&mut self, key: &Key<W>) {
        if let Some(peer) = self.peer.as_deref_mut() {
            self.store.release(peer, key)
        }
    }
    fn modules(&self) -> Modules {
        modules::<W>()
    }
    /// A file chosen later wakes the page, which drains its screens.
    fn waker(&self) -> ark_app::host::Waker {
        Arc::new(|| crate::send(HostMsg::<Msg<W>>::Answered))
    }
    fn want(&mut self, file: ark::file::FileRef) {
        if let Some(peer) = self.peer.as_deref_mut() {
            let _ = peer.open_file(&file);
        }
    }
    fn keep(&mut self, bytes: &[u8], media: &str) -> Option<ark::file::FileRef> {
        self.peer.as_deref_mut()?.put_file(bytes, media).ok()
    }
    fn session(&self) -> Session {
        session_of::<W>(self.peer.as_deref(), self.live, self.visitor, self.here)
    }
}

/// The time on the page: the clock, and noon on the session's day where
/// there is none (a page written natively) — the app's part puts a demo's
/// fixed day on its own (`HostedApp::now_ms`).
fn clock(today: &str) -> i64 {
    match cfg!(target_arch = "wasm32") {
        true => ark_peer::now_ms(),
        false => noon(today),
    }
}

/// The page as a place only read (a page drawn, the idle signal): it sends
/// and holds nothing.
pub struct Seen<'a, W: WebHosted> {
    peer: Option<&'a Peer>,
    store: &'a Store<Key<W>>,
    live: Option<&'a Live>,
    visitor: Option<&'a str>,
    here: &'a Here,
}

impl<W: WebHosted> Place<Key<W>, Change<W>> for Seen<'_, W> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<Key<W>>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        clock(&self.here.today)
    }
    fn send(&mut self, _: Change<W>) -> Result<(), String> {
        Err("a page drawn sends nothing".into())
    }
    fn hold(&mut self, _: Key<W>) {}
    fn release(&mut self, _: &Key<W>) {}
    fn session(&self) -> Session {
        session_of::<W>(self.peer, self.live, self.visitor, self.here)
    }
}

/// The app on the page the browser loaded: taking over the page its export
/// or its server wrote (`data-ark-mode`), or drawing it alone (the SPA). An
/// app with no data is the page alone; one with a domain is its demo's
/// household on a page published as the demo (`data-ark-demo`, seeded to
/// `?today=`), else its server's, the page's own origin.
pub fn start<W: WebHosted>() {
    crate::set_app_mark(W::MARK);
    let now = ark_format::day_of(ark_peer::now_ms());
    let today = crate::query("today").filter(|d| ark_format::noon_of(d).is_some()).unwrap_or(now.clone());
    let written = crate::root_element()
        .and_then(|r| r.get_attribute("data-ark-mode"))
        .and_then(|m| crate::page::Mode::of_word(&m))
        .is_some_and(|m| m != crate::page::Mode::Spa);
    let host = match (W::domain(), crate::page_demo()) {
        (None, _) => WebHost::<W>::over(None, Here::of_page(today), None),
        (Some(_), true) => WebHost::<W>::demo(Here::of_page(today)),
        (Some(_), false) => WebHost::<W>::on_server(origin(), Here::of_page(now)),
    };
    // A page written ahead is taken over by a module built to (`hydrate`).
    #[cfg(feature = "hydrate")]
    if written {
        return crate::hydrate(host);
    }
    let _ = written;
    crate::run(host)
}

/// Whoever served the page: an app's server serves it beside its socket
/// and sign-in, so the page's own origin is its server.
fn origin() -> String {
    web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default()
}

impl<W: WebHosted> WebHost<W> {
    /// The app over its demo's household, seeded to `here`'s day; the page
    /// alone for an app with no demo.
    pub fn demo(here: Here) -> WebHost<W> {
        let now = noon(&here.today);
        WebHost::over(W::demo(&here.today, now), here, None)
    }

    /// The page as a server writes it for `visitor` over its replica of
    /// the server's own hub (arkui-serve).
    pub fn visiting(peer: Peer, here: Here, visitor: String) -> WebHost<W> {
        WebHost::open(Some(peer), here, None, Some(visitor))
    }

    /// The page over `peer` (on its server when `live`; none for a page
    /// with no data): the app's part opened on its session.
    pub fn over(peer: Option<Peer>, here: Here, live: Option<Live>) -> WebHost<W> {
        WebHost::open(peer, here, live, None)
    }

    fn open(mut peer: Option<Peer>, here: Here, live: Option<Live>, visitor: Option<String>) -> WebHost<W> {
        let mut store = Store::new();
        let mut changes = 0;
        let mut refused = None;
        let hosting = {
            let mut place = WebPlace::<W> {
                peer: peer.as_mut(),
                store: &mut store,
                live: live.as_ref(),
                visitor: visitor.as_deref(),
                here: &here,
                changes: &mut changes,
                refused: &mut refused,
                dry: false,
            };
            let session = place.session();
            Hosting::open(&session, &mut place)
        };
        WebHost {
            peer,
            store,
            here,
            hosting,
            changes,
            dry: false,
            live,
            visitor,
            refused,
            w: PhantomData,
        }
    }

    /// The page, as a place read.
    pub fn seen(&self) -> Seen<'_, W> {
        Seen {
            peer: self.peer.as_ref(),
            store: &self.store,
            live: self.live.as_ref(),
            visitor: self.visitor.as_deref(),
            here: &self.here,
        }
    }

    /// The app's part, as it is.
    pub fn app(&self) -> &App<W> {
        self.hosting.app()
    }

    /// The time its pages are worked out at, now.
    pub fn now_ms(&self) -> i64 {
        self.hosting.now_ms(&self.seen())
    }

    /// Why the last change the page sent was refused, in the domain's
    /// words, taken.
    pub fn take_refused(&mut self) -> Option<String> {
        self.refused.take()
    }

    /// `f` over the app's part and the page as its place; what it asked of
    /// the page carried out after: signing out, and the rest by the app's
    /// part in its window (Settings shown).
    pub(crate) fn host<R>(&mut self, f: impl FnOnce(&mut Hosting<App<W>>, &mut WebPlace<'_, W>) -> R) -> R {
        let r = self.with_place(f);
        for n in self.hosting.take_natives() {
            if n == ark_app::host::ShellAsk::SignOut {
                self.sign_out();
            }
            self.with_place(|h, place| h.with(place, |app, cx| app.native(n, cx)));
        }
        self.appearance();
        r
    }

    fn with_place<R>(&mut self, f: impl FnOnce(&mut Hosting<App<W>>, &mut WebPlace<'_, W>) -> R) -> R {
        let WebHost {
            peer,
            store,
            here,
            hosting,
            changes,
            dry,
            live,
            visitor,
            refused,
            ..
        } = self;
        let mut place = WebPlace::<W> {
            peer: peer.as_mut(),
            store,
            live: live.as_ref(),
            visitor: visitor.as_deref(),
            here,
            changes,
            refused,
            dry: *dry,
        };
        f(hosting, &mut place)
    }

    /// Signed out of the page's server: the session revoked, the sign-in
    /// machine back to its start. The demo's page has no way out.
    fn sign_out(&mut self) {
        if self.live.is_some() {
            if let Some(peer) = self.peer.as_mut() {
                let _ = peer.sign_out();
            }
            self.onboard(ark_auth::onboarding::Event::SignOut);
        }
    }

    /// What the window cannot follow: a page the app opens in a tab of its
    /// own, taken as closed once opened.
    fn follow(&mut self) {
        let now = self.now_ms();
        if let Some((url, opened)) = self.app().opens(&self.seen(), now) {
            #[cfg(target_arch = "wasm32")]
            if let Some(w) = web_sys::window() {
                let _ = w.open_with_url_and_target(&url, "_blank");
            }
            let _ = url;
            self.host(|h, place| h.with(place, |app, cx| app.take(opened, cx)));
        }
    }

    /// Whether nothing is left to come on the page; with
    /// [`WebHost::version`] unchanged, what a screenshot driver waits for.
    pub fn is_idle(&self) -> bool {
        self.hosting.idle(&self.seen())
    }

    /// The screens' versions summed: grows with every message any took.
    pub fn version(&self) -> u64 {
        self.hosting.version()
    }

    /// The ready signal on the page's root, read by the screenshot drivers
    /// (ui/docs/app.md, Effects and Ports): `data-ark-idle` is `1` once
    /// nothing is left to come, `data-ark-version` the version drawn; and
    /// `data-ark-address` where the app is (`Windowed::address`), what a
    /// load of that address brings back.
    fn say_idle(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = crate::root_element() {
            let _ = root.set_attribute("data-ark-idle", if self.is_idle() { "1" } else { "0" });
            let _ = root.set_attribute("data-ark-version", &self.version().to_string());
            if self.live.as_ref().is_none_or(|l| l.running()) {
                let _ = root.set_attribute("data-ark-address", &self.app().address(&self.seen(), self.now_ms()));
            }
        }
    }

    /// The appearance the app asks for over the system's (a launch's,
    /// Settings'): the page's `data-theme`, or else the address's own
    /// `?theme=`, or none.
    fn appearance(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(root) = crate::root_element() {
            let asked = self
                .app()
                .appearance()
                .map(String::from)
                .or_else(|| crate::query("theme").filter(|t| t == "light" || t == "dark"));
            let _ = match asked {
                Some(t) => root.set_attribute("data-theme", &t),
                None => root.remove_attribute("data-theme"),
            };
        }
    }

    /// The window the app's part says: its shell, each name it hosts, its
    /// sheets over it, and Settings in a window of its own.
    fn window(&self) -> AnyView {
        let seen = self.seen();
        let now = self.now_ms();
        let app = self.app();
        // Each drawn as its screen, its elements' walk ids under its name.
        let hosts = |name: &'static str| crate::under(name, || crate::render(app.hosted(name, &seen, now).map(HostMsg::App)));
        let shell = <App<W> as ark_app::manifest::Declared>::SHELL;
        let window = crate::under(shell, || crate::shell(app.window(&seen, now).map(HostMsg::App), &hosts));
        let settings = crate::under(shell, || {
            app.settings(&seen, now)
                .map(|(panes, done)| crate::parts::settings_window(panes.map(HostMsg::App), &hosts, HostMsg::App(done)))
        });
        let over = crate::under(<App<W> as ark_app::manifest::Declared>::OVER, || {
            crate::render(app.over(&seen, now).map(HostMsg::App))
        });
        (window, settings, over).into_any()
    }

    /// The page's view: the first run's pages while the app is not running
    /// yet, and its pages of what the app is over it while they show; else
    /// the window.
    pub fn view(&self) -> AnyView {
        self.say_idle();
        if let Some(live) = self.live.as_ref().filter(|l| !l.running() || l.first_run.page().intro.is_some()) {
            return crate::render(ark_app::welcome::welcome::view(&live.first_run.page()).map(HostMsg::<Msg<W>>::Welcome));
        }
        self.window()
    }

    /// A message taken.
    pub fn update(&mut self, msg: HostMsg<Msg<W>>) {
        match msg {
            HostMsg::App(m) => self.host(|h, place| h.with(place, |app, cx| app.take(m, cx))),
            HostMsg::Wake => {
                self.wake();
                self.host(|h, place| h.moved(place));
            }
            HostMsg::Answered => self.host(|h, place| h.moved(place)),
            HostMsg::Onboard(e) => self.onboard(*e),
            HostMsg::Welcome(m) => self.first_run(m),
            HostMsg::Arrive(link) => self.host(|h, place| h.link(link, place)),
        }
        self.follow();
    }

    /// The menu bar's commands: their shortcuts are the keys', their
    /// titles the command box's.
    pub fn commands(&self) -> Vec<ark_app::shell::CommandMenu<HostMsg<Msg<W>>>> {
        self.app()
            .commands()
            .into_iter()
            .map(|m| ark_app::shell::CommandMenu {
                place: m.place,
                items: m
                    .items
                    .into_iter()
                    .map(|c| ark_app::shell::CommandItem {
                        title: c.title,
                        shortcut: c.shortcut,
                        run: HostMsg::App(c.run),
                    })
                    .collect(),
            })
            .collect()
    }

    /// What the field `field` (`search`, or a form's own id) suggests for
    /// what is typed in it.
    pub fn suggest(&self, field: &str, typed: &str) -> Vec<String> {
        self.app().suggest(field, typed, &self.seen())
    }

    /// What the page's address asks for: the messages that bring the app
    /// there. The app's pages of what it is by their path (`/welcome/`),
    /// over it; the launch arguments when the query names a screen, else
    /// the address itself, taken as a link is (`HostedApp::link`). A
    /// static page arrives at its path alone and then, once the module
    /// runs, at the whole address.
    pub fn arrive(&mut self, route: &Address) -> Vec<HostMsg<Msg<W>>> {
        if route.path.first().map(String::as_str) == Some("welcome") {
            self.show_intro();
        }
        let mut args = vec![];
        for key in LAUNCH_KEYS {
            if let Some(v) = route
                .get(key)
                .map(str::to_string)
                .or_else(|| crate::page::args().into_iter().find(|(k, _)| k == key).map(|(_, v)| v))
            {
                args.push(format!("-{key}"));
                args.push(v);
            }
        }
        // A launch names its screen (`?screen=budget&inspector=1`); any
        // other address is read whole, its path the place and its query the
        // UI over it (`/settings/?pane=banks`).
        vec![HostMsg::Arrive(match args.first().map(String::as_str) == Some("-screen") {
            true => Link::Launch(args),
            false => Link::Address(route.url()),
        })]
    }

    /// Where the app is, as the address that brings it back there
    /// (`arrive` reads it): its screen, its selection, its sheet. A page
    /// written as HTML is at its route, and a press that moves the app
    /// leads to its new one (`page`).
    pub fn route(&self) -> Address {
        Address::of_url(&self.app().address(&self.seen(), self.now_ms()), "/")
    }

    /// The window's title (`Windowed::title`), or the app's name.
    pub fn title(&self) -> String {
        self.app().title(&self.seen(), self.now_ms()).unwrap_or_else(|| W::NAME.to_string())
    }

    /// What the window shows, in a sentence, where the app says one.
    pub fn description(&self) -> Option<String> {
        self.app().description(&self.seen(), self.now_ms())
    }

    /// Every path the export writes a page of (`Windowed::paths`), each
    /// bare.
    pub fn paths(&self) -> Vec<Address> {
        let mut out: Vec<Address> = vec![];
        for p in self.app().paths(&self.seen()) {
            let a = Address::of_url(&p, "/").bare();
            if !out.contains(&a) {
                out.push(a);
            }
        }
        out
    }

    /// How many changes the app has made to its data: a press that makes
    /// one is an action, not a way to another page.
    pub fn changes(&self) -> u64 {
        self.changes
    }

    /// While `on`, a change the app would send its data is counted
    /// ([`WebHost::changes`]) and not sent: the export presses every
    /// control, and one that would change the data is an action, not a way.
    pub fn dry_run(&mut self, on: bool) {
        self.dry = on;
    }

    /// The app's part opened again over the same data: every screen as a
    /// load finds it, what the export presses each control from and what
    /// back and forward start from.
    pub fn rewind(&mut self) {
        let fresh = self.with_place(|_, place| {
            let session = place.session();
            Hosting::open(&session, place)
        });
        let old = std::mem::replace(&mut self.hosting, fresh);
        self.with_place(|_, place| old.close(place));
        self.follow();
    }

    /// Back or forward: the app's part opened again as [`WebHost::rewind`]
    /// opens it, for the address to bring where it says, with what the
    /// device holds kept (`HostedApp::keep`): a sound that plays plays on,
    /// its bar and controls with it, as a phone's back leaves it.
    pub fn went(&mut self) {
        self.with_place(|hosting, place| {
            let session = place.session();
            hosting.reopen_keeping(&session, place)
        });
        self.follow();
    }
}

/// The launch arguments every shell reads, as the address's query says
/// them (`?screen=budget&inspector=1`), or as the page was written with.
const LAUNCH_KEYS: [&str; 5] = ["screen", "inspector", "appearance", "pane", "settingsTab"];
