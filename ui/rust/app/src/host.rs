//! An app's part of its host, written once and driven by every host: the
//! phones' (ark-ffi's `ArkHost`), the browser's and the desktop's
//! (ui/docs/app.md, "The App's Part of Its Host").
//!
//! A host is two halves. The **place** ([`Place`]) is the platform's: the
//! views its store holds, the time, a change sent to its peer, its device
//! modules, a file kept, a sentence said, who is signed in where
//! ([`Session`]). The **app's part** ([`HostedApp`]) is the app's alone and
//! toolkit-neutral: the runtimes it runs over one session (each a
//! [`Screen`]), what it makes of a route or a request one of them asked —
//! a message for another runtime, its month moved, the bank asked — what
//! one runtime's page says that another follows, a launch's link, and what
//! only the toolkit's shell can do ([`HostedApp::Native`], taken by the
//! host after). [`Hosting`] is the loop between them: a message in, the
//! routes and requests it asked carried out once no runtime is mid-message,
//! the views every runtime names held, and the idle signal and version
//! summed.
//!
//! Nothing here touches the world: a place is handed in by reference for
//! each call, so the same app part runs behind UniFFI, in the iced window
//! and in wasm, and in a test over a store.

use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;

use crate::{App, Ask, Host, Inbox, Modules, Runtime, Ticket, ViewKey, Views};

/// Who is using the app and where, as the platform knows it: what the
/// app's Settings says, and what it decides by (the demo's household).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// The demo's household, on this device alone.
    pub demo: bool,
    /// The session's day (`2026-10-26`): the demo's fixed day, or today.
    pub today: String,
    /// Their name, their login and their email; empty while not known, and
    /// in the demo.
    pub name: String,
    pub user: String,
    pub email: String,
    /// Where their picture is loaded from; empty for none.
    pub picture: String,
    /// The server, as it is shown (`money.example.com`); empty for the demo.
    pub server: String,
    /// The link to it: up, down, or not known yet.
    pub linked: Option<bool>,
    /// Changes made here not yet synced.
    pub pending: i64,
    /// The app's version, as the platform says it (`0.1 (12)`).
    pub version: String,
    /// This host can sign out or leave the demo.
    pub leaves: bool,
    /// This host can make a code for another device.
    pub adds_devices: bool,
}

/// Something to call when a module answers after its message (the camera,
/// once the person is done): the host is woken, and drains its runtimes
/// ([`Hosting::moved`]) on its own thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The platform's half of a host: what every app's part is run over.
pub trait Place<K: ViewKey, C> {
    /// `f` of the views the store holds, as they are now.
    fn read(&self, f: &mut dyn FnMut(&dyn Views<K>));
    /// The device's clock, in milliseconds since the epoch.
    fn now_ms(&self) -> i64;
    /// `change` sent to the peer: taken, or the domain's sentence for why not.
    fn send(&mut self, change: C) -> Result<(), String>;
    /// `key` held once more: its views opened the first time.
    fn hold(&mut self, key: K);
    /// `key` held once less: its views closed when nothing holds it.
    fn release(&mut self, key: &K);
    /// The device modules the platform registered.
    fn modules(&self) -> Modules {
        Modules::new()
    }
    /// What a module that answers later wakes.
    fn waker(&self) -> Waker {
        Arc::new(|| {})
    }
    /// A file a screen is about to show, fetched when this device lacks it.
    fn want(&mut self, _file: ark::file::FileRef) {}
    /// `bytes` kept as a file of `media` on this device and sent to the
    /// server with the next sync; none when it cannot keep files.
    fn keep(&mut self, _bytes: &[u8], _media: &str) -> Option<ark::file::FileRef> {
        None
    }
    /// Whether the toolkit draws between one message and the next (SwiftUI,
    /// Compose): what waits on a draw waits for `drawn`.
    fn draws_between(&self) -> bool {
        false
    }
    /// Whether a page opened goes on the section's stack (a phone), rather
    /// than being one of the window's places (the browser, the desktop).
    fn stacks(&self) -> bool {
        false
    }
    /// Who is signed in where.
    fn session(&self) -> Session;
    /// A sentence for the shell to say once.
    fn notice(&mut self, _said: String) {}
    /// The server dialled now, if the link was resting.
    fn dial(&mut self) {}
}

/// What a runtime asked of its host, carried out once none is mid-message.
enum Job<R, Q> {
    Open(R),
    Back,
    Request(Q),
}

/// The app's part of a host at work on one call: the place, and what was
/// asked meanwhile.
pub struct Cx<'a, H: HostedApp> {
    pub place: &'a mut dyn Place<H::Key, H::Change>,
    /// The time the call is worked out at ([`HostedApp::now_ms`]).
    pub now_ms: i64,
    jobs: &'a mut VecDeque<Job<H::Route, H::Request>>,
    natives: &'a mut Vec<H::Native>,
}

impl<H: HostedApp> Cx<'_, H> {
    /// Something only the toolkit's shell does, for it to take after.
    pub fn native(&mut self, n: H::Native) {
        self.natives.push(n)
    }

    /// `request` carried out after what is being carried out now.
    pub fn request(&mut self, request: H::Request) {
        self.jobs.push_back(Job::Request(request))
    }

    /// The rows of the view `key` as `T`, copied; none while it is not held.
    pub fn rows<T: Clone + 'static>(&self, key: &H::Key) -> Vec<T> {
        let mut out = vec![];
        self.place.read(&mut |v| out = v.rows::<T>(key).to_vec());
        out
    }
}

/// One of an app's screens running: its runtime, the answers its modules
/// gave later, and the views its state names, held.
pub struct Screen<A: App> {
    runtime: Runtime<A>,
    inbox: Inbox<A::Msg>,
    held: Vec<A::Key>,
}

impl<A: App> Default for Screen<A> {
    fn default() -> Self {
        Screen {
            runtime: Runtime::new(),
            inbox: Inbox::default(),
            held: vec![],
        }
    }
}

/// A screen's runtime's host for one message: the place, and the app's
/// part's queue for what it asked.
struct Over<'c, 'a, H: HostedApp, M> {
    cx: &'c mut Cx<'a, H>,
    inbox: Inbox<M>,
}

impl<A, H> Host<A> for Over<'_, '_, H, A::Msg>
where
    H: HostedApp,
    A: App<Key = H::Key, Change = H::Change, Route = H::Route, Request = H::Request>,
{
    fn read<R>(&self, f: impl FnOnce(&dyn Views<A::Key>) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        self.cx.place.read(&mut |v| out = f.take().map(|f| f(v)));
        out.expect("a place reads once")
    }
    fn now_ms(&self) -> i64 {
        self.cx.now_ms
    }
    fn send(&mut self, change: A::Change) -> Result<(), String> {
        self.cx.place.send(change)
    }
    fn open(&mut self, route: A::Route) {
        self.cx.jobs.push_back(Job::Open(route))
    }
    fn back(&mut self) {
        self.cx.jobs.push_back(Job::Back)
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<A::Msg>) {
        let waker = self.cx.place.waker();
        self.inbox.ask_telling(&self.cx.place.modules(), ticket, ask, move || waker());
    }
    fn request(&mut self, r: A::Request) {
        self.cx.jobs.push_back(Job::Request(r))
    }
    fn want(&mut self, file: ark::file::FileRef) {
        self.cx.place.want(file)
    }
    fn draws_between(&self) -> bool {
        self.cx.place.draws_between()
    }
}

impl<A: App> Screen<A>
where
    A::Msg: 'static,
{
    pub fn new() -> Screen<A> {
        Screen::default()
    }

    /// Its state as it is.
    pub fn state(&self) -> &A::State {
        self.runtime.state()
    }

    /// `msg` taken, and what modules answered since.
    pub fn send<H>(&mut self, msg: A::Msg, cx: &mut Cx<'_, H>)
    where
        H: HostedApp<Key = A::Key, Change = A::Change, Route = A::Route, Request = A::Request>,
    {
        self.run(cx, |rt, host| {
            rt.dispatch(msg, host);
            true
        });
    }

    /// What its screen shows now.
    pub fn page(&self, place: &dyn Place<A::Key, A::Change>, now_ms: i64) -> A::Screen {
        let mut out = None;
        place.read(&mut |v| out = Some(self.runtime.screen_of(v, now_ms)));
        out.expect("a place reads once")
    }

    /// The page drawn: what waited on it taken; whether any did.
    pub fn drawn<H>(&mut self, cx: &mut Cx<'_, H>) -> bool
    where
        H: HostedApp<Key = A::Key, Change = A::Change, Route = A::Route, Request = A::Request>,
    {
        self.run(cx, |rt, host| rt.drawn(host))
    }

    /// Whether a message waits for the page to be drawn.
    pub fn awaits_draw(&self) -> bool {
        self.runtime.awaits_draw()
    }

    /// A number that grows with every message taken.
    pub fn version(&self) -> u64 {
        self.runtime.version()
    }

    /// Whether nothing is left to come ([`Runtime::is_idle`]).
    pub fn is_idle(&self, place: &dyn Place<A::Key, A::Change>) -> bool {
        let mut idle = false;
        place.read(&mut |v| idle = self.runtime.is_idle(v));
        idle
    }

    /// The answers modules gave since, run; whether any message ran.
    pub fn drain<H>(&mut self, cx: &mut Cx<'_, H>) -> bool
    where
        H: HostedApp<Key = A::Key, Change = A::Change, Route = A::Route, Request = A::Request>,
    {
        let before = self.runtime.version();
        self.run(cx, |_, _| false);
        self.runtime.version() != before
    }

    /// Every view it holds let go: the screen is gone.
    pub fn close(&mut self, place: &mut dyn Place<A::Key, A::Change>) {
        for k in std::mem::take(&mut self.held) {
            place.release(&k)
        }
    }

    /// `f` over the runtime and its host, then its answers taken and its
    /// views held as its state names them now; what `f` said.
    fn run<H>(&mut self, cx: &mut Cx<'_, H>, f: impl FnOnce(&mut Runtime<A>, &mut Over<'_, '_, H, A::Msg>) -> bool) -> bool
    where
        H: HostedApp<Key = A::Key, Change = A::Change, Route = A::Route, Request = A::Request>,
    {
        let ran = {
            let mut over = Over {
                cx: &mut *cx,
                inbox: self.inbox.clone(),
            };
            let ran = f(&mut self.runtime, &mut over);
            self.inbox.clone().drain(&mut self.runtime, &mut over);
            ran
        };
        self.hold(cx.place);
        ran
    }

    /// The views the state names held, and those it no longer names let go.
    fn hold(&mut self, place: &mut dyn Place<A::Key, A::Change>) {
        let want = self.runtime.holds();
        if want == self.held {
            return;
        }
        for k in &want {
            place.hold(k.clone())
        }
        for k in std::mem::replace(&mut self.held, want) {
            place.release(&k)
        }
    }
}

/// One of the app's part's screens, whatever its app: what the loop drains,
/// sums and lets go of.
pub trait Running<H: HostedApp> {
    fn drain(&mut self, cx: &mut Cx<'_, H>) -> bool;
    fn is_idle(&self, place: &dyn Place<H::Key, H::Change>) -> bool;
    fn version(&self) -> u64;
    fn close(&mut self, place: &mut dyn Place<H::Key, H::Change>);
}

impl<A, H> Running<H> for Screen<A>
where
    H: HostedApp,
    A: App<Key = H::Key, Change = H::Change, Route = H::Route, Request = H::Request>,
{
    fn drain(&mut self, cx: &mut Cx<'_, H>) -> bool {
        Screen::drain(self, cx)
    }
    fn is_idle(&self, place: &dyn Place<H::Key, H::Change>) -> bool {
        Screen::is_idle(self, place)
    }
    fn version(&self) -> u64 {
        Screen::version(self)
    }
    fn close(&mut self, place: &mut dyn Place<H::Key, H::Change>) {
        Screen::close(self, place)
    }
}

/// A link that came for the app while it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// A launch's arguments (`-screen budget`).
    Launch(Vec<String>),
    /// An address (`money:///budget`, a page's path).
    Address(String),
}

/// The app's part of its host: written once per app, in its app crate, and
/// driven by every host through [`Hosting`].
pub trait HostedApp: Sized + 'static {
    /// The views its screens read, the changes they send, where they can
    /// be and what they ask of the host: the same for every screen of it.
    type Key: ViewKey;
    type Change: Clone + Debug;
    type Route: Clone + Debug;
    type Request: Clone + Debug + PartialEq;
    /// What only the toolkit's shell can do (a page pushed, Settings put
    /// up, signed out), taken after each call ([`Hosting::take_natives`]).
    type Native: Clone + Debug + PartialEq;

    /// Its runtimes made over a session just opened.
    fn open(session: &Session, cx: &mut Cx<'_, Self>) -> Self;
    /// Every runtime it holds, to drain, sum and let go of.
    fn screens(&mut self) -> Vec<&mut dyn Running<Self>>;
    fn screens_ref(&self) -> Vec<&dyn Running<Self>>;
    /// The views it reads itself, beside its screens'.
    fn holds(&self) -> Vec<Self::Key> {
        vec![]
    }
    /// The time a call is worked out at, of the device's clock.
    fn now_ms(&self, clock_ms: i64) -> i64 {
        clock_ms
    }
    /// A route one of its screens opened.
    fn route(&mut self, route: Self::Route, cx: &mut Cx<'_, Self>);
    /// Back, from one of its screens.
    fn back(&mut self, _cx: &mut Cx<'_, Self>) {}
    /// A request one of its screens made.
    fn request(&mut self, request: Self::Request, cx: &mut Cx<'_, Self>);
    /// After every call: what one runtime's page says that another follows.
    fn follow(&mut self, _cx: &mut Cx<'_, Self>) {}
    /// The store moved, or a module answered later: what waited for it.
    fn moved(&mut self, _cx: &mut Cx<'_, Self>) {}
    /// A link taken while the app shows: what shows put away, then gone to.
    fn link(&mut self, link: Link, cx: &mut Cx<'_, Self>);
    /// The launch's own arguments, once its runtimes run.
    fn launched(&mut self, args: Vec<String>, cx: &mut Cx<'_, Self>);
}

/// An app's part that runs `A` as one of its screens.
pub trait Has<A: App> {
    fn screen(&self) -> &Screen<A>;
    fn screen_mut(&mut self) -> &mut Screen<A>;
}

/// An app's part at work over a session: the loop every host drives.
pub struct Hosting<H: HostedApp> {
    app: H,
    jobs: VecDeque<Job<H::Route, H::Request>>,
    natives: Vec<H::Native>,
    held: Vec<H::Key>,
    ran: u64,
}

impl<H: HostedApp> Hosting<H> {
    /// The app's part over a session just opened, what it asked carried out.
    pub fn open(session: &Session, place: &mut dyn Place<H::Key, H::Change>) -> Hosting<H> {
        let (mut jobs, mut natives) = (VecDeque::new(), vec![]);
        let now_ms = place.now_ms();
        let app = {
            let mut cx = Cx {
                place: &mut *place,
                now_ms,
                jobs: &mut jobs,
                natives: &mut natives,
            };
            H::open(session, &mut cx)
        };
        let mut h = Hosting {
            app,
            jobs,
            natives,
            held: vec![],
            ran: 0,
        };
        h.settle(place);
        h
    }

    /// The app's part, as it is.
    pub fn app(&self) -> &H {
        &self.app
    }

    /// `f` over the app's part, then what it asked carried out: for what a
    /// host runs beside its screens (a register pushed on a phone).
    pub fn with<R>(&mut self, place: &mut dyn Place<H::Key, H::Change>, f: impl FnOnce(&mut H, &mut Cx<'_, H>) -> R) -> R {
        let now_ms = self.app.now_ms(place.now_ms());
        let r = {
            let mut cx = Cx {
                place: &mut *place,
                now_ms,
                jobs: &mut self.jobs,
                natives: &mut self.natives,
            };
            f(&mut self.app, &mut cx)
        };
        self.settle(place);
        r
    }

    /// The time its pages are worked out at, now.
    pub fn now_ms(&self, place: &dyn Place<H::Key, H::Change>) -> i64 {
        self.app.now_ms(place.now_ms())
    }

    /// `msg` taken by the screen `A`.
    pub fn send<A>(&mut self, msg: A::Msg, place: &mut dyn Place<H::Key, H::Change>)
    where
        H: Has<A>,
        A: App<Key = H::Key, Change = H::Change, Route = H::Route, Request = H::Request>,
    {
        self.with(place, |app, cx| app.screen_mut().send(msg, cx))
    }

    /// What the screen `A` shows now.
    pub fn page<A>(&self, place: &dyn Place<H::Key, H::Change>) -> A::Screen
    where
        H: Has<A>,
        A: App<Key = H::Key, Change = H::Change, Route = H::Route, Request = H::Request>,
    {
        self.app.screen().page(place, self.now_ms(place))
    }

    /// The screen `A`'s page drawn: whether anything waited on it.
    pub fn drawn<A>(&mut self, place: &mut dyn Place<H::Key, H::Change>) -> bool
    where
        H: Has<A>,
        A: App<Key = H::Key, Change = H::Change, Route = H::Route, Request = H::Request>,
    {
        self.with(place, |app, cx| app.screen_mut().drawn(cx))
    }

    /// The store moved, or a module answered later: every screen's answers
    /// taken, and what waited for it.
    pub fn moved(&mut self, place: &mut dyn Place<H::Key, H::Change>) {
        self.with(place, |app, cx| {
            for s in app.screens() {
                s.drain(cx);
            }
            app.moved(cx);
        })
    }

    /// A link taken while the app shows.
    pub fn link(&mut self, link: Link, place: &mut dyn Place<H::Key, H::Change>) {
        self.with(place, |app, cx| app.link(link, cx))
    }

    /// The launch's own arguments.
    pub fn launched(&mut self, args: Vec<String>, place: &mut dyn Place<H::Key, H::Change>) {
        self.with(place, |app, cx| app.launched(args, cx))
    }

    /// Whether nothing of the app's is left to come.
    pub fn idle(&self, place: &dyn Place<H::Key, H::Change>) -> bool {
        self.app.screens_ref().iter().all(|s| s.is_idle(place))
    }

    /// Its runtimes' versions summed: grows with every message any took.
    pub fn version(&self) -> u64 {
        self.app.screens_ref().iter().map(|s| s.version()).sum()
    }

    /// How many routes and requests were carried out: a host whose shell
    /// reads again only when told knows to tell it.
    pub fn ran(&self) -> u64 {
        self.ran
    }

    /// What its toolkit's shell is asked, in order, taken.
    pub fn take_natives(&mut self) -> Vec<H::Native> {
        std::mem::take(&mut self.natives)
    }

    /// The session closed: every view held let go.
    pub fn close(mut self, place: &mut dyn Place<H::Key, H::Change>) {
        for s in self.app.screens() {
            s.close(place);
        }
        for k in std::mem::take(&mut self.held) {
            place.release(&k)
        }
    }

    /// Every job asked so far carried out, in order, and what they asked in
    /// turn; then what follows, and the app's own views held.
    fn settle(&mut self, place: &mut dyn Place<H::Key, H::Change>) {
        let now_ms = self.app.now_ms(place.now_ms());
        // A follow that asks again and again is a bug; it stops, not hangs.
        for _ in 0..64 {
            let mut cx = Cx {
                place: &mut *place,
                now_ms,
                jobs: &mut self.jobs,
                natives: &mut self.natives,
            };
            while let Some(job) = cx.jobs.pop_front() {
                self.ran += 1;
                match job {
                    Job::Open(r) => self.app.route(r, &mut cx),
                    Job::Back => self.app.back(&mut cx),
                    Job::Request(q) => self.app.request(q, &mut cx),
                }
            }
            self.app.follow(&mut cx);
            if self.jobs.is_empty() {
                break;
            }
        }
        let want = self.app.holds();
        if want != self.held {
            for k in &want {
                place.hold(k.clone())
            }
            for k in std::mem::replace(&mut self.held, want) {
                place.release(&k)
            }
        }
    }
}
