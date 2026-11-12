//! An Ark app's runtime: the whole app — its state and every rule of it —
//! in Rust, and each platform's UI a native view of it (ui/docs/app.md).
//!
//! An [`App`] has a [`App::State`], the [`App::Msg`]s that change it, and
//! an [`App::update`] that takes one and returns the [`Command`]s it wants
//! carried out — a change sent to the peer, a route opened, a module's
//! operation, a request of the host — as data, never as a call. What a
//! screen draws is [`App::screen`]: a record worked out from the state and
//! the views the store holds (the page record), which the generated
//! SwiftUI, Compose, iced and Leptos read field by field.
//!
//! It is built on what the shells already share, not beside it:
//!
//! - **data** is the views held for it, read through the kernel's port
//!   ([`ark::views::Views`]; a peer's `ark_peer::store::Store` is one): an
//!   app says which views its screen needs ([`App::holds`]) and reads their
//!   rows; it never copies them into its state.
//! - **writes** are the domain's typed changes (`MoneyChange`), sent by
//!   the host ([`Host::send`]) — what became of one comes back as a
//!   message when the update asked ([`Command::Send`]'s `then`).
//! - **navigation** is the domain's `Route` ([`Command::Open`]): the host
//!   hands it to its Navigator, its back stack, its address bar.
//! - **the device** is [`ark_module`]: an [`Op`] whose output is typed,
//!   carried out through what the host registered, what came of it a
//!   message again ([`Command::Ask`]), now or (the camera) later
//!   ([`Runtime::answer`]).
//! - **the host's own work** is the app's [`App::Request`]: a typed value
//!   for what only the host can do yet (money's bank sync, a sheet still
//!   drawn by hand), carried out by [`Host::request`]. There is no
//!   stringly-typed call into a shell.
//!
//! [`Runtime`] owns the loop: a message in, the state updated, the
//! commands carried out (their messages run in turn), and
//! [`Runtime::version`] grown — the number a native shell watches, as it
//! watches the store's (`Session::version`), to know to read the screen
//! again; [`Runtime::is_idle`] says nothing is left to come (the
//! screenshot drivers wait on it). It is UI-free and needs no clock or
//! thread of its own, so the same runtime runs behind the phones' bridge,
//! in the iced window and in the browser's wasm.

// `#[view]`'s paths are `ark_app::…`, here too (`welcome`).
extern crate self as ark_app;

use std::any::Any;
use std::collections::{BTreeSet, VecDeque};
use std::fmt::{self, Debug};
use std::sync::{Arc, Mutex};

pub use ark::views::{ViewKey, Views};
pub use ark_module::{self as modules, Answer, ModuleError, Modules, Op, Reply};

/// An app: its state, its messages, how one changes the other, and what
/// its screen shows.
pub trait App: 'static {
    /// The views it reads, as the store keys them (money's `MoneyView`).
    type Key: ViewKey;
    /// What the app remembers that is not the domain's: a form being
    /// filled, a row being renamed. The domain's rows stay in the store.
    type State: Default;
    /// What it is told. `Send`, as a module's answer may come from
    /// another thread.
    type Msg: Clone + Debug + Send;
    /// A change of the domain's (money's `MoneyChange`).
    type Change: Clone + Debug;
    /// Somewhere the app can be (money's `Route`).
    type Route: Clone + Debug;
    /// What only its host can do for it, typed (money's `MoneyRequest`):
    /// `()` for an app that asks nothing of the kind.
    type Request: Clone + Debug + PartialEq;
    /// What the screen draws: every figure and word worked out.
    type Screen;

    /// The views the screen needs held while it shows.
    fn holds(state: &Self::State) -> Vec<Self::Key>;

    /// `msg` taken into `state`: what is to be done about it.
    fn update(state: &mut Self::State, msg: Self::Msg, data: &Data<'_, Self::Key>) -> Vec<Command<Self>>;

    /// What the screen shows of `state` and the views held.
    fn screen(state: &Self::State, data: &Data<'_, Self::Key>) -> Self::Screen;

    /// The message a wake-up is to the app ([`Woken`]): the OS woke the
    /// process for it, with no screen. None, the app has nothing to do then.
    fn woken(_reason: Woken) -> Option<Self::Msg> {
        None
    }
}

/// Why the OS woke the process with no screen (ui/docs/app.md, Background
/// And Notifications): what the host that woke it hands
/// [`Runtime::wake`], which the app takes as its own message
/// ([`App::woken`]) and answers with commands like any other. The host
/// waits until the runtime is idle — a [`Command::Sync`] and every module's
/// answer count as work — or its time from the OS runs out, then lets the
/// process go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Woken {
    /// The periodic background sync app.toml asks for (`[background] sync`).
    Sync,
    /// A push came: a wake-up and nothing of the app's data; the app syncs
    /// and says what came from its own rows.
    Push,
    /// A button on one of its notices was pressed (`notice` is the notice's
    /// id, `action` the button's).
    Action { notice: String, action: String },
    /// The device started (Android, app.toml's `[background] boot`).
    Boot,
}

/// What an update and a screen read besides the state: the views the store
/// holds, and the time the host says it is (the browser has no clock
/// `std` can read).
pub struct Data<'a, K: ViewKey> {
    pub store: &'a dyn Views<K>,
    pub now_ms: i64,
}

impl<'a, K: ViewKey> Data<'a, K> {
    /// The rows of the view `key`, as `T`; none while it is not held.
    pub fn rows<T: 'static>(&self, key: &K) -> &'a [T] {
        self.store.rows(key)
    }
}

/// What a message comes back as when a change sent was taken or refused.
pub type Sent<M> = fn(Result<(), String>) -> M;
/// What a message comes back as when a module's operation, whose output is
/// `T`, was done or not.
pub type Answered<M, T> = fn(Answer<T>) -> M;

// * Asking a module

/// A module's operation, with the message what comes of it is to become:
/// data still (it prints and compares as its operation), its output's type
/// known only inside.
pub struct Ask<M> {
    asking: Box<dyn Asking<M>>,
}

/// An operation and its `then`, the output's type erased.
trait Asking<M>: Send + Sync {
    fn run(self: Box<Self>, modules: &Modules, answered: Box<dyn FnOnce(Option<M>) + Send>);
    fn module(&self) -> &'static str;
    fn wants(&self) -> bool;
    fn debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn same(&self, other: &dyn Asking<M>) -> bool;
    fn as_any(&self) -> &dyn Any;
}

struct Asked<O: Op, M> {
    op: O,
    then: Option<Answered<M, O::Output>>,
}

impl<O: Op, M: 'static> Asking<M> for Asked<O, M> {
    fn run(self: Box<Self>, modules: &Modules, answered: Box<dyn FnOnce(Option<M>) + Send>) {
        let then = self.then;
        modules.run(&self.op, move |a| answered(then.map(|t| t(a))))
    }
    fn module(&self) -> &'static str {
        O::MODULE
    }
    fn wants(&self) -> bool {
        self.then.is_some()
    }
    fn debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.op, f)
    }
    fn same(&self, other: &dyn Asking<M>) -> bool {
        other.as_any().downcast_ref::<Asked<O, M>>().is_some_and(|o| {
            o.op == self.op
                && match (o.then, self.then) {
                    (Some(a), Some(b)) => std::ptr::fn_addr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<M: 'static> Ask<M> {
    /// `op`, what comes of it not wanted.
    pub fn new<O: Op>(op: O) -> Ask<M> {
        Ask {
            asking: Box::new(Asked::<O, M> { op, then: None }),
        }
    }

    /// `op`, what comes of it `then`'s message.
    pub fn then<O: Op>(op: O, then: Answered<M, O::Output>) -> Ask<M> {
        Ask {
            asking: Box::new(Asked { op, then: Some(then) }),
        }
    }

    /// The module it is for (`"ark-haptics"`).
    pub fn module(&self) -> &'static str {
        self.asking.module()
    }

    /// The operation, as its module's own type, when it is one.
    pub fn op<O: Op>(&self) -> Option<&O> {
        let a: &dyn Any = self.asking.as_any();
        a.downcast_ref::<Asked<O, M>>().map(|a| &a.op)
    }

    /// Whether a message is wanted of what comes of it.
    pub fn wants_answer(&self) -> bool {
        self.asking.wants()
    }

    /// Carried out by `modules`; the message what came of it became (none
    /// when none was wanted) handed to `answered`, at once or later, once.
    pub fn run(self, modules: &Modules, answered: impl FnOnce(Option<M>) + Send + 'static) {
        self.asking.run(modules, Box::new(answered))
    }
}

impl<M> Debug for Ask<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.asking.debug(f)?;
        if self.asking.wants() {
            f.write_str(", then")?;
        }
        Ok(())
    }
}

impl<M> PartialEq for Ask<M> {
    fn eq(&self, other: &Ask<M>) -> bool {
        self.asking.same(&*other.asking)
    }
}

// * Commands

/// Something an update wants done: data, so it can be tested, logged and
/// carried across a bridge.
pub enum Command<A: App + ?Sized> {
    /// A change sent to the peer; what became of it is `then`'s message.
    Send { change: A::Change, then: Option<Sent<A::Msg>> },
    /// A route opened.
    Open(A::Route),
    /// Back from the page shown.
    Back,
    /// A module's operation; what came of it, `then`'s message.
    Ask(Ask<A::Msg>),
    /// Something only the host can do, typed ([`App::Request`]).
    Request(A::Request),
    /// A message to run next, after this one's commands.
    Then(A::Msg),
    /// A message to run once the screen as this one leaves it has been
    /// drawn: what must be seen to go before what comes next — a sheet or
    /// an inspector put away while the place it is over still shows,
    /// before a route opens somewhere else (SwiftUI never puts away a sheet
    /// of a tab switched from in the same update). A host that draws
    /// nothing in between ([`Host::draws_between`] false) runs it next, as
    /// [`Command::Then`].
    AfterDraw(A::Msg),
    /// A file a row names, fetched when this device does not hold it: what a
    /// screen about to show it asks ([`Views::file_url`] says where it is
    /// read once it is here; the views are rung when it arrives).
    Want(ark::file::FileRef),
    /// The replica synced with its server now — dialled if it was not
    /// connected, what is pending pushed and what is new pulled — and then
    /// `then`, if any, run over what it holds: what a wake-up does before
    /// it tells the person anything. Work until it is answered
    /// ([`Runtime::is_idle`]), so a process woken for it is not let go
    /// mid-sync.
    Sync { then: Option<A::Msg> },
}

impl<A: App + ?Sized> Command<A> {
    /// A change sent, nothing said of what became of it but the store's
    /// refusal (`Store::refusal`).
    pub fn send(change: A::Change) -> Command<A> {
        Command::Send { change, then: None }
    }

    /// A module's operation, what comes of it not wanted.
    pub fn ask(op: impl Op) -> Command<A> {
        Command::Ask(Ask::new(op))
    }

    /// A module's operation, what comes of it `then`'s message.
    pub fn ask_then<O: Op>(op: O, then: Answered<A::Msg, O::Output>) -> Command<A> {
        Command::Ask(Ask::then(op, then))
    }
}

impl<A: App + ?Sized> Debug for Command<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Send { change, then } => write!(f, "Send({change:?}{})", if then.is_some() { ", then" } else { "" }),
            Command::Open(r) => write!(f, "Open({r:?})"),
            Command::Back => write!(f, "Back"),
            Command::Ask(a) => write!(f, "Ask({a:?})"),
            Command::Request(r) => write!(f, "Request({r:?})"),
            Command::Then(m) => write!(f, "Then({m:?})"),
            Command::AfterDraw(m) => write!(f, "AfterDraw({m:?})"),
            Command::Want(file) => write!(f, "Want({})", file.hash),
            Command::Sync { then } => write!(f, "Sync({then:?})"),
        }
    }
}

impl<A: App + ?Sized> PartialEq for Command<A>
where
    A::Change: PartialEq,
    A::Route: PartialEq,
    A::Msg: PartialEq,
{
    fn eq(&self, other: &Command<A>) -> bool {
        match (self, other) {
            (Command::Send { change: a, then: x }, Command::Send { change: b, then: y }) => {
                a == b
                    && match (x, y) {
                        (Some(x), Some(y)) => std::ptr::fn_addr_eq(*x, *y),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (Command::Open(a), Command::Open(b)) => a == b,
            (Command::Back, Command::Back) => true,
            (Command::Ask(a), Command::Ask(b)) => a == b,
            (Command::Request(a), Command::Request(b)) => a == b,
            (Command::Then(a), Command::Then(b)) => a == b,
            (Command::AfterDraw(a), Command::AfterDraw(b)) => a == b,
            (Command::Want(a), Command::Want(b)) => a == b,
            (Command::Sync { then: a }, Command::Sync { then: b }) => a == b,
            _ => false,
        }
    }
}

/// A ticket for a module's answer that comes later: what the host hands
/// back to [`Runtime::answer`].
pub type Ticket = u64;

/// Where an app runs: the store it reads, and what carries its commands
/// out. Each shell has one — the phones' bridge, the iced window, the
/// browser.
pub trait Host<A: App + ?Sized> {
    /// `f` of the store, as it is now.
    fn read<R>(&self, f: impl FnOnce(&dyn Views<A::Key>) -> R) -> R;
    /// The time, in milliseconds since the epoch.
    fn now_ms(&self) -> i64;
    /// `change` sent to the peer: taken, or the domain's sentence for why not.
    fn send(&mut self, change: A::Change) -> Result<(), String>;
    /// `route` opened.
    fn open(&mut self, route: A::Route);
    /// Back from the page shown.
    fn back(&mut self) {}
    /// `ask` carried out (`Ask::run` over the host's modules), the message
    /// it became handed to [`Runtime::answer`] with `ticket` — at once, or
    /// whenever it comes. A host with no modules runs it over none, which
    /// answers `Unavailable`: every ticket is answered.
    fn ask(&mut self, ticket: Ticket, ask: Ask<A::Msg>);
    /// What only the host can do ([`App::Request`]).
    fn request(&mut self, _request: A::Request) {}
    /// `file` fetched from the server when this device does not hold it
    /// ([`Command::Want`]): a peer's `open_file`. A host that keeps no
    /// files does nothing.
    fn want(&mut self, _file: ark::file::FileRef) {}
    /// Whether the screen is drawn between one message and the next one
    /// sent: a phone's toolkit, which draws a page after it reads it and
    /// then tells [`Runtime::drawn`]. An iced window or the browser draws
    /// what the last message left, so what waits on a draw runs at once.
    fn draws_between(&self) -> bool {
        false
    }
    /// The replica synced now ([`Command::Sync`]): dialled if it is not
    /// connected, then caught up with its server. True when it is done
    /// already (a host with nothing to sync: the desktop's own store, a
    /// test's); false when it will be, and the host then hands `ticket` to
    /// [`Runtime::answer`] with the sync's message once the replica has
    /// caught up, or its time ran out.
    fn sync(&mut self, _ticket: Ticket) -> bool {
        true
    }
}

/// An app running: its state, the answers it waits for, and the number
/// that grows with every message.
pub struct Runtime<A: App> {
    state: A::State,
    version: u64,
    waiting: BTreeSet<Ticket>,
    tickets: Ticket,
    /// Answers that came while a message was running, taken after it.
    answers: VecDeque<(Ticket, Option<A::Msg>)>,
    /// What waits for the screen to be drawn ([`Command::AfterDraw`]).
    after_draw: VecDeque<A::Msg>,
    /// The message each sync waited on is to run when it is done
    /// ([`Command::Sync`]), by its ticket.
    syncs: std::collections::BTreeMap<Ticket, Option<A::Msg>>,
    running: bool,
}

impl<A: App> Default for Runtime<A> {
    fn default() -> Self {
        Runtime {
            state: A::State::default(),
            version: 0,
            waiting: BTreeSet::new(),
            tickets: 0,
            answers: VecDeque::new(),
            after_draw: VecDeque::new(),
            syncs: Default::default(),
            running: false,
        }
    }
}

impl<A: App> Runtime<A> {
    pub fn new() -> Runtime<A> {
        Runtime::default()
    }

    /// The state as it is.
    pub fn state(&self) -> &A::State {
        &self.state
    }

    /// A number that grows whenever a message was taken: a shell that drew
    /// the screen at an older one draws it again.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The views the screen needs held now.
    pub fn holds(&self) -> Vec<A::Key> {
        A::holds(&self.state)
    }

    /// Whether nothing is left to come: no module's answer awaited, no
    /// message queued, and every view the screen needs held in `store`.
    /// What a screenshot driver waits for, with [`Runtime::version`]
    /// unchanged since it last looked (ui/docs/app.md, Effects and Ports).
    pub fn is_idle(&self, store: &dyn Views<A::Key>) -> bool {
        !self.running
            && self.waiting.is_empty()
            && self.answers.is_empty()
            && self.after_draw.is_empty()
            && self.holds().iter().all(|k| store.holds(k))
    }

    /// Whether a message waits for the screen to be drawn
    /// ([`Command::AfterDraw`]): a host that draws between messages calls
    /// [`Runtime::drawn`] once it has.
    pub fn awaits_draw(&self) -> bool {
        !self.after_draw.is_empty()
    }

    /// The screen drawn: what waited for it taken, in order (what they ask
    /// to wait for a draw waits for the next one); whether any did.
    pub fn drawn(&mut self, host: &mut impl Host<A>) -> bool {
        let waiting = std::mem::take(&mut self.after_draw);
        let any = !waiting.is_empty();
        for m in waiting {
            self.dispatch(m, host);
        }
        any
    }

    /// What the screen shows now.
    pub fn screen(&self, host: &impl Host<A>) -> A::Screen {
        let now_ms = host.now_ms();
        host.read(|store| A::screen(&self.state, &Data { store, now_ms }))
    }

    /// What the screen shows of `store` at `now_ms`: for a host that
    /// draws from a shared borrow (iced's `view`).
    pub fn screen_of(&self, store: &dyn Views<A::Key>, now_ms: i64) -> A::Screen {
        A::screen(&self.state, &Data { store, now_ms })
    }

    /// `msg` taken: the state updated and its commands carried out, each
    /// message they come back as taken in turn.
    pub fn dispatch(&mut self, msg: A::Msg, host: &mut impl Host<A>) {
        let mut queue = VecDeque::from([msg]);
        self.running = true;
        loop {
            while let Some(msg) = queue.pop_front() {
                let now_ms = host.now_ms();
                let commands = host.read(|store| A::update(&mut self.state, msg, &Data { store, now_ms }));
                self.version += 1;
                for c in commands {
                    self.carry(c, host, &mut queue);
                }
            }
            // An answer given at once, by a host that answers inside `ask`.
            match self.answers.pop_front() {
                Some((ticket, msg)) => {
                    if self.waiting.remove(&ticket) {
                        queue.extend(msg)
                    }
                }
                None => break,
            }
        }
        self.running = false;
    }

    /// The OS woke the process for `reason`: the app's message for it taken
    /// ([`App::woken`]); whether the app had one. The host waits for
    /// [`Runtime::is_idle`] after, within its time.
    pub fn wake(&mut self, reason: Woken, host: &mut impl Host<A>) -> bool {
        match A::woken(reason) {
            Some(m) => {
                self.dispatch(m, host);
                true
            }
            None => false,
        }
    }

    /// The message a sync's ticket comes back as ([`Command::Sync`]): kept
    /// until the host answers it.
    fn synced(&mut self, ticket: Ticket) -> Option<A::Msg> {
        self.syncs.remove(&ticket).flatten()
    }

    /// What came of the operation `ticket` was given for: the message it
    /// became taken, when the update wanted one.
    pub fn answer(&mut self, ticket: Ticket, msg: Option<A::Msg>, host: &mut impl Host<A>) {
        // A sync's answer is the message its command kept.
        let msg = match self.syncs.contains_key(&ticket) {
            true => self.synced(ticket),
            false => msg,
        };
        if self.running {
            self.answers.push_back((ticket, msg));
            return;
        }
        if self.waiting.remove(&ticket) {
            match msg {
                Some(m) => self.dispatch(m, host),
                // Nothing to run, but the screen may wait on it no longer.
                None => self.version += 1,
            }
        }
    }

    /// Whether `ticket`'s answer is still awaited.
    pub fn awaits(&self, ticket: Ticket) -> bool {
        self.waiting.contains(&ticket)
    }

    fn carry(&mut self, c: Command<A>, host: &mut impl Host<A>, queue: &mut VecDeque<A::Msg>) {
        match c {
            Command::Send { change, then } => {
                let r = host.send(change);
                if let Some(then) = then {
                    queue.push_back(then(r))
                }
            }
            Command::Open(route) => host.open(route),
            Command::Back => host.back(),
            Command::Ask(ask) => {
                self.tickets += 1;
                let ticket = self.tickets;
                self.waiting.insert(ticket);
                host.ask(ticket, ask)
            }
            Command::Request(r) => host.request(r),
            Command::Want(file) => host.want(file),
            Command::Then(m) => queue.push_back(m),
            Command::AfterDraw(m) if host.draws_between() => self.after_draw.push_back(m),
            Command::AfterDraw(m) => queue.push_back(m),
            Command::Sync { then } => {
                self.tickets += 1;
                let ticket = self.tickets;
                match host.sync(ticket) {
                    true => queue.extend(then),
                    false => {
                        self.waiting.insert(ticket);
                        self.syncs.insert(ticket, then);
                    }
                }
            }
        }
    }
}

/// The answers a host gathered from modules, handed to the runtime after:
/// a host that holds modules can't reach the runtime from inside an
/// operation it is running for it, and a module that answers later (the
/// camera) answers from wherever it is.
pub struct Inbox<M> {
    answers: Arc<Mutex<Answers<M>>>,
}

/// The answers an [`Inbox`] keeps: each ticket and the message it became.
type Answers<M> = Vec<(Ticket, Option<M>)>;

impl<M> Default for Inbox<M> {
    fn default() -> Self {
        Inbox {
            answers: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl<M> Clone for Inbox<M> {
    fn clone(&self) -> Self {
        Inbox {
            answers: self.answers.clone(),
        }
    }
}

impl<M: Send + 'static> Inbox<M> {
    /// `ask` carried out by `modules`, the message it became kept for `ticket`.
    pub fn ask(&self, modules: &Modules, ticket: Ticket, ask: Ask<M>) {
        self.ask_telling(modules, ticket, ask, || {})
    }

    /// As [`Inbox::ask`], and `told` once the answer is kept, wherever it
    /// came from: what wakes a host whose module answers later (the
    /// camera, when the person is done) to drain it.
    pub fn ask_telling(&self, modules: &Modules, ticket: Ticket, ask: Ask<M>, told: impl FnOnce() + Send + 'static) {
        let answers = self.answers.clone();
        ask.run(modules, move |m| {
            answers.lock().unwrap_or_else(|e| e.into_inner()).push((ticket, m));
            told()
        });
    }

    /// The answers that came, oldest first, taken.
    pub fn take(&self) -> Vec<(Ticket, Option<M>)> {
        std::mem::take(&mut *self.answers.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Every answer that came taken into `rt`, and those that came of
    /// them, until none is left.
    pub fn drain<A: App<Msg = M>>(&self, rt: &mut Runtime<A>, host: &mut impl Host<A>) {
        loop {
            let answers = self.take();
            if answers.is_empty() {
                return;
            }
            for (t, m) in answers {
                rt.answer(t, m, host);
            }
        }
    }
}

pub mod background;
pub mod host;
pub mod shell;
pub mod view;
pub mod welcome;
pub mod writing;

/// `#[view]`: a function of a screen's record laid out once for every
/// toolkit ([`view`]).
pub use arkui_macros::view;

#[cfg(test)]
mod tests;
