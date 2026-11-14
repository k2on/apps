//! An app's part of its host (`ark_app::host::HostedApp`) on the phones:
//! the place a phone gives it ([`PhonePlace`]: the session's views over the
//! process's peer, the process's device modules, the login), and the
//! [`HostApp`] that runs it under the [`ArkHost`] every app has
//! ([`Hosted`]). An app's bridge names only what is its own
//! ([`PhoneApp`]: how its replica and its demo open, how a change is
//! encoded) and exports the objects the phone holds over this, one per
//! screen ([`hosted_screen!`]).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use ark_app::host::{Has, HostedApp, Hosting, Link, Place, Screen, Session as Facts, ShellAsk, Waker};
use ark_app::{App, ViewKey, Views};
use ark_auth::onboarding::AppInfo;

use crate::{ArkProcess, Handle, Hold, HostApp, Login, Opened, Peer, Refusal, Session};

/// What an app is on the phones, beside its part of its host: its name and
/// scheme (app.toml's), the domain its replica's log is of, and its demo.
/// What the app's generated library ([`phone_app!`], from app.toml's
/// `[ffi]`) implements; everything else a phone asks of the app is its
/// part's ([`HostedApp`]).
pub trait PhoneApp: Send + Sync + 'static {
    /// The app's part of its host (money's `money_app::host::Money`).
    type App: HostedApp<Route: Send, Request: Send> + Send;
    /// Its name as a person reads it (`Money`), and the scheme its sign-in
    /// and its links come back on (`money`).
    const NAME: &'static str;
    const SCHEME: &'static str;
    /// Whether it has a demo's household to try with no server.
    const DEMO: bool;
    /// The domain its replica's log is of.
    fn domain() -> Arc<ark::domain::Domain>;
    /// Its demo's household on a peer of its own, seeded up to `today` at
    /// `now_ms`; never asked of an app with no demo.
    fn demo(today: &str, now_ms: i64) -> ark_peer::Peer;
}

/// Who the app is, to the sign-in machine.
fn info<P: PhoneApp>() -> AppInfo {
    AppInfo {
        name: P::NAME.into(),
        scheme: P::SCHEME.into(),
        demo: P::DEMO,
        what: <P::App as HostedApp>::KEEPS.into(),
    }
}

/// The domain the app carries, as a server's `/healthz` names it: its
/// schema's hash and its version (the server's `domain` line).
pub fn domain_line<P: PhoneApp>() -> String {
    let d = P::domain();
    format!("{} {}", d.schema_hash(), d.version)
}

/// The app's replica on the disk at `dir`, as whoever last signed in there
/// or as `login`.
pub fn open<P: PhoneApp>(dir: &str, login: Option<Login>) -> Result<Arc<Peer>, Refusal> {
    Peer::open(ark_peer::Config::of(P::domain()), dir, login)
}

/// The app's replica on the disk at `dir`, as the process's one runtime
/// ([`ArkProcess::shared`]): opened the first time, the same peer for every
/// Activity, Scene or window after, and running while none shows; another
/// `dir` (another server) lets the old one go.
pub fn process_open<P: PhoneApp>(dir: &str, login: Option<Login>) -> Result<Arc<Peer>, Refusal> {
    ArkProcess::shared().open(dir, || open::<P>(dir, login))
}

/// The app's demo's household, seeded up to `today` (`2026-10-26`) — today
/// when none, and a fixed day is the same household every time, which is
/// what a screenshot wants — in memory, its own authority. Nothing it does
/// leaves the process, and opening it again starts from the seed again.
pub fn demo<P: PhoneApp>(today: Option<String>) -> Result<Arc<Peer>, Refusal> {
    let (today, now) = match today {
        Some(t) => {
            let now = ark_format::noon_of(&t).ok_or_else(|| format!("a day, as 2026-10-26: {t}"))?;
            (t, now)
        }
        None => {
            let now = ark_peer::now_ms();
            (ark_format::day_of(now), now)
        }
    };
    Peer::of(P::demo(&today, now))
}

/// Today, as a domain that reads days in UTC writes it (`2026-10-26`).
pub fn today() -> String {
    ark_format::day_of(ark_peer::now_ms())
}

type Key<P> = <<P as PhoneApp>::App as HostedApp>::Key;
type Change<P> = <<P as PhoneApp>::App as HostedApp>::Change;

/// The phone, as the place an app's part runs over.
pub struct PhonePlace<P: PhoneApp> {
    session: Arc<Session<Key<P>>>,
    how: Opened,
    handle: Handle,
    today: String,
    held: BTreeMap<Key<P>, Vec<Hold<Key<P>>>>,
    /// What was said meanwhile, said once the host's lock is let go.
    notices: Vec<String>,
}

impl<P: PhoneApp> Place<Key<P>, Change<P>> for PhonePlace<P> {
    fn read(&self, f: &mut dyn FnMut(&dyn Views<Key<P>>)) {
        // A file a row names is read where the phone keeps it (`file:`).
        self.session.read(|st, peer| {
            f(&ark::views::WithFiles {
                views: st,
                url: |file: &ark::file::FileRef| peer.file_url(file),
            })
        })
    }
    fn now_ms(&self) -> i64 {
        ark_peer::now_ms()
    }
    fn send(&mut self, change: Change<P>) -> Result<(), String> {
        let (name, input) = <P::App as HostedApp>::encode(&change);
        match self.session.send(|p| p.mutate_encoded(name, input)) {
            Ok(_) => Ok(()),
            Err(Refusal::Refused { why }) => Err(why),
        }
    }
    fn hold(&mut self, key: Key<P>) {
        let h = self.session.hold(key.clone());
        self.held.entry(key).or_default().push(h);
    }
    fn release(&mut self, key: &Key<P>) {
        if let Some(v) = self.held.get_mut(key) {
            v.pop();
            if v.is_empty() {
                self.held.remove(key);
            }
        }
    }
    fn modules(&self) -> ark_app::Modules {
        ArkProcess::shared().modules()
    }
    /// An answer that comes later (the camera's) wakes the host, which
    /// drains every runtime on its own thread.
    fn waker(&self) -> Waker {
        let h = self.handle.clone();
        Arc::new(move || h.moved())
    }
    fn want(&mut self, file: ark::file::FileRef) {
        let _ = self.session.peer().file(file.hash, file.size, file.media);
    }
    fn keep(&mut self, bytes: &[u8], media: &str) -> Option<ark::file::FileRef> {
        let peer = self.session.peer();
        peer.put_file(bytes.to_vec(), media.into()).ok()?;
        Some(ark::file::FileRef::of(bytes, media))
    }
    /// SwiftUI and Compose draw the page after they read it.
    fn draws_between(&self) -> bool {
        true
    }
    fn session(&self) -> Facts {
        let peer = self.session.peer();
        let st = peer.status();
        let (demo, login, server) = match &self.how {
            Opened::Server { server, login } => (false, Some(peer.signed_in().unwrap_or_else(|| login.clone())), Some(server.clone())),
            Opened::Demo { .. } => (true, None, None),
        };
        let l = login.unwrap_or_default();
        Facts {
            demo,
            today: self.today.clone(),
            picture: match &server {
                Some(s) if !demo => ark_auth::picture_url(s, &l.user, &l.picture).unwrap_or_default(),
                _ => String::new(),
            },
            server: server.map(|s| ark_auth::probe::display(&s).to_string()).unwrap_or_default(),
            name: l.name,
            user: l.user,
            email: l.email,
            linked: Some(st.linked),
            pending: st.pending as i64,
            version: self.handle.version(),
            leaves: true,
            adds_devices: !demo,
        }
    }
    fn notice(&mut self, said: String) {
        self.notices.push(said)
    }
    fn dial(&mut self) {
        self.session.peer().dial_now()
    }
}

/// A session open: the app's part and the place it runs over.
pub struct Open<P: PhoneApp> {
    pub hosting: Hosting<P::App>,
    pub place: PhonePlace<P>,
}

/// An app's part under the app host every Ark app has: the [`HostApp`] its
/// bridge hands [`crate::ArkHost::new`].
pub struct Hosted<P: PhoneApp> {
    phone: std::marker::PhantomData<fn() -> P>,
    open: Mutex<Option<Open<P>>>,
    handle: Mutex<Option<Handle>>,
    /// What only the phone's shell does, taken by it.
    natives: Mutex<Vec<ShellAsk>>,
    generation: AtomicU64,
    /// Its screens' views as a screenshot walk reaches them (`HostApp::
    /// shots`): what [`phone_host!`] hands it, each screen by its name.
    views: Mutex<Option<ShotsViews>>,
    /// The screens run beside it the phone holds now, by app.toml's name
    /// (`register`), the newest last.
    besides: Mutex<Vec<(&'static str, Weak<dyn ShotsNodes>)>>,
}

/// Every hosted screen's view, as a walk's steps reach it.
type ShotsViews = Box<dyn Fn() -> Vec<ark_app::shots::Screen> + Send + Sync>;

/// A screen run beside the app's part, as a walk's steps reach its view.
pub trait ShotsNodes: Send + Sync {
    fn nodes(&self) -> Vec<ark_app::shots::Node>;
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<P: PhoneApp> Hosted<P> {
    pub fn new() -> Arc<Hosted<P>> {
        Arc::new(Hosted {
            phone: std::marker::PhantomData,
            open: Mutex::new(None),
            handle: Mutex::new(None),
            natives: Mutex::new(vec![]),
            generation: AtomicU64::new(0),
            views: Mutex::new(None),
            besides: Mutex::new(vec![]),
        })
    }

    /// Its screens' views, for a walk's steps ([`phone_host!`]'s).
    pub fn set_views(&self, views: ShotsViews) {
        *lock(&self.views) = Some(views);
    }

    /// A screen run beside it (`name`, app.toml's), now held by the phone.
    pub fn shown_beside(&self, name: &'static str, screen: Weak<dyn ShotsNodes>) {
        let mut b = lock(&self.besides);
        b.retain(|(_, w)| w.strong_count() > 0);
        b.push((name, screen));
    }

    /// `f` over the session open, then what it said and asked carried out
    /// once the lock is let go; none while no session is.
    pub fn with<R>(&self, f: impl FnOnce(&mut Open<P>) -> R) -> Option<R> {
        let (r, notices, natives, moved) = {
            let mut g = lock(&self.open);
            let o = g.as_mut()?;
            let before = (o.hosting.version(), o.hosting.ran());
            let r = f(o);
            let moved = before != (o.hosting.version(), o.hosting.ran());
            (r, std::mem::take(&mut o.place.notices), o.hosting.take_natives(), moved)
        };
        let handle = lock(&self.handle).clone();
        let mut asked = false;
        for n in natives {
            if n == ShellAsk::SignOut {
                if let Some(h) = &handle {
                    h.sign_out()
                }
            } else {
                lock(&self.natives).push(n);
                asked = true;
            }
        }
        if let Some(h) = &handle {
            for said in notices {
                h.notice(said)
            }
            if asked || moved {
                h.changed()
            }
        }
        Some(r)
    }

    /// `msg` taken by the screen `A`.
    pub fn send<A>(&self, msg: A::Msg)
    where
        P::App: Has<A>,
        A: App<Key = Key<P>, Change = Change<P>, Route = <P::App as HostedApp>::Route, Request = <P::App as HostedApp>::Request>,
    {
        self.with(|o| o.hosting.send::<A>(msg, &mut o.place));
    }

    /// What the screen `A` shows now; its blank page while no session is.
    pub fn page<A>(&self) -> A::Screen
    where
        P::App: Has<A>,
        A: App<Key = Key<P>, Change = Change<P>, Route = <P::App as HostedApp>::Route, Request = <P::App as HostedApp>::Request>,
    {
        let g = lock(&self.open);
        match g.as_ref() {
            Some(o) => o.hosting.page::<A>(&o.place),
            None => blank::<A>(),
        }
    }

    /// The screen `A`'s page drawn: whether anything waited on it.
    pub fn drawn<A>(&self) -> bool
    where
        P::App: Has<A>,
        A: App<Key = Key<P>, Change = Change<P>, Route = <P::App as HostedApp>::Route, Request = <P::App as HostedApp>::Request>,
    {
        self.with(|o| o.hosting.drawn::<A>(&mut o.place)).unwrap_or(false)
    }

    /// The screen `A`'s version, whether it is idle, and whether something
    /// waits for its page to be drawn.
    pub fn state<A>(&self) -> (u64, bool, bool)
    where
        P::App: Has<A>,
        A: App<Key = Key<P>, Change = Change<P>, Route = <P::App as HostedApp>::Route, Request = <P::App as HostedApp>::Request>,
    {
        let g = lock(&self.open);
        match g.as_ref() {
            Some(o) => {
                let s: &Screen<A> = o.hosting.app().screen();
                (s.version(), s.is_idle(&o.place), s.awaits_draw())
            }
            None => (0, true, false),
        }
    }

    /// A screen the phone runs beside the app's part (a register pushed on
    /// a stack), `f` of it over the session's place; none while none is.
    pub fn beside<A, R>(&self, screen: &mut Screen<A>, f: impl FnOnce(&mut Screen<A>, &mut ark_app::host::Cx<'_, P::App>) -> R) -> Option<R>
    where
        A: App<Key = Key<P>, Change = Change<P>, Route = <P::App as HostedApp>::Route, Request = <P::App as HostedApp>::Request>,
    {
        self.with(|o| o.hosting.with(&mut o.place, |_, cx| f(screen, cx)))
    }

    /// A screen beside it, read: `f` of it and the place.
    pub fn read_beside<A, R>(&self, screen: &Screen<A>, f: impl FnOnce(&Screen<A>, &dyn Place<Key<P>, Change<P>>, i64) -> R) -> Option<R>
    where
        A: App<Key = Key<P>, Change = Change<P>>,
    {
        let g = lock(&self.open);
        g.as_ref().map(|o| f(screen, &o.place, o.hosting.now_ms(&o.place)))
    }

    /// A screen beside it let go.
    pub fn close_beside<A>(&self, screen: &mut Screen<A>)
    where
        A: App<Key = Key<P>, Change = Change<P>>,
    {
        if let Some(o) = lock(&self.open).as_mut() {
            screen.close(&mut o.place)
        }
    }

    /// The app's part and its place, read.
    pub fn read<R>(&self, f: impl FnOnce(&P::App, &PhonePlace<P>) -> R) -> Option<R> {
        let g = lock(&self.open);
        g.as_ref().map(|o| f(o.hosting.app(), &o.place))
    }

    /// Whether a session is open.
    pub fn is_open(&self) -> bool {
        lock(&self.open).is_some()
    }

    /// The session's day (`2026-10-26`).
    pub fn today(&self) -> String {
        self.read(|_, p| p.today.clone()).unwrap_or_else(today)
    }
}

/// A screen's page before any session: its state's default over nothing.
pub fn blank<A: App>() -> A::Screen {
    let store = ark_peer::store::Store::<A::Key>::new();
    let data = ark_app::Data { store: &store, now_ms: 0 };
    A::screen(&Default::default(), &data)
}

impl<P: PhoneApp> HostApp for Hosted<P>
where
    Key<P>: ViewKey,
{
    fn info(&self) -> AppInfo {
        info::<P>()
    }
    fn copy(&self, scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy {
        <P::App as HostedApp>::welcome(scan, demo)
    }
    fn domain(&self) -> String {
        domain_line::<P>()
    }
    fn open(&self, dir: &str, login: Login) -> Result<Arc<Peer>, Refusal> {
        process_open::<P>(dir, Some(login))
    }
    fn demo(&self, today: Option<String>) -> Result<Arc<Peer>, Refusal> {
        demo::<P>(today)
    }

    fn opened(&self, peer: Arc<Peer>, how: Opened, host: Handle) {
        *lock(&self.handle) = Some(host.clone());
        let today = match &how {
            Opened::Demo { today: Some(t) } => t.clone(),
            _ => today(),
        };
        let session = Arc::new(Session::<Key<P>>::new(peer));
        // Whichever views moved, the screens read again: the shell is told.
        let h = host.clone();
        session.set_listener(Some(Arc::new(move |_: Vec<Key<P>>| h.changed())));
        let mut place = PhonePlace::<P> {
            session,
            how,
            handle: host,
            today,
            held: BTreeMap::new(),
            notices: vec![],
        };
        let facts = place.session();
        let hosting = Hosting::open(&facts, &mut place);
        if let Some(old) = lock(&self.open).replace(Open { hosting, place }) {
            drop(old)
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.with(|_| ());
    }

    fn closed(&self) {
        let gone = lock(&self.open).take();
        if let Some(mut o) = gone {
            o.place.session.set_listener(None);
            o.hosting.close(&mut o.place);
        }
        lock(&self.natives).clear();
    }

    fn moved(&self) {
        self.with(|o| o.hosting.moved(&mut o.place));
    }

    fn generation(&self) -> u64 {
        match self.is_open() {
            true => self.generation.load(Ordering::Acquire),
            false => 0,
        }
    }

    fn natives(&self) -> Vec<ShellAsk> {
        std::mem::take(&mut *lock(&self.natives))
    }

    fn link(&self, args: Option<Vec<String>>, address: String) {
        let link = match args {
            Some(args) => Link::Launch(args),
            None => Link::Address(address),
        };
        self.with(|o| o.hosting.link(link, &mut o.place));
    }

    fn launched(&self, args: Vec<String>) {
        self.with(|o| o.hosting.launched(args, &mut o.place));
    }

    /// The app's part made anew over the session's place
    /// ([`Hosting::reopen`]), then a link with no arguments: what the
    /// phone's shell shows of its own (a page pushed, a sheet) put away.
    fn reset(&self) {
        self.with(|o| {
            let facts = o.place.session();
            o.hosting.reopen(&facts, &mut o.place);
            o.hosting.link(Link::Launch(vec![]), &mut o.place);
        });
    }

    fn idle(&self) -> bool {
        lock(&self.open).as_ref().is_none_or(|o| o.hosting.idle(&o.place))
    }

    fn version(&self) -> u64 {
        lock(&self.open).as_ref().map_or(0, |o| o.hosting.version())
    }

    /// It does: every call that moved a runtime (`Hosted::with`) and every
    /// view that moved (the session's listener) tells the shell.
    fn tells_its_changes(&self) -> bool {
        true
    }

    fn busy(&self) -> Option<String> {
        lock(&self.open).as_ref().and_then(|o| o.hosting.busy(&o.place))
    }

    /// Every hosted screen's view, then the newest of each screen run
    /// beside them the phone still holds.
    fn shots(&self) -> Vec<ark_app::shots::Screen> {
        let mut out = lock(&self.views).as_ref().map_or_else(Vec::new, |v| v());
        let besides: Vec<_> = lock(&self.besides).clone();
        for (name, w) in besides.iter().rev() {
            if out.iter().any(|s| s.name == *name) {
                continue;
            }
            if let Some(s) = w.upgrade() {
                out.push(ark_app::shots::Screen {
                    name: name.to_string(),
                    nodes: s.nodes(),
                });
            }
        }
        out
    }
}

/// A screen of the app's part as UniFFI exports it: `$name` over the
/// bridge's `$host` (a `Hosted` of its phone app), running `$app`, its
/// messages' twin `$msg` and its page's `$page`. What an app's bridge writes
/// once per app.toml `[screens]` entry.
#[macro_export]
macro_rules! hosted_screen {
    ($(#[$doc:meta])* $name:ident, $phone:ty, $app:ty, $msg:ty, $page:ty, view: $view:path) => {
        $(#[$doc])*
        #[derive(uniffi::Object)]
        pub struct $name(pub(crate) std::sync::Arc<$crate::hosted::Hosted<$phone>>);

        impl $name {
            /// Its view as a screenshot walk's steps reach it, each element's
            /// message sent as the phone sends it.
            pub fn shots_nodes(host: &std::sync::Arc<$crate::hosted::Hosted<$phone>>) -> Vec<$crate::ark_app::shots::Node> {
                let page = host.page::<$app>();
                let host = std::sync::Arc::downgrade(host);
                $crate::ark_app::shots::nodes(
                    &$view(&page),
                    std::rc::Rc::new(move |m| {
                        if let Some(h) = host.upgrade() {
                            h.send::<$app>(m)
                        }
                    }),
                )
            }
        }

        #[uniffi::export]
        impl $name {
            /// `msg` taken: the state updated, its changes sent, its routes
            /// opened, the views it names held.
            pub fn send(&self, msg: $msg) {
                use $crate::Twin;
                self.0.send::<$app>(<<$app as $crate::ark_app::App>::Msg>::from_ffi(msg))
            }

            /// What the screen shows now.
            pub fn page(&self) -> $page {
                use $crate::Twin;
                self.0.page::<$app>().ffi()
            }

            /// A number that grows with every message taken: with the
            /// session's versions, what says to read the page again.
            pub fn version(&self) -> u64 {
                self.0.state::<$app>().0
            }

            /// Whether nothing is left to come (`Runtime::is_idle`).
            pub fn is_idle(&self) -> bool {
                self.0.state::<$app>().1
            }

            /// The page as last read has been drawn: what waited on that
            /// (`Command::AfterDraw`) taken; whether any did.
            pub fn drawn(&self) -> bool {
                self.0.drawn::<$app>()
            }

            /// Whether something waits for the page to be drawn.
            pub fn awaits_draw(&self) -> bool {
                self.0.state::<$app>().2
            }
        }
    };
}

/// A screen the phone runs beside the app's part for as long as it shows
/// it (a register pushed on a stack): its own runtime over the session,
/// made with the message that says what it shows. What [`phone_host!`]
/// hands out for app.toml's `[screens]` entries that say `beside`.
pub struct Beside<P: PhoneApp, A: App<Key = Key<P>, Change = Change<P>>> {
    host: Arc<Hosted<P>>,
    screen: Mutex<Screen<A>>,
}

impl<P: PhoneApp, A> Beside<P, A>
where
    A: App<Key = Key<P>, Change = Change<P>, Route = <P::App as HostedApp>::Route, Request = <P::App as HostedApp>::Request>,
{
    /// The screen over `host`'s session, told `first`; none while no
    /// session is open.
    pub fn over(host: Arc<Hosted<P>>, first: A::Msg) -> Option<Beside<P, A>> {
        let mut screen = Screen::new();
        host.beside(&mut screen, |s, cx| s.send(first, cx))?;
        Some(Beside {
            host,
            screen: Mutex::new(screen),
        })
    }

    fn screen(&self) -> MutexGuard<'_, Screen<A>> {
        lock(&self.screen)
    }

    pub fn send(&self, msg: A::Msg) {
        let mut s = self.screen();
        self.host.beside(&mut s, |s, cx| s.send(msg, cx));
    }

    pub fn page(&self) -> A::Screen {
        let s = self.screen();
        self.host.read_beside(&s, |s, place, now| s.page(place, now)).unwrap_or_else(blank::<A>)
    }

    pub fn version(&self) -> u64 {
        self.screen().version()
    }

    pub fn is_idle(&self) -> bool {
        let s = self.screen();
        self.host.read_beside(&s, |s, place, _| s.is_idle(place)).unwrap_or(true)
    }

    pub fn drawn(&self) -> bool {
        let mut s = self.screen();
        self.host.beside(&mut s, |s, cx| s.drawn(cx)).unwrap_or(false)
    }

    pub fn awaits_draw(&self) -> bool {
        self.screen().awaits_draw()
    }
}

impl<P: PhoneApp, A: App<Key = Key<P>, Change = Change<P>>> Drop for Beside<P, A> {
    fn drop(&mut self) {
        let mut s = lock(&self.screen);
        self.host.close_beside(&mut s);
    }
}

/// A screen run beside the app's part as UniFFI exports it: `$name` over
/// the library's `$phone`, running `$app`, its messages' twin `$msg` and
/// its page's `$page` ([`Beside`]).
#[macro_export]
macro_rules! beside_screen {
    ($(#[$doc:meta])* $name:ident, $phone:ty, $app:ty, $msg:ty, $page:ty, view: $view:path) => {
        $(#[$doc])*
        #[derive(uniffi::Object)]
        pub struct $name($crate::hosted::Beside<$phone, $app>, std::sync::Weak<$name>);

        impl $name {
            /// The screen over `host`'s session, told `first` (twin).
            pub fn over(host: std::sync::Arc<$crate::hosted::Hosted<$phone>>, first: $msg) -> Option<std::sync::Arc<$name>> {
                use $crate::Twin;
                $crate::hosted::Beside::over(host, <<$app as $crate::ark_app::App>::Msg>::from_ffi(first))
                    .map(|b| std::sync::Arc::new_cyclic(|me| $name(b, me.clone())))
            }
        }

        impl $crate::hosted::ShotsNodes for $name {
            fn nodes(&self) -> Vec<$crate::ark_app::shots::Node> {
                let page = self.0.page();
                let me = self.1.clone();
                $crate::ark_app::shots::nodes(
                    &$view(&page),
                    std::rc::Rc::new(move |m| {
                        if let Some(s) = me.upgrade() {
                            s.0.send(m)
                        }
                    }),
                )
            }
        }

        #[uniffi::export]
        impl $name {
            /// `msg` taken.
            pub fn send(&self, msg: $msg) {
                use $crate::Twin;
                self.0.send(<<$app as $crate::ark_app::App>::Msg>::from_ffi(msg))
            }

            /// What the screen shows now.
            pub fn page(&self) -> $page {
                use $crate::Twin;
                self.0.page().ffi()
            }

            pub fn version(&self) -> u64 {
                self.0.version()
            }

            pub fn is_idle(&self) -> bool {
                self.0.is_idle()
            }

            /// The page drawn: what waited on it taken; whether any did.
            pub fn drawn(&self) -> bool {
                self.0.drawn()
            }

            pub fn awaits_draw(&self) -> bool {
                self.0.awaits_draw()
            }
        }
    };
}

/// An app's library for its phones, as UniFFI exports it: the object the
/// phone holds (`$host`, app.toml's `[ffi] host`) — the app host every Ark
/// app has ([`crate::ArkHost`], `host()`), the first run's pages
/// (`welcome()`, over the library's `WelcomeApp`), each screen's runtime by
/// its app.toml `[screens]` name (`$screen()`, reading its blank page while
/// no session is open), each screen run beside it (`$beside(first)`)
/// and the session's day (the session's generation and what only the
/// phone's shell does are the app host's: `ArkHost.generation()`,
/// `ArkHost.natives()`) — and its
/// routes read from a link (`route_from_url`, the `Route` twin `$route_ffi`
/// of `$route`). Written by the app's generated library (ui/prebuild.py's
/// `ffi`), after its twins.
#[macro_export]
// `crate` is the library's on purpose: its UniFFI tag, its twins.
#[allow(clippy::crate_in_macro_def)]
macro_rules! phone_host {
    (
        $(#[$doc:meta])* $host:ident, $phone:ty,
        route: $route:ty => $route_ffi:ty,
        screens { $($screen:ident: $screen_ty:ident),* $(,)? }
        beside { $($beside:ident: $beside_ty:ident ($beside_msg:ty)),* $(,)? }
    ) => {
        $(#[$doc])*
        #[derive(uniffi::Object)]
        pub struct $host {
            host: std::sync::Arc<$crate::ArkHost>,
            app: std::sync::Arc<$crate::hosted::Hosted<$phone>>,
        }

        #[uniffi::export]
        impl $host {
            /// The app as the phone launched it, its login kept in `store`
            /// (ark-secure-store's `PhoneSecureStore`; none: for this run
            /// alone).
            #[uniffi::constructor]
            pub fn new(
                launch: $crate::Launch,
                store: Option<std::sync::Arc<dyn $crate::ark_secure_store::phone::SecureStore>>,
            ) -> std::sync::Arc<$host> {
                let app = $crate::hosted::Hosted::<$phone>::new();
                // Every screen's view, for a screenshot walk's steps.
                let weak = std::sync::Arc::downgrade(&app);
                app.set_views(Box::new(move || match weak.upgrade() {
                    Some(a) => vec![$($crate::ark_app::shots::Screen {
                        name: stringify!($screen).into(),
                        nodes: $screen_ty::shots_nodes(&a),
                    }),*],
                    None => vec![],
                }));
                let host = $crate::ArkHost::new(app.clone(), launch, store);
                std::sync::Arc::new($host { host, app })
            }

            /// The app host every Ark app has: its first run, its link, the
            /// shell's notices.
            pub fn host(&self) -> std::sync::Arc<$crate::ArkHost> {
                self.host.clone()
            }

            /// The first run's pages, as the generated first run reads them.
            pub fn welcome(&self) -> std::sync::Arc<WelcomeApp> {
                WelcomeApp::new(self.host.clone())
            }

            $(
                /// The screen's runtime: its blank page while no session is
                /// open, the session's after.
                pub fn $screen(&self) -> std::sync::Arc<$screen_ty> {
                    std::sync::Arc::new($screen_ty(self.app.clone()))
                }
            )*

            $(
                /// A runtime of its own for as long as the phone shows it,
                /// told `first`; none while no session is open.
                pub fn $beside(&self, first: $beside_msg) -> Option<std::sync::Arc<$beside_ty>> {
                    let b = $beside_ty::over(self.app.clone(), first)?;
                    let shown: std::sync::Arc<dyn $crate::hosted::ShotsNodes> = b.clone();
                    self.app.shown_beside(stringify!($beside), std::sync::Arc::downgrade(&shown));
                    Some(b)
                }
            )*

            /// The session's day (`2026-10-26`): the demo's in the demo.
            pub fn today(&self) -> String {
                self.app.today()
            }
        }

        /// The route a link names (`/budget/`, `<scheme>://account/<id>/`),
        /// or none.
        #[uniffi::export]
        pub fn route_from_url(url: String) -> Option<$route_ffi> {
            <$route as $crate::ark_router::Routable>::from_url(&url).map(<$route as $crate::Twin<crate::UniFfiTag>>::ffi)
        }

        #[uniffi::export(name = "Route")]
        impl $route_ffi {
            /// The route's link, at the root (`/account/<id>/checking/`).
            #[uniffi::method(name = "url")]
            pub fn ffi_url(&self) -> String {
                use $crate::Twin;
                <$route as $crate::ark_router::Routable>::url(&<$route>::from_ffi(self.clone()))
            }
        }
    };
}
