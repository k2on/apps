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
use std::sync::{Arc, Mutex, MutexGuard};

use ark_app::host::{Has, HostedApp, Hosting, Link, Place, Screen, Session as Facts, Waker};
use ark_app::{App, ViewKey, Views};
use ark_auth::onboarding::AppInfo;

use crate::{ArkProcess, Handle, Hold, HostApp, Login, Opened, Peer, Refusal, Session};

/// What an app's bridge says of itself on the phones.
pub trait PhoneApp: Send + Sync + 'static {
    /// The app's part of its host (money's `money_app::host::Money`).
    type App: HostedApp<Route: Send, Request: Send, Native: Send> + Send;
    /// Who the app is, to the sign-in machine.
    fn info(&self) -> AppInfo;
    /// The app's words on its first run's pages.
    fn copy(&self, scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy;
    /// The domain it carries, as a server's `/healthz` names it.
    fn domain(&self) -> String;
    /// Its replica on the disk at `dir`, as `login`.
    fn open(&self, dir: &str, login: Login) -> Result<Arc<Peer>, Refusal>;
    /// Its demo's household, on `today` (today when none).
    fn demo(&self, today: Option<String>) -> Result<Arc<Peer>, Refusal>;
    /// Today, as the domain writes a day (`2026-10-26`).
    fn today(&self) -> String;
    /// A change of its domain's, as the peer takes it: its function's name
    /// and its input's bytes.
    fn encode(change: &<Self::App as HostedApp>::Change) -> (&'static str, Vec<u8>);
    /// Whether a native is signing out, which the host does itself.
    fn signs_out(native: &<Self::App as HostedApp>::Native) -> bool;
}

type Key<P> = <<P as PhoneApp>::App as HostedApp>::Key;
type Change<P> = <<P as PhoneApp>::App as HostedApp>::Change;
type Native<P> = <<P as PhoneApp>::App as HostedApp>::Native;

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
        let (name, input) = P::encode(&change);
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
    /// A page opened goes on the section's stack.
    fn stacks(&self) -> bool {
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
    phone: P,
    open: Mutex<Option<Open<P>>>,
    handle: Mutex<Option<Handle>>,
    /// What only the phone's shell does, taken by it.
    natives: Mutex<Vec<Native<P>>>,
    generation: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<P: PhoneApp> Hosted<P> {
    pub fn new(phone: P) -> Arc<Hosted<P>> {
        Arc::new(Hosted {
            phone,
            open: Mutex::new(None),
            handle: Mutex::new(None),
            natives: Mutex::new(vec![]),
            generation: AtomicU64::new(0),
        })
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
            if P::signs_out(&n) {
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

    /// Grows with every session opened; zero while none is.
    pub fn generation(&self) -> u64 {
        match self.is_open() {
            true => self.generation.load(Ordering::Acquire),
            false => 0,
        }
    }

    /// What the phone's shell is asked, in order, taken.
    pub fn take_natives(&self) -> Vec<Native<P>> {
        std::mem::take(&mut *lock(&self.natives))
    }

    /// The session's day (`2026-10-26`).
    pub fn today(&self) -> String {
        self.read(|_, p| p.today.clone()).unwrap_or_else(|| self.phone.today())
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
        self.phone.info()
    }
    fn copy(&self, scan: bool, demo: bool) -> ark_app::welcome::WelcomeCopy {
        self.phone.copy(scan, demo)
    }
    fn domain(&self) -> String {
        self.phone.domain()
    }
    fn open(&self, dir: &str, login: Login) -> Result<Arc<Peer>, Refusal> {
        self.phone.open(dir, login)
    }
    fn demo(&self, today: Option<String>) -> Result<Arc<Peer>, Refusal> {
        self.phone.demo(today)
    }

    fn opened(&self, peer: Arc<Peer>, how: Opened, host: Handle) {
        *lock(&self.handle) = Some(host.clone());
        let today = match &how {
            Opened::Demo { today: Some(t) } => t.clone(),
            _ => self.phone.today(),
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

    fn idle(&self) -> bool {
        lock(&self.open).as_ref().is_none_or(|o| o.hosting.idle(&o.place))
    }

    fn version(&self) -> u64 {
        lock(&self.open).as_ref().map_or(0, |o| o.hosting.version())
    }
}

/// A screen of the app's part as UniFFI exports it: `$name` over the
/// bridge's `$host` (a `Hosted` of its phone app), running `$app`, its
/// messages' twin `$msg` and its page's `$page`. What an app's bridge writes
/// once per app.toml `[screens]` entry.
#[macro_export]
macro_rules! hosted_screen {
    ($(#[$doc:meta])* $name:ident, $phone:ty, $app:ty, $msg:ty, $page:ty) => {
        $(#[$doc])*
        #[derive(uniffi::Object)]
        pub struct $name(pub(crate) std::sync::Arc<$crate::hosted::Hosted<$phone>>);

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
