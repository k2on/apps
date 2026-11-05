//! The ArkDB v5 peer: a driver of the one machine (§1.7), twice — the
//! log's and, when the domain has live state, the volatile space's (§1.9)
//! — over one disk. A device and a server are the same [`Peer`] in two
//! roles:
//!
//! - **a device** ([`Peer::open`]) is a replica; it **dials** a server
//!   ([`Peer::connect`]: the link, `link`) when it has one, keeps its
//!   pending intents on its disk — with no server, or none reached, they
//!   are simply pending — relays its ephemeral calls, and keeps the
//!   maintained views a UI draws (§1.10);
//! - **a server** ([`Peer::serve`]) is the authority of each space; it
//!   **listens** (`hub`: the connections, each with its identity and its
//!   feed, routed; the journal beside the log), on a thread of its own
//!   (`handle`, a device's too) and, with the `server` feature, on a socket (`serve`:
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
//! its session and signs out through the domain; a server's hub judges
//! every token by the row it names and closes what is revoked or expired,
//! as `ark_auth::server::Auth` says (`serve`).
//!
//! **A peer is two instances** (`instance`: a machine and what keeps
//! it) wired together, the log's and the live space's, and records beside
//! them on the same disk (`local`): a server's journal and volatile space,
//! a device's replica and pending intents.
//!
//! **Handlers** (`jobs`) act on the world outside: a handler claims, runs
//! and finishes the jobs of a kind (`ark::effect`) on the peer that
//! registered it.
//!
//! Every input either machine of a device is given is recorded, when asked
//! (`record`), as an [`Act`]: a device is a pure function of these, which
//! the trace player (`arkdb`) holds it to.

pub mod app;
pub mod disk;
pub mod files;
pub mod handle;
mod hub;
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

pub use app::{nobody, Arg, Outcome};
pub use ark::file::FileRef;
pub use ark::peer::{Fold, Scoping};
pub use disk::{Disk, Files, Memory};
#[cfg(not(target_arch = "wasm32"))]
pub use handle::Authenticate;
pub use handle::{dev, Handle};
pub use hub::{Called, ConnId, Deliver};
pub use instance::Instance;
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

/// What a peer is made of: the log's domain and the live space's, each its
/// schema and its functions by name (§1.16, §1.9); as a server, how each
/// connection is fed (§1.8), the entries each space keeps above its horizon
/// (all of them when none), and the functions of its own a device may call
/// over its link (`calls`); as a device, what it says it is at each
/// connection (a call before its `Hello`s, `ark::frames::DEVICE`)
/// and the live functions it relays as ephemeral, and how often.
///
/// A server authors the live domain's `on_join` (a function whose name's
/// last part is that) when a session's first connection says what its
/// device is — on what it said, as the connection's login — its
/// `on_leave` when the last closes, and its `on_restore` over what a
/// restart kept.
#[derive(Clone)]
pub struct Config {
    pub log: Arc<Domain>,
    pub live: Option<Arc<Domain>>,
    /// The scope each connection is fed by (§1.8), by who it is; none,
    /// the log whole.
    pub feed: Scoping,
    pub keep: Option<usize>,
    pub live_keep: Option<usize>,
    pub device: Option<Vec<u8>>,
    pub ephemeral: BTreeSet<String>,
    pub every_ms: i64,
    /// A server's functions a device calls by name over its link, outside
    /// both machines ([`Frame::Call`](ark::frames::Frame)): never logged,
    /// journaled, recorded or told to another connection.
    pub calls: BTreeMap<String, Call>,
}

/// One of a server's [`Config::calls`]: given the caller, verified as its
/// connection is, and the call's input; what it gives back, or why not. It
/// runs on a thread of its own ([`Handle`]), so it may take its time.
pub type Call = Arc<dyn Fn(&Ctx, &[u8]) -> Result<Vec<u8>, String> + Send + Sync>;

impl Config {
    /// A peer of `log`, with no live space.
    pub fn of(log: Arc<Domain>) -> Config {
        Config {
            log,
            live: None,
            feed: Arc::new(|_| None),
            keep: None,
            live_keep: None,
            device: None,
            ephemeral: BTreeSet::new(),
            every_ms: 0,
            calls: BTreeMap::new(),
        }
    }

    /// The domain of `space`.
    fn domain_of(&self, space: Space) -> Option<Arc<Domain>> {
        match space {
            Space::Durable => Some(self.log.clone()),
            Space::Volatile => self.live.clone(),
        }
    }

    /// The live domain's function whose name's last part is `last`: what a
    /// server authors as devices come and go, and after a restart.
    pub(crate) fn hook(&self, last: &str) -> Option<String> {
        self.live.as_ref()?.binding(last).map(String::from)
    }
}

/// Why an authority's disk is not opened under its schema.
pub const ANOTHER: &str =
    "the rows on disk are of a schema this one does not grow from: a schema only grows (append a table, or a column that is an Option or defaulted)";

/// A patch of a view's rows, each its bytes.
pub type Patch = ark::plan::Patch<Vec<u8>>;

/// Two instances wired, the log's machine on the disk and the live one,
/// the records beside them on the same disk, and what wires them.
pub struct Peer {
    pub config: Config,
    pub log: Instance,
    pub live: Option<Instance>,
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
    /// change it was last told, so that what it does in answer (a job, a
    /// view's reader) is a child of what it answered. A server
    /// authors under the opened one only.
    span: Option<Span>,
    woke: [Option<Span>; 2],
    /// What became of its own intents, and what its views did.
    told: app::Told,
    /// What it keeps beside its log, on the same disk, and its last
    /// refusal, while it lasts.
    pub local: Box<dyn Disk>,
    local_failure: Option<String>,
    /// What it keeps of the log's pending intents, as last written.
    saved: Saved,
    /// The handlers registered here, and what changed while they ran.
    runners: Vec<Box<dyn jobs::Runner>>,
    missed: Option<Vec<Told>>,
    /// The files rows name, and where each transfer is (`files`).
    pub(crate) files: files::Files,
}

/// What a device last wrote of its pending intents ([`local::Kept`]): its
/// last `local`, how many times they had been run again, the `local` of
/// the last and how many, and how many records the blob holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Saved {
    local: i64,
    reruns: u64,
    back: Option<i64>,
    len: usize,
    records: usize,
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

fn machine(d: &Arc<Domain>, role: Role, me: &Ctx, origin: Option<Origin>, space: Space) -> Instance {
    let mut p = Machine::new(d.clone(), role, me.clone()).in_space(space);
    if let Some(o) = origin {
        p.origin = o;
    }
    Instance::new(p, None)
}

/// A device's live machine as `me`, its origin its log's `origin` at its
/// `opens`th open. The live space keeps nothing across an open, so its
/// `local`s start again at 1; under one origin for every open, a reloaded
/// page's calls were taken for ones the server had judged already, for as
/// long as it ran.
fn live_of(config: &Config, me: &Ctx, origin: Origin, opens: u64) -> Option<Instance> {
    let live = config.live.as_ref()?;
    let mut p = machine(live, Role::Replica, me, Some(live_origin(origin, opens)), Space::Volatile);
    p.fold = Some(Fold::REPLICA);
    Some(p)
}

/// The live machine's origin at the `opens`th open of a peer whose log's
/// is `origin`.
pub(crate) fn live_origin(origin: Origin, opens: u64) -> Origin {
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

impl Peer {
    fn with(config: Config, log: Instance, live: Option<Instance>, local: Box<dyn Disk>) -> Peer {
        Peer {
            config,
            log,
            live,
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
            local_failure: None,
            saved: Saved::default(),
            runners: vec![],
            missed: None,
            files: files::Files::default(),
        }
    }

    /// A device opened as `me` from what `disk` holds.
    pub fn open(config: Config, me: Ctx, disk: Box<dyn Disk>) -> Peer {
        let files = disk.files();
        let (disk, local) = local::beside(disk);
        let mut p = Peer::opened(config, me, disk, local, link::Link::default());
        p.keep_files(files);
        p
    }

    pub(crate) fn opened(config: Config, me: Ctx, disk: Box<dyn Disk>, mut local: Box<dyn Disk>, link: link::Link) -> Peer {
        let r = local::Replica::read(&mut *local);
        // Pending intents are read only beside the replica they were drawn
        // from: a disk whose replica does not read (none yet, or an older
        // build's) starts afresh.
        let blob = local.get(local::PENDING).ok().flatten().filter(|_| r.origin != Origin::default());
        let (pending, kept, records) = local::Kept::read(&blob.unwrap_or_default());
        let origin = match r.origin {
            o if o != Origin::default() => o,
            _ => draw_origin(),
        };
        let opens = r.opens + 1;
        let _ = local.put(local::REPLICA, &local::Replica { origin, opens, ..r.clone() }.to_bytes());
        let live = live_of(&config, &me, origin, opens);
        let mut log = machine(&config.log, Role::Replica, &me, Some(origin), Space::Durable);
        let mut disk = disk;
        // Rows of a schema this one does not grow from (a build older than
        // a schema that only grows) are sent again from empty (`disk::own`).
        let schema = &config.log.schema;
        let empty = |disk: &mut dyn Disk| disk.record(&Record::Rows(log.confirmed_rows()).to_bytes());
        let read = match disk::own(&mut *disk, schema) {
            Ok(false) => empty(&mut *disk).and_then(|_| disk::put_schema(&mut *disk, schema)),
            Ok(true) => disk::put_schema(&mut *disk, schema),
            Err(e) => Err(e),
        };
        if let Err(e) = read {
            log.failure = Some(e.to_string());
        }
        log.disk = Some(disk);
        let mut p = Peer::with(config, log, live, local);
        p.link = link;
        let local = r.local.max(kept.local);
        p.saved = Saved {
            local,
            back: pending.last().map(|p| p.entry.local),
            len: pending.len(),
            records,
            ..Saved::default()
        };
        let mut stored = p.log.stored();
        stored.push(Record::Pending { pending, at: kept.at, local });
        p.step(Space::Durable, Input::Open(stored));
        p
    }

    /// This device stopped and opened again from its disk, unlinked; its
    /// link's server, login and transport kept.
    pub fn reopen(self) -> Peer {
        let me = self.log.me.clone();
        let mut link = self.link;
        link.wire = None;
        let files = self.files.store;
        let mut p = Peer::opened(self.config, me, self.log.into_disk(), self.local, link);
        p.keep_files(files);
        p
    }

    pub fn machine(&self, space: Space) -> Option<&Machine> {
        match space {
            Space::Durable => Some(&self.log),
            Space::Volatile => self.live.as_deref(),
        }
    }

    /// What keeps a disk from being written, while it lasts: the log's, or
    /// its records' beside it.
    pub fn failure(&self) -> Option<&str> {
        self.log.failure.as_deref().or(self.local_failure.as_deref())
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
        let label = self.label();
        let Some(m) = self.instance(space) else { return vec![] };
        let space_name = ["durable", "volatile"][space as usize];
        let out = tracing::debug_span!(target: "ark", "step", peer = %label, space = space_name, conn = from).in_scope(|| m.step(input));
        for o in &out {
            match o {
                Output::Send(To::Back, b) => match from {
                    Some(c) => self.hub.held.push((c, b.clone())),
                    None if self.linked => self.frames.push(b.clone()),
                    None => {}
                },
                Output::Send(To::All, b) => self.fanout(space, b),
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
                Output::Settled(id, Err(why)) if !self.hub.serving => self.told.refused(space, *id, why),
                Output::Settled(id, Ok(())) if !self.hub.serving => self.told.confirmed(space, *id),
                _ => {}
            }
        }
        match (self.hub.serving, space) {
            (true, Space::Durable) => self.kept(),
            (false, Space::Durable) => self.keep_pending(),
            _ => {}
        }
        out
    }

    /// Its replica, changed by `f`, kept.
    pub(crate) fn replica(&mut self, f: impl FnOnce(&mut local::Replica)) {
        let mut r = local::Replica::read(&mut *self.local);
        f(&mut r);
        self.put_beside(local::REPLICA, &r.to_bytes());
    }

    /// A record kept beside its log, on its disk; a failure kept as its
    /// failure.
    pub(crate) fn put_beside(&mut self, name: &str, bytes: &[u8]) {
        if let Err(e) = self.local.put(name, bytes) {
            self.local_failure = Some(e.to_string());
        }
    }

    /// A device's pending intents, their changes and its last `local` kept
    /// as they change, one record appended ([`local::Kept`]): the intents
    /// confirmed or refused off the front, those new at the back — so a
    /// mutate writes its one, whatever is pending — or, when they were run
    /// again (a rebase, a renumbering, an open), or the records outnumber
    /// them, all of them, written afresh; and the head they stand on.
    fn keep_pending(&mut self) {
        let (m, s) = (&self.log, &mut self.saved);
        let back = m.pending.back().map(|p| p.entry.local);
        if (m.local, m.reruns, back, m.pending.len()) == (s.local, s.reruns, s.back, s.len) {
            return;
        }
        let whole = m.reruns != s.reruns || s.records > m.pending.len() + 64;
        let new = match whole {
            true => m.pending.len(),
            false => m.pending.iter().rev().take_while(|p| s.back.is_none_or(|b| p.entry.local > b)).count(),
        };
        let kept = local::Kept {
            whole,
            gone: if whole { 0 } else { (s.len + new - m.pending.len()) as u64 },
            put: m.pending.iter().skip(m.pending.len() - new).cloned().collect(),
            local: m.local,
            at: Some(m.log().head()),
        };
        *s = Saved {
            local: m.local,
            reruns: m.reruns,
            back,
            len: m.pending.len(),
            records: if whole { 1 } else { s.records + 1 },
        };
        let bytes = disk::frame(&kept.to_bytes());
        let wrote = match whole {
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
        let store = match space {
            Space::Durable => &self.log.store,
            Space::Volatile => &self.live.as_ref().expect("a machine that spoke").store,
        };
        let domain = self.config.domain_of(space);
        // Read once for every view, each table's rows only when a view's
        // plan asks for them.
        let p = ark::plan::Pushed::of(store, cs);
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

    /// An intent of this peer's own of the function `func` on `space`'s
    /// machine ([`Peer::mutate`]; a server's of a device's arrival), an
    /// ephemeral one through the relay, and the verdict it was given here
    /// and now: a preview's refusal, or the authority's.
    pub(crate) fn author(&mut self, space: Space, func: &str, input: Vec<u8>, now: i64) -> Result<(), String> {
        if space == Space::Volatile && self.config.ephemeral.contains(func) {
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
    pub fn authored(&self, id: &EntryId) -> Option<Ctx> {
        let pending = self.log.pending.iter().map(|p| &p.entry).find(|e| e.id == *id).cloned();
        let head = self.log.log().head().0;
        pending.or_else(|| {
            (head.saturating_sub(8)..=head)
                .rev()
                .filter_map(|s| self.log.log().at(s))
                .map(|l| l.entry)
                .find(|e| e.id == *id)
        })
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

    /// Facts the authority writes itself, raw: an entry naming no function,
    /// its input their encoding, judged by the store as any write is.
    pub fn write(&mut self, facts: Vec<Change>, now: i64) -> Result<(), String> {
        let (func, input) = (String::new(), ark::format::encode(&facts).expect("changes encode"));
        self.verdict(
            Space::Durable,
            Input::Mutate {
                func,
                input,
                now,
                cause: None,
            },
        )
    }

    /// One input of this peer's own, routed, and the verdict it was given
    /// here and now.
    fn verdict(&mut self, space: Space, input: Input) -> Result<(), String> {
        if self.machine(space).is_none() {
            return Err(format!("no {space:?} machine here"));
        }
        let said = self.route(space, None, input);
        let verdict = said.into_iter().find_map(|o| match o {
            Output::Settled(_, r) => Some(r),
            _ => None,
        });
        verdict.unwrap_or(if self.hub.serving { Err("no verdict".into()) } else { Ok(()) })
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
        let first = self.log.pending.front().map(|p| p.entry.local);
        tracing::info!(target: "ark", peer = %self.label(), local = self.log.local, pending = self.log.pending.len(), first, "replica");
        // What it is, before its `Hello`s, to a call nobody answers.
        if let Some(input) = self.config.live.as_ref().and(self.config.device.clone()) {
            self.frames.push(
                Frame::Call {
                    id: 0,
                    name: ark::frames::DEVICE.into(),
                    input,
                }
                .enc(),
            );
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

    /// A call of one of its server's own functions ([`Config::calls`]),
    /// sent now: its id, for [`Peer::take_reply`]. Only while linked — a
    /// call is never kept to send later.
    pub fn request(&mut self, name: &str, input: Vec<u8>) -> Result<u64, String> {
        if !self.linked {
            return Err("not connected to the server".into());
        }
        let id = self.told.asked();
        self.frames.push(
            Frame::Call {
                id,
                name: name.into(),
                input,
            }
            .enc(),
        );
        // Out now, as a change is: nobody pumps for an answer not yet asked.
        self.flush();
        Ok(id)
    }

    /// A frame from its server, to the machine of its space; a call's
    /// reply, to what it was asked.
    pub fn receive(&mut self, bytes: Vec<u8>) {
        let space = match Frame::dec(&bytes) {
            Ok(Frame::Reply { id, reply }) => return self.told.replied(id, reply),
            Ok(f) if f.is_file() => return self.file_told(f),
            f => f.map_or(Space::Durable, |f| f.space()),
        };
        self.step(space, Input::Frame { from: None, bytes });
    }

    /// Another login: every pending intent the signer's, its id kept — or,
    /// for another actor than the one this device authored as, authored
    /// again from a new origin's first `local` — and the live machine
    /// started over.
    pub fn sign_in(&mut self, who: Ctx) {
        let was = &self.log.me;
        let other = !was.actor.is_empty() && was.actor != who.actor;
        if other {
            self.log.origin = Origin::named([&self.log.origin.0[..], who.session.as_bytes()].concat());
            self.log.local = 0;
            let o = self.log.origin;
            self.replica(|r| r.origin = o);
        }
        let origin = self.log.origin;
        let mut pending: Vec<Ctx> = self.log.pending.iter().map(|p| p.entry.clone()).collect();
        for (n, e) in pending.iter_mut().enumerate() {
            if other {
                (e.origin, e.local) = (origin, n as i64 + 1);
                e.id = EntryId::of(origin, e.local);
            }
            (e.actor, e.session, e.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        }
        self.log.me = who.clone();
        let opens = local::Replica::read(&mut *self.local).opens;
        if let Some(mut p) = live_of(&self.config, &who, self.log.origin, opens) {
            // The live origin goes on counting, unless the actor is new.
            if let Some(x) = self.live.as_ref().filter(|_| !other) {
                (p.origin, p.local) = (x.origin, x.local);
            }
            self.live = Some(p);
        }
        let local = if other { pending.len() as i64 } else { self.log.local };
        let pending = pending.into_iter().map(|e| Logged::intent(e, vec![])).collect();
        let records = vec![Record::Rows(self.log.confirmed_rows()), Record::Pending { pending, at: None, local }];
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
        let d = self.config.domain_of(space).ok_or_else(|| String::from("no domain here"))?;
        let store = &self.machine(space).ok_or_else(|| String::from("no machine of that space"))?.store;
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

    /// What this peer said of itself since, out of the outbox and to its
    /// server as a call nobody answers ([`ark::frames::EVENTS`]), a page at
    /// a time.
    pub(crate) fn ship(&mut self) {
        let events = observe::take(&self.label(), 256);
        if !events.is_empty() {
            let (id, name, input) = (0, ark::frames::EVENTS.into(), events.to_bytes());
            self.frames.push(Frame::Call { id, name, input }.enc());
        }
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
