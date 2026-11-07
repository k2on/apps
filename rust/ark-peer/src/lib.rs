//! The ArkDB v5 peer: a driver of the one machine (§1.7), twice — the
//! log's and, when the domain has live state, the volatile space's (§1.9)
//! — over one disk. A device and a server are two drivers over one
//! [`Core`]:
//!
//! - **a device** ([`Peer`], [`Peer::open`]) is a replica; it **dials** a
//!   server ([`Peer::connect`]: the link, `link`) when it has one, keeps
//!   its pending intents on its disk — with no server, or none reached,
//!   they are simply pending — relays its ephemeral calls, keeps the
//!   maintained views a UI draws (§1.10) and runs the handlers registered
//!   on it (`jobs`);
//! - **a server** ([`Authority`], [`Authority::serve`]) is the authority of
//!   each space; it **listens** (`authority`: the connections, each with
//!   its identity and its feed, routed; the journal beside the log), on a
//!   thread of its own (`handle`, a device's too) and, with the `server`
//!   feature, on a socket (`serve`: the sync socket, `/healthz`, the web
//!   build's routes and sign-in — axum and tokio, which a phone or a
//!   browser never links).
//!
//! Each is sans-io like the machines: a UI, a test or a thread feeds it
//! what happens (an intent, a frame, a connection up or down, the clock)
//! and it does four things with what the machines say. A device's frame
//! for its server goes out while it is linked and is dropped while it is
//! not; a server's frame for a connection is delivered once what the
//! machine made durable is on the disk — **written before delivered**, and
//! while a write fails every delivery waits with it, in order
//! ([`Core::failure`]); every change is pushed into a device's views
//! watching its space; and every verdict on a device's own intents is kept
//! (`app`: what a UI asks by name). A UI holds a view's rows as its own
//! type, patched as the view moves (`watched`).
//!
//! **The relay** of ephemeral calls is the driver's policy, as §1.9 says:
//! a call to a function the config names ephemeral goes to the volatile
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
//! its session and signs out through the domain; a server judges every
//! token by the row it names and closes what is revoked or expired, as
//! `ark_auth::server::Auth` says (`serve`).
//!
//! **A core is two instances** (`instance`: a machine and what keeps it),
//! the log's and the live space's, records beside them on the same disk
//! (`local`: a server's journal and volatile space, a device's replica and
//! pending intents) and the files its rows name (`files`).
//!
//! Every input either machine of a device is given is recorded, when asked
//! (`record`), as an [`Act`]: a device is a pure function of these, which
//! the trace player (`arkdb`) holds it to.

pub mod app;
mod authority;
pub mod disk;
pub mod files;
pub mod handle;
pub mod instance;
mod jobs;
pub mod link;
pub mod local;
pub mod observe;
pub mod signed;
pub mod watched;

#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub mod serve;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub mod settings;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub mod web;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
mod worker;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::{EntryId, Logged, Origin, Record, Span};
use ark::peer::{Ctx, Input, Output, Peer as Machine, Role, To, Told};
use ark::plan::Maintained;
use ark::schema::Space;
use ark::store::Change;
use ark::Domain;
use local::Replica;

pub use app::{nobody, Arg, Outcome};
pub use ark::file::FileRef;
pub use ark::peer::{Fold, Scoping};
pub use authority::{Authority, Call, Called, ConnId, Deliver, ServerConfig, Sink};
pub use disk::{Disk, Files, Memory};
#[cfg(not(target_arch = "wasm32"))]
pub use handle::Authenticate;
pub use handle::{dev, Handle};
pub use instance::Instance;
pub use jobs::Work;
pub use link::{Event, Pumped, Wake};
pub use local::Logins;
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub use serve::{Running, Server};
#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
pub use settings::{Serve, SignIn};
pub use watched::Watched;

/// How long a trace an answer joins lasts, from its beginning: what one
/// hand caused is over by then ([`Peer::cause`]).
pub const TRACE_MS: i64 = 30_000;

/// What a device is made of: the log's domain and the live space's, each
/// its schema and its functions by name (§1.16, §1.9); what it says it is
/// at each connection (a call before its `Hello`s, `ark::frames::DEVICE`);
/// and the live functions it relays as ephemeral, and how often. A
/// server's is a [`ServerConfig`].
#[derive(Clone)]
pub struct Config {
    pub log: Arc<Domain>,
    pub live: Option<Arc<Domain>>,
    pub device: Option<Vec<u8>>,
    pub ephemeral: BTreeSet<String>,
    pub every_ms: i64,
}

impl Config {
    /// A device of `log`, with no live space.
    pub fn of(log: Arc<Domain>) -> Config {
        Config {
            log,
            live: None,
            device: None,
            ephemeral: BTreeSet::new(),
            every_ms: 0,
        }
    }
}

/// Why an authority's disk is not opened under its schema.
pub const ANOTHER: &str =
    "the rows on disk are of a schema this one does not grow from: a schema only grows (append a table, or a column that is an Option or defaulted)";

/// A patch of a view's rows, each its bytes.
pub type Patch = ark::plan::Patch<Vec<u8>>;

/// What a domain is, in a word: its schema's hash and its version — what a
/// server's `/healthz` says it runs (`domain <hash> <version> current`),
/// and what a phone's build holds its own to.
pub fn version(d: &Domain) -> String {
    format!("{} {}", d.schema_hash(), d.version)
}

/// What a device and a server are both made of: two instances, the log's
/// machine on the disk and the live one beside it; the records each keeps
/// beside its log on the same disk; and the store of the files its rows
/// name. A [`Peer`] and an [`Authority`] are each one, and what its role
/// adds.
pub struct Core {
    pub log: Instance,
    pub live: Option<Instance>,
    /// What it keeps beside its log, on the same disk, and its last
    /// refusal, while it lasts.
    pub local: Box<dyn Disk>,
    local_failure: Option<String>,
    pub(crate) files: files::Store,
}

/// A disk apart: the log's blobs, its records beside them (`local.*`), and
/// the store of its files ([`local::beside`]).
pub(crate) type Apart = (Box<dyn Disk>, Box<dyn Disk>, files::Store);

/// A view a device keeps (§1.10): what it is made of — its space, the
/// domain's function, its input and who reads — and the view, while it
/// can be made.
struct Kept {
    space: Space,
    name: String,
    input: Vec<u8>,
    ctx: Ctx,
    view: Option<Box<dyn Maintained>>,
}

fn machine(d: &Arc<Domain>, role: Role, me: &Ctx, origin: Option<Origin>, space: Space) -> Instance {
    let mut p = Machine::new(d.clone(), role, me.clone()).in_space(space);
    if let Some(o) = origin {
        p.origin = o;
    }
    Instance::new(p, None)
}

/// The live machine's origin at the `opens`th open of a peer whose log's
/// is `origin`. The live space keeps nothing across an open, so its
/// `local`s start again at 1; under one origin for every open, a reloaded
/// page's calls were taken for ones the server had judged already, for as
/// long as it ran.
fn live_origin(origin: Origin, opens: u64) -> Origin {
    Origin::named([&origin.0[..], b"live", &opens.to_be_bytes()].concat())
}

/// Sixteen bytes no other replica draws: the process's hasher keys (which
/// the platform seeds), the clock and the process, through sha256.
pub(crate) fn draw_origin() -> Origin {
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
pub fn who(l: &ark_auth::Login) -> Ctx {
    Ctx::who(l.user.id.clone(), l.session.clone(), l.user.roles.clone())
}

impl Core {
    /// Either role's disk opened: its replica read and kept with its opens
    /// counted, as `f` changes it; the log's machine on its disk in `role`
    /// as `me`, its origin the replica's once it has one, and the live
    /// space's, whose origin is another at every open; its files' store,
    /// its parts left [`files::STALE`] dropped. Nothing is read from the
    /// disk yet; the replica as it was is said.
    fn open(log: &Arc<Domain>, live: Option<&Arc<Domain>>, role: Role, me: &Ctx, disk: Apart, f: impl FnOnce(&mut Replica)) -> (Core, Replica) {
        let (disk, mut local, files) = disk;
        let was = Replica::read(&mut *local);
        let mut r = was.clone();
        r.opens += 1;
        f(&mut r);
        let _ = local.put(local::REPLICA, &r.to_bytes());
        let mut log = machine(log, role.clone(), me, (r.origin != Origin::default()).then_some(r.origin), Space::Durable);
        (log.local, log.disk) = (r.local, Some(disk));
        let live = live.map(|l| machine(l, role, me, Some(live_origin(log.origin, r.opens)), Space::Volatile));
        files.sweep();
        let core = Core {
            log,
            live,
            local,
            local_failure: None,
            files,
        };
        (core, was)
    }

    pub fn machine(&self, space: Space) -> Option<&Machine> {
        match space {
            Space::Durable => Some(&self.log),
            Space::Volatile => self.live.as_deref(),
        }
    }

    fn instance(&mut self, space: Space) -> Option<&mut Instance> {
        match space {
            Space::Durable => Some(&mut self.log),
            Space::Volatile => self.live.as_mut(),
        }
    }

    /// One input to `space`'s machine, from a connection or from itself,
    /// in the span its events are said under: what the machine said.
    fn step(&mut self, space: Space, conn: Option<ConnId>, input: Input) -> Vec<Output> {
        let label = self.label();
        let Some(m) = self.instance(space) else { return vec![] };
        let space = ["durable", "volatile"][space as usize];
        tracing::debug_span!(target: "ark", "step", peer = %label, space, conn).in_scope(|| m.step(input))
    }

    /// What keeps a disk from being written, while it lasts: the log's, or
    /// its records' beside it.
    pub fn failure(&self) -> Option<&str> {
        self.log.failure.as_deref().or(self.local_failure.as_deref())
    }

    /// A record kept beside its log, on its disk; a failure kept as its
    /// failure.
    pub(crate) fn put_beside(&mut self, name: &str, bytes: &[u8]) {
        if let Err(e) = self.local.put(name, bytes) {
            self.local_failure = Some(e.to_string());
        }
    }

    /// Who it is in what it says of itself: its session, and its origin's
    /// first bytes.
    pub fn label(&self) -> String {
        format!("{}/{}", self.log.me.session, ark::hash::hex(&self.log.origin.0[..4]))
    }

    /// An intent of its own of the function `name` on `space`'s machine:
    /// the function's whole name and the id the intent will have.
    fn intent(&self, name: &str) -> Result<(Space, String, EntryId), String> {
        let f = self.function(name)?;
        let id = self.machine(f.space).ok_or_else(|| format!("no {:?} machine here", f.space))?.next_id();
        Ok((f.space, f.name.clone(), id))
    }
}

/// A device: a core, and what a replica dialling its server adds — its
/// link and the frames for it, its views, its relay, what became of its
/// own intents, its handlers and the files it asked for.
pub struct Peer {
    pub(crate) core: Core,
    pub config: Config,
    /// Whether a connection to its server is up.
    pub linked: bool,
    /// The link: the server, the login's token, the connection and when it
    /// is dialled again.
    pub link: link::Link,
    /// Frames for its server.
    frames: Vec<Vec<u8>>,
    views: Vec<Option<Kept>>,
    relay: BTreeMap<String, (Vec<u8>, i64, Option<Span>)>,
    relayed: Option<i64>,
    /// The span it authors under: the one opened ([`Peer::begin`],
    /// [`Peer::under`]), or else, in each space, the span of the entry
    /// whose change it was last told, so that what it does in answer (a
    /// job, a view's reader) is a child of what it answered.
    span: Option<Span>,
    woke: [Option<Span>; 2],
    /// What became of its own intents, and what its views did.
    told: app::Told,
    /// The handlers registered here, and what changed while they ran.
    runners: Vec<Box<dyn jobs::Runner>>,
    missed: Option<Vec<Told>>,
    /// Its handle, once it has a thread of its own: its handlers run on
    /// threads of their own then.
    pub(crate) driven: Option<handle::Weak<Peer>>,
    /// The files rows name it asked its server for, and where each stands.
    pub(crate) transfers: files::Device,
}

impl std::ops::Deref for Peer {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

impl std::ops::DerefMut for Peer {
    fn deref_mut(&mut self) -> &mut Core {
        &mut self.core
    }
}

impl Peer {
    /// A device opened as `me` from what `disk` holds.
    pub fn open(config: Config, me: Ctx, disk: Box<dyn Disk>) -> Peer {
        Peer::opened(config, me, local::beside(disk), link::Link::default())
    }

    fn opened(config: Config, me: Ctx, disk: Apart, link: link::Link) -> Peer {
        let drawn = |r: &mut Replica| r.origin = Some(r.origin).filter(|o| *o != Origin::default()).unwrap_or_else(draw_origin);
        let (mut core, was) = Core::open(&config.log, config.live.as_ref(), Role::Replica, &me, disk, drawn);
        if let Some(l) = &mut core.live {
            l.fold = Some(Fold::REPLICA);
        }
        // Pending intents are read only beside the replica they were drawn
        // from: a disk whose replica does not read (none yet, or an older
        // build's) starts afresh.
        let blob = core.local.get(local::PENDING).ok().flatten().filter(|_| was.origin != Origin::default());
        let pending = disk::framed::<local::Kept>(&blob.unwrap_or_default()).0;
        // Rows of a schema this one does not grow from (a build older than
        // a schema that only grows) are sent again from empty (`disk::own`).
        let (schema, empty) = (&config.log.schema, Record::Rows(core.log.confirmed_rows()));
        let disk = core.log.disk.as_deref_mut().expect("a device's disk");
        let read = match disk::own(disk, schema) {
            Ok(false) => disk.record(&empty).and_then(|_| disk::put_schema(disk, schema)),
            Ok(true) => disk::put_schema(disk, schema),
            Err(e) => Err(e),
        };
        if let Err(e) = read {
            core.log.failure = Some(e.to_string());
        }
        let mut p = Peer {
            core,
            config,
            linked: false,
            link,
            frames: vec![],
            views: vec![],
            relay: BTreeMap::new(),
            relayed: None,
            span: None,
            woke: [None, None],
            told: app::Told::default(),
            runners: vec![],
            missed: None,
            driven: None,
            transfers: files::Device::default(),
        };
        let mut stored = p.log.stored();
        let source = ark::peer::Source::of(&mut stored);
        stored.extend(pending.into_iter().map(Record::Pending));
        p.step(Space::Durable, Input::Open(stored));
        if let Some(source) = source {
            p.log.read_from(source);
        }
        p
    }

    /// This device stopped and opened again from its disk, unlinked; its
    /// link's server, login and transport kept.
    pub fn reopen(self) -> Peer {
        let me = self.log.me.clone();
        let mut link = self.link;
        link.wire = None;
        let Core { log, local, files, .. } = self.core;
        Peer::opened(self.config, me, (log.into_disk(), local, files), link)
    }

    /// One input to one machine of the device, and what it says routed: a
    /// frame back to its server; its pending intents kept beside its log
    /// (its instance writes its log's); changes to the views, its handlers
    /// and its roles; verdicts kept. Filling, the machine is told first the
    /// tables its views read, which its fill takes first. The verdict it
    /// gave an intent of its own, if it gave one.
    pub fn step(&mut self, space: Space, input: Input) -> Option<Result<(), String>> {
        let filling = self.machine(space).is_some_and(|m| m.filling()) && matches!(input, Input::Frame { .. } | Input::Read);
        let wants = filling.then(|| self.reads(space));
        let m = self.instance(space)?;
        m.wants = wants.unwrap_or(std::mem::take(&mut m.wants));
        let mut verdict = None;
        for o in self.core.step(space, None, input) {
            match o {
                Output::Send(To::Back, f) if self.linked => self.frames.push(f.enc()),
                Output::Send(..) => {}
                Output::Persist(Record::Pending(k)) => self.keep_pending(&k),
                Output::Persist(_) => {}
                Output::Changes(told) => {
                    if let Some(by) = told.iter().rev().find_map(|t| t.by) {
                        self.woke[space as usize] = Some(by);
                    }
                    let cs: &Vec<Change> = &told.iter().map(|t| t.change.clone()).collect();
                    self.changed(space, cs);
                    if space == Space::Durable {
                        self.runners.iter_mut().for_each(|r| r.changed(&told));
                        self.missed.iter_mut().for_each(|m| m.extend(told.iter().cloned()));
                        self.signs(cs);
                    }
                }
                Output::Settled(id, r) => {
                    match &r {
                        Err(why) => self.told.refused(space, id, why),
                        Ok(()) => self.told.confirmed(space, id),
                    }
                    verdict.get_or_insert(r);
                }
            }
        }
        verdict
    }

    /// The tables its views of `space` read: what its fill takes first.
    fn reads(&self, space: Space) -> Vec<String> {
        let views = self.views.iter().flatten().filter(|w| w.space == space);
        views.filter_map(|w| w.view.as_ref()).flat_map(|v| v.reads()).collect()
    }

    /// One of its disk's answers taken, while its disk is the upstream it
    /// fills from first: what a driver carrying frames by hand takes
    /// between them. Whether there was one.
    pub fn read_one(&mut self) -> bool {
        let reading = self.log.reading();
        if reading {
            self.step(Space::Durable, Input::Read);
        }
        reading
    }

    /// Its disk read to the end now: what a driver reading its store at
    /// once, before it pumps, asks first.
    pub fn read_whole(&mut self) {
        while self.read_one() {}
    }

    /// Its replica, changed by `f`, kept.
    pub(crate) fn replica(&mut self, f: impl FnOnce(&mut local::Replica)) {
        let mut r = local::Replica::read(&mut *self.local);
        f(&mut r);
        self.put_beside(local::REPLICA, &r.to_bytes());
    }

    /// A device's pending intents as its machine said they moved, kept
    /// ([`local::Kept`]): one record appended, or, whole, the blob written
    /// afresh.
    fn keep_pending(&mut self, kept: &local::Kept) {
        let bytes = disk::frame(&kept.to_bytes());
        let wrote = match kept.whole {
            true => self.local.put(local::PENDING, &bytes),
            false => self.local.append(local::PENDING, &bytes),
        };
        if let Err(e) = wrote {
            self.local_failure = Some(format!("the pending intents cannot be written: {e}"));
        }
    }

    /// Every view of `space` told a settle's changes. One that cannot be
    /// kept up, or could not be made, is made again over the store as it
    /// is, under its number: its rows as they were removed, and as they are
    /// inserted ([`watched::again`]).
    fn changed(&mut self, space: Space, cs: &[Change]) {
        if !self.views.iter().flatten().any(|w| w.space == space) {
            return;
        }
        let domain = self.machine(space).map(|m| m.domain.clone());
        // Read once for every view, of the tables they read: what they are
        // told of a table none reads (a fill's) is nothing to them.
        let reads = self.reads(space);
        let cs: Vec<Change> = cs.iter().filter(|c| reads.iter().any(|t| t == c.table())).cloned().collect();
        let store = &self.core.machine(space).expect("a machine that spoke").store;
        let p = ark::plan::Pushed::of(store, &cs);
        for (i, w) in self.views.iter_mut().enumerate() {
            let Some(w) = w.as_mut().filter(|w| w.space == space) else { continue };
            let pushed = w.view.as_mut().map(|v| v.pushed(&p));
            match pushed {
                Some(Ok(patches)) => self.told.patched(i, patches),
                _ => {
                    let old = w.view.as_ref().map_or(0, |v| v.rows().len());
                    w.view = domain.as_ref().and_then(|d| d.watch(store, &w.ctx, &w.name, &w.input).ok());
                    let rows = w.view.as_ref().map(|v| v.rows()).unwrap_or_default();
                    self.told.patched(i, watched::again(old, rows));
                }
            }
        }
    }

    /// An intent of this device's own of the function `func` on `space`'s
    /// machine ([`Peer::mutate`]), an ephemeral one through the relay, and
    /// the verdict it was given here and now: its preview's refusal.
    pub(crate) fn author(&mut self, space: Space, func: &str, input: Vec<u8>, now: i64) -> Result<(), String> {
        let cause = self.cause(space, now);
        if space == Space::Volatile && self.config.ephemeral.contains(func) {
            self.relay.insert(func.into(), (input, now, cause));
            self.tick(now);
            return Ok(());
        }
        let func = func.into();
        self.step(space, Input::Mutate { func, input, now, cause }).unwrap_or(Ok(()))
    }

    /// The span an intent of `space` authored at `now` is under: the one
    /// opened, or the span of the entry whose change woke it, while that
    /// trace is younger than [`TRACE_MS`]. A trace is what one hand caused:
    /// an answer to an answer to a press, minutes on (a player's position,
    /// reported each second), is a trace of its own.
    pub fn cause(&self, space: Space, now: i64) -> Option<Span> {
        let woke = self.woke[space as usize].filter(|w| now - w.began < TRACE_MS);
        self.span.or(woke)
    }

    /// `f` with this device authoring under `span` (none: under what woke
    /// it), the span it had put back after.
    pub fn under<T>(&mut self, span: Option<Span>, f: impl FnOnce(&mut Peer) -> T) -> T {
        let was = std::mem::replace(&mut self.span, span);
        let out = f(self);
        self.span = was;
        out
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

    /// An intent of its own just authored on the log, while it is pending.
    pub fn authored(&self, id: &EntryId) -> Option<Ctx> {
        self.log.pending().iter().map(|p| &p.entry).find(|e| e.id == *id).cloned()
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

    /// The clock: the relay goes when its interval has passed.
    pub fn tick(&mut self, now: i64) {
        if self.relay.is_empty() || self.relayed.is_some_and(|t| now - t < self.config.every_ms) {
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
        (self.link.epoch, self.link.wait, self.link.opened) = (self.link.epoch + 1, self.link.backoff.0, true);
        self.linked = true;
        self.frames.clear();
        // Where this device's own intents stand, beside where the server
        // has them (`arkdb origins`): a first pending past the server's
        // last + 1 is a gap, which its pushes say and the server passes.
        let first = self.log.pending().front().map(|p| p.entry.local);
        tracing::info!(target: "ark", peer = %self.label(), local = self.log.local, pending = self.log.pending().len(), first, "replica");
        // What it is, before its `Hello`s, to a call nobody answers.
        if let Some(input) = self.config.live.as_ref().and(self.config.device.clone()) {
            self.call(0, ark::frames::DEVICE, input);
        }
        self.step(Space::Durable, Input::Connected);
        self.step(Space::Volatile, Input::Connected);
        self.files_connected();
    }

    pub fn disconnected(&mut self) {
        self.linked = false;
        self.frames.clear();
        // A call is answered on the connection it went out on, or never.
        self.told.unanswered("the connection to the server closed before it answered");
    }

    /// A call of one of its server's own functions ([`ServerConfig::calls`]),
    /// sent now: its id, for [`Peer::take_reply`]. Only while linked — a
    /// call is never kept to send later.
    pub fn request(&mut self, name: &str, input: Vec<u8>) -> Result<u64, String> {
        if !self.linked {
            return Err("not connected to the server".into());
        }
        let id = self.told.asked();
        self.call(id, name, input);
        // Out now, as a change is: nobody pumps for an answer not yet asked.
        self.flush();
        Ok(id)
    }

    /// A frame from its server, read once, here: to the machine of its
    /// space; a call's reply, to what it was asked; a file's, to its files.
    /// Its space and whether it was a fill's part; none, for bytes that are
    /// no frame. Told it cannot be served (§1.2), it closes its link until
    /// the app is updated.
    pub fn receive(&mut self, bytes: Vec<u8>) -> Option<(Space, bool)> {
        let f = Frame::dec(&bytes).ok()?;
        let said = (f.space(), matches!(f, Frame::Part { .. }));
        match f {
            Frame::Reply { id, reply } => self.told.replied(id, reply),
            f if f.is_file() => self.file_told(f),
            f => drop(self.step(f.space(), Input::Frame { from: None, frame: f })),
        }
        if self.update_needed().is_some() && !self.link.offline {
            tracing::warn!(target: "ark", peer = %self.label(), why = self.update_needed(), "update needed");
            self.disconnect();
        }
        Some(said)
    }

    /// Another login: every pending intent the signer's, its id kept — or,
    /// for another actor than the one this device authored as, authored
    /// again from a new origin's first `local` — and the live machine
    /// started over.
    pub fn sign_in(&mut self, who: Ctx) {
        self.read_whole();
        let was = &self.log.me;
        let other = !was.actor.is_empty() && was.actor != who.actor;
        if other {
            self.log.origin = Origin::named([&self.log.origin.0[..], who.session.as_bytes()].concat());
            self.log.local = 0;
            let o = self.log.origin;
            self.replica(|r| r.origin = o);
        }
        let origin = self.log.origin;
        let mut pending: Vec<Ctx> = self.log.pending().iter().map(|p| p.entry.clone()).collect();
        for (n, e) in pending.iter_mut().enumerate() {
            if other {
                (e.origin, e.local) = (origin, n as i64 + 1);
                e.id = EntryId::of(origin, e.local);
            }
            (e.actor, e.session, e.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        }
        self.log.me = who.clone();
        let opens = local::Replica::read(&mut *self.local).opens;
        if let Some(live) = &self.config.live {
            let mut p = machine(live, Role::Replica, &who, Some(live_origin(self.log.origin, opens)), Space::Volatile);
            p.fold = Some(Fold::REPLICA);
            // The live origin goes on counting, unless the actor is new.
            if let Some(x) = self.live.as_ref().filter(|_| !other) {
                (p.origin, p.local) = (x.origin, x.local);
            }
            self.live = Some(p);
        }
        let local = if other { pending.len() as i64 } else { self.log.local };
        let pending = pending.into_iter().map(|e| Logged::intent(e, vec![])).collect();
        let records = vec![
            Record::Rows(self.log.confirmed_rows()),
            Record::Pending(local::Kept::all(pending, None, local)),
        ];
        self.step(Space::Durable, Input::Open(records));
        if self.linked {
            self.connected();
        }
    }

    /// A view's rows as they stand, each its bytes; none while it cannot be
    /// made.
    pub fn rows(&self, view: usize) -> Option<Vec<Vec<u8>>> {
        self.views.get(view)?.as_ref()?.view.as_ref().map(|v| v.rows())
    }

    /// A maintained view of the domain's function `name` on `input` over
    /// `space`'s optimistic state, as `ctx` reads it (§1.16); what it does
    /// comes as its patches ([`Peer::update`]), under its number for as
    /// long as it is held.
    pub fn watch(&mut self, space: Space, name: &str, input: &[u8], ctx: Ctx) -> Result<usize, String> {
        let m = self.machine(space).ok_or_else(|| String::from("no machine of that space"))?;
        let view = Some(m.domain.watch(&m.store, &ctx, name, input)?);
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

    /// What this peer said of itself since, out of the outbox and to its
    /// server as a call nobody answers ([`ark::frames::EVENTS`]), a page at
    /// a time.
    pub(crate) fn ship(&mut self) {
        let events = observe::take(&self.label(), 256);
        if !events.is_empty() {
            self.call(0, ark::frames::EVENTS, events.to_bytes());
        }
    }

    /// A call for its server (`id` 0: one nobody answers).
    fn call(&mut self, id: u64, name: &str, input: Vec<u8>) {
        self.frames.push(
            Frame::Call {
                id,
                name: name.into(),
                input,
            }
            .enc(),
        );
    }

    /// The frames for its server, taken — what it said of itself among
    /// them while linked, as a link sends them — by a driver carrying them
    /// itself.
    pub fn take_frames(&mut self) -> Vec<Vec<u8>> {
        if self.linked {
            self.ship();
        }
        std::mem::take(&mut self.frames)
    }
}
