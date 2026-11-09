//! An Ark app's runtime: the whole app — its state and every rule of it —
//! in Rust, and each platform's UI a native view of it (docs/ark-app.md).
//!
//! The shape is Elm's (and Crux's): an [`App`] has a [`App::State`], the
//! [`App::Msg`]s that change it, and an [`App::update`] that takes one and
//! returns the [`Command`]s it wants carried out — a change sent to the
//! peer, a route opened, a module asked — as data, never as a call. What a
//! screen draws is [`App::screen`]: a record worked out from the state and
//! the views the store holds (Crux's view model), which the generated
//! SwiftUI, Compose, iced and Leptos read field by field.
//!
//! It is built on what the shells already share, not beside it:
//!
//! - **data** is the store's ([`ark_peer::store::Store`]): an app says
//!   which views its screen needs ([`App::holds`]) and reads their rows; it
//!   never copies them into its state.
//! - **writes** are the domain's typed changes (`MoneyChange`), sent by
//!   the host ([`Host::send`]) — what became of one comes back as a
//!   message when the update asked ([`Command::Send`]'s `then`).
//! - **navigation** is the domain's `Route` ([`Command::Open`]): the host
//!   hands it to its Navigator, its back stack, its address bar.
//! - **the device** is [`ark_modules`]: a [`ModuleCall`] the host carries
//!   out through what it registered, its answer a message again, now or
//!   (the camera) later ([`Runtime::answer`]).
//!
//! [`Runtime`] owns the loop: a message in, the state updated, the
//! commands carried out (their messages run in turn), and
//! [`Runtime::version`] grown — the number a native shell watches, as it
//! watches the store's (`Session::version`), to know to read the screen
//! again. It is UI-free and needs no clock or thread of its own, so the
//! same runtime runs behind the phones' bridge, in the iced window and in
//! the browser's wasm.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Debug;

pub use ark_modules::{self as modules, ModuleAnswer, ModuleCall, Modules};
use ark_peer::store::{Store, ViewKey};

/// An app: its state, its messages, how one changes the other, and what
/// its screen shows.
pub trait App: 'static {
    /// The views it reads, as the store keys them (money's `MoneyView`).
    type Key: ViewKey;
    /// What the app remembers that is not the domain's: a form being
    /// filled, a row being renamed. The domain's rows stay in the store.
    type State: Default;
    type Msg: Clone + Debug;
    /// A change of the domain's (money's `MoneyChange`).
    type Change: Clone + Debug;
    /// Somewhere the app can be (money's `Route`).
    type Route: Clone + Debug;
    /// What the screen draws: every figure and word worked out.
    type Screen;

    /// The views the screen needs held while it shows.
    fn holds(state: &Self::State) -> Vec<Self::Key>;

    /// `msg` taken into `state`: what is to be done about it.
    fn update(state: &mut Self::State, msg: Self::Msg, data: &Data<'_, Self::Key>) -> Vec<Command<Self>>;

    /// What the screen shows of `state` and the views held.
    fn screen(state: &Self::State, data: &Data<'_, Self::Key>) -> Self::Screen;
}

/// What an update and a screen read besides the state: the views the store
/// holds, and the time the host says it is (the browser has no clock
/// `std` can read).
pub struct Data<'a, K: ViewKey> {
    pub store: &'a Store<K>,
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
/// What a message comes back as when a module answered.
pub type Answered<M> = fn(ModuleAnswer) -> M;

/// Something an update wants done: data, so it can be tested, logged and
/// carried across a bridge.
pub enum Command<A: App + ?Sized> {
    /// A change sent to the peer; what became of it is `then`'s message.
    Send { change: A::Change, then: Option<Sent<A::Msg>> },
    /// A route opened.
    Open(A::Route),
    /// Back from the page shown.
    Back,
    /// A module asked; its answer is `then`'s message.
    Ask { call: ModuleCall, then: Option<Answered<A::Msg>> },
    /// A message to run next, after this one's commands.
    Then(A::Msg),
    /// Something only the shell can do, by its name: an escape hatch for
    /// what is not the app's yet (money's bank sync is its phones' model's).
    Native(String),
}

impl<A: App + ?Sized> Command<A> {
    /// A change sent, nothing said of what became of it but the store's
    /// refusal (`Store::refusal`).
    pub fn send(change: A::Change) -> Command<A> {
        Command::Send { change, then: None }
    }

    /// A module asked, its answer not wanted.
    pub fn ask(call: ModuleCall) -> Command<A> {
        Command::Ask { call, then: None }
    }
}

impl<A: App + ?Sized> Debug for Command<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Send { change, then } => write!(f, "Send({change:?}{})", if then.is_some() { ", then" } else { "" }),
            Command::Open(r) => write!(f, "Open({r:?})"),
            Command::Back => write!(f, "Back"),
            Command::Ask { call, then } => write!(f, "Ask({call:?}{})", if then.is_some() { ", then" } else { "" }),
            Command::Then(m) => write!(f, "Then({m:?})"),
            Command::Native(n) => write!(f, "Native({n})"),
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
    fn read<R>(&self, f: impl FnOnce(&Store<A::Key>) -> R) -> R;
    /// The time, in milliseconds since the epoch.
    fn now_ms(&self) -> i64;
    /// `change` sent to the peer: taken, or the domain's sentence for why not.
    fn send(&mut self, change: A::Change) -> Result<(), String>;
    /// `route` opened.
    fn open(&mut self, route: A::Route);
    /// Back from the page shown.
    fn back(&mut self) {}
    /// `call` carried out, its answer handed to [`Runtime::answer`] with
    /// `ticket` — at once, or whenever it comes.
    fn ask(&mut self, ticket: Ticket, call: ModuleCall);
    /// What only the shell can do, by name ([`Command::Native`]).
    fn native(&mut self, _name: &str) {}
}

/// An app running: its state, the answers it waits for, and the number
/// that grows with every message.
pub struct Runtime<A: App> {
    state: A::State,
    version: u64,
    waiting: BTreeMap<Ticket, Answered<A::Msg>>,
    tickets: Ticket,
    /// Answers that came while a message was running, taken after it.
    answers: VecDeque<(Ticket, ModuleAnswer)>,
    running: bool,
}

impl<A: App> Default for Runtime<A> {
    fn default() -> Self {
        Runtime {
            state: A::State::default(),
            version: 0,
            waiting: BTreeMap::new(),
            tickets: 0,
            answers: VecDeque::new(),
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

    /// What the screen shows now.
    pub fn screen(&self, host: &impl Host<A>) -> A::Screen {
        let now_ms = host.now_ms();
        host.read(|store| A::screen(&self.state, &Data { store, now_ms }))
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
                Some((ticket, answer)) => {
                    if let Some(then) = self.waiting.remove(&ticket) {
                        queue.push_back(then(answer))
                    }
                }
                None => break,
            }
        }
        self.running = false;
    }

    /// A module's answer to the call `ticket` was given for: its message
    /// taken, when the update wanted one.
    pub fn answer(&mut self, ticket: Ticket, answer: ModuleAnswer, host: &mut impl Host<A>) {
        if self.running {
            self.answers.push_back((ticket, answer));
            return;
        }
        if let Some(then) = self.waiting.remove(&ticket) {
            self.dispatch(then(answer), host)
        }
    }

    /// Whether `ticket`'s answer is still awaited.
    pub fn awaits(&self, ticket: Ticket) -> bool {
        self.waiting.contains_key(&ticket)
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
            Command::Ask { call, then } => {
                self.tickets += 1;
                let ticket = self.tickets;
                if let Some(then) = then {
                    self.waiting.insert(ticket, then);
                }
                host.ask(ticket, call)
            }
            Command::Then(m) => queue.push_back(m),
            Command::Native(n) => host.native(&n),
        }
    }
}

/// The answers a host gathered from modules that answered inside its
/// `ask`, handed to the runtime after: a host that holds modules can't
/// reach the runtime from inside a call it is making for it.
#[derive(Default)]
pub struct Inbox {
    answers: std::sync::Arc<std::sync::Mutex<Vec<(Ticket, ModuleAnswer)>>>,
}

impl Clone for Inbox {
    fn clone(&self) -> Self {
        Inbox {
            answers: self.answers.clone(),
        }
    }
}

impl Inbox {
    /// `call` carried out by `modules`, its answer kept for `ticket`.
    pub fn ask(&self, modules: &Modules, ticket: Ticket, call: ModuleCall) {
        let answers = self.answers.clone();
        modules.call(call, move |a| answers.lock().unwrap_or_else(|e| e.into_inner()).push((ticket, a)));
    }

    /// The answers that came, oldest first, taken.
    pub fn take(&self) -> Vec<(Ticket, ModuleAnswer)> {
        std::mem::take(&mut *self.answers.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(test)]
mod tests;
