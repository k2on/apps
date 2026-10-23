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
//!   feed, routed; the journal beside the log), natively on a thread of its
//!   own (`handle`) and, with the `server` feature, on a socket (`serve`:
//!   the sync socket, the web build's routes and
//!   sign-in — axum and tokio, which a phone or a browser never links).
//!
//! It is sans-io like the machines: a UI, a test or the hub's thread feeds
//! it what happens (an intent, a frame, a connection up or down, the clock)
//! and it does four things with what the machines say. A frame for its
//! server goes out while it is linked and is dropped while it is not; a
//! frame for a connection is delivered once what the machine made durable
//! is on the disk — **written before delivered**, and while a write fails
//! every delivery waits with it, in order ([`Peer::failure`]); every change
//! is pushed into the views watching its space; and every verdict on its
//! own intents is kept (`app`: what a UI asks by name). A UI holds a view's
//! rows as its own type, patched as the view moves (`watched`).
//!
//! **The relay** of ephemeral calls is the driver's policy, as §1.9 says:
//! a call to a function the module names ephemeral goes to the volatile
//! machine at most once every `every_ms`, coalesced to the newest call per
//! function; the rest go at once.
//!
//! **The origin** (§1.6) is sixteen bytes a device draws at its first open
//! and keeps on its disk (its `replica` row, `local`); every intent's id is its origin's and
//! its `local`'s, whoever is signed in. **Sign-in** gives the device
//! another login: its pending intents are the signer's from then on, their
//! ids and the rows they drew kept. A login of another actor than the one
//! the device authored as gets an origin of its own, named by the old one
//! and its session, and the pending intents are authored again from its
//! first `local` — an authority binds an origin to the actor whose intents
//! first came from it.
//!
//! **Who is signed in** is `ark-auth`'s rows (`signed`): a device reads
//! its session and signs out through the domain; a server's hub judges
//! every token by the row it names and closes what is revoked or expired,
//! as `ark_auth::server::Auth` says (`serve`).
//!
//! **A peer is three instances** (`instance`: a machine and what keeps
//! it) wired together: the log's, the live space's, and the local one
//! beside them on the same disk, whose rows (`local`) are what a peer keeps
//! beside its log — a server's journal and volatile space, a device's
//! replica and pending intents.
//!
//! **Handlers and reconcilers** (`jobs`) act on the world outside: a
//! handler claims, runs and finishes the jobs of a kind (`ark::effect`) on
//! the peer that registered it; a reconciler is told a view's rows.
//!
//! Every input either machine of a device is given is recorded, when asked
//! (`record`), as an [`Act`]: a device is a pure function of these, which
//! the trace player (`arkdb`) holds it to.

pub mod app;
pub mod disk;
mod hub;
pub mod instance;
mod jobs;
pub mod link;
pub mod observe;
pub mod local;
pub mod signed;
pub mod watched;

#[cfg(not(target_arch = "wasm32"))]
pub mod handle;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub mod serve;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub mod settings;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub mod web;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ark::frames::Frame;
use ark::hash::EntryHash;
use ark::log::{Durable, Entry, EntryId, Origin, Seq, Span};
use ark::peer::{Ctx, Feed, Functions, Input, Journal, Output, Peer as Machine, Role, Told, Who};
use ark::plan::Maintained;
use ark::schema::{Schema, Space};
use ark::store::Change;
use ark::{Domain, Refusal};

pub use app::{nobody, Standing, Status, Update};
pub use disk::{Disk, Files, Memory};
#[cfg(not(target_arch = "wasm32"))]
pub use handle::{dev, Authenticate, Handle};
pub use hub::{ConnId, Deliver};
pub use instance::{Fold, Instance};
pub use link::{Event, Pumped, Wake};
pub use local::{Local, Logins, Shared};
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub use serve::{Running, Server};
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub use settings::{Keepalive, Retention, Serve, SignIn};
pub use watched::Watched;

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
    /// Where a device's live machine draws its origin from, once per open
    /// ([`draw_origin`]; a simulator's is seeded).
    pub origin: Draw,
}

/// Where an origin is drawn from.
pub type Draw = Arc<dyn Fn() -> Origin + Send + Sync>;

/// How long a trace an answer joins lasts, from its beginning: what one
/// hand caused is over by then ([`Peer::cause`]).
pub const TRACE_MS: i64 = 30_000;

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
            origin: Arc::new(draw_origin),
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

    /// The domain `space`'s views are made by.
    fn domain_of(&self, space: Space) -> Option<Arc<Domain>> {
        match space {
            Space::Durable => self.log.domain.clone(),
            Space::Volatile => self.live.as_ref()?.module.domain.clone(),
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

/// One input to one machine; or both machines opened afresh as a login,
/// as at [`Peer::open`] — the log's alone from its fork when it was
/// alone; or a sign-in: the log's machine authoring as the login from its first
/// `local`, the live one afresh. Each names the log's origin and the live
/// machine's, which is drawn at every open.
#[derive(Clone, Debug)]
pub enum Act {
    Open(Who, Origin, Option<(Seq, EntryHash)>, Option<Origin>),
    SignIn(Who, Origin, Option<Origin>),
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

/// Three instances wired: the log's machine on the disk, the live one, and
/// the local instance beside them on the same disk; and what wires them.
pub struct Peer {
    pub config: Config,
    pub log: Instance,
    pub live: Option<Instance>,
    /// Every input given, when recording.
    pub record: Option<Vec<Recorded>>,
    /// Whether a connection to its server is up.
    pub linked: bool,
    /// The link: the server, the login's token, the connection and when it
    /// is dialled again.
    pub link: link::Link,
    /// The connections it serves.
    hub: hub::Hub,
    /// Frames for its server.
    frames: Vec<Vec<u8>>,
    views: Vec<Option<Kept>>,
    relay: BTreeMap<String, (Vec<u8>, i64, Option<Span>)>,
    relayed: Option<i64>,
    /// The span it authors under: the one opened ([`Peer::begin`],
    /// [`Peer::under`]), or else, in each space, the span of the entry whose
    /// change it was last told, so that what it does in answer (a
    /// reconciler, a view's reader) is a child of what it answered. A server
    /// authors under the opened one only.
    span: Option<Span>,
    woke: [Option<Span>; 2],
    /// What became of its own intents, and what its views did.
    told: app::Told,
    /// What it keeps beside its log, on the same disk.
    pub local: Instance,
    /// The `local` and the pending intents (each its `local` and id) its
    /// local instance holds.
    saved: (i64, Vec<(i64, EntryId)>),
    /// The handlers registered here, and what changed while they ran.
    runners: Vec<Box<dyn jobs::Runner>>,
    missed: Option<Vec<Told>>,
    /// The reconcilers, by their view.
    reconcilers: BTreeMap<usize, jobs::Reconciler>,
}

/// A view a peer keeps (§1.10): what it is made of — its space, the
/// domain's function, its input and who reads — and the view, while it
/// can be made.
struct Kept {
    space: Space,
    name: String,
    input: Vec<u8>,
    ctx: Ctx,
    view: Option<Box<dyn Maintained>>,
}

/// The role of a device alone since `fork`.
pub fn alone_at(fork: Option<(Seq, EntryHash)>) -> Role {
    Role::Authority {
        journal: Journal { fork, ..Journal::default() },
    }
}

fn machine(m: &Module, role: Role, me: &Who, origin: Option<Origin>, space: Space) -> Instance {
    let mut p = Machine::new(m.schema.clone(), m.functions.clone(), role, me.clone()).in_space(space);
    if let Some(o) = origin {
        p.origin = o;
    }
    Instance::new(p, None)
}

/// A device's live machine as `me`, from an origin drawn for it. The live
/// space keeps nothing across an open, so its `local`s start again at 1;
/// under the log's origin, which the disk keeps, a reloaded page's calls
/// were refused as overtaken by what the page before it had said, for as
/// long as the server ran.
fn live_of(config: &Config, me: &Who) -> Option<Instance> {
    let l = config.live.as_ref()?;
    let mut p = machine(&l.module, Role::Replica, me, Some((l.origin)()), Space::Volatile);
    (p.device, p.fold) = (l.device.clone(), Some(Fold::REPLICA));
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
    fn with(config: Config, log: Instance, live: Option<Instance>, local: Instance) -> Peer {
        Peer {
            config,
            log,
            live,
            record: None,
            linked: false,
            link: link::Link::default(),
            hub: hub::Hub::default(),
            frames: vec![],
            views: vec![],
            relay: BTreeMap::new(),
            relayed: None,
            span: None,
            woke: [None, None],
            told: app::Told::default(),
            local,
            saved: (0, vec![]),
            runners: vec![],
            missed: None,
            reconcilers: BTreeMap::new(),
        }
    }

    /// A device opened as `me` from what `disk` holds.
    pub fn open(config: Config, me: Who, disk: Box<dyn Disk>, record: bool) -> Peer {
        let (disk, local) = local::beside(disk);
        Peer::opened(config, me, disk, local, record.then(Vec::new), link::Link::default())
    }

    pub(crate) fn opened(
        config: Config,
        me: Who,
        disk: Box<dyn Disk>,
        mut local: Instance,
        mut record: Option<Vec<Recorded>>,
        link: link::Link,
    ) -> Peer {
        let (r, intents) = local.read::<Local, _>(|db| (local::replica(db).unwrap_or_default(), db.pending.scan()));
        let origin = match r.origin {
            o if o != Origin::default() => o,
            _ => {
                let origin = draw_origin();
                let _ = local.write::<Local, ark::db::Durable, _>(|db| db.replica.put(local::Replica { origin, ..r.clone() }));
                origin
            }
        };
        let pending: Vec<Entry> = intents.into_iter().map(|i| i.entry).collect();
        let live = live_of(&config, &me);
        if let Some(rec) = &mut record {
            rec.push((Act::Open(me.clone(), origin, r.fork, live.as_ref().map(|l| l.origin)), vec![], None));
        }
        let role = if r.fork.is_some() { alone_at(r.fork) } else { Role::Replica };
        let mut log = machine(&config.log, role, &me, Some(origin), Space::Durable);
        log.disk = Some(disk);
        let mut p = Peer::with(config, log, live, local);
        (p.record, p.link) = (record, link);
        p.saved = (r.local, pending.iter().map(|e| (e.local, e.id)).collect());
        let durable = Durable {
            pending,
            local: r.local,
            ..p.log.stored()
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
        let local = Instance::open::<Local>(self.local.into_disk());
        Peer::opened(self.config, me, self.log.into_disk(), local, self.record, link)
    }

    pub fn machine(&self, space: Space) -> Option<&Machine> {
        match space {
            Space::Durable => Some(&self.log),
            Space::Volatile => self.live.as_deref(),
        }
    }

    /// What keeps a disk from being written, while it lasts: the log's, or
    /// its local instance's.
    pub fn failure(&self) -> Option<&str> {
        self.log.failure.as_deref().or(self.local.failure.as_deref())
    }

    fn instance(&mut self, space: Space) -> Option<&mut Instance> {
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
        let seen = tracing::event_enabled!(target: "ark", tracing::Level::WARN).then(|| input.clone());
        let Some(m) = self.instance(space) else { return vec![] };
        let out = m.step(input);
        if let Some(i) = &seen {
            observe::step(&self.label(), space, from, i, &out);
        }
        for o in &out {
            match o {
                Output::Send(b) => match from {
                    Some(c) => self.hub.held.push((c, b.clone())),
                    None if self.linked => self.frames.push(b.clone()),
                    None => {}
                },
                Output::Fanout(b) => self.hub.fanout(space, b),
                Output::To { session, bytes } => self.hub.to(session, bytes),
                Output::Changes(told) => {
                    if let Some(by) = told.iter().rev().find_map(|t| t.by) {
                        self.woke[space as usize] = Some(by);
                    }
                    let cs: &Vec<Change> = &told.iter().map(|t| t.change.clone()).collect();
                    self.changed(space, cs);
                    if space == Space::Durable {
                        self.runners.iter_mut().for_each(|r| r.changed(told));
                        self.missed.iter_mut().for_each(|m| m.extend(told.iter().cloned()));
                        self.signs(cs);
                        if self.hub.watched {
                            self.hub.seen.extend(cs.iter().cloned());
                        }
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
            (true, Space::Durable) => self.kept(),
            (false, Space::Durable) => self.keep_pending(),
            _ => {}
        }
        out
    }

    /// Its replica row, changed by `f`, kept.
    pub(crate) fn replica(&mut self, f: impl FnOnce(&mut local::Replica)) {
        self.keep(|db| {
            let mut r = local::replica(db).unwrap_or_default();
            f(&mut r);
            db.replica.put(r)
        });
    }

    /// `f` written to its local instance; a refusal kept as its failure.
    pub(crate) fn keep(&mut self, f: impl for<'t> FnOnce(&mut ark::db::Db<'t, Local>) -> Result<(), Refusal>) {
        if let Err(e) = self.local.write::<Local, ark::db::Durable, _>(f) {
            self.local.failure = Some(e.0);
        }
    }

    /// A device's pending intents and its last `local` kept as they change, as the change to what is kept: the intents
    /// confirmed or refused off the front deleted, those new at the back
    /// put — so a mutate writes its one row — or, when the intents are
    /// others (authored again from a new origin), all of them.
    fn keep_pending(&mut self) {
        let ids: Vec<(i64, EntryId)> = self.log.pending.iter().map(|p| (p.entry.local, p.entry.id)).collect();
        if (self.log.local, &ids) == (self.saved.0, &self.saved.1) {
            return;
        }
        let was = std::mem::take(&mut self.saved.1);
        let gone = (0..=was.len()).find(|d| ids.starts_with(&was[*d..]));
        let (gone, kept) = gone.map_or((was.len(), 0), |d| (d, was.len() - d));
        let local = self.log.local;
        let fresh: Vec<Entry> = self.log.pending[kept..].iter().map(|p| p.entry.clone()).collect();
        self.keep(|db| {
            for (l, _) in &was[..gone] {
                db.pending.delete(*l)?;
            }
            for entry in fresh {
                db.pending.put(local::Intent { local: entry.local, entry })?;
            }
            let mut r = local::replica(db).unwrap_or_default();
            r.local = local;
            db.replica.put(r)
        });
        self.saved = (local, ids);
    }

    /// Every view of `space` told a settle's changes. One that cannot be
    /// kept up, or could not be made, is made again over the store as it
    /// is, under its number: its rows are read afresh ([`Update::Reset`]).
    fn changed(&mut self, space: Space, cs: &[Change]) {
        let store = match space {
            Space::Durable => &self.log.store,
            Space::Volatile => &self.live.as_ref().expect("a machine that spoke").store,
        };
        let domain = self.config.domain_of(space);
        for (i, w) in self.views.iter_mut().enumerate() {
            let Some(w) = w.as_mut().filter(|w| w.space == space) else { continue };
            let pushed = w.view.as_mut().map(|v| v.push(cs, store));
            match pushed {
                Some(Ok(patches)) => {
                    if let (false, Some(f), Some(v)) = (patches.is_empty(), self.reconcilers.get_mut(&i), &w.view) {
                        f(&v.rows());
                    }
                    self.told.patched(i, patches)
                }
                was => {
                    w.view = domain.as_ref().and_then(|d| d.watch(store, &w.ctx, &w.name, &w.input).ok());
                    if was.is_some() || w.view.is_some() {
                        if let (Some(f), Some(v)) = (self.reconcilers.get_mut(&i), &w.view) {
                            f(&v.rows());
                        }
                        self.told.reset(i);
                    }
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
            let cause = self.cause(space, now);
            self.relay.insert(func.into(), (input, now, cause));
            self.tick(now);
            return Ok(());
        }
        let (func, cause) = (func.into(), self.cause(space, now));
        self.verdict(space, Input::Mutate { func, input, now, cause })
    }

    /// The span an intent of `space` authored at `now` is under: the one
    /// opened, or — not serving — the span of the entry whose change woke
    /// it, while that trace is younger than [`TRACE_MS`]. A trace is what one
    /// hand caused: an answer to an answer to a press, minutes on (a
    /// player's position, reported each second), is a trace of its own.
    pub fn cause(&self, space: Space, now: i64) -> Option<Span> {
        let woke = self.woke[space as usize].filter(|w| !self.hub.serving && now - w.began < TRACE_MS);
        self.span.or(woke)
    }

    /// A trace of its own begun, named `name` (a tap, a scan), and authored
    /// under until [`Peer::end`]: its root span.
    pub fn begin(&mut self, name: &str) -> Span {
        let span = Span {
            trace: ark::log::TraceId(draw_origin().0),
            id: draw_origin().0,
            began: now_ms(),
        };
        tracing::info!(target: "ark", peer = %self.label(), trace = %ark::hash::hex(&span.trace.0), span = %ark::hash::hex(&span.id), name, "begin");
        self.span = Some(span);
        span
    }

    /// An intent of its own just authored on the log: pending, or — an
    /// authority's, decided as it was made — at the head.
    pub fn authored(&self, id: &EntryId) -> Option<Entry> {
        let pending = self.log.pending.iter().map(|p| &p.entry).find(|e| e.id == *id).cloned();
        let head = self.log.log().head().0;
        pending.or_else(|| (head.saturating_sub(8)..=head).rev().filter_map(|s| self.log.log().at(s)).map(|l| l.entry).find(|e| e.id == *id))
    }

    /// The span opened ([`Peer::begin`], [`Peer::under`]), if one is.
    pub fn open_span(&self) -> Option<Span> {
        self.span
    }

    /// The span opened by [`Peer::begin`] closed: it authors under what woke
    /// it again.
    pub fn end(&mut self) {
        self.span = None;
    }

    /// `f` with this peer authoring under `span` (none: under what woke it),
    /// the span it had put back after.
    pub fn under<T>(&mut self, span: Option<Span>, f: impl FnOnce(&mut Peer) -> T) -> T {
        let was = std::mem::replace(&mut self.span, span);
        let out = f(self);
        self.span = was;
        out
    }

    /// Who it is in what it says of itself: its session, and its origin's
    /// first bytes.
    pub fn label(&self) -> String {
        format!("{}/{}", self.log.me.session, ark::hash::hex(&self.log.origin.0[..4]))
    }

    /// Facts the authority writes itself, raw: an entry naming no function
    /// (`Input::Write`), judged by the store as any write is.
    pub fn write(&mut self, facts: Vec<Change>, now: i64) -> Result<(), String> {
        self.verdict(Space::Durable, Input::Write { facts, now })
    }

    /// One input of this peer's own, routed, and the verdict it was given
    /// here and now.
    fn verdict(&mut self, space: Space, input: Input) -> Result<(), String> {
        if self.machine(space).is_none() {
            return Err(format!("no {space:?} machine here"));
        }
        let said = self.route(space, None, input);
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
        for (func, (input, now, cause)) in std::mem::take(&mut self.relay) {
            self.step(Space::Volatile, Input::Mutate { func, input, now, cause });
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
            let f = *f;
            self.replica(|r| r.fork = Some(f));
        }
    }

    /// Joined again: the chain made alone is pending intents (§1.7), kept
    /// so before the log forgets it at the fork.
    pub fn join(&mut self) {
        if let Role::Authority {
            journal: Journal { fork: Some((at, _)), .. },
        } = &self.log.role
        {
            let chain: Vec<Entry> = self.log.log().after(*at).map(|l| l.entry).collect();
            self.keep(|db| {
                chain
                    .into_iter()
                    .try_for_each(|entry| db.pending.put(local::Intent { local: entry.local, entry }))
            });
        }
        self.step(Space::Durable, Input::Join);
        self.replica(|r| r.fork = None);
    }

    /// Another login: every pending intent the signer's, its id kept — or,
    /// for another actor than the one this device authored as, authored
    /// again from a new origin's first `local` — and the live machine
    /// started over.
    pub fn sign_in(&mut self, who: Who) {
        let was = &self.log.me;
        let other = !was.actor.is_empty() && was.actor != who.actor;
        if other {
            self.log.origin = Origin::named([&self.log.origin.0[..], who.session.as_bytes()].concat());
            self.log.local = 0;
            let o = self.log.origin;
            self.replica(|r| r.origin = o);
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
        self.log.me = who.clone();
        if let Some(mut p) = live_of(&self.config, &who) {
            // The live origin goes on counting, unless the actor is new.
            if let Some(x) = self.live.as_ref().filter(|_| !other) {
                (p.origin, p.local) = (x.origin, x.local);
            }
            self.live = Some(p);
        }
        if let Some(r) = &mut self.record {
            r.push((Act::SignIn(who.clone(), origin, self.live.as_ref().map(|l| l.origin)), vec![], None));
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
        self.views.get(view)?.as_ref()?.view.as_ref().map(|v| v.rows())
    }

    /// A maintained view of the domain's function `name` on `input` over
    /// `space`'s optimistic state, as `ctx` reads it (§1.16); what it does
    /// comes as its [`Update`]s, under its number for as long as it is
    /// held.
    pub fn watch(&mut self, space: Space, name: &str, input: &[u8], ctx: Ctx) -> Result<usize, Refusal> {
        let d = self.config.domain_of(space).ok_or_else(|| Refusal::new("no domain here"))?;
        let store = &self.machine(space).ok_or_else(|| Refusal::new("no machine of that space"))?.store;
        let view = Some(d.watch(store, &ctx, name, input)?);
        let (name, input) = (name.into(), input.to_vec());
        self.views.push(Some(Kept {
            space,
            name,
            input,
            ctx,
            view,
        }));
        Ok(self.views.len() - 1)
    }

    /// A view held no more: no change is pushed into it again, and its
    /// number is not given to another.
    pub fn unwatch(&mut self, view: usize) {
        if let Some(w) = self.views.get_mut(view) {
            *w = None;
        }
        self.told.updates.remove(&view);
    }

    /// The frames for its server, taken: a driver carrying them itself.
    /// What this peer said of itself since, out of the outbox and to its
    /// server ([`Frame::Telemetry`](ark::frames::Frame)), a page at a time.
    pub fn ship(&mut self) {
        let events = observe::take(&self.label(), 256);
        if !events.is_empty() {
            self.frames.push(ark::frames::Frame::Telemetry { events }.enc());
        }
    }

    pub fn take_frames(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.frames)
    }
}
