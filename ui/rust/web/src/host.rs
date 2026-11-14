//! An Ark app in the browser, hosted once for every app (feature `host`;
//! ui/docs/app.md, "The App's Part of Its Host"): the page is the place its
//! part runs over ([`WebPlace`]: a replica of its domain in the page's
//! memory and the views it holds, the time, its device modules, who is
//! signed in where), [`WebHost`] drives `ark_app::host::Hosting` over it
//! and draws the window the part says (`ark_app::window::Windowed`): its
//! shell, the names it hosts, its sheets and its Settings.
//!
//! The household is the server's that served the page ([`crate::live`]):
//! the page signs in through it as the phones do, holds a replica and syncs
//! over its socket; the first run is every app's (`ark_app::welcome`). With
//! `?demo` it is the app's demo, in the page's memory, its own authority,
//! with no server, seeded to `?today=`: what the screenshots and a public
//! demo run.
//!
//! An app names nothing of its own here: its entry point is generated from
//! its app.toml (ui/prebuild.py's `web`, the phones' library's twin), a
//! [`WebHosted`] of its name, scheme, accent, domain and demo, and
//! [`run`]. The address opens any screen with no steps: the launch
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
use crate::{Address, AnyView, WebApp};

/// What an app is in the browser, beside its part of its host: what its
/// generated entry point says of its app.toml.
pub trait WebHosted: 'static {
    /// Its part of its host, drawn in a window (money's `money_app::host::Money`).
    type App: Windowed;
    /// Its name as a person reads it (`Money`), and the scheme its links
    /// come back on (`money`).
    const NAME: &'static str;
    const SCHEME: &'static str;
    /// Its accent (an `[accent.<name>]` of ui/theme.toml) and version.
    const ACCENT: &'static str;
    const VERSION: &'static str;
    /// Its icon (icon.svg): the mark its first run's pages show.
    const MARK: &'static str;
    /// The domain its replica's log is of.
    fn domain() -> Arc<ark::domain::Domain>;
    /// Its demo's household on a peer of its own, seeded up to `today` at
    /// `now_ms`.
    fn demo(today: &str, now_ms: i64) -> Peer;
    /// The browser's devices of the modules its crate depends on
    /// (module.toml's `[web]`).
    fn modules(_modules: &mut Modules) {}
}

type App<W> = <W as WebHosted>::App;
type Key<W> = <App<W> as HostedApp>::Key;
type Change<W> = <App<W> as HostedApp>::Change;
type Msg<W> = <App<W> as Windowed>::Msg;

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

/// The page: its replica and the views it holds, and the app's part
/// running every screen over it.
pub struct WebHost<W: WebHosted> {
    pub peer: Peer,
    pub store: Store<Key<W>>,
    /// The session's day (`2026-11-10`).
    pub today: String,
    pub hosting: Hosting<App<W>>,
    /// How many changes this page has sent its data.
    pub changes: u64,
    /// Changes counted and not sent (`WebApp::dry_run`).
    pub dry: bool,
    /// The page on its server, signed in there; none for the demo.
    pub live: Option<Live>,
    w: PhantomData<W>,
}

/// Noon on `today`, in milliseconds: the time a written page is worked out
/// at, and a demo's when its day is not the clock's.
pub fn noon(today: &str) -> i64 {
    ark_format::noon_of(today).unwrap_or(0)
}

/// The page as the place the app's part runs over (`ark_app::host::Place`).
pub struct WebPlace<'a, W: WebHosted> {
    peer: &'a mut Peer,
    store: &'a mut Store<Key<W>>,
    live: Option<&'a Live>,
    today: &'a str,
    changes: &'a mut u64,
    dry: bool,
}

/// Who is signed in where, as the page knows it: its server and login, or
/// the demo in the page alone (which has no way out).
fn session_of<W: WebHosted>(peer: &Peer, live: Option<&Live>, today: &str) -> Session {
    let version = W::VERSION.to_string();
    let Some(live) = live else {
        return Session {
            demo: true,
            today: today.into(),
            version,
            ..Default::default()
        };
    };
    let status = peer.status();
    let user = live.login.as_ref().map(|l| l.user.clone()).unwrap_or_default();
    Session {
        demo: false,
        today: today.into(),
        name: user.name.clone(),
        user: user.id.clone(),
        email: user.email.clone(),
        picture: ark_auth::picture_url(&live.server, &user.id, &user.picture).unwrap_or_default(),
        server: ark_auth::onboarding::display(&live.server),
        linked: Some(status.linked),
        pending: status.pending as i64,
        version,
        leaves: live.login.is_some(),
        // The browser makes no code for another device.
        adds_devices: false,
    }
}

/// The views as a screen reads them: a file a row names read from the
/// replica's memory (`data:`).
fn read_files<K: ark_app::ViewKey>(peer: &Peer, store: &Store<K>, f: &mut dyn FnMut(&dyn ark::views::Views<K>)) {
    f(&ark::views::WithFiles {
        views: store,
        url: |file: &ark::file::FileRef| peer.file_url(file),
    })
}

/// The browser's devices: the app's modules' (module.toml's `[web]`).
fn modules<W: WebHosted>() -> Modules {
    let mut m = Modules::new();
    W::modules(&mut m);
    m
}

impl<W: WebHosted> Place<Key<W>, Change<W>> for WebPlace<'_, W> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<Key<W>>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        clock(self.today)
    }
    fn send(&mut self, change: Change<W>) -> Result<(), String> {
        *self.changes += 1;
        if self.dry {
            return Ok(());
        }
        let (name, input) = App::<W>::encode(&change);
        let r = self.peer.mutate_bytes(name, input).map(|_| ());
        self.store.settle(self.peer);
        r
    }
    fn hold(&mut self, key: Key<W>) {
        self.store.hold(self.peer, key);
        self.store.settle(self.peer);
    }
    fn release(&mut self, key: &Key<W>) {
        self.store.release(self.peer, key)
    }
    fn modules(&self) -> Modules {
        modules::<W>()
    }
    /// A file chosen later wakes the page, which drains its screens.
    fn waker(&self) -> ark_app::host::Waker {
        Arc::new(|| crate::send(HostMsg::<Msg<W>>::Answered))
    }
    fn want(&mut self, file: ark::file::FileRef) {
        let _ = self.peer.open_file(&file);
    }
    fn keep(&mut self, bytes: &[u8], media: &str) -> Option<ark::file::FileRef> {
        self.peer.put_file(bytes, media).ok()
    }
    fn session(&self) -> Session {
        session_of::<W>(self.peer, self.live, self.today)
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
    peer: &'a Peer,
    store: &'a Store<Key<W>>,
    live: Option<&'a Live>,
    today: &'a str,
}

impl<W: WebHosted> Place<Key<W>, Change<W>> for Seen<'_, W> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<Key<W>>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        clock(self.today)
    }
    fn send(&mut self, _: Change<W>) -> Result<(), String> {
        Err("a page drawn sends nothing".into())
    }
    fn hold(&mut self, _: Key<W>) {}
    fn release(&mut self, _: &Key<W>) {}
    fn session(&self) -> Session {
        session_of::<W>(self.peer, self.live, self.today)
    }
}

/// The app on the page, from its address: the demo's household with
/// `?demo` (seeded to `?today=`), else its server's, the page's own origin.
pub fn run<W: WebHosted>() {
    crate::set_app_mark(W::MARK);
    let now = ark_format::day_of(ark_peer::now_ms());
    let host = match crate::query("demo") {
        Some(_) => WebHost::<W>::demo(crate::query("today").filter(|d| ark_format::noon_of(d).is_some()).unwrap_or(now)),
        None => WebHost::<W>::on_server(origin(), now),
    };
    crate::run(host, W::ACCENT, "");
}

/// Whoever served the page: an app's server serves it beside its socket
/// and sign-in, so the page's own origin is its server.
fn origin() -> String {
    web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default()
}

impl<W: WebHosted> WebHost<W> {
    /// The app over its demo's household, seeded to `today`.
    pub fn demo(today: String) -> WebHost<W> {
        let now = noon(&today);
        WebHost::over(W::demo(&today, now), today, None)
    }

    /// The page over `peer` (on its server when `live`): the app's part
    /// opened on its session.
    pub fn over(mut peer: Peer, today: String, live: Option<Live>) -> WebHost<W> {
        let mut store = Store::new();
        let mut changes = 0;
        let hosting = {
            let mut place = WebPlace::<W> {
                peer: &mut peer,
                store: &mut store,
                live: live.as_ref(),
                today: &today,
                changes: &mut changes,
                dry: false,
            };
            let session = place.session();
            Hosting::open(&session, &mut place)
        };
        WebHost {
            peer,
            store,
            today,
            hosting,
            changes,
            dry: false,
            live,
            w: PhantomData,
        }
    }

    /// The page, as a place read.
    pub fn seen(&self) -> Seen<'_, W> {
        Seen {
            peer: &self.peer,
            store: &self.store,
            live: self.live.as_ref(),
            today: &self.today,
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
            today,
            hosting,
            changes,
            dry,
            live,
            ..
        } = self;
        let mut place = WebPlace::<W> {
            peer,
            store,
            live: live.as_ref(),
            today,
            changes,
            dry: *dry,
        };
        f(hosting, &mut place)
    }

    /// Signed out of the page's server: the session revoked, the sign-in
    /// machine back to its start. The demo's page has no way out.
    fn sign_out(&mut self) {
        if self.live.is_some() {
            let _ = self.peer.sign_out();
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
        if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
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
        if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
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
        let shell = <App<W> as Windowed>::WINDOW;
        let window = crate::under(shell, || crate::shell(app.window(&seen, now).map(HostMsg::App), &hosts));
        let settings = crate::under(shell, || {
            app.settings(&seen, now)
                .map(|(panes, done)| crate::parts::settings_window(panes.map(HostMsg::App), &hosts, HostMsg::App(done)))
        });
        let over = crate::under(<App<W> as Windowed>::OVER, || crate::render(app.over(&seen, now).map(HostMsg::App)));
        (window, settings, over).into_any()
    }
}

/// The launch arguments every shell reads, as the address's query says
/// them (`?screen=budget&inspector=1`), or as the page was written with.
const LAUNCH_KEYS: [&str; 5] = ["screen", "inspector", "appearance", "pane", "settingsTab"];

impl<W: WebHosted> WebApp for WebHost<W> {
    type Msg = HostMsg<Msg<W>>;

    fn view(&self) -> AnyView {
        self.say_idle();
        // The first run's pages while the app is not running yet, and its
        // pages of what the app is over it while they show.
        if let Some(live) = self.live.as_ref().filter(|l| !l.running() || l.first_run.page().intro.is_some()) {
            return crate::render(ark_app::welcome::welcome::view(&live.first_run.page()).map(HostMsg::<Msg<W>>::Welcome));
        }
        self.window()
    }

    fn update(&mut self, msg: Self::Msg) {
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

    fn commands(&self) -> Vec<ark_app::shell::CommandMenu<Self::Msg>> {
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

    fn suggest(&self, field: &str, typed: &str) -> Vec<String> {
        self.app().suggest(field, typed, &self.seen())
    }

    /// The app's pages of what it is by their path (`/welcome/`), over it;
    /// the launch arguments when the query names a screen, else the address
    /// itself, taken as a link is (`HostedApp::link`).
    fn arrive(&mut self, route: &Address) -> Vec<Self::Msg> {
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

    fn route(&self) -> Address {
        Address::of_url(&self.app().address(&self.seen(), self.now_ms()), "/")
    }

    fn changes(&self) -> u64 {
        self.changes
    }

    fn dry_run(&mut self, on: bool) {
        self.dry = on;
    }

    /// The app's part opened again over the same data: every screen as a
    /// load finds it.
    fn rewind(&mut self) -> bool {
        let fresh = self.with_place(|_, place| {
            let session = place.session();
            Hosting::open(&session, place)
        });
        let old = std::mem::replace(&mut self.hosting, fresh);
        self.with_place(|_, place| old.close(place));
        self.follow();
        true
    }
}
