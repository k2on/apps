//! The loop: a [`Node`] takes one entry at a time ([`Node::step`]),
//! runs its handler in a transaction on its space's rows, and returns the
//! entries the handler made, which the host routes by their space
//! ([`Route`]).
//!
//! A handler is found by the number its body names ([`Body::handler`]) in
//! the node's table of them, written by the build. It reads and writes
//! through [`Cx::db`] (a [`Txn`]: an [`Overlay`] over its space's rows),
//! whose writes are kept whole when it returns `Ok` and dropped when it
//! refuses. A refusal is a code ([`Refusal`]).
//!
//! **Each space's rows** are one [`Versioned`] store: the host's
//! confirmed rows ([`Store`], given only confirmed writes, whole), what
//! the pending entries wrote over them, the rows held apart until a fill
//! or a subscription is all here, and each row's value before the settle
//! under way. Every write goes through it, so what changed is known
//! without noting it ([`Node::take_changes`]), and a settle's plans read
//! the rows as they were from it, with the cells as they were beside them.
//!
//! **The shared space** is the master log's, on the server (an
//! [`crate::Authority`]). A node keeps two things of it: the confirmed
//! rows, the log's facts applied up to its cursor, and its own entries the
//! server has not confirmed, each kept as a [`Pending`] record with what
//! its run here wrote. It reads the shared space as the confirmed rows with
//! every pending entry's writes over them, so it sees its own writes at
//! once, connected or not: a node with no server is a replica that isn't
//! connected, nothing more.
//!
//! - Its own shared entry runs at once on that view, is kept pending, and
//!   is pushed upstream ([`Up::Push`]), now and again at every connection.
//! - Another node's shared entry is never run here: it arrives logged, with
//!   its facts, which are applied to the confirmed rows ([`Down::Logged`]).
//! - After facts, the pending entries are run again, in order, over the new
//!   confirmed rows: the rebase. One that refuses there stays pending with
//!   no writes; the server's run decides. When the facts are this node's
//!   oldest pending entry's and say what its run here wrote, the entries
//!   after it ran over those very rows, so nothing is run again.
//! - A node too far behind for its upstream's log is sent a fill: the
//!   confirmed rows at a `seq` ([`Down::Rows`], ending in [`End::Fill`]), in
//!   place of the log up to it, then the log after it. Its parts are held
//!   until the last, which (a whole fill) empties the shared rows, takes the rows
//!   at once; then the pending entries are rebased. One interrupted is let
//!   go, the rows as they were, and started again at the next connection.
//! - An entry is let go once it is judged, which the log says: it comes
//!   back logged, kept (its facts) or refused (its one fact the refusal's
//!   row, [`crate::sync::REFUSALS`]), or a fill says this node's origin is
//!   judged up to it, its rows the kept ones' and the refused ones'
//!   refusal rows. A refused entry is dropped and its refusal kept in the
//!   node's journal of refusals for the host to show; its row is not kept.
//!   It never blocks the entries after it: each is judged on its own.
//!
//! The entries a shared handler emits are taken from its first run here
//! only, never from a rebase.
//!
//! **Subscriptions.** A node that holds only what its queries read
//! subscribes to them ([`Node::subscribe`]), says them again at every
//! connection before its hello, and takes the rows its upstream sends for
//! them ([`Down::Rows`]) into its confirmed rows, as it takes facts. It
//! keeps the queries it wants and whether each is got, as Zero's desired
//! and got sets: while one it wants is not got, the rows that move are
//! held apart, unread, and a fact for a row held there is made there, so
//! a view never shows half a subscription. They are taken at once when
//! every query it wants is got ([`End::Got`]), and a query is complete
//! ([`Node::complete`]) only then. A connection lost with rows held lets
//! them go and says, at its next hello, the cursor it had when it began
//! to hold them, so its upstream sends them again.
//!
//! **The session space** is live state each node holds in memory: its own
//! session entries are stepped here and sent upstream, and another node's
//! arrive from it ([`Down::Live`]) and are stepped here too, never logged.
//! A playback position is an [`crate::Anchor`] its owner sets whole: a
//! view extrapolates it, with no stream of updates.
//!
//! - **Owners.** A row whose key begins with a node's origin
//!   ([`Origin::owns`]) is that node's: it is the row's authority, as a
//!   phone's player is for its playback, and a phone answers for its
//!   watch's rows as for its own ([`Node::answer_for`]). Others set it as
//!   controls, which the owner takes when they are newer than what it has.
//! - **Live controls don't queue.** With no link upstream (before
//!   [`Node::connect`], after [`Node::disconnect`]) a session entry for a
//!   row the node does not own is refused ([`Refusal::OFFLINE`]): not
//!   applied, not kept, never sent later, so the host shows that the owner
//!   can't be reached. One for its own row is applied.
//! - **Rows it does not own are its upstream's**: it takes what comes,
//!   its own entries said back too, so it ends where its upstream does;
//!   its own control shows at once, until the upstream says otherwise.
//! - **Every hello heals**, by three rules. Its upstream answers a hello
//!   with the latest of every row the node may see, its own included, and
//!   ends the answer with [`Down::Welcome`]. A row it does not own takes
//!   the answer's, and one the answer leaves out goes (an upstream that
//!   started again lost it), unless the node set it since it connected. A
//!   row it owns and holds takes nothing from the answer, so no control
//!   made while it was away fires later; one it holds none of (it started
//!   again) takes the answer's. And at the welcome every row it owns is
//!   said again, as a new entry caused by its last: newer than anything
//!   its upstream kept while it was away.
//! - **The tree's clock.** A hello says the node's time and the welcome
//!   says it back with the upstream's estimate of the tree's time (its
//!   root's clock), so a node measures its offset NTP's way, from the
//!   shortest round trip of its last four hellos, and of the pings a node
//!   connected for long says now and then ([`Node::ping`]), so it does not
//!   drift ([`Node::offset`]); an
//!   upstream's own offset is in what it says, so the offsets compose down
//!   the tree. An owner writes its anchors in the tree's time
//!   ([`Node::time`]), and a view extrapolates them in it.
//!
//! **What changed.** The node notes each row of each space that its own
//! writes, facts, fills, rebases and session entries touch, with what it
//! read before; [`Node::take_changes`] answers what changed in a space
//! since the host last asked, for the queries over its view
//! ([`crate::plan`]). The host takes them after each step or receive;
//! until then they gather.
//!
//! **Cells and the settle.** A handler of a local, device or session entry
//! reads and writes the node's cells ([`Cx::get`], [`Cx::set`]); its writes
//! are journaled beside its rows', kept with them or dropped with them. A
//! shared run sees none, as it must give the same rows on every node.
//! [`Node::settle`] ends a step or a receive: it takes each space's changes
//! (and tells the sink, which keeps what a disk keeps), pushes them through
//! the watched plans, and tells the sink each reader whose reads changed,
//! once. Nothing is worked out when it is written: a reader pulls when it
//! draws, and its sink cuts it off there by equality.

use alloc::vec::Vec;

use crate::cell::{hit, key, Cell, Reader, Scope, Sink, Sources, Watch};
use crate::codec::Decode;
use crate::entry::{Actor, Entry, EntryId, Hlc, Origin, Seq, Space, Version};
use crate::plan::{Changes, Live, Plan, Recorder};
use crate::slot::Boxed;
use crate::store::{Layer, Nothing, Overlay, Read, Store, TableId, Txn, Versioned, View, Write, Writes, EMPTY};
use crate::sync::{Down, End, Logged, Pending, QueryId, Refused, Up, REFUSALS};

/// A handler, by the number its build gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandlerId(pub u32);

/// A body names the handler that runs it.
pub trait Body {
    fn handler(&self) -> HandlerId;

    /// The one session row this entry sets whole, if it does: a later
    /// entry for the same row replaces it, so a node sent the entries at a
    /// rate is sent only the latest ([`crate::sync::Feed`]). `None`, the
    /// default, is never held back.
    fn row(&self) -> Option<(TableId, Vec<u8>)> {
        None
    }
}

/// Why an entry was not run: a code the host words. Codes below 256 are
/// the kernel's; an app's handlers use the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Refusal(pub u16);

impl Refusal {
    /// No handler has the number the body names.
    pub const NO_HANDLER: Refusal = Refusal(1);
    /// Not an entry the authority judges: not shared, or already ordered.
    pub const NOT_PUSHABLE: Refusal = Refusal(2);
    /// Another node's shared entry, which a node takes as facts, never
    /// runs.
    pub const NOT_OWN: Refusal = Refusal(3);
    /// An entry that says it is from an actor other than its connection's.
    pub const NOT_YOURS: Refusal = Refusal(4);
    /// A session entry made with no link upstream, for a row this node does
    /// not own: live controls don't queue, so it is neither applied nor
    /// kept, and the host shows that the row's owner can't be reached.
    pub const OFFLINE: Refusal = Refusal(5);
}

/// What a handler runs with: the entry, its space's rows, the node's cells
/// (but in a shared run, which a server makes too), and the entries it
/// makes.
pub struct Cx<'a, B> {
    pub entry: &'a Entry<B>,
    /// The entry's space's rows, as this run has written them.
    pub db: &'a mut dyn Txn,
    src: Option<&'a Sources>,
    /// The cells this run wrote, kept with its rows or dropped with them.
    journal: Vec<(u32, Boxed)>,
    made: Vec<(Space, B)>,
}

impl<B> Cx<'_, B> {
    /// Makes an entry in `space`, caused by this one, for the host to route
    /// once this run is kept.
    pub fn emit(&mut self, space: Space, body: B) {
        self.made.push((space, body));
    }

    /// A cell's value, as this run has written it: none in a shared run,
    /// which must give the same rows on every node.
    pub fn get<T: 'static>(&self, c: Cell<T>) -> Option<&T> {
        match self.journal.iter().rev().find(|j| j.0 == c.0) {
            Some((_, v)) => v.get(),
            None => self.src?.slots.get(c.0, false),
        }
    }

    /// Writes a cell, kept if this run is: an equal value changes nothing.
    /// A shared run writes none.
    pub fn set<T: PartialEq + 'static>(&mut self, c: Cell<T>, v: T) {
        if self.src.is_some() {
            self.journal.push((c.0, Boxed::new(v)));
        }
    }
}

/// A handler: reads and writes `cx.db`, emits entries, or refuses.
pub type Handler<B> = fn(&mut Cx<'_, B>) -> Result<(), Refusal>;

/// What a handler's run made: its writes, the entries it emitted and the
/// cells it wrote.
pub(crate) type Ran<B> = (Writes, Vec<(Space, B)>, Vec<(u32, Boxed)>);

/// Runs `e`'s handler over `base`, writing nothing to it, with the node's
/// cells in `src` (none in a shared run).
pub(crate) fn run<B: Body>(handlers: &[Handler<B>], base: &dyn Read, e: &Entry<B>, src: Option<&Sources>) -> Result<Ran<B>, Refusal> {
    let handler = handlers.get(e.body.handler().0 as usize).ok_or(Refusal::NO_HANDLER)?;
    let mut db = Overlay::new(base);
    let mut cx = Cx {
        entry: e,
        db: &mut db,
        src,
        journal: Vec::new(),
        made: Vec::new(),
    };
    handler(&mut cx)?;
    let (made, journal) = (cx.made, cx.journal);
    Ok((db.into_writes(), made, journal))
}

/// One store for each space that keeps rows: every space but `local`,
/// whose state is cells.
pub struct Spaces<S> {
    pub device: S,
    pub session: S,
    /// The shared space's confirmed rows.
    pub shared: S,
}

impl<S> Spaces<S> {
    pub fn get(&self, space: Space) -> Option<&S> {
        match space {
            Space::Local => None,
            Space::Device => Some(&self.device),
            Space::Session => Some(&self.session),
            Space::Shared => Some(&self.shared),
        }
    }

    pub fn get_mut(&mut self, space: Space) -> Option<&mut S> {
        match space {
            Space::Local => None,
            Space::Device => Some(&mut self.device),
            Space::Session => Some(&mut self.session),
            Space::Shared => Some(&mut self.shared),
        }
    }
}

/// What the host does with an entry this node made, by its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `local` and `device`: stepped here.
    Step,
    /// `shared`: stepped here, kept as pending, and pushed upstream.
    Pending,
    /// `session`: stepped here and sent upstream, to the user's other
    /// nodes; never logged.
    Send,
}

impl Route {
    pub fn of(space: Space) -> Route {
        match space {
            Space::Local | Space::Device => Route::Step,
            Space::Shared => Route::Pending,
            Space::Session => Route::Send,
        }
    }
}

/// The way to this node's upstream: a socket, a phone, a test.
pub trait Link<B> {
    fn send(&mut self, up: Up<B>);
}

/// What is to go upstream, held: a link a host or a test drains.
impl<B> Link<B> for Vec<Up<B>> {
    fn send(&mut self, up: Up<B>) {
        self.push(up);
    }
}

/// A session row a node took: the entry it took last for it, and whether
/// its upstream has said it since the node last connected.
struct Kept<B> {
    table: TableId,
    key: Vec<u8>,
    entry: Entry<B>,
    said: bool,
}

/// How many hellos' round trips a node keeps, to take its clock's offset
/// from the shortest.
const SAMPLES: usize = 4;

/// One process running the loop.
pub struct Node<B: 'static, S> {
    origin: Origin,
    made: u64,
    actor: Actor,
    version: Version,
    clock: Hlc,
    handlers: &'static [Handler<B>],
    /// Each space's rows, every version of them: the host's confirmed
    /// rows, the pending entries' writes over the shared, the rows held
    /// apart, and what each read before the settle under way.
    stores: Spaces<Versioned<S>>,
    /// The last `seq` whose facts the confirmed rows hold.
    cursor: Seq,
    /// This node's shared entries not yet confirmed, oldest first.
    pending: Vec<Pending<B>>,
    refused: Vec<(EntryId, Refusal)>,
    /// The cursor when the rows held apart began to come.
    since: Seq,
    /// Its subscriptions, each with whether its rows are all here.
    subs: Vec<(QueryId, Vec<u8>, bool)>,
    /// The session rows it took, each by its latest entry, and the
    /// origins of the downstream nodes whose rows it answers for.
    rows: Vec<Kept<B>>,
    below: Vec<Origin>,
    /// The last count judged of each origin it answers for below it, as
    /// the log and its fills said: what it fills its downstream with.
    #[cfg(feature = "upstream")]
    judged: Vec<(Origin, u64)>,
    /// Whether its link upstream is up, and whether the answer to its
    /// hello is still coming.
    linked: bool,
    greeting: bool,
    /// The round trip and the offset of each of the last hellos answered,
    /// the next to replace, and the offset of the shortest.
    samples: [(u64, i64); SAMPLES],
    sample: usize,
    offset: i64,
    /// Its cells, watches and readers.
    pub(crate) sources: Sources,
    /// How many confirmations ran nothing again: what the tests see of a
    /// skipped replay.
    #[cfg(any(test, ark_selftest))]
    pub(crate) skipped: u32,
}

impl<B: Body + Clone + 'static, S: Store> Node<B, S> {
    /// A node of `origin`, which has made `made` entries before (the host
    /// keeps the count), running `handlers` over `stores`, its shared
    /// confirmed rows empty.
    pub fn new(origin: Origin, made: u64, version: Version, handlers: &'static [Handler<B>], stores: Spaces<S>) -> Node<B, S> {
        Node {
            origin,
            made,
            actor: Actor::NONE,
            version,
            clock: Hlc::default(),
            handlers,
            stores: Spaces {
                device: Versioned::new(stores.device),
                session: Versioned::new(stores.session),
                shared: Versioned::new(stores.shared),
            },
            cursor: Seq(0),
            pending: Vec::new(),
            refused: Vec::new(),
            since: Seq(0),
            subs: Vec::new(),
            rows: Vec::new(),
            below: Vec::new(),
            #[cfg(feature = "upstream")]
            judged: Vec::new(),
            linked: false,
            greeting: false,
            samples: [(u64::MAX, 0); SAMPLES],
            sample: 0,
            offset: 0,
            sources: Sources::default(),
            #[cfg(any(test, ark_selftest))]
            skipped: 0,
        }
    }

    /// Puts back what the host kept: the confirmed rows (in `stores`) are
    /// the log's facts up to `cursor`, and `pending` the records of this
    /// node's entries not confirmed then. Their writes are put back as they
    /// were, not run again.
    pub fn resume(&mut self, cursor: Seq, pending: Vec<Pending<B>>) {
        self.cursor = cursor;
        let mut layer = Layer::new();
        for p in &pending {
            layer.apply(&p.writes);
        }
        self.stores.shared.resume(layer);
        self.pending = pending;
    }

    /// What changed in a space as this node reads it ([`Node::view`])
    /// since this was last called for it, its rows as they read now.
    #[inline(never)]
    pub fn take_changes(&mut self, space: Space) -> Changes {
        self.stores.get_mut(space).map(Versioned::take_changes).unwrap_or_default()
    }

    /// Who the entries this node makes are from.
    pub fn set_actor(&mut self, actor: Actor) {
        self.actor = actor;
    }

    /// Where this node's entries are made.
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// How many entries this node's origin has made: for the host to keep.
    pub fn made(&self) -> u64 {
        self.made
    }

    /// The node's clock: the latest time it has made or seen.
    pub fn clock(&self) -> Hlc {
        self.clock
    }

    /// The last `seq` the shared space's confirmed rows hold.
    pub fn cursor(&self) -> Seq {
        self.cursor
    }

    /// The records of the shared entries not yet confirmed, oldest first:
    /// for the host to keep.
    pub fn pending(&self) -> &[Pending<B>] {
        &self.pending
    }

    /// A space's store: for `shared`, its confirmed rows alone; none for
    /// `local`, whose state is cells.
    pub fn store(&self, space: Space) -> Option<&S> {
        self.stores.get(space).map(Versioned::base)
    }

    /// A space as this node reads it: for `shared`, the confirmed rows
    /// with the pending entries' writes over them.
    #[inline(never)]
    pub fn view(&self, space: Space) -> View<'_> {
        match self.stores.get(space) {
            Some(s) => s.now(),
            None => View {
                base: &Nothing,
                layer: &EMPTY,
            },
        }
    }

    /// A space's rows: none for `local`.
    fn rows(&self, space: Space) -> &dyn Read {
        match self.stores.get(space) {
            Some(s) => s,
            None => &Nothing,
        }
    }

    /// Every space's changes told.
    fn told(&mut self) {
        self.stores.device.told();
        self.stores.session.told();
        self.stores.shared.told();
    }

    /// The rows as they were before the settle under way, and the cells:
    /// what a plan reads as `was` ([`crate::plan::Settle`]).
    fn was<'a>(&'a self, rows: &'a [crate::store::Before<'a>; 3]) -> Scope<'a> {
        Scope {
            rows: [&rows[0], &rows[1], &rows[2]],
            src: &self.sources,
            was: true,
        }
    }

    /// What its plans and readers read: every space's rows as it
    /// reads them, the shared through `shared` (its view), and its sources.
    #[inline(never)]
    fn scope<'a>(&'a self, shared: &'a View<'a>, was: bool) -> Scope<'a> {
        Scope {
            rows: [&self.stores.device, &self.stores.session, shared],
            src: &self.sources,
            was,
        }
    }

    /// A new cell holding `v`.
    pub fn cell<T: PartialEq + 'static>(&mut self, v: T) -> Cell<T> {
        self.sources.cell(v)
    }

    /// Keeps a plan's answer live, over every space's rows and the node's
    /// cells, pushed at every settle in the order watched: the plan
    /// `query` makes with `args`, made once. Asked again for the same pair,
    /// as a view asks each time it is built, it answers the same watch: a
    /// function that returns a plan is a node of the graph, kept by its
    /// query and its arguments.
    pub fn watch(&mut self, query: u32, args: &[u8], plan: impl FnOnce() -> Plan) -> Watch {
        if let Some(l) = self.sources.lives.iter().find(|l| l.1 .0 == query && l.1 .1 == args) {
            return Watch(l.0);
        }
        let live = {
            let shared = self.view(Space::Shared);
            plan().live(&self.scope(&shared, false))
        };
        let id = self.sources.slots.add(None);
        self.sources.lives.push((id, (query, args.to_vec()), live));
        Watch(id)
    }

    /// A watch's answer, as it stands, unrecorded.
    pub fn answer(&self, w: Watch) -> Option<&Live> {
        self.sources.live(w)
    }

    /// A new reader, for a sink's leaf: it has read nothing yet.
    pub fn reader(&mut self) -> Reader {
        self.sources.readers.push(Vec::new());
        Reader(self.sources.readers.len() as u32 - 1)
    }

    /// Runs `f` as `reader` pulls: what it reads of rows, cells and
    /// watches, through whatever functions it calls, is recorded in place
    /// of what it read last, and the settle tells its sink when any of it
    /// changes. A keyed watch it asked for that the node did not keep yet
    /// ([`crate::source`]) is made, and `f` run again over it.
    pub fn read<R>(&mut self, reader: Reader, mut f: impl FnMut(&dyn Read) -> R) -> R {
        loop {
            let (out, read) = {
                let shared = self.view(Space::Shared);
                let scope = self.scope(&shared, false);
                let r = Recorder::new(&scope, true);
                let out = f(&r);
                (out, r.spans.map(core::cell::RefCell::into_inner).unwrap_or_default())
            };
            if !self.watch_wanted() {
                if let Some(r) = self.sources.readers.get_mut(reader.0 as usize) {
                    *r = read;
                }
                return out;
            }
        }
    }

    /// Makes the keyed watches a read asked for, if it asked for any,
    /// forgetting what was shared over their absence.
    #[inline(never)]
    fn watch_wanted(&mut self) -> bool {
        let wanted = core::mem::take(self.sources.wanted.get_mut());
        if wanted.is_empty() {
            return false;
        }
        for (key, args, plan) in wanted {
            self.watch(key, &args, || plan);
        }
        self.sources.calls.get_mut().clear();
        true
    }

    /// [`Node::read`] for a caller with many kinds of reader (a toolkit's
    /// sink): one copy linked, whatever `f` is.
    #[inline(never)]
    pub fn record(&mut self, reader: Reader, f: &mut dyn FnMut(&dyn Read)) {
        self.read(reader, |db| f(db));
    }

    /// A cell's value now, unrecorded.
    pub fn get<T: 'static>(&self, c: Cell<T>) -> Option<&T> {
        self.sources.slots.get(c.0, false)
    }

    /// How many times a cell's value has changed: what a bridge to a
    /// toolkit's own observation compares.
    pub fn version<T>(&self, c: Cell<T>) -> u32 {
        self.sources.slots.version(c.0)
    }

    /// Writes a cell from outside a step (the host's link, the time): the
    /// next settle tells its readers, unless the value is equal.
    pub fn set<T: PartialEq + 'static>(&mut self, c: Cell<T>, v: T) {
        self.sources.write(c.0, Boxed::new(v));
    }

    /// Ends a step or a receive: each space's changes go to the sink (what a
    /// disk keeps), then through every watch, in the order watched, each
    /// patch said; then each reader whose reads changed (a row, a cell, a
    /// watch's answer) is told, once. Nothing is worked out but the plans:
    /// a reader pulls when it next draws.
    // Not inlined: the watch links one copy (checks.kernel-size).
    #[inline(never)]
    pub fn settle(&mut self, sink: &mut dyn Sink) {
        // What changed, kept until the plans have read the rows as they
        // were.
        let mut rows = Changes::default();
        for space in [Space::Device, Space::Session, Space::Shared] {
            let c = self.stores.get(space).map(Versioned::changes).unwrap_or_default();
            sink.changes(space, &c);
            rows.reset |= c.reset;
            rows.rows.extend(c.rows);
        }
        self.sources.begin();
        let keys = &mut self.sources.keys;
        keys.extend(rows.rows.iter().map(|(t, d)| (*t, d.key.clone())));
        if keys.is_empty() && !rows.reset {
            self.told();
            return self.sources.end();
        }
        let mut lives = core::mem::take(&mut self.sources.lives);
        let mut changed = Vec::new();
        {
            let shared = self.view(Space::Shared);
            let before = [self.stores.device.before(), self.stores.session.before(), self.stores.shared.before()];
            let (now, was) = (self.scope(&shared, false), self.was(&before));
            for (id, _, live) in &mut lives {
                let patches = live.push_over(&now, &was, &rows);
                if !patches.is_empty() {
                    changed.push(key(*id));
                    sink.patches(Watch(*id), &patches);
                }
            }
        }
        self.sources.lives = lives;
        self.told();
        self.sources.keys.append(&mut changed);
        for (i, read) in self.sources.readers.iter().enumerate() {
            if rows.reset || hit(read, &self.sources.keys) {
                sink.dirty(Reader(i as u32));
            }
        }
        self.sources.end();
    }

    /// The entries refused since the last call, oldest first.
    pub fn take_refused(&mut self) -> Vec<(EntryId, Refusal)> {
        core::mem::take(&mut self.refused)
    }

    /// The entries refused and not yet taken, oldest first: for a host to
    /// see whether the entry it just routed was.
    pub fn refusals(&self) -> &[(EntryId, Refusal)] {
        &self.refused
    }

    /// Whether its link upstream is up: from [`Node::connect`] to
    /// [`Node::disconnect`].
    pub fn linked(&self) -> bool {
        self.linked
    }

    /// Answers for the session rows `origin` owns as for its own: a
    /// downstream node's it serves, which its upstream reaches only
    /// through it ([`crate::Relay`] says so at the node's hello).
    #[cfg(feature = "upstream")]
    pub fn answer_for(&mut self, origin: Origin) {
        if !self.below.contains(&origin) {
            self.below.push(origin);
        }
    }

    /// Whether it answers for a session row: one it or a node below it
    /// owns.
    fn owns(&self, key: &[u8]) -> bool {
        self.origin.owns(key) || self.below.iter().any(|o| o.owns(key))
    }

    /// The latest session entry it took for each row.
    pub fn latest(&self) -> impl Iterator<Item = &Entry<B>> {
        self.rows.iter().map(|k| &k.entry)
    }

    /// How far the tree's clock (its root's: the server's, or a phone's
    /// with none) is ahead of this node's, in milliseconds, as its hellos
    /// measured it: 0 until one is answered.
    pub fn offset(&self) -> i64 {
        self.offset
    }

    /// The tree's time at this node's time `now`: what an anchor's owner
    /// writes and a view extrapolates it at.
    pub fn time(&self, now: u64) -> u64 {
        now.wrapping_add_signed(self.offset)
    }

    /// A new entry of this node's, at wall time `now`: a tap, a timer.
    // Not inlined: the watch links one copy (checks.kernel-size).
    #[inline(never)]
    pub fn make(&mut self, space: Space, body: B, now: u64) -> Entry<B> {
        self.entry(space, body, self.actor, None, now)
    }

    /// An entry `cause`'s handler emitted, made this node's.
    fn made_by(&mut self, cause: &Entry<B>, (space, body): (Space, B)) -> Entry<B> {
        self.entry(space, body, cause.actor, Some(cause.id), self.clock.wall)
    }

    /// The entries `cause`'s handler emitted, made this node's.
    fn made_all(&mut self, cause: &Entry<B>, made: Vec<(Space, B)>) -> Vec<Entry<B>> {
        let mut out = Vec::new();
        for m in made {
            out.push(self.made_by(cause, m));
        }
        out
    }

    /// The next entry of this node's origin, at wall time `now`.
    fn entry(&mut self, space: Space, body: B, actor: Actor, cause: Option<EntryId>, now: u64) -> Entry<B> {
        self.clock = self.clock.tick(now);
        self.made += 1;
        Entry {
            id: self.next_id(),
            space,
            actor,
            at: self.clock,
            cause,
            version: self.version,
            seq: None,
            body,
        }
    }

    fn next_id(&self) -> EntryId {
        EntryId {
            origin: self.origin,
            n: self.made,
        }
    }

    /// Runs `e`'s handler in a transaction on its space, and returns the
    /// entries it made. A refusal returns none and is journaled, except a
    /// shared entry's: that stays pending for the server to judge.
    pub fn step(&mut self, e: Entry<B>) -> Vec<Entry<B>> {
        // The node has no clock of its own: an entry's time is when it is
        // taken in.
        self.clock = self.clock.witness(e.at, e.at.wall);
        if e.space == Space::Shared {
            return self.propose(e);
        }
        match run(self.handlers, self.rows(e.space), &e, Some(&self.sources)) {
            Ok((writes, made, journal)) => {
                if let Some(s) = self.stores.get_mut(e.space) {
                    s.confirm(&writes);
                }
                for (id, v) in journal {
                    self.sources.write(id, v);
                }
                self.made_all(&e, made)
            }
            Err(why) => {
                self.refused.push((e.id, why));
                Vec::new()
            }
        }
    }

    /// Runs this node's own shared entry on the shared view and keeps it
    /// pending.
    fn propose(&mut self, e: Entry<B>) -> Vec<Entry<B>> {
        if e.id.origin != self.origin || e.seq.is_some() {
            self.refused.push((e.id, Refusal::NOT_OWN));
            return Vec::new();
        }
        let (writes, made, _) = run(self.handlers, &self.stores.shared, &e, None).unwrap_or_default();
        self.stores.shared.propose(&writes);
        let made = self.made_all(&e, made);
        self.pending.push(Pending { entry: e, writes });
        made
    }

    /// Routes an entry this node made, and what stepping it makes, until
    /// nothing is left to step: in order, each `local` and `device` entry
    /// stepped here, each `shared` one stepped and pushed, each `session`
    /// one stepped and sent.
    // Not inlined: the watch links one copy (checks.kernel-size).
    #[inline(never)]
    pub fn route(&mut self, e: Entry<B>, link: &mut dyn Link<B>) {
        self.run(Vec::from([e]), link);
    }

    fn run(&mut self, mut queue: Vec<Entry<B>>, link: &mut dyn Link<B>) {
        // First in, first out: a queue is a few entries, which a `Vec`
        // shifts with code the kernel links anyway.
        while !queue.is_empty() {
            let e = queue.remove(0);
            match Route::of(e.space) {
                Route::Step => queue.extend(self.step(e)),
                Route::Pending => {
                    let made = self.step(e.clone());
                    link.send(Up::Push(e));
                    queue.extend(made);
                }
                Route::Send => {
                    let own = e.body.row().is_some_and(|(_, key)| self.owns(&key));
                    if !self.linked && !own {
                        self.refused.push((e.id, Refusal::OFFLINE));
                        continue;
                    }
                    let made = self.step(e.clone());
                    self.keep(&e);
                    link.send(Up::Live(e));
                    queue.extend(made);
                }
            }
        }
    }

    /// Notes the entry a session row was last set by.
    fn keep(&mut self, e: &Entry<B>) {
        let Some((table, key)) = e.body.row() else { return };
        match self.rows.iter_mut().find(|k| k.table == table && k.key == key) {
            Some(k) => {
                k.entry = e.clone();
                k.said = true;
            }
            None => self.rows.push(Kept {
                table,
                key,
                entry: e.clone(),
                said: true,
            }),
        }
    }

    /// Takes another node's session entry, from its upstream (`upstream`)
    /// or a downstream node it serves, and routes what it makes: whether it
    /// took it. A row it does not own is its upstream's: it takes what
    /// comes, its own said back too. A row it owns takes only a newer entry,
    /// and none from its hello's answer, which says it only to show what
    /// its upstream has.
    pub(crate) fn take_live(&mut self, e: Entry<B>, upstream: bool, link: &mut dyn Link<B>) -> bool {
        self.clock = self.clock.witness(e.at, e.at.wall);
        let Some((table, key)) = e.body.row() else {
            // Its own, said back to it, was stepped when it was made.
            if e.id.origin == self.origin {
                return false;
            }
            let made = self.step(e);
            self.run(made, link);
            return true;
        };
        if self.owns(&key) {
            let kept = self.rows.iter().find(|k| k.table == table && k.key == key);
            // Its hello's answer says it only to show what its upstream
            // has, unless it holds none (it started again): then its
            // upstream's is its own.
            if kept.is_some_and(|k| (upstream && self.greeting) || (k.entry.at, k.entry.id) >= (e.at, e.id)) {
                return false;
            }
        }
        self.keep(&e);
        let made = self.step(e);
        self.run(made, link);
        true
    }

    /// Its upstream's clock, sent at the tree's time `time` in answer to a
    /// hello or a ping said at this node's `sent`, taken at `now`; and the
    /// end of the answer to its hello. Of its session rows, one it does
    /// not own that the answer did not say is gone upstream, and goes;
    /// every one it owns is said again, newer than anything its upstream
    /// kept while it was away.
    fn welcome(&mut self, sent: u64, time: u64, now: u64, link: &mut dyn Link<B>) {
        if let Some(trip) = now.checked_sub(sent) {
            // NTP's estimate, the answer taken as made halfway: the offset
            // of the shortest round trip of the last few is the least wrong.
            let offset = (time as i64).wrapping_add((trip / 2) as i64).wrapping_sub(now as i64);
            self.samples[self.sample] = (trip, offset);
            self.sample = (self.sample + 1) % SAMPLES;
            self.offset = self.samples.iter().min_by_key(|s| s.0).map_or(0, |s| s.1);
        }
        if !core::mem::take(&mut self.greeting) {
            return;
        }
        let mut i = 0;
        while let Some(k) = self.rows.get(i) {
            let (said, own) = (k.said, self.owns(&k.key));
            if own {
                self.clock = self.clock.tick(self.clock.wall);
                self.made += 1;
                let id = self.next_id();
                let k = &mut self.rows[i];
                k.entry.cause = Some(k.entry.id);
                k.entry.id = id;
                k.entry.at = self.clock;
                link.send(Up::Live(k.entry.clone()));
                i += 1;
            } else if said {
                i += 1;
            } else {
                let k = self.rows.swap_remove(i);
                let gone = Write {
                    table: k.table,
                    key: k.key,
                    value: None,
                };
                self.stores.session.confirm(&Writes(Vec::from([gone])));
            }
        }
    }

    /// Holds the rows a query reads with `args`, from now and at every
    /// connection.
    pub fn subscribe(&mut self, query: QueryId, args: Vec<u8>, link: &mut dyn Link<B>) {
        if !self.subs.iter().any(|s| s.0 == query && s.1 == args) {
            self.subs.push((query, args.clone(), false));
            link.send(Up::Subscribe { query, args });
        }
    }

    /// Lets go of a subscription; its upstream moves out what no other
    /// holds.
    pub fn unsubscribe(&mut self, query: QueryId, args: Vec<u8>, link: &mut dyn Link<B>) {
        let before = self.subs.len();
        self.subs.retain(|s| s.0 != query || s.1 != args);
        if self.subs.len() < before {
            link.send(Up::Unsubscribe { query, args });
        }
    }

    /// Whether a subscription's rows are all here, and taken.
    pub fn complete(&self, query: QueryId, args: &[u8]) -> bool {
        self.stores.shared.holds_none() && self.subs.iter().any(|s| s.0 == query && s.1 == args && s.2)
    }

    /// Its link upstream is up, at this node's time `now`: what it says at
    /// each connection is its subscriptions, its hello (its cursor and
    /// `now`), then every pending entry again, oldest first. The upstream
    /// judges each once. A fill the last connection left unfinished, and
    /// rows held for queries not got, are let go: the hello says the cursor
    /// from before they came. Session entries for any row are sent from now.
    // Not inlined: the watch links one copy (checks.kernel-size).
    #[inline(never)]
    pub fn connect(&mut self, link: &mut dyn Link<B>, now: u64) {
        self.linked = true;
        self.greeting = true;
        for k in &mut self.rows {
            k.said = false;
        }
        let cursor = if self.stores.shared.holds_none() { self.cursor } else { self.since };
        self.stores.shared.take_held();
        for (query, args, got) in &mut self.subs {
            *got = false;
            link.send(Up::Subscribe {
                query: *query,
                args: args.clone(),
            });
        }
        link.send(Up::Hello { cursor, time: now });
        for p in &self.pending {
            link.send(Up::Push(p.entry.clone()));
        }
    }

    /// Measures its offset to its upstream again, at this node's time `now`:
    /// what a node connected for long says now and then, so that it does
    /// not drift. Nothing while it has no link.
    pub fn ping(&self, link: &mut dyn Link<B>, now: u64) {
        if self.linked {
            link.send(Up::Ping { time: now });
        }
    }

    /// Its link upstream went down: what it says is lost until it connects
    /// again, so session entries for rows it does not own are refused.
    pub fn disconnect(&mut self) {
        self.linked = false;
        self.greeting = false;
    }

    /// Takes in what the upstream sent at this node's time `now`, and
    /// routes what it makes.
    // Not inlined: the watch links one copy (checks.kernel-size).
    #[inline(never)]
    pub fn receive(&mut self, down: Down<B>, link: &mut dyn Link<B>, now: u64) {
        match down {
            Down::Logged(l) => self.confirm(l),
            Down::Live(e) => {
                self.take_live(e, true, link);
            }
            Down::Welcome { sent, time } => self.welcome(sent, time, now, link),
            Down::Rows { rows, end } => {
                self.hold(&rows);
                match end {
                    End::More => {}
                    End::Moved => self.flush(),
                    End::Fill { seq, whole, judged } => self.filled(seq, whole, &judged),
                    End::Got { query, args } => {
                        for s in &mut self.subs {
                            s.2 |= s.0 == query && s.1 == args;
                        }
                        self.flush();
                    }
                }
            }
        }
    }

    /// Holds rows apart until they are all here.
    fn hold(&mut self, rows: &Writes) {
        if self.stores.shared.holds_none() {
            self.since = self.cursor;
        }
        self.stores.shared.hold(rows);
    }

    /// Takes the rows held, once every query this node wants is got.
    fn flush(&mut self) {
        if self.stores.shared.holds_none() || self.subs.iter().any(|s| !s.2) {
            return;
        }
        let rows = self.stores.shared.take_held();
        self.stores.shared.confirm(&rows);
        self.rebase();
    }

    /// Ends a fill whose every part has come: its rows taken at once (in
    /// place of every row, when whole), the node is at its `seq`.
    /// Its pending entries up to the count it says for this node's origin
    /// are judged: those refused are journaled by their rows, the rest are
    /// in these rows.
    fn filled(&mut self, seq: Seq, whole: bool, judged: &[(Origin, u64)]) {
        if whole {
            // The refusal rows it keeps for the nodes below it are theirs
            // until they have seen them, whatever its upstream let go (a
            // node that serves none keeps none).
            #[cfg(feature = "upstream")]
            let mut kept = Vec::new();
            #[cfg(feature = "upstream")]
            self.stores.shared.base().scan(REFUSALS, crate::store::Range::all(), &mut |k, v| {
                kept.push(Write {
                    table: REFUSALS,
                    key: k.to_vec(),
                    value: Some(v.to_vec()),
                });
                true
            });
            self.stores.shared.clear();
            #[cfg(feature = "upstream")]
            self.stores.shared.confirm(&Writes(kept));
        }
        let mut rows = self.stores.shared.take_held();
        self.take_refusals(&mut rows.0);
        self.stores.shared.confirm(&rows);
        self.stores.shared.reset();
        self.cursor = seq;
        let origin = self.origin;
        for &(o, n) in judged {
            if o == origin {
                self.pending.retain(|p| p.entry.id.n > n);
            }
            #[cfg(feature = "upstream")]
            self.judge(o, n);
        }
        self.rebase();
    }

    /// The refusal rows among `rows`, as this node keeps them: its own
    /// taken, each of a pending entry let go and journaled, and never kept;
    /// those of a node it answers for kept, for that node; any other's let
    /// go.
    fn take_refusals(&mut self, rows: &mut Vec<Write>) {
        let (origin, below, mut refused) = (self.origin, &self.below, Vec::new());
        rows.retain(|w| {
            let Some(id) = (w.table == REFUSALS).then(|| Refused::id(&w.key)) else {
                return true;
            };
            match id {
                Some(id) if id.origin == origin => {
                    // Its first field is why: all this node reads of it.
                    let why = w.value.as_deref().and_then(|v| Refusal::decode(&mut crate::codec::Reader::new(v)).ok());
                    refused.extend(why.map(|why| (id, why)));
                    false
                }
                Some(id) => below.contains(&id.origin),
                None => false,
            }
        });
        for (id, why) in refused {
            if let Some(i) = self.pending.iter().position(|p| p.entry.id == id) {
                self.pending.remove(i);
                self.refused.push((id, why));
            }
        }
    }

    /// Notes that `origin`, below it, is judged up to `n`.
    #[cfg(feature = "upstream")]
    fn judge(&mut self, origin: Origin, n: u64) {
        if origin == self.origin || !self.below.contains(&origin) {
            return;
        }
        match self.judged.iter_mut().find(|j| j.0 == origin) {
            Some(j) => j.1 = j.1.max(n),
            None => self.judged.push((origin, n)),
        }
    }

    /// The last count judged of `origin`'s, one below it, as the log and its
    /// fills said.
    #[cfg(feature = "upstream")]
    pub fn judged(&self, origin: Origin) -> u64 {
        self.judged.iter().find(|j| j.0 == origin).map_or(0, |j| j.1)
    }

    /// Lets go of the refusal rows it keeps for `origin`, below it, that
    /// were logged at or before `seq`: its hello says it has applied them.
    #[cfg(feature = "upstream")]
    pub fn forget(&mut self, origin: Origin, seq: Seq) {
        let mut gone = Vec::new();
        self.stores.shared.base().scan(REFUSALS, crate::store::Range::all(), &mut |k, v| {
            let mine = Refused::id(k).is_some_and(|id| id.origin == origin);
            if mine && crate::codec::from_slice::<Refused>(v).is_ok_and(|r| r.seq <= seq) {
                gone.push(Write {
                    table: REFUSALS,
                    key: k.to_vec(),
                    value: None,
                });
            }
            true
        });
        if gone.is_empty() {
            return;
        }
        self.stores.shared.confirm(&Writes(gone));
    }

    /// Applies the log's next entry by its facts, lets go of what it
    /// confirms, and rebases. An entry this node has, or one past a gap
    /// (which the next connection's hello fills), is passed over.
    fn confirm(&mut self, l: Logged<B>) {
        let Some(seq) = l.entry.seq.filter(|s| s.0 == self.cursor.0 + 1) else {
            return;
        };
        self.clock = self.clock.witness(l.entry.at, l.entry.at.wall);
        // A refusal's row is taken, and a fact for a row held apart is
        // made there, to be taken with it.
        let mut facts = l.facts;
        self.take_refusals(&mut facts.0);
        #[cfg(feature = "upstream")]
        self.judge(l.entry.id.origin, l.entry.id.n);
        let shared = &mut self.stores.shared;
        facts.0.retain(|w| !shared.update_held(w));
        shared.confirm(&facts);
        self.cursor = seq;
        let id = l.entry.id;
        let previewed = self.pending.first().is_some_and(|p| p.entry.id == id && p.writes == facts);
        let before = self.pending.len();
        self.pending.retain(|p| p.entry.id != id);
        if previewed && self.pending.len() + 1 == before {
            // The server wrote what this node's run did, so the confirmed
            // rows are what the entries after it ran over: their writes
            // stand, and only the layer is made again without it, which
            // reads the same.
            let mut layer = Layer::new();
            for p in &self.pending {
                layer.apply(&p.writes);
            }
            self.stores.shared.repend(layer);
            #[cfg(any(test, ark_selftest))]
            {
                self.skipped += 1;
            }
        } else {
            self.rebase();
        }
    }

    /// Runs every pending entry again, in order, over the confirmed rows.
    fn rebase(&mut self) {
        let mut layer = Layer::new();
        for p in &mut self.pending {
            let view = View {
                base: self.stores.shared.base(),
                layer: &layer,
            };
            p.writes = run(self.handlers, &view, &p.entry, None).map(|(writes, ..)| writes).unwrap_or_default();
            layer.apply(&p.writes);
        }
        self.stores.shared.repend(layer);
    }
}

/// The tests' app, shared by the node's, the authority's and the
/// convergence tests.
#[cfg(any(test, ark_selftest))]
pub(crate) mod app {
    use super::*;
    use crate::store::{Memory, TableId};

    pub(crate) const COUNTS: TableId = TableId(1);
    /// A session table: a position per key, set whole.
    pub(crate) const POSITIONS: TableId = TableId(2);
    /// Notes on a count: rows keyed by the count's key, then the note's;
    /// what a query's closures read, and its counts' children.
    pub(crate) const NOTES: TableId = TableId(3);

    /// A position's row: its key after its owner's origin, which is the
    /// node of origin 1 for keys 0 to 9, 2 for 10 to 19, and so on.
    pub(crate) fn at(key: u8) -> Vec<u8> {
        let mut row = Origin(u64::from(key / 10) + 1).key().to_vec();
        row.push(key);
        row
    }

    /// A counter app: `Add` adds to a count, `Take` takes one and refuses
    /// at none, `Tap` asks for a shared `Add` and a local `Seen`, `Seen`
    /// counts taps, `Refuse` writes and refuses, `Note` puts a count's note
    /// or takes it away.
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[cfg_attr(not(feature = "upstream"), allow(dead_code))]
    pub(crate) enum Msg {
        Add { key: u8, by: u64 },
        Tap { key: u8 },
        Seen,
        Refuse,
        Take { key: u8 },
        Set { key: u8, value: u64 },
        Note { key: u8, n: u8 },
        Unknown,
    }

    /// How many taps were seen: the app's one cell, the first its node
    /// makes.
    pub(crate) const SEEN: Cell<u64> = Cell::at(0);

    fn seen(cx: &mut Cx<'_, Msg>, n: u64) {
        let seen = cx.get(SEEN).copied().unwrap_or_default();
        cx.set(SEEN, seen + n);
    }

    /// What `Take` refuses with.
    pub(crate) const NONE_LEFT: Refusal = Refusal(301);

    impl Body for Msg {
        fn handler(&self) -> HandlerId {
            HandlerId(match self {
                Msg::Add { .. } => 0,
                Msg::Tap { .. } => 1,
                Msg::Seen => 2,
                Msg::Refuse => 3,
                Msg::Take { .. } => 4,
                Msg::Set { .. } => 5,
                Msg::Note { .. } => 6,
                Msg::Unknown => 9,
            })
        }

        fn row(&self) -> Option<(TableId, Vec<u8>)> {
            match self {
                Msg::Set { key, .. } => Some((POSITIONS, at(*key))),
                _ => None,
            }
        }
    }

    pub(crate) fn count(db: &dyn Read, key: u8) -> u64 {
        db.get(COUNTS, &[key]).map_or(0, |v| u64::from_le_bytes(v.try_into().unwrap()))
    }

    fn add(cx: &mut Cx<'_, Msg>, key: u8, by: u64) {
        let n = count(cx.db, key) + by;
        cx.db.put(COUNTS, &[key], &n.to_le_bytes());
    }

    pub(crate) static HANDLERS: [Handler<Msg>; 7] = [
        |cx| {
            let Msg::Add { key, by } = cx.entry.body else { unreachable!() };
            add(cx, key, by);
            Ok(())
        },
        |cx| {
            let Msg::Tap { key } = cx.entry.body else { unreachable!() };
            cx.emit(Space::Shared, Msg::Add { key, by: 1 });
            cx.emit(Space::Local, Msg::Seen);
            Ok(())
        },
        |cx| {
            seen(cx, 1);
            Ok(())
        },
        |cx| {
            add(cx, 0, 100);
            seen(cx, 100);
            cx.emit(Space::Shared, Msg::Seen);
            Err(Refusal(300))
        },
        |cx| {
            let Msg::Take { key } = cx.entry.body else { unreachable!() };
            let n = count(cx.db, key);
            if n == 0 {
                return Err(NONE_LEFT);
            }
            if n == 1 {
                cx.db.delete(COUNTS, &[key]);
            } else {
                cx.db.put(COUNTS, &[key], &(n - 1).to_le_bytes());
            }
            Ok(())
        },
        |cx| {
            let Msg::Set { key, value } = cx.entry.body else { unreachable!() };
            cx.db.put(POSITIONS, &at(key), &value.to_le_bytes());
            Ok(())
        },
        |cx| {
            let Msg::Note { key, n } = cx.entry.body else { unreachable!() };
            match cx.db.get(NOTES, &[key, n]) {
                Some(_) => cx.db.delete(NOTES, &[key, n]),
                None => cx.db.put(NOTES, &[key, n], &[n]),
            }
            Ok(())
        },
    ];

    pub(crate) fn node(origin: u64) -> Node<Msg, Memory> {
        let stores = Spaces {
            device: Memory::new(),
            session: Memory::new(),
            shared: Memory::new(),
        };
        let mut n = Node::new(Origin(origin), 0, Version(1), &HANDLERS, stores);
        n.cell(0u64);
        n
    }

    /// Every row of the shared tables a store or view holds, by its table.
    #[cfg_attr(not(feature = "upstream"), allow(dead_code))]
    pub(crate) fn shared(db: &dyn Read) -> Vec<(TableId, Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        for t in [COUNTS, NOTES] {
            db.scan(t, crate::store::Range::all(), &mut |k, v| {
                out.push((t, k.to_vec(), v.to_vec()));
                true
            });
        }
        out
    }

    /// Every count a store or view holds.
    pub(crate) fn rows(db: &dyn Read) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        db.scan(COUNTS, crate::store::Range::all(), &mut |k, v| {
            out.push((k.to_vec(), v.to_vec()));
            true
        });
        out
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::app::*;
    use super::*;
    use crate::store::Write;
    use alloc::vec;

    fn logged(e: &Entry<Msg>, seq: u64, facts: Writes) -> Down<Msg> {
        Down::Logged(Logged {
            entry: Entry {
                seq: Some(Seq(seq)),
                ..e.clone()
            },
            facts,
        })
    }

    fn part(rows: Writes, end: End) -> Down<Msg> {
        Down::Rows { rows, end }
    }

    fn count_is(key: u8, n: u64) -> Writes {
        Writes(vec![Write {
            table: COUNTS,
            key: vec![key],
            value: Some(n.to_le_bytes().to_vec()),
        }])
    }

    #[cfg_attr(test, test)]
    fn a_step_runs_on_its_space() {
        let mut n = node(1);
        let e = n.make(Space::Device, Msg::Add { key: 4, by: 2 }, 1000);
        assert_eq!(e.id, EntryId { origin: Origin(1), n: 1 });
        assert_eq!(n.step(e.clone()), []);
        assert_eq!(n.step(e), []);
        assert_eq!(count(n.store(Space::Device).unwrap(), 4), 4);
        assert_eq!(n.made(), 1);
    }

    #[cfg_attr(test, test)]
    fn a_shared_write_is_seen_at_once_and_pushed() {
        let mut n = node(1);
        n.set_actor(Actor(5));
        let tap = n.make(Space::Local, Msg::Tap { key: 7 }, 1000);
        let mut up = Vec::new();
        n.route(tap.clone(), &mut up);
        // The local `Seen` was stepped; the shared `Add` is pending, seen
        // over the confirmed rows at once, and pushed.
        assert_eq!(n.get(SEEN), Some(&1));
        assert_eq!(count(&n.view(Space::Shared), 7), 1);
        assert_eq!(count(n.store(Space::Shared).unwrap(), 7), 0);
        let [Up::Push(add)] = &up[..] else { panic!("one push: {up:?}") };
        let add = add.clone();
        assert_eq!(add.body, Msg::Add { key: 7, by: 1 });
        assert_eq!((add.cause, add.actor, add.id.n, add.seq), (Some(tap.id), Actor(5), 2, None));
        assert!(add.at > tap.at);
        assert_eq!(n.pending().len(), 1);

        // Confirmed, it is the confirmed rows' and no longer pending.
        n.receive(logged(&add, 1, count_is(7, 1)), &mut up, 0);
        assert_eq!(count(n.store(Space::Shared).unwrap(), 7), 1);
        assert_eq!(count(&n.view(Space::Shared), 7), 1);
        assert_eq!(n.pending(), []);
        assert_eq!(n.cursor(), Seq(1));
        // Again, or past a gap, it is passed over.
        n.receive(logged(&add, 1, count_is(7, 9)), &mut up, 0);
        n.receive(logged(&add, 3, count_is(7, 9)), &mut up, 0);
        assert_eq!(count(n.store(Space::Shared).unwrap(), 7), 1);
    }

    /// Another node's facts arrive under a pending entry: they are applied,
    /// never run, and the pending entry is run again over them.
    #[cfg_attr(test, test)]
    fn pending_entries_rebase_over_facts() {
        let mut a = node(1);
        let mut b = node(2);
        let mut up = Vec::new();
        let mine = a.make(Space::Shared, Msg::Add { key: 1, by: 2 }, 10);
        a.route(mine.clone(), &mut up);
        let theirs = b.make(Space::Shared, Msg::Add { key: 1, by: 5 }, 10);
        // Facts that say 5, whatever running it here would say.
        a.receive(logged(&theirs, 1, count_is(1, 5)), &mut up, 0);
        assert_eq!(count(a.store(Space::Shared).unwrap(), 1), 5);
        assert_eq!(count(&a.view(Space::Shared), 1), 7);
        assert_eq!(a.pending()[0].writes, count_is(1, 7));
        a.receive(logged(&mine, 2, count_is(1, 7)), &mut up, 0);
        assert_eq!(a.pending(), []);
        assert_eq!(rows(&a.view(Space::Shared)), rows(a.store(Space::Shared).unwrap()));
    }

    /// A refusal's row of a refused entry's: logged at its place, its one
    /// fact, as the upstream sends it.
    pub(crate) fn refusal(e: &Entry<Msg>, seq: u64, why: Refusal) -> Down<Msg> {
        let row = Refused {
            why,
            actor: e.actor,
            via: e.id.origin,
            seq: Seq(seq),
        };
        let facts = Writes(alloc::vec![Write {
            table: REFUSALS,
            key: Refused::key(e.id).to_vec(),
            value: Some(crate::codec::to_vec(&row)),
        }]);
        logged(e, seq, facts)
    }

    /// A refused entry is dropped and journaled, by its row, which is not
    /// kept; the one after it stays, rebased without it.
    #[cfg_attr(test, test)]
    fn a_refusal_drops_one_entry_only() {
        let mut n = node(1);
        let mut up = Vec::new();
        let first = n.make(Space::Shared, Msg::Add { key: 2, by: 1 }, 10);
        let second = n.make(Space::Shared, Msg::Add { key: 2, by: 10 }, 10);
        n.route(first.clone(), &mut up);
        n.route(second, &mut up);
        assert_eq!(count(&n.view(Space::Shared), 2), 11);
        n.receive(refusal(&first, 1, Refusal(300)), &mut up, 0);
        assert_eq!(n.take_refused(), [(first.id, Refusal(300))]);
        assert_eq!((n.pending().len(), n.cursor()), (1, Seq(1)));
        assert_eq!(count(&n.view(Space::Shared), 2), 10);
        assert_eq!(n.store(Space::Shared).unwrap().len(REFUSALS), 0);
        // Another node's refusal is not its own, and not kept.
        let theirs = node(2).make(Space::Shared, Msg::Take { key: 1 }, 10);
        n.receive(refusal(&theirs, 2, Refusal(301)), &mut up, 0);
        assert_eq!((n.take_refused(), n.store(Space::Shared).unwrap().len(REFUSALS)), (vec![], 0));
    }

    /// A local refusal of a shared entry keeps it pending, unwritten, for
    /// the server to judge; a step's refusal elsewhere is journaled.
    #[cfg_attr(test, test)]
    fn local_refusals() {
        let mut n = node(1);
        let mut up = Vec::new();
        let take = n.make(Space::Shared, Msg::Take { key: 3 }, 10);
        n.route(take, &mut up);
        assert_eq!((n.pending().len(), n.pending()[0].writes.0.len(), up.len()), (1, 0, 1));
        let refuse = n.make(Space::Device, Msg::Refuse, 10);
        let unknown = n.make(Space::Local, Msg::Unknown, 10);
        assert_eq!(n.step(refuse.clone()), []);
        assert_eq!(n.step(unknown.clone()), []);
        assert_eq!(rows(n.store(Space::Device).unwrap()), []);
        assert_eq!(n.get(SEEN), Some(&0));
        // Another node's shared entry is taken as facts, never run.
        let foreign = Entry {
            id: EntryId { origin: Origin(9), n: 1 },
            space: Space::Shared,
            ..refuse.clone()
        };
        assert_eq!(n.step(foreign.clone()), []);
        assert_eq!(
            n.take_refused(),
            [
                (refuse.id, Refusal(300)),
                (unknown.id, Refusal::NO_HANDLER),
                (foreign.id, Refusal::NOT_OWN)
            ]
        );
    }

    /// A fill says how far its upstream judged this node's entries: those
    /// pending up to it are let go, kept (in its rows) or refused (by their
    /// rows among them, journaled), and the rest stay.
    #[cfg_attr(test, test)]
    fn a_fill_lets_go_what_it_judged() {
        let mut n = node(1);
        let mut up = Vec::new();
        let kept = n.make(Space::Shared, Msg::Add { key: 4, by: 1 }, 10);
        let refused = n.make(Space::Shared, Msg::Take { key: 5 }, 10);
        let late = n.make(Space::Shared, Msg::Add { key: 4, by: 2 }, 10);
        for e in [&kept, &refused, &late] {
            n.route(e.clone(), &mut up);
        }
        let Down::Logged(row) = refusal(&refused, 2, NONE_LEFT) else {
            unreachable!()
        };
        let mut rows = count_is(4, 1);
        rows.0.extend(row.facts.0);
        let fill = End::Fill {
            seq: Seq(3),
            whole: true,
            judged: alloc::vec![(Origin(1), 2), (Origin(7), 9)],
        };
        n.receive(part(rows, fill), &mut up, 0);
        assert_eq!(n.take_refused(), [(refused.id, NONE_LEFT)]);
        assert_eq!(n.pending().iter().map(|p| p.entry.id).collect::<Vec<_>>(), [late.id]);
        assert_eq!(count(&n.view(Space::Shared), 4), 3);
        assert_eq!(n.store(Space::Shared).unwrap().len(REFUSALS), 0);
    }

    /// Pending records survive a restart and are put back with their
    /// writes, not run again, then pushed again at the connection.
    #[cfg_attr(test, test)]
    fn pending_records_are_put_back() {
        let mut n = node(1);
        let mut up = Vec::new();
        let e = n.make(Space::Shared, Msg::Add { key: 6, by: 1 }, 10);
        n.route(e.clone(), &mut up);
        let mut kept = n.pending().to_vec();
        // Writes the handler would not make show that they are not run.
        kept[0].writes = count_is(6, 40);
        let mut again = node(1);
        again.resume(Seq(0), kept);
        assert_eq!(count(&again.view(Space::Shared), 6), 40);
        let mut up = Vec::new();
        again.connect(&mut up, 0);
        assert_eq!(up, [Up::Hello { cursor: Seq(0), time: 0 }, Up::Push(e)]);
    }

    /// Facts that are what this node's run of its oldest pending entry
    /// wrote run nothing again; other facts rebase what is pending.
    #[cfg_attr(test, test)]
    fn a_confirmed_preview_is_not_run_again() {
        let mut n = node(1);
        let mut up = Vec::new();
        let first = n.make(Space::Shared, Msg::Add { key: 6, by: 1 }, 10);
        let second = n.make(Space::Shared, Msg::Add { key: 6, by: 2 }, 10);
        n.route(first.clone(), &mut up);
        n.route(second, &mut up);
        let mut kept = n.pending().to_vec();
        // Writes no run makes show which are run again.
        kept[1].writes = count_is(6, 40);
        n.resume(Seq(0), kept);
        n.receive(logged(&first, 1, count_is(6, 1)), &mut up, 0);
        assert_eq!((n.skipped, n.pending().len()), (1, 1));
        assert_eq!(n.pending()[0].writes, count_is(6, 40));
        assert_eq!(count(&n.view(Space::Shared), 6), 40);
        // Another node's facts: the rest is run again over them.
        let theirs = node(2).make(Space::Shared, Msg::Add { key: 6, by: 4 }, 10);
        n.receive(logged(&theirs, 2, count_is(6, 5)), &mut up, 0);
        assert_eq!(n.skipped, 1);
        assert_eq!(n.pending()[0].writes, count_is(6, 7));
        // The node's own facts unlike its preview: run again too.
        let mut m = node(3);
        let a = m.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        let b = m.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        m.route(a.clone(), &mut up);
        m.route(b, &mut up);
        m.receive(logged(&a, 1, count_is(1, 9)), &mut up, 0);
        assert_eq!((m.skipped, count(&m.view(Space::Shared), 1)), (0, 10));
    }

    /// A fill holds its parts apart; at its end it empties the tables it
    /// names and takes its rows at once, puts the node at its `seq`, lets
    /// go of what that confirms, and rebases the rest. One interrupted is
    /// let go at the next connection, the rows as they were.
    #[cfg_attr(test, test)]
    fn a_fill_replaces_the_confirmed_rows() {
        let mut n = node(1);
        let mut up = Vec::new();
        let theirs = node(2).make(Space::Shared, Msg::Add { key: 1, by: 3 }, 10);
        n.receive(logged(&theirs, 1, count_is(1, 3)), &mut up, 0);
        let mine = n.make(Space::Shared, Msg::Add { key: 2, by: 1 }, 10);
        let late = n.make(Space::Shared, Msg::Add { key: 2, by: 10 }, 10);
        n.route(mine.clone(), &mut up);
        n.route(late.clone(), &mut up);
        // Judged at 5, in the part of the log the upstream let go: the
        // fill says so.
        n.connect(&mut up, 0);
        n.take_changes(Space::Shared);
        assert_eq!((count(n.store(Space::Shared).unwrap(), 1), n.cursor()), (3, Seq(1)));
        // A fill's parts are not read until the last.
        n.receive(part(count_is(2, 1), End::More), &mut up, 0);
        assert_eq!(n.cursor(), Seq(1));
        assert_eq!(
            (count(n.store(Space::Shared).unwrap(), 1), count(n.store(Space::Shared).unwrap(), 2)),
            (3, 0)
        );
        assert_eq!(n.take_changes(Space::Shared), Changes::default());
        let fill = End::Fill {
            seq: Seq(5),
            whole: true,
            judged: alloc::vec![(Origin(1), mine.id.n)],
        };
        n.receive(part(count_is(3, 4), fill), &mut up, 0);
        assert_eq!(n.cursor(), Seq(5));
        assert!(n.take_changes(Space::Shared).reset);
        assert_eq!(
            rows(n.store(Space::Shared).unwrap()),
            [(vec![2], 1u64.to_le_bytes().to_vec()), (vec![3], 4u64.to_le_bytes().to_vec())]
        );
        assert_eq!(n.pending().len(), 1);
        assert_eq!(count(&n.view(Space::Shared), 2), 11);
        n.receive(logged(&late, 6, count_is(2, 11)), &mut up, 0);
        assert_eq!((n.pending(), n.cursor()), (&[][..], Seq(6)));

        // Interrupted: the rest of its parts are not taken after a new
        // connection, whose rows are moves, taken as they come.
        n.receive(part(count_is(7, 1), End::More), &mut up, 0);
        up.clear();
        n.connect(&mut up, 0);
        assert_eq!(up, [Up::Hello { cursor: Seq(6), time: 0 }]);
        n.receive(part(count_is(8, 1), End::Moved), &mut up, 0);
        assert_eq!(
            (
                count(n.store(Space::Shared).unwrap(), 7),
                count(n.store(Space::Shared).unwrap(), 8),
                n.cursor()
            ),
            (0, 1, Seq(6))
        );
        assert_eq!(count(n.store(Space::Shared).unwrap(), 2), 11);
        // An empty fill ends at once.
        let fill = End::Fill {
            seq: Seq(9),
            whole: true,
            judged: alloc::vec![],
        };
        n.receive(part(Writes::default(), fill), &mut up, 0);
        assert_eq!((rows(n.store(Space::Shared).unwrap()), n.cursor()), (vec![], Seq(9)));
    }

    /// A subscription's rows are held apart until every query the node
    /// wants is got, then taken at once; a fact for a row held is made
    /// there; a connection lost lets them go, and its hello asks again
    /// from the cursor before they came. Rows that move while every query
    /// is got are taken as they come.
    #[cfg_attr(test, test)]
    fn a_subscriptions_rows_come_at_once() {
        let mut n = node(1);
        let mut up = Vec::new();
        let theirs = node(2).make(Space::Shared, Msg::Add { key: 9, by: 1 }, 10);
        n.subscribe(QueryId(0), vec![], &mut up);
        n.subscribe(QueryId(1), vec![], &mut up);
        n.receive(part(count_is(1, 1), End::Moved), &mut up, 0);
        n.receive(part(count_is(2, 2), End::Moved), &mut up, 0);
        // A fact for a row held, and one for another.
        let facts = Writes(vec![count_is(2, 5).0[0].clone(), count_is(9, 1).0[0].clone()]);
        n.receive(logged(&theirs, 1, facts), &mut up, 0);
        assert_eq!(rows(n.store(Space::Shared).unwrap()), [(vec![9], 1u64.to_le_bytes().to_vec())]);
        let got = End::Got {
            query: QueryId(0),
            args: vec![],
        };
        n.receive(part(Writes::default(), got), &mut up, 0);
        // The first is got, the second not: nothing is taken yet.
        assert_eq!(n.store(Space::Shared).unwrap().len(COUNTS), 1);
        assert!(!n.complete(QueryId(0), &[]));
        n.take_changes(Space::Shared);
        n.receive(part(count_is(3, 3), End::Moved), &mut up, 0);
        let got = End::Got {
            query: QueryId(1),
            args: vec![],
        };
        n.receive(part(Writes::default(), got), &mut up, 0);
        assert!(n.complete(QueryId(0), &[]) && n.complete(QueryId(1), &[]));
        assert_eq!(
            (count(n.store(Space::Shared).unwrap(), 2), n.store(Space::Shared).unwrap().len(COUNTS)),
            (5, 4)
        );
        assert_eq!(n.take_changes(Space::Shared).rows.len(), 3);
        // Got, a move is taken as it comes.
        n.receive(part(count_is(4, 4), End::Moved), &mut up, 0);
        assert_eq!(count(n.store(Space::Shared).unwrap(), 4), 4);
        // A third, its rows lost with the connection: the hello asks from
        // the cursor they came at, though the log went on.
        n.subscribe(QueryId(2), vec![], &mut up);
        n.receive(part(count_is(5, 5), End::Moved), &mut up, 0);
        n.receive(logged(&theirs, 2, Writes::default()), &mut up, 0);
        up.clear();
        n.connect(&mut up, 0);
        assert_eq!(up[3], Up::Hello { cursor: Seq(1), time: 0 });
        assert_eq!((count(n.store(Space::Shared).unwrap(), 5), n.cursor()), (0, Seq(2)));
        assert!(!n.complete(QueryId(0), &[]));
    }

    /// A session entry is stepped where it is made and sent; another
    /// node steps it as it arrives, and its own said back is not stepped
    /// again.
    #[cfg_attr(test, test)]
    fn session_entries_are_sent_and_taken() {
        let mut a = node(1);
        let mut up = Vec::new();
        a.connect(&mut up, 0);
        up.clear();
        let e = a.make(Space::Session, Msg::Add { key: 1, by: 3 }, 10);
        a.route(e.clone(), &mut up);
        assert_eq!(up, [Up::Live(e.clone())]);
        assert_eq!(count(a.store(Space::Session).unwrap(), 1), 3);
        a.receive(Down::Live(e.clone()), &mut up, 0);
        assert_eq!(count(a.store(Space::Session).unwrap(), 1), 3);
        let mut b = node(2);
        b.receive(Down::Live(e), &mut up, 0);
        assert_eq!(count(b.store(Space::Session).unwrap(), 1), 3);
    }

    /// Each space's changes are taken on their own: a session row set
    /// here and one set by another node, each once, with what it read
    /// before; a local step's; and the shared view's apart from them.
    #[cfg_attr(test, test)]
    fn each_spaces_changes_are_taken() {
        let value = |n: u64| Some(n.to_le_bytes().to_vec());
        let change = |table, key: u8, old, new| (table, crate::plan::Diff { key: vec![key], old, new });
        let mut n = node(1);
        let mut up = Vec::new();
        let set = n.make(Space::Session, Msg::Set { key: 1, value: 5 }, 10);
        n.route(set, &mut up);
        let again = n.make(Space::Session, Msg::Set { key: 1, value: 6 }, 10);
        n.route(again, &mut up);
        let theirs = node(2).make(Space::Session, Msg::Set { key: 2, value: 9 }, 10);
        n.receive(Down::Live(theirs), &mut up, 0);
        let tap = n.make(Space::Local, Msg::Tap { key: 3 }, 10);
        n.route(tap, &mut up);
        let session = n.take_changes(Space::Session);
        let pos = |key, new| {
            (
                POSITIONS,
                crate::plan::Diff {
                    key: at(key),
                    old: None,
                    new,
                },
            )
        };
        assert_eq!(session.rows, [pos(1, value(6)), pos(2, value(9))]);
        assert!(!session.reset);
        assert_eq!(n.take_changes(Space::Local), Changes::default());
        assert_eq!(n.get(SEEN), Some(&1));
        assert_eq!(n.take_changes(Space::Shared).rows, [change(COUNTS, 3, None, value(1))]);
        // Taken, they are gone; a value set back to what it was is no
        // change.
        assert_eq!(n.take_changes(Space::Session), Changes::default());
        let back = n.make(Space::Session, Msg::Set { key: 1, value: 7 }, 10);
        n.route(back, &mut up);
        let back = n.make(Space::Session, Msg::Set { key: 1, value: 6 }, 10);
        n.route(back, &mut up);
        assert_eq!(n.take_changes(Space::Session), Changes::default());
        assert_eq!(n.take_changes(Space::Device), Changes::default());
    }

    /// Live controls don't queue: with no link upstream, a session entry
    /// for a row the node does not own is refused, neither applied nor
    /// sent; one for its own row is. Linked, both are.
    #[cfg_attr(test, test)]
    fn live_controls_dont_queue() {
        // The watch, origin 2: rows 10 to 19 are its own; the phone's,
        // origin 1, are 0 to 9.
        let mut watch = node(2);
        let mut up = Vec::new();
        let pause = watch.make(Space::Session, Msg::Set { key: 1, value: 5 }, 10);
        watch.route(pause.clone(), &mut up);
        let tap = watch.make(Space::Session, Msg::Add { key: 1, by: 1 }, 10);
        watch.route(tap.clone(), &mut up);
        assert_eq!(watch.take_refused(), [(pause.id, Refusal::OFFLINE), (tap.id, Refusal::OFFLINE)]);
        assert_eq!((up.len(), watch.take_changes(Space::Session)), (0, Changes::default()));
        let mine = watch.make(Space::Session, Msg::Set { key: 15, value: 2 }, 10);
        watch.route(mine.clone(), &mut up);
        assert_eq!(up, [Up::Live(mine)]);
        assert_eq!(
            watch.store(Space::Session).unwrap().get(POSITIONS, &at(15)),
            Some(2u64.to_le_bytes().to_vec())
        );
        // Linked, a control is applied at once and sent.
        up.clear();
        watch.connect(&mut up, 20);
        assert!(watch.linked());
        up.clear();
        let pause = watch.make(Space::Session, Msg::Set { key: 1, value: 6 }, 30);
        watch.route(pause.clone(), &mut up);
        assert_eq!((up, watch.take_refused()), (vec![Up::Live(pause)], vec![]));
        assert_eq!(
            watch.store(Space::Session).unwrap().get(POSITIONS, &at(1)),
            Some(6u64.to_le_bytes().to_vec())
        );
        // The link down: refused again, the row as it was.
        watch.disconnect();
        let mut up = Vec::new();
        let late = watch.make(Space::Session, Msg::Set { key: 1, value: 7 }, 40);
        watch.route(late.clone(), &mut up);
        assert_eq!((up.len(), watch.take_refused()), (0, vec![(late.id, Refusal::OFFLINE)]));
        assert_eq!(
            watch.store(Space::Session).unwrap().get(POSITIONS, &at(1)),
            Some(6u64.to_le_bytes().to_vec())
        );
    }

    /// The answer to a hello heals the session: a row the node does not
    /// own takes its upstream's latest (a control lost in flight is
    /// undone) and one its upstream did not say goes, unless the node set
    /// it since it connected; a row it owns takes nothing from the answer
    /// (but one it holds none of, as after a restart), and every one it
    /// holds is said again, as a new entry. After the answer, a row it owns
    /// takes only newer entries.
    #[cfg_attr(test, test)]
    fn a_hello_heals_the_session() {
        let value = |db: &dyn Read, key| db.get(POSITIONS, &at(key)).map(|v| u64::from_le_bytes(v.try_into().unwrap()));
        let mut phone = node(1);
        let mut watch = node(2);
        let mut up = Vec::new();
        watch.connect(&mut up, 0);
        let playing = phone.make(Space::Session, Msg::Set { key: 1, value: 3 }, 10);
        watch.receive(Down::Live(playing.clone()), &mut up, 10);
        // Lost with the link: a pause, a row of another's, and its own
        // two rows.
        let mut lost = Vec::new();
        for (key, value) in [(1, 5), (2, 8), (15, 1), (16, 4)] {
            let e = watch.make(Space::Session, Msg::Set { key, value }, 20);
            watch.route(e.clone(), &mut lost);
        }
        let (Up::Live(fifteen), Up::Live(sixteen)) = (lost[2].clone(), lost[3].clone()) else {
            panic!()
        };
        watch.disconnect();
        up.clear();
        watch.connect(&mut up, 100);
        assert_eq!(up.last(), Some(&Up::Hello { cursor: Seq(0), time: 100 }));
        // Set since it connected: kept whatever the answer says.
        let since = watch.make(Space::Session, Msg::Set { key: 3, value: 9 }, 101);
        watch.route(since, &mut up);
        watch.take_changes(Space::Session);
        up.clear();
        // The answer: the phone's playing, an old entry of its row 15, its
        // own latest of 16, and its row 17, which it holds none of.
        let old = Entry {
            id: EntryId { origin: Origin(3), n: 1 },
            ..sixteen.clone()
        };
        let old = Entry {
            body: Msg::Set { key: 15, value: 0 },
            ..old
        };
        let seventeen = Entry {
            id: EntryId { origin: Origin(3), n: 2 },
            body: Msg::Set { key: 17, value: 2 },
            ..sixteen.clone()
        };
        for e in [playing.clone(), old, sixteen.clone(), seventeen] {
            watch.receive(Down::Live(e), &mut up, 110);
        }
        assert_eq!(
            (
                value(watch.store(Space::Session).unwrap(), 1),
                value(watch.store(Space::Session).unwrap(), 15)
            ),
            (Some(3), Some(1))
        );
        watch.receive(Down::Welcome { sent: 100, time: 5110 }, &mut up, 120);
        let s = watch.store(Space::Session).unwrap();
        assert_eq!(
            [1, 2, 3, 15, 16, 17].map(|k| value(s, k)),
            [Some(3), None, Some(9), Some(1), Some(4), Some(2)]
        );
        let session = watch.take_changes(Space::Session);
        assert_eq!(
            session.rows.iter().map(|(_, d)| d.key.clone()).collect::<Vec<_>>(),
            [at(1), at(2), at(17)]
        );
        // Each of its rows said again, newer than anything the answer held,
        // caused by the entry it says again.
        let said: Vec<_> = up
            .iter()
            .map(|u| {
                if let Up::Live(e) = u {
                    (e.body.clone(), e.cause)
                } else {
                    panic!("{up:?}")
                }
            })
            .collect();
        assert_eq!(said.len(), 3);
        assert!(said.contains(&(Msg::Set { key: 15, value: 1 }, Some(fifteen.id))));
        let Some(Up::Live(again)) = up.last() else { panic!() };
        assert!(again.at > sixteen.at && again.id.n == watch.made());
        // The clock: half the 20 ms trip after 5110, against 120.
        assert_eq!((watch.offset(), watch.time(1000)), (5000, 6000));
        // After the answer, the phone's control of row 15: newer is taken,
        // older not.
        let mut control = phone.make(Space::Session, Msg::Set { key: 15, value: 7 }, 10);
        control.at = Hlc { wall: 1, logical: 0 };
        watch.receive(Down::Live(control.clone()), &mut up, 130);
        assert_eq!(value(watch.store(Space::Session).unwrap(), 15), Some(1));
        control.at = Hlc { wall: 200, logical: 0 };
        watch.receive(Down::Live(control), &mut up, 130);
        assert_eq!(value(watch.store(Space::Session).unwrap(), 15), Some(7));
        assert_eq!(watch.latest().count(), 5);
    }

    /// The offset is the one of the shortest round trip of the last few
    /// hellos and pings: a slow answer does not move it until the fast one
    /// is four answers old.
    #[cfg_attr(test, test)]
    fn the_clock_takes_the_shortest_trip() {
        let mut n = node(1);
        let mut up = Vec::new();
        let mut hello = |n: &mut Node<Msg, crate::store::Memory>, sent: u64, time: u64, now: u64| {
            n.connect(&mut up, sent);
            n.receive(Down::Welcome { sent, time }, &mut up, now);
            n.offset()
        };
        assert_eq!(hello(&mut n, 1000, 900, 1010), -105);
        // Slow, with an offset its trip makes wrong.
        assert_eq!(hello(&mut n, 2000, 2500, 2600), -105);
        assert_eq!(hello(&mut n, 3000, 3500, 3600), -105);
        assert_eq!(hello(&mut n, 4000, 4500, 4600), -105);
        assert_eq!(hello(&mut n, 5000, 5500, 5600), 200);
        // Connected for long, it pings: each answer is a sample, so the
        // offset follows its clock's drift without a hello.
        let mut said = Vec::new();
        n.ping(&mut said, 6000);
        assert_eq!(said, [Up::Ping { time: 6000 }]);
        for t in [6000, 7000, 8000, 9000] {
            n.receive(Down::Welcome { sent: t, time: t + 300 }, &mut up, t + 10);
        }
        assert_eq!(n.offset(), 295);
        // With no link, it says nothing.
        n.disconnect();
        said.clear();
        n.ping(&mut said, 10_000);
        assert_eq!(said, []);
    }

    #[cfg_attr(test, test)]
    fn routes() {
        assert_eq!(Space::ALL.map(Route::of), [Route::Step, Route::Step, Route::Send, Route::Pending]);
    }

    crate::listed!(
        a_step_runs_on_its_space,
        a_shared_write_is_seen_at_once_and_pushed,
        pending_entries_rebase_over_facts,
        a_refusal_drops_one_entry_only,
        local_refusals,
        a_fill_lets_go_what_it_judged,
        pending_records_are_put_back,
        a_confirmed_preview_is_not_run_again,
        a_fill_replaces_the_confirmed_rows,
        session_entries_are_sent_and_taken,
        each_spaces_changes_are_taken,
        live_controls_dont_queue,
        a_hello_heals_the_session,
        the_clock_takes_the_shortest_trip,
        a_subscriptions_rows_come_at_once,
        routes,
    );
}
