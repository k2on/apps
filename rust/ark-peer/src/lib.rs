//! The ArkDB v5 peer: a driver of the one machine (§1.7), twice — the
//! log's and, when the module has live state, the volatile space's (§1.9)
//! — over one disk. A device and a server are the same [`Peer`] in two
//! roles:
//!
//! - **a device** ([`Peer::open`]) is a replica, or an authority alone
//!   since its fork; it **dials** a server ([`Peer::connect`]: the link,
//!   `link`), keeps its pending intents on its disk, relays its ephemeral
//!   calls, and keeps the maintained views a UI draws (§1.10);
//! - **a server** ([`Peer::serve`]) is the authority of each space; it
//!   **listens** (`hub`: the connections, each with its identity and its
//!   feed, routed; the journal beside the log) and, natively, on a socket
//!   (`serve`: the hub on a thread of its own, the sync socket, the admin
//!   listener, the web build's routes and sign-in).
//!
//! It is sans-io like the machines: a UI, a test or the hub's thread feeds
//! it what happens (an intent, a frame, a connection up or down, the clock)
//! and it does four things with what the machines say. A frame for its
//! server goes out while it is linked and is dropped while it is not; a
//! frame for a connection is delivered once what the machine made durable
//! is on the disk — **written before delivered**, and while a write fails
//! every delivery waits with it, in order ([`Peer::failure`]); every change
//! is pushed into the views watching its space; and every verdict on its
//! own intents is kept (`app`: what a UI asks by name).
//!
//! **The relay** of ephemeral calls is the driver's policy, as §1.9 says:
//! a call to a function the module names ephemeral goes to the volatile
//! machine at most once every `every_ms`, coalesced to the newest call per
//! function; the rest go at once.
//!
//! **The origin** (§1.6) is sixteen bytes a device draws at its first open
//! and keeps on its disk (`origin`); every intent's id is its origin's and
//! its `local`'s, whoever is signed in. **Sign-in** gives the device
//! another login: its pending intents are the signer's from then on, their
//! ids and the rows they drew kept. A login of another actor than the one
//! the device authored as gets an origin of its own, named by the old one
//! and its session, and the pending intents are authored again from its
//! first `local` — an authority binds an origin to the actor whose intents
//! first came from it.
//!
//! **Who is signed in** is the domain's rows (`signed`): a device reads
//! its session and signs out through the domain, a server judges every
//! token by the row it names and closes what is revoked or expired.
//!
//! Every input either machine of a device is given is recorded, when asked
//! (`record`), as an [`Act`]: a device is a pure function of these, which
//! the trace player (`ark-dev`) holds it to.

pub mod app;
pub mod disk;
mod hub;
pub mod link;
pub mod signed;

#[cfg(not(target_arch = "wasm32"))]
pub mod admin;
#[cfg(not(target_arch = "wasm32"))]
pub mod serve;
#[cfg(not(target_arch = "wasm32"))]
pub mod web;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ark::format::Encode;
use ark::frames::Frame;
use ark::hash::EntryHash;
use ark::log::{Durable, Entry, EntryId, Logged, Origin, Seq};
use ark::peer::{Ctx, Feed, Functions, Input, Journal, Output, Peer as Machine, Role, Who};
use ark::plan::Maintained;
use ark::schema::{Schema, Space};
use ark::store::Change;
use ark::{Domain, Refusal};

pub use app::{nobody, Standing, Status, Update};
pub use disk::{Disk, Files, Memory};
pub use hub::{ConnId, Deliver};
pub use link::{Event, Pumped, Wake};
#[cfg(not(target_arch = "wasm32"))]
pub use serve::{dev, Authenticate, Handle, Running, Server};
pub use signed::Signed;

/// One space's module: its schema, the functions a peer runs by name, and
/// the domain its views are made by.
#[derive(Clone)]
pub struct Module {
    pub schema: Arc<Schema>,
    pub functions: Functions,
    pub domain: Option<Arc<Domain>>,
}

impl Module {
    /// A domain as a peer holds it (§1.16).
    pub fn of(d: &Domain) -> Module {
        Module {
            schema: d.schema.clone(),
            functions: d.functions(),
            domain: Some(Arc::new(d.clone())),
        }
    }
}

/// What `on_join` and `on_leave` are authored with: from who a connection
/// is and what its device said, the function's input, or none.
pub type Arrive = Arc<dyn Fn(&Who, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

/// The live space: its module; as a device, what its `Hello` says it is
/// and the functions relayed as ephemeral, and how often; as a server, the
/// functions it authors as devices come and go and the input it authors
/// them with, and the entries kept above the horizon.
#[derive(Clone)]
pub struct Live {
    pub module: Module,
    pub device: Option<Vec<u8>>,
    pub ephemeral: BTreeSet<String>,
    pub every_ms: i64,
    pub on_join: Option<String>,
    pub on_leave: Option<String>,
    pub on_restore: Option<String>,
    pub arrive: Arrive,
    pub keep: Option<usize>,
}

impl Live {
    pub fn new(module: Module) -> Live {
        Live {
            module,
            device: None,
            ephemeral: BTreeSet::new(),
            every_ms: 0,
            on_join: None,
            on_leave: None,
            on_restore: None,
            arrive: Arc::new(|_, _| None),
            keep: None,
        }
    }
}

/// What a peer is made of: the log's module and the live one; the domain
/// its functions are called by name in, when there is one (`app`); and, as
/// a server, how each connection is fed (§1.8) and the entries kept above
/// the horizon (all of them when none).
#[derive(Clone)]
pub struct Config {
    pub log: Module,
    pub live: Option<Live>,
    pub domain: Option<Arc<Domain>>,
    pub feed: Arc<dyn Fn(&Who) -> Feed + Send + Sync>,
    pub keep: Option<usize>,
}

impl Config {
    pub fn new(log: Module) -> Config {
        Config {
            log,
            live: None,
            domain: None,
            feed: Arc::new(|_| Feed::Whole),
            keep: None,
        }
    }

    /// A domain's: its module for the log, its functions by name.
    pub fn of(domain: Arc<Domain>) -> Config {
        Config {
            domain: Some(domain.clone()),
            ..Config::new(Module::of(&domain))
        }
    }
}

/// A patch of a view's rows, each its bytes.
pub type Patch = ark::plan::Patch<Vec<u8>>;

/// What a joined peer hands over again until the server answers it.
pub type Joining = (Seq, EntryHash, Vec<Logged>);

/// One input to one machine; or both machines opened afresh as a login,
/// as at [`Peer::open`] — the log's alone from its fork when it was
/// alone, handing its chain over again when it had joined unanswered; or a
/// sign-in: the log's machine authoring as the login from its first
/// `local`, the live one afresh.
#[derive(Clone, Debug)]
pub enum Act {
    Open(Who, Origin, Option<(Seq, EntryHash)>, Option<Joining>),
    SignIn(Who, Origin),
    Step(Space, Input),
}

/// Where a machine stands after a step: its head, its state, its pending
/// intents — what a trace holds a replay to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct At {
    pub head: (Seq, EntryHash),
    pub state: ark::hash::StateHash,
    pub pending: Vec<EntryId>,
}

impl At {
    pub fn of(p: &Machine) -> At {
        At {
            head: p.log().head(),
            state: p.store.state_hash(),
            pending: p.pending.iter().map(|x| x.entry.id).collect(),
        }
    }
}

/// One recorded act, what it said, and where its machine stood after.
pub type Recorded = (Act, Vec<Output>, Option<At>);

pub struct Peer {
    pub config: Config,
    pub log: Machine,
    pub live: Option<Machine>,
    pub disk: Box<dyn Disk>,
    /// Every input given, when recording.
    pub record: Option<Vec<Recorded>>,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
    /// Whether a connection to its server is up.
    pub linked: bool,
    /// The link: the server, the login's token, the connection and when it
    /// is dialled again.
    pub link: link::Link,
    /// The connections it serves.
    hub: hub::Hub,
    /// Records not yet written: everything delivered waits for them.
    unwritten: Vec<Vec<u8>>,
    /// Frames for its server.
    frames: Vec<Vec<u8>>,
    views: Vec<(Space, Option<Box<dyn Maintained>>)>,
    relay: BTreeMap<String, (Vec<u8>, i64)>,
    relayed: Option<i64>,
    /// What became of its own intents, and what its views did.
    told: app::Told,
    saved: (i64, Vec<EntryId>),
    /// How much the `pending` journal holds, in entries and records; `None`
    /// until it is written whole, as it is first after an open.
    journal: Option<usize>,
    joined: bool,
}

/// The role of a device alone since `fork`.
pub fn alone_at(fork: Option<(Seq, EntryHash)>) -> Role {
    Role::Authority {
        journal: Journal { fork, ..Journal::default() },
    }
}

fn machine(m: &Module, role: Role, me: &Who, origin: Option<Origin>, space: Space) -> Machine {
    let mut p = Machine::new(m.schema.clone(), m.functions.clone(), role, me.clone()).in_space(space);
    if let Some(o) = origin {
        p.origin = o;
    }
    p
}

/// A device's live machine as `me`.
fn live_of(config: &Config, me: &Who, origin: Origin) -> Option<Machine> {
    let l = config.live.as_ref()?;
    let mut p = machine(&l.module, Role::Replica, me, Some(origin), Space::Volatile);
    p.device = l.device.clone();
    Some(p)
}

/// Sixteen bytes no other replica draws: the process's hasher keys (which
/// the platform seeds), the clock and the process, through sha256.
pub fn draw_origin() -> Origin {
    use std::hash::{BuildHasher, Hasher};
    let nanos = (now_ms() as u128) << 20 | monotonic_ms() as u128;
    let mut b = nanos.to_be_bytes().to_vec();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        b.extend(h.finish().to_be_bytes());
    }
    #[cfg(not(target_arch = "wasm32"))]
    b.extend(std::process::id().to_be_bytes());
    Origin::named(b)
}

/// The wall clock, in milliseconds: what an intent's `now` is.
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now() as i64;
    #[cfg(not(target_arch = "wasm32"))]
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A clock that only goes forward, in milliseconds from some start: what a
/// deadline is (`Instant` panics on wasm32-unknown-unknown).
pub fn monotonic_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now() as u64;
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }
}

/// Who a login is, as a server verifies it: the account, the session, the
/// roles it holds.
pub fn who(l: &ark_auth::Login) -> Who {
    Who {
        actor: l.user.id.clone(),
        session: l.session.clone(),
        roles: l.user.roles.clone(),
    }
}

impl Peer {
    fn with(config: Config, log: Machine, live: Option<Machine>, disk: Box<dyn Disk>) -> Peer {
        Peer {
            config,
            log,
            live,
            disk,
            record: None,
            failure: None,
            linked: false,
            link: link::Link::default(),
            hub: hub::Hub::default(),
            unwritten: vec![],
            frames: vec![],
            views: vec![],
            relay: BTreeMap::new(),
            relayed: None,
            told: app::Told::default(),
            saved: (0, vec![]),
            journal: None,
            joined: false,
        }
    }

    /// A device opened as `me` from what `disk` holds.
    pub fn open(config: Config, me: Who, disk: Box<dyn Disk>, record: bool) -> Peer {
        Peer::opened(config, me, disk, record.then(Vec::new), link::Link::default())
    }

    fn opened(config: Config, me: Who, mut disk: Box<dyn Disk>, mut record: Option<Vec<Recorded>>, link: link::Link) -> Peer {
        let fork = disk.get("fork").ok().flatten().and_then(|b| <(Seq, EntryHash)>::from_bytes(&b).ok());
        let joining = disk
            .get("joining")
            .ok()
            .flatten()
            .and_then(|b| <((Seq, EntryHash), Vec<Logged>)>::from_bytes(&b).ok())
            .map(|((n, h), c)| (n, h, c));
        let origin = match disk.get("origin").ok().flatten().and_then(|b| Origin::from_bytes(&b).ok()) {
            Some(o) => o,
            None => {
                let o = draw_origin();
                let _ = disk.put("origin", &o.to_bytes());
                o
            }
        };
        if let Some(r) = &mut record {
            r.push((Act::Open(me.clone(), origin, fork, joining.clone()), vec![], None));
        }
        let role = if fork.is_some() { alone_at(fork) } else { Role::Replica };
        let mut log = machine(&config.log, role, &me, Some(origin), Space::Durable);
        log.joining = joining;
        let live = live_of(&config, &me, origin);
        let mut p = Peer::with(config, log, live, disk);
        (p.record, p.link, p.joined) = (record, link, p.log.joining.is_some());
        let read = p.disk.load().and_then(|s| Ok((s, p.disk.get("pending")?)));
        let ((stored, pending), error) = match read {
            Ok(s) => (s, None),
            Err(e) => (Default::default(), Some(e.to_string())),
        };
        p.failure = error;
        let (local, pending) = pending.map(|b| disk::pending(&b)).unwrap_or_default();
        let durable = Durable {
            rows: stored.rows.unwrap_or_else(|| p.log.confirmed_rows()),
            logged: stored.logged,
            pending,
            local,
        };
        p.step(Space::Durable, Input::Open(durable));
        p
    }

    /// This device stopped and opened again from its disk, unlinked; its
    /// link's server, login and transport kept.
    pub fn reopen(self) -> Peer {
        let me = self.log.me.clone();
        let mut link = self.link;
        link.wire = None;
        Peer::opened(self.config, me, self.disk, self.record, link)
    }

    pub fn machine(&self, space: Space) -> Option<&Machine> {
        match space {
            Space::Durable => Some(&self.log),
            Space::Volatile => self.live.as_ref(),
        }
    }

    fn machine_mut(&mut self, space: Space) -> Option<&mut Machine> {
        match space {
            Space::Durable => Some(&mut self.log),
            Space::Volatile => self.live.as_mut(),
        }
    }

    /// One input to one machine of a device, and what it says, routed.
    pub fn step(&mut self, space: Space, input: Input) {
        self.route(space, None, input);
    }

    /// One input to one machine, from a connection or from this peer, and
    /// what it says routed: a frame back to that connection (or to its
    /// server), to every connection it fans out to, or to a session's; a
    /// record to the disk; changes to the views; verdicts kept.
    fn route(&mut self, space: Space, from: Option<ConnId>, input: Input) -> Vec<Output> {
        let rec = self.record.is_some().then(|| input.clone());
        let Some(m) = self.machine_mut(space) else { return vec![] };
        let out = m.step(input);
        for o in &out {
            match o {
                Output::Send(b) => match from {
                    Some(c) => self.hub.held.push((c, b.clone())),
                    None if self.linked => self.frames.push(b.clone()),
                    None => {}
                },
                Output::Fanout(b) => self.hub.fanout(space, b),
                Output::To { session, bytes } => self.hub.to(session, bytes),
                Output::Persist(b) => {
                    self.unwritten.push(b.clone());
                    self.write();
                }
                Output::Changes(cs) => {
                    self.changed(space, cs);
                    if space == Space::Durable {
                        self.signs(cs);
                    }
                }
                Output::Refused { id, why } if !self.hub.serving => self.told.refused(space, *id, why),
                Output::Confirmed(id) if !self.hub.serving => self.told.confirmed(space, *id),
                _ => {}
            }
        }
        if let Some(i) = rec {
            let at = At::of(self.machine(space).expect("a machine that spoke"));
            self.record
                .as_mut()
                .expect("recording")
                .push((Act::Step(space, i), out.clone(), Some(at)));
        }
        match (self.hub.serving, space) {
            (true, _) => self.kept(space),
            (false, Space::Durable) => self.keep_pending(),
            _ => {}
        }
        out
    }

    /// What is unwritten, written in order, up to the first write that
    /// fails: whether all of it is.
    fn write(&mut self) -> bool {
        while let Some(r) = self.unwritten.first() {
            if let Err(e) = self.disk.record(r) {
                self.failure = Some(format!("the log cannot be written: {e}"));
                return false;
            }
            self.unwritten.remove(0);
        }
        self.failure = None;
        true
    }

    fn blob(&mut self, name: &str, b: &[u8]) {
        if let Err(e) = self.disk.put(name, b) {
            self.failure = Some(e.to_string());
        }
    }

    /// A device's joined chain and pending intents kept on its disk as they
    /// change.
    fn keep_pending(&mut self) {
        if self.log.joining.is_some() != self.joined {
            self.joined = self.log.joining.is_some();
            let b = self.log.joining.clone().map_or(vec![], |(n, h, c)| ((n, h), c).to_bytes());
            self.blob("joining", &b);
        }
        let now = (self.log.local, self.log.pending.iter().map(|p| p.entry.id).collect());
        if now != self.saved {
            self.save_pending(now);
        }
    }

    /// The pending intents made durable as a change to what was saved: one
    /// record appended — the `local`, how many left the front, the intents
    /// new at the back — so a mutate writes its one entry; written whole
    /// when the journal holds more than twice what is pending.
    fn save_pending(&mut self, now: (i64, Vec<EntryId>)) {
        let (was, ids) = (&self.saved.1, &now.1);
        let gone = (0..=was.len()).find(|d| ids.starts_with(&was[*d..])).unwrap_or(was.len());
        let kept = was.len() - gone;
        let held = self.journal.map(|j| j + ids.len() - kept + 1);
        let entries = |from: usize| -> Vec<Entry> { self.log.pending[from..].iter().map(|p| p.entry.clone()).collect() };
        let (record, append) = match held {
            Some(j) if j <= 2 * (ids.len() + 1) + 64 => {
                self.journal = Some(j);
                (disk::pending_record(now.0, gone, entries(kept)), true)
            }
            _ => {
                self.journal = Some(ids.len() + 1);
                (disk::pending_record(now.0, 0, entries(0)), false)
            }
        };
        let wrote = if append {
            self.disk.append("pending", &record)
        } else {
            self.disk.put("pending", &record)
        };
        if let Err(e) = wrote {
            self.failure = Some(e.to_string());
            self.journal = None;
        }
        self.saved = now;
    }

    fn changed(&mut self, space: Space, cs: &[Change]) {
        let store = match space {
            Space::Durable => &self.log.store,
            Space::Volatile => &self.live.as_ref().expect("a machine that spoke").store,
        };
        for (i, (_, w)) in self.views.iter_mut().enumerate().filter(|(_, (s, _))| *s == space) {
            let Some(v) = w else { continue };
            match v.push(cs, store) {
                Ok(patches) => self.told.patched(i, patches),
                Err(_) => {
                    *w = None;
                    self.told.reset(i);
                }
            }
        }
    }

    /// An intent of this peer's own on `space`'s machine — a device's on the
    /// log, a live call (an ephemeral one through the relay), or a server's
    /// own entry (the server half of a mutator, a raw write, a device's
    /// arrival) — and the verdict it was given here and now: a preview's
    /// refusal, or the authority's.
    pub fn author(&mut self, space: Space, func: &str, input: Vec<u8>, now: i64) -> Result<(), String> {
        let ephemeral = self.config.live.as_ref().is_some_and(|l| l.ephemeral.contains(func));
        if space == Space::Volatile && ephemeral {
            self.relay.insert(func.into(), (input, now));
            self.tick(now);
            return Ok(());
        }
        if self.machine(space).is_none() {
            return Err(format!("no {space:?} machine here"));
        }
        let said = self.route(
            space,
            None,
            Input::Mutate {
                func: func.into(),
                input,
                now,
            },
        );
        let verdict = said.into_iter().find_map(|o| match o {
            Output::Refused { why, .. } => Some(Err(why)),
            Output::Confirmed(_) => Some(Ok(())),
            _ => None,
        });
        verdict.unwrap_or(if self.hub.serving { Err("no verdict".into()) } else { Ok(()) })
    }

    /// The clock: the relay goes when its interval has passed.
    pub fn tick(&mut self, now: i64) {
        let every = self.config.live.as_ref().map_or(0, |l| l.every_ms);
        if self.relay.is_empty() || self.relayed.is_some_and(|t| now - t < every) {
            return;
        }
        self.relayed = Some(now);
        for (func, (input, now)) in std::mem::take(&mut self.relay) {
            self.step(Space::Volatile, Input::Mutate { func, input, now });
        }
    }

    /// A connection to its server is up — a new sight of it: a `Hello` in
    /// each space, and its pending intents pushed again.
    pub fn connected(&mut self) {
        (self.link.epoch, self.link.wait) = (self.link.epoch + 1, self.link.backoff.0);
        self.linked = true;
        self.frames.clear();
        self.step(Space::Durable, Input::Connected);
        self.step(Space::Volatile, Input::Connected);
    }

    pub fn disconnected(&mut self) {
        self.linked = false;
        self.frames.clear();
    }

    /// A frame from its server, to the machine of its space.
    pub fn receive(&mut self, bytes: Vec<u8>) {
        let space = Frame::dec(&bytes).map_or(Space::Durable, |f| f.space());
        self.step(space, Input::Frame { from: None, bytes });
    }

    /// Alone, its fork kept on its disk so that a reopen is alone still.
    pub fn alone(&mut self) {
        self.step(Space::Durable, Input::Alone);
        if let Role::Authority {
            journal: Journal { fork: Some(f), .. },
        } = &self.log.role
        {
            let b = f.to_bytes();
            self.blob("fork", &b);
        }
    }

    pub fn join(&mut self) {
        self.step(Space::Durable, Input::Join);
        self.blob("fork", &[]);
    }

    /// Another login: every pending intent the signer's, its id kept — or,
    /// for another actor than the one this device authored as, authored
    /// again from a new origin's first `local` — and the live machine
    /// started over. A chain handed over as another actor is not handed
    /// over again: its facts are not the signer's, and its intents are
    /// pushed as the signer's instead.
    pub fn sign_in(&mut self, who: Who) {
        let was = &self.log.me;
        let other = !was.actor.is_empty() && was.actor != who.actor;
        if (&was.actor, &was.roles) != (&who.actor, &who.roles) {
            self.log.joining = None;
        }
        if other {
            self.log.origin = Origin::named([&self.log.origin.0[..], who.session.as_bytes()].concat());
            self.log.local = 0;
            let _ = self.disk.put("origin", &self.log.origin.to_bytes());
        }
        let origin = self.log.origin;
        let mut pending: Vec<Entry> = self.log.pending.iter().map(|p| p.entry.clone()).collect();
        for (n, e) in pending.iter_mut().enumerate() {
            if other {
                (e.origin, e.local) = (origin, n as i64 + 1);
                e.id = EntryId::of(origin, e.local);
            }
            (e.actor, e.session, e.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        }
        if let Some(r) = &mut self.record {
            r.push((Act::SignIn(who.clone(), origin), vec![], None));
        }
        self.log.me = who.clone();
        if let Some(mut p) = live_of(&self.config, &who, origin) {
            // The origin goes on counting in the live space too, unless new.
            p.local = self.live.as_ref().filter(|_| !other).map_or(0, |x| x.local);
            self.live = Some(p);
        }
        let durable = Durable {
            rows: self.log.confirmed_rows(),
            logged: vec![],
            local: if other { pending.len() as i64 } else { self.log.local },
            pending,
        };
        self.step(Space::Durable, Input::Open(durable));
        if self.linked {
            self.connected();
        }
    }

    /// A view's answer as it stands.
    pub fn values(&self, view: usize) -> Option<Vec<Vec<u8>>> {
        self.views.get(view)?.1.as_ref().map(|v| v.rows())
    }

    /// A maintained view of the domain's function `name` on `input` over
    /// `space`'s optimistic state, as `ctx` reads it (§1.16); what it does
    /// comes as its [`Update`]s.
    pub fn watch(&mut self, space: Space, name: &str, input: &[u8], ctx: Ctx) -> Result<usize, Refusal> {
        let m = match space {
            Space::Durable => Some(&self.config.log),
            Space::Volatile => self.config.live.as_ref().map(|l| &l.module),
        };
        let d = m.and_then(|m| m.domain.clone()).ok_or_else(|| Refusal::new("no domain here"))?;
        let store = &self.machine(space).ok_or_else(|| Refusal::new("no machine of that space"))?.store;
        let view = d.watch(store, &ctx, name, input)?;
        self.views.push((space, Some(view)));
        Ok(self.views.len() - 1)
    }

    /// A view held no more: no change is pushed into it again, and its
    /// number is not given to another.
    pub fn unwatch(&mut self, view: usize) {
        if let Some(w) = self.views.get_mut(view) {
            w.1 = None;
        }
        self.told.updates.remove(&view);
    }

    /// The frames for its server, taken: a driver carrying them itself.
    pub fn take_frames(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.frames)
    }
}
