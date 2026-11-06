//! §1.7 One peer machine: a log, a store, the functions it can run, its
//! own intents pending, and a role.
//!
//! ```text
//! Role  = Replica | Authority { journal, feed }
//! Input = Frame { from, frame } | Mutate { fn, input, now, cause } | Open(records) | Connected | Read
//! Output= Send(Back | All, frame) | Persist(record) | Changes(by span) | Settled(id, Ok | Err(why))
//! ```
//!
//! Sans-io: [`Peer::step`] takes one input and says what to do — no
//! socket, clock or file is inside. A hub, a client, a simulator and a
//! trace player are drivers that feed it and route what it says, as
//! values: bytes are the driver's, at its socket and its disk. A frame
//! `Send` back where the input came from (a replica's upstream), or to
//! every connection, each through its feed ([`Feed::fed`]); and a record
//! to keep, written before anything after it is sent ([`Record`]): a row
//! appended to the store's `_log` table, which is the log (§1.6) and holds
//! the entry's facts, or the store's rows whole — and, beside them, a
//! replica's pending intents as they move, an authority's journal whole.
//! `from` on a frame is the identity the driver verified for its
//! connection; an authority judges and stamps by it.
//!
//! **Effects are values.** A function (its domain's, §1.16) reads the store and returns its
//! changes; the machine applies them. A replica runs code for its own
//! pending intents only (that is the optimistic view) and applies every
//! confirmed entry by its facts, each a row as it was and as it is: facts
//! that do not apply to what a replica holds begin its fill again, from
//! nothing.
//!
//! **The rebase.** Confirmed entries arriving under pending intents: the
//! pending changes are undone, newest-first, only as far as they must be —
//! an entry that is the oldest pending intent as it was previewed (a
//! pending intent is a `Logged` at sequence 0, its preview its facts) is in
//! the store already, and is taken off the front — and at the first that
//! is not, each entry's facts are applied, and the intents still pending
//! run again over the new store, once for the page, their new changes
//! kept. A view is told every change made, rebases included, as
//! `Output::Changes`.
//!
//! **The authority** judges each pushed intent once — dedupe by `(origin,
//! local)`, a gap in a replica's pushes waits for them in order — stamps it
//! with the actor, session and roles of the connection it judged it on,
//! runs it, and logs it with its facts. An origin is bound to the actor
//! whose intents first came from it; another's pushed from it is refused. It runs exactly the functions its program
//! was built with, by name (§1.16): nothing it is sent is code. **Every
//! push is answered**, by one rule: refused, or where its origin stands
//! (`At { seq, last }`: the last `local` of it judged, and the head). Only
//! the next of its origin is judged; one at or below `last` is in the log,
//! and one past a gap, or held, is not judged — its `local` not passed, so
//! one pushed after it is not judged either. A device's intent at or below
//! `last` is confirmed. A push says where its device's pending intents
//! begin (its `from`, the `local` before the first): the ones of
//! its origin up to there it no longer holds — its disk lost them, or the
//! authority's log is older than the one that judged them — and they are
//! passed, so a device is never stuck behind an intent nobody holds, and
//! an intent's number, and the ids it draws, never change.
//!
//! **A schema grows** (§1.2). A peer's store reads the rows and facts of
//! any schema its own grows from or into ([`crate::schema::Table::fit`],
//! [`Store::apply_facts`]), and a function its inputs
//! ([`crate::json::fit`]), so a replica of an older or a newer build than
//! its authority syncs with it. A replica says its schema at each
//! connection ([`Frame::Schema`]) and names it in each `Hello` and push;
//! its authority serves one its own grows from or into ([`Stance`]), and
//! answers one that is neither with [`Frame::Update`] — it is to update,
//! asks nothing more and takes nothing — and holds its pushes. An intent
//! naming a function the authority does not run is held from a newer
//! build (its server is to update), and refused as needing an update from
//! any other, so its device's queue moves on. A schema
//! only grows: rows move within it by an export, a transform and an import
//! ([`crate::dynamic`]).
//!
//! **A fill** is how a replica's rows come to it, from an upstream: its
//! authority, for one the log cannot page, or its own disk, as it opens
//! ([`Source`], answering its `Hello`s one [`Input::Read`] at a time). The first part says only where the rows stand; then the
//! replica asks, by a `Hello` saying where its fill stands, for each part
//! once the one before is held — of about [`PART`] bytes, of one table,
//! from where it stands to where the next begins — and when a table is
//! done, for the next: the first its views read ([`Peer::wants`]) not
//! taken yet, and then its schema's. A part is no frozen snapshot: it is
//! its rows as they stand when it is sent, at the replica's head or
//! before it, and the facts confirmed since the fill began on its table
//! are put again over it, so a replica puts its rows and is right there.
//! Entries come as they are logged, their facts put as they are over rows
//! not all sent yet; nothing is judged. A replica makes nothing durable
//! while filling, and the store whole when the last part is taken: one
//! reopened mid-fill fills again. Its own intents are the authority's to
//! judge, one its store refuses pending unrun. A connection fed by a
//! scope, and the volatile space, are filled so too.
//!
//! **An authority's own facts** are an intent naming no function, its input
//! their encoding: run, they are applied as they are, judged by the store as
//! any write is, and logged with no input — the facts are the entry, and
//! nothing runs it again. Only an authority's own: one pushed is not judged.
//!
//! **Partial sync is a view** (§1.8). A connection whose identity the
//! authority's feed ([`Peer::feed`]) gives a scope — a domain's function
//! of its own kind, asked of a table's row under the connection's identity
//! ([`Peer::feed_of`]) — is sent everything through it, and the authority
//! keeps nothing of it:
//! each entry with the facts the scope's filter makes of its own
//! (`scoped`), and a fill of the rows the scope holds, its union, each
//! part's rows through it as they are read. Its chain is the log's, each hash keyed by who it is and its scope, so a
//! `Hello` stands in the log only for a replica fed as it is now: one fed
//! otherwise is filled afresh. What goes to all goes through each
//! connection's scope ([`Feed::fed`]). The replica is the same machine: it
//! applies facts like every replica, its own intents previewed over what
//! it holds and confirmed by the authority's facts, a difference being the
//! authority's answer. A replica takes the chain it is sent as it is: where
//! it stands is the authority's to judge, at its `Hello`.
//!
//! **Offline is not a mode.** A replica with no server, or one not reached
//! for a while, is a replica not connected: its intents stay pending, the
//! store their preview, and are pushed when it connects. What it says to
//! keep of them as they move — each intent and its preview's changes, and
//! the head they stand on ([`Record::Pending`]) — is opened without running
//! them again; their confirmations, in order, take the previewed path.
//!
//! **Live state is a second machine** (§1.9): a peer [`Peer::in_space`]
//! `Volatile` is this machine over the volatile tables, sharing the
//! connection, every frame of its log naming its space. Its store hashes and
//! snapshots those tables only; it makes nothing durable; an intent whose
//! changes touch a table of the other space is refused, in either space.
//! A device says what it is in a call at each connection
//! ([`crate::frames::DEVICE`]); its `on_join` and `on_leave` are entries the authority authors through
//! `Mutate`, as the driver sees connections come and go. `resync` is a
//! `Hello` with a hash the authority lacks, answered with a fill. It runs
//! the log's rules: a volatile authority restarted from nothing has a
//! chain its replicas lack, so each is filled from it, and passes what
//! their pushes say they no longer hold, as a durable one does.
//!
//! **The horizon** is moved by the machine as its [`Fold`] says, after a
//! step, in either role, up to the head: a replica's pending
//! intents are rebased by their own changes over the head, never by an
//! entry below it, so a replica folds as an authority does.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use crate::domain::{Domain, Holds};
use crate::format::Encode;
use crate::frames::{Frame, Verdict, UPDATE};
use crate::hash::{EntryHash, SchemaHash};
pub use crate::log::{Ctx, Journal};
use crate::log::{Cursor, EntryId, Kept, Logged, Origin, Record, Rows, Seq, Span};
use crate::schema::Space;
use crate::store::{Change, Log, Store, LOG};

/// An event of the machine's, `tracing`'s at target `ark`, said where what
/// it says happens: an entry's with its id, its trace and span (an entry's
/// span is its id) and the span it was a child of, so that one tap's trace
/// is read across every peer that saw it. The driver's span around a step
/// names the peer and the space.
macro_rules! said {
    ($level:ident, $what:expr, $e:expr $(, $k:ident = $v:expr)*) => {{
        let e: &Ctx = $e;
        let s = e.span();
        let parent = e.cause.map(|c| crate::hash::hex(&c.id)).unwrap_or_default();
        tracing::$level!(target: "ark", entry = %crate::hash::hex(&e.id.0), trace = %crate::hash::hex(&s.trace.0),
            span = %crate::hash::hex(&s.id), parent = %parent, func = %e.func, actor = %e.actor,
            session = %e.session, local = e.local $(, $k = $v)*, "{}", $what)
    }};
}

/// What a peer is: a replica, or the authority — its journal, the scope
/// each connection is fed by, by who it is (none: every one the log
/// whole), and what it made of each build's schema it was told, by its
/// hash.
#[derive(Clone)]
pub enum Role {
    Replica,
    Authority {
        journal: Journal,
        feed: Option<Scoping>,
        builds: BTreeMap<SchemaHash, Stance>,
    },
}

impl Role {
    /// The authority, from nothing.
    pub fn authority() -> Role {
        Role::Authority {
            journal: Journal::default(),
            feed: None,
            builds: BTreeMap::new(),
        }
    }
}

/// What an authority makes of a replica's schema beside its own (§1.2):
/// the same; one its own grows from, an older build's; one that grows from
/// its own, a newer build's; or neither, which it cannot serve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Same,
    Older,
    Newer,
    Other,
}

/// The scope a connection is fed by, by who it is (§1.8): its union, by
/// the scope's name; none, the log whole.
pub type Scoping = Arc<dyn Fn(&Ctx) -> Option<String> + Send + Sync>;

/// When a machine folds its log into its rows, the one rule, after every
/// step: once the entries above its horizon outnumber `keep` and `slack`
/// (and the rows it holds, when `rows`), all but the last `keep` are
/// folded. A server keeps `keep` entries and folds at twice that; a local
/// instance keeps none and folds once its entries outnumber its rows (and
/// 64), so a fold, which writes every row, is paid for by as many writes; a
/// device's live machine keeps none and folds at 64 ([`Fold::REPLICA`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub keep: usize,
    pub slack: usize,
    pub rows: bool,
}

impl Fold {
    /// A device's live machine's, a replica's: none kept, folded once 64
    /// are held — a replica reads no entry below its head again.
    pub const REPLICA: Fold = Fold {
        keep: 0,
        slack: 64,
        rows: false,
    };

    /// A server's: `keep` entries above the horizon (none kept when none).
    pub fn keep(keep: Option<usize>) -> Option<Fold> {
        keep.map(|keep| Fold {
            keep,
            slack: keep,
            rows: false,
        })
    }
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Input {
    Frame {
        from: Option<Ctx>,
        frame: Frame,
    },
    Mutate {
        func: String,
        input: Vec<u8>,
        now: i64,
        /// The span it is authored under, frozen in the entry.
        cause: Option<Span>,
    },
    /// What it made durable, in order ([`Record`]).
    Open(Vec<Record>),
    Connected,
    /// Its disk's next answer, while its disk is its upstream ([`Source`]).
    Read,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// A frame, and where it goes.
    Send(To, Frame),
    /// A record to keep ([`Record`]), before anything after it is sent.
    Persist(Record),
    /// What changed, each change with the span of the entry that made it.
    Changes(Vec<Told>),
    /// What became of an intent of this peer's own: confirmed, or refused
    /// and why.
    Settled(EntryId, Result<(), String>),
}

/// Where a frame goes: back where the input came from (a replica's
/// upstream, or the connection a frame came on), or to every connection,
/// each through its feed ([`Feed::fed`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    Back,
    All,
}

/// A change as a machine tells it: with the span of the entry that made it
/// (the one run, the one confirmed, the pending one undone), and none for
/// a whole state's difference (a fill, a divergence, an open).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Told {
    pub by: Option<Span>,
    pub change: Change,
}

#[derive(Default)]
struct Out {
    out: Vec<Output>,
    told: Vec<Told>,
}

impl Out {
    fn send(&mut self, f: Frame) {
        self.out.push(Output::Send(To::Back, f));
    }
    fn tell(&mut self, by: Option<Span>, changes: impl IntoIterator<Item = Change>) {
        self.told.extend(changes.into_iter().map(|change| Told { by, change }));
    }
    fn persist(&mut self, r: Record) {
        self.out.push(Output::Persist(r));
    }
}

/// A replica filling (§1.7): where its next part begins, the tables taken
/// whole, and the facts confirmed since the fill began on tables not yet,
/// put again over each part of theirs. Nothing is made durable until the
/// fill is done.
#[derive(Clone)]
struct Filling {
    at: Cursor,
    taken: BTreeSet<String>,
    waiting: Vec<Change>,
}

/// What a replica said of its pending intents ([`Record::Pending`]): in
/// how many records; and what moved since — how many were taken off the
/// front (confirmed as previewed) and put on the back (a mutate), and
/// whether they were run again (a rebase, a renumbering, an open), when
/// they are said whole.
#[derive(Clone, Copy, Default)]
struct Keeping {
    records: usize,
    gone: usize,
    put: usize,
    again: bool,
}

#[derive(Clone)]
pub struct Peer {
    /// The confirmed state with the pending intents' changes applied, and
    /// the log as its table (§1.6).
    pub store: Store,
    /// What it runs, by name, its schema and which tables a fill sends
    /// last (§1.16): its domain's.
    pub domain: Arc<Domain>,
    /// This peer's own intents not yet sequenced, each at sequence 0 with
    /// what its last run changed as its facts ([`Logged::intent`]).
    pending: VecDeque<Logged>,
    pub role: Role,
    pub me: Ctx,
    space: Space,
    /// Where this peer's intents come from (§1.6), and the last `local` it
    /// drew.
    pub origin: Origin,
    pub local: i64,
    /// How many times a confirmed entry did not apply here: each time, the
    /// peer was filled again from nothing.
    diverged: usize,
    /// The cursor a `Hello` is outstanding at, and the fill's it asked.
    asked: Option<(Seq, Option<Cursor>)>,
    /// When it folds its log (none: never).
    pub fold: Option<Fold>,
    fill: Option<Filling>,
    /// The tables its views read, in order: what its fill takes first. Its
    /// driver's to say, as its views come and go, while it fills.
    pub wants: Vec<String>,
    /// How many bytes a replica would have each frame it is sent of rows or
    /// entries carry — a part of a fill, a page of the log — and each page
    /// of its own pushes, about, said in each `Hello`; none: [`PART`].
    pub part: Option<usize>,
    /// Its disk, while it is the upstream it fills from first.
    upstream: Option<Source>,
    /// Why its authority cannot serve it, once it said so ([`Frame::Update`]):
    /// it asks nothing more, and takes nothing it is sent.
    update: Option<String>,
    keeping: Keeping,
}

/// How many bytes of rows a fill's part, or of entries a page, carries at
/// most, about: past it, the frame ends at the row or the entry after; none
/// is cut, and each frame holds one. What a device taking frames on a
/// thread of its own is sent; one taking them on the thread it draws on
/// asks for less ([`Peer::part`]): its `Hello` says.
pub const PART: usize = 256 << 10;

/// `items` in pages of about `most` bytes, each as `weight` says: a page
/// ends at the item that takes it to `most`, so each holds one at least.
fn pages<T>(items: &[T], most: usize, weight: impl Fn(&T) -> usize) -> Vec<&[T]> {
    let (mut out, mut from, mut size) = (vec![], 0, 0);
    for (i, x) in items.iter().enumerate() {
        size += weight(x);
        if size >= most || i + 1 == items.len() {
            out.push(&items[from..=i]);
            (from, size) = (i + 1, 0);
        }
    }
    out
}

impl Peer {
    /// A peer from empty; its origin named by its session's bytes until
    /// its driver gives it the one it drew ([`Origin`]).
    pub fn new(domain: Arc<Domain>, role: Role, me: Ctx) -> Peer {
        let origin = Origin::named(&me.session);
        let mut p = Peer {
            store: Store::new(domain.schema.clone(), Space::Durable),
            domain,
            pending: VecDeque::new(),
            role,
            me,
            space: Space::Durable,
            origin,
            local: 0,
            diverged: 0,
            asked: None,
            fold: None,
            fill: None,
            wants: vec![],
            part: None,
            upstream: None,
            update: None,
            keeping: Keeping::default(),
        };
        p.store = p.blank();
        p
    }

    /// This peer over `space`'s tables (§1.9), from empty.
    pub fn in_space(mut self, space: Space) -> Peer {
        self.space = space;
        self.store = self.blank();
        self
    }

    /// An empty store of this peer's space, its log empty.
    fn blank(&self) -> Store {
        self.standing(0, EntryHash::default())
    }

    /// An empty store of this peer's space, its log standing on `(seq, hash)`.
    fn standing(&self, seq: Seq, hash: EntryHash) -> Store {
        let mut s = Store::new(self.store.schema_arc().clone(), self.space);
        s.start_log([Logged::base(seq, hash).to_bytes()]).expect("a first row starts a log");
        s
    }

    /// The log, as the store's table holds it (§1.6).
    pub fn log(&self) -> Log<'_> {
        self.store.log()
    }

    /// Its own intents not yet sequenced, oldest first.
    pub fn pending(&self) -> &VecDeque<Logged> {
        &self.pending
    }

    /// How many times it was filled again from nothing, a confirmed entry
    /// not applying here.
    pub fn diverged(&self) -> usize {
        self.diverged
    }

    /// Whether a fill is under way: nothing it holds is durable yet.
    pub fn filling(&self) -> bool {
        self.fill.is_some()
    }

    /// Whether its disk is the upstream it fills from ([`Input::Read`]).
    pub fn reading(&self) -> bool {
        self.upstream.is_some()
    }

    /// Why its authority cannot serve this build, once it said so: it is to
    /// update (§1.2).
    pub fn update_needed(&self) -> Option<&str> {
        self.update.as_deref()
    }

    /// The store's rows as confirmed — the pending intents' changes
    /// undone — and the log's, and a fill's: what this peer makes durable
    /// whole.
    pub fn confirmed_rows(&self) -> Rows {
        if self.pending.is_empty() {
            return self.store.kv_rows();
        }
        let mut s = self.store.clone();
        s.undo(self.pending.iter().flat_map(|p| &p.facts)).expect("a pending change undoes");
        s.kv_rows()
    }

    /// A logged entry made part of the log, its facts already applied, and
    /// durable as its row of the log's table alone: the facts are in it —
    /// unless filling, when nothing is durable until the fill is done.
    /// The entry follows the head: this peer chained it, or was sent it so.
    fn logged(&mut self, l: &Logged, o: &mut Out) {
        debug_assert!(l.seq == self.log().head().0 + 1);
        if self.fill.is_none() {
            o.persist(Record::Logged(l.clone()));
        }
        self.store.log_push(l);
    }

    /// The id this peer's next intent will have (§1.6).
    pub fn next_id(&self) -> EntryId {
        EntryId::of(self.origin, self.local + 1)
    }

    pub fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority { .. })
    }

    /// An authority's journal.
    pub fn journal(&self) -> Option<&Journal> {
        match &self.role {
            Role::Authority { journal, .. } => Some(journal),
            Role::Replica => None,
        }
    }

    fn journal_mut(&mut self) -> &mut Journal {
        match &mut self.role {
            Role::Authority { journal, .. } => journal,
            Role::Replica => panic!("a replica keeps no journal"),
        }
    }

    /// An authority's feed: the scope each connection is fed by.
    pub fn feed(&mut self, scoping: Scoping) {
        if let Role::Authority { feed, .. } = &mut self.role {
            *feed = Some(scoping);
        }
    }

    pub fn step(&mut self, input: Input) -> Vec<Output> {
        let mut o = Out::default();
        match input {
            // Told to update, it takes nothing it is sent.
            Input::Frame { .. } if self.update.is_some() => {}
            Input::Frame { from, frame } => self.frame(from, frame, &mut o),
            Input::Mutate { func, input, now, cause } => self.mutate(func, input, now, cause, &mut o),
            Input::Open(d) => self.open(d, &mut o),
            Input::Connected => self.connected(&mut o),
            Input::Read => self.read(&mut o),
        }
        let (head, base, rows) = (self.log().head().0, self.log().base_seq(), self.store.held());
        if let Some(f) = self.fold.filter(|f| head - base > (f.keep + f.slack + f.rows as usize * rows) as i64) {
            self.compact_to(head - f.keep as i64, &mut o);
        }
        if !self.is_authority() && self.space == Space::Durable {
            self.keep(&mut o);
        }
        if !o.told.is_empty() {
            o.out.insert(0, Output::Changes(o.told));
        }
        if self.update.is_some() {
            o.out.retain(|x| !matches!(x, Output::Send(..)));
        }
        if self.space == Space::Volatile {
            o.out.retain(|x| !matches!(x, Output::Persist(_)));
        }
        o.out
    }

    /// What moved of its pending intents since it last said, said: those
    /// confirmed off the front and those new at the back — so a mutate
    /// says its one, whatever is pending — or, run again, or their records
    /// outnumbering them, all of them, afresh; and the head they stand on.
    fn keep(&mut self, o: &mut Out) {
        let (k, n) = (self.keeping, self.pending.len());
        let whole = k.again || k.records > n + 64;
        if !whole && k.gone + k.put == 0 {
            return;
        }
        let put = self.pending.iter().skip(if whole { 0 } else { n - k.put }).cloned().collect();
        let (gone, local, at) = (if whole { 0 } else { k.gone as u64 }, self.local, Some(self.log().head()));
        o.persist(Record::Pending(Kept { whole, gone, put, local, at }));
        let records = if whole { 1 } else { k.records + 1 };
        self.keeping = Keeping {
            records,
            ..Keeping::default()
        };
    }

    // * Running

    /// Runs an intent over the store and applies what it did, all or none.
    fn attempt(&mut self, e: &Ctx, o: &mut Out) -> Result<Vec<Change>, String> {
        let f = self.domain.fns.get(&e.func);
        let p = match f.and_then(|f| f.proc.clone()) {
            Some(p) => p,
            // An authority's own facts, as they are.
            None if e.func.is_empty() && e.origin == self.origin && self.is_authority() => {
                let facts = Vec::<Change>::from_bytes(&e.input)?;
                self.store.apply_all(&facts)?;
                o.tell(Some(e.span()), facts.iter().cloned());
                return Ok(facts);
            }
            None if f.is_some() => return Err(format!("{} writes nothing", e.func)),
            None => return Err(format!("no function {}", e.func)),
        };
        let s = e.span();
        let run = tracing::debug_span!(target: "ark", "run", func = %e.func, trace = %crate::hash::hex(&s.trace.0), span = %crate::hash::hex(&s.id));
        let changes = run.in_scope(|| p(&mut self.store, e, &e.input))?;
        let schema = self.store.schema_arc().clone();
        if let Some(c) = changes.iter().find(|c| schema.table(c.table()).is_some_and(|t| t.1.space != self.space)) {
            self.store.undo(&changes).expect("an inverse applies");
            return Err(format!("{} is not in this space", c.table()));
        }
        o.tell(Some(e.span()), changes.iter().cloned());
        Ok(changes)
    }

    /// The pending intents' changes undone, newest first — put back as
    /// they were, judging nothing, while filling: the rows they ran over
    /// may name rows not sent yet.
    fn undo_pending(&mut self, o: &mut Out) {
        for p in self.pending.iter().rev() {
            let undone = match self.fill {
                Some(_) => self.store.force_all(&p.facts.iter().rev().map(Change::inverse).collect::<Vec<_>>()),
                None => self.store.undo(&p.facts),
            };
            o.tell(Some(p.entry.span()), undone.expect("a pending change undoes"));
        }
    }

    /// Runs the pending intents again, in order, over the store as it is;
    /// one that now refuses stays pending with no changes, the authority's
    /// to judge.
    fn redo_pending(&mut self, o: &mut Out) {
        self.keeping.again = true;
        for p in std::mem::take(&mut self.pending) {
            let changes = self.attempt(&p.entry, o).unwrap_or_default();
            self.pending.push_back(Logged::intent(p.entry, changes));
        }
    }

    /// Takes an intent out of the pending ones, the rest rebased.
    fn settle(&mut self, id: &EntryId, o: &mut Out) -> bool {
        let Some(i) = self.pending.iter().position(|p| p.entry.id == *id) else {
            return false;
        };
        self.undo_pending(o);
        self.pending.remove(i);
        self.redo_pending(o);
        true
    }

    fn mutate(&mut self, func: String, input: Vec<u8>, now: i64, cause: Option<Span>, o: &mut Out) {
        let mut entry = Ctx {
            cause,
            ..Ctx::new(&self.me, self.origin, self.local + 1, func, input, now)
        };
        let (id, local) = (entry.id, entry.local);
        said!(debug, "entry", &entry);
        if self.is_authority() {
            let run = self.attempt(&entry, o);
            if entry.func.is_empty() {
                // Its facts are the entry.
                entry.input.clear();
            }
            // Logged, its `local` drawn; or refused, drawing none.
            let v = self.judged(entry, run, o).map(|_| ());
            if v.is_ok() {
                self.local = local;
            }
            return o.out.push(Output::Settled(id, v));
        }
        // Filling, a refusal may be of rows not sent yet: the authority's to
        // judge, the intent pending unrun.
        match self.attempt(&entry, o) {
            Err(why) if self.fill.is_none() => {
                said!(warn, "refused", &entry, why = why.as_str());
                o.out.push(Output::Settled(id, Err(why)))
            }
            run => {
                self.local = local;
                let from = self.pending.front().map_or(local, |p| p.entry.local) - 1;
                o.send(Frame::push(self.space, self.store.schema_hash(), from, [entry.clone()]));
                self.pending.push_back(Logged::intent(entry, run.unwrap_or_default()));
                self.keeping.put += 1;
            }
        }
    }

    /// An intent judged by what its run did: its origin's `local` passed,
    /// and it logged with its facts and told everyone, or its refusal kept.
    fn judged(&mut self, entry: Ctx, run: Result<Vec<Change>, String>, o: &mut Out) -> Result<Seq, String> {
        let id = entry.id;
        self.journal_mut().pass(entry.origin, &entry.actor, entry.local);
        let facts = match run {
            Ok(facts) => facts,
            Err(why) => {
                tracing::warn!(target: "ark", entry = %crate::hash::hex(&id.0), why = %why, "refused");
                return Err(why);
            }
        };
        let (n, prev) = self.log().head();
        tracing::debug!(target: "ark", entry = %crate::hash::hex(&id.0), seq = n + 1, "ok");
        let l = Logged {
            seq: n + 1,
            hash: Logged::chain(&prev, n + 1, &entry, &facts),
            entry,
            facts,
        };
        self.logged(&l, o);
        let page = Frame::Page {
            space: self.space,
            from: n,
            logged: vec![l],
        };
        o.out.push(Output::Send(To::All, page));
        Ok(n + 1)
    }

    // * Frames

    fn frame(&mut self, from: Option<Ctx>, f: Frame, o: &mut Out) {
        if f.space() != self.space {
            return;
        }
        match (f, self.is_authority(), from) {
            (Frame::Schema { schema, .. }, true, _) => self.judge(schema),
            (
                Frame::Hello {
                    cursor,
                    hash,
                    fill,
                    most,
                    schema,
                    ..
                },
                true,
                who,
            ) => {
                if self.stance(schema) == Stance::Other {
                    let why = format!("{UPDATE}: this server's schema neither grows from this build's nor into it");
                    return o.send(Frame::Update { space: self.space, why });
                }
                // Never past [`PART`].
                let feed = who.and_then(|w| self.feed_of(&w));
                self.serve(feed, (cursor, hash), fill, (most as usize).clamp(1, PART), o)
            }
            // A peer's intents, each a push.
            (Frame::Push { schema, from, intents, .. }, true, Some(who)) => {
                let stance = self.stance(schema);
                intents.into_iter().for_each(|e| self.push(&who, from, e, stance, o))
            }
            (Frame::Update { why, .. }, false, _) => {
                tracing::warn!(target: "ark", why = %why, "update needed");
                self.update = Some(why);
            }
            (Frame::Page { logged, .. }, false, _) => self.confirm(logged, o),
            (
                Frame::Part {
                    seq, hash, from, rows, next, ..
                },
                false,
                _,
            ) => self.take((seq, hash), from, rows, next, o),
            (Frame::Verdict { id, verdict, .. }, false, _) => match verdict {
                // In the log at or below where this peer stands, an intent
                // still pending here is in the state it holds (a fill carried
                // it).
                Verdict::At { seq, last } => {
                    let taken = seq <= self.log().head().0 && self.pending.iter().any(|p| p.entry.id == id && p.entry.local <= last);
                    if taken && self.settle(&id, o) {
                        o.out.push(Output::Settled(id, Ok(())));
                    }
                }
                Verdict::Refused { why } if self.settle(&id, o) => {
                    tracing::warn!(target: "ark", entry = %crate::hash::hex(&id.0), why = %why, "refused");
                    o.out.push(Output::Settled(id, Err(why)))
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// A `Hello` at `at` from a connection fed as `feed` says: the pages
    /// from there, each of about `most` bytes, when this log has that hash
    /// there, and then, when its fill stands at `fill`, the part from
    /// there, of about `most` bytes of rows; a fill's first part when it
    /// does not.
    fn serve(&mut self, feed: Option<Feed>, at: (Seq, EntryHash), fill: Option<Cursor>, most: usize, o: &mut Out) {
        let key = |h: EntryHash| feed.as_ref().map_or(h, |f| f.key(h));
        let has = self.log().hash_at(at.0).is_some_and(|h| key(h) == at.1);
        if has {
            let after: Result<Vec<Logged>, String> = self
                .log()
                .after(at.0)
                .map(|l| match &feed {
                    Some(f) => f.logged(l),
                    None => Ok(l),
                })
                .collect();
            let Ok(after) = after else { return };
            for page in pages(&after, most, Logged::weight) {
                o.send(Frame::Page {
                    space: self.space,
                    from: page[0].seq - 1,
                    logged: page.to_vec(),
                });
            }
        }
        if !has || fill.is_some() {
            let at = (self.log().head().0, key(self.log().head().1));
            let from = fill.filter(|_| has);
            let c = from.clone().unwrap_or_default();
            // A scope's rows, its union, as they are read (§1.8).
            let held = |r: &&[u8]| feed.as_ref().is_none_or(|f| f.holds(&c.table, r).unwrap_or(false));
            let rows = self.store.keyed_from(&c.table, &c.key).filter(|(_, r)| held(r));
            o.send(part(self.space, at, from, rows, most));
        }
    }

    /// How `who` is fed (§1.8): none, the log whole; or by its scope — what
    /// a driver keeps for each connection, to send it what goes to all
    /// ([`Feed::fed`]).
    pub fn feed_of(&self, who: &Ctx) -> Option<Feed> {
        let Role::Authority { feed: Some(feed), .. } = &self.role else {
            return None;
        };
        let scope = feed(who)?;
        let holds = self.domain.fns.get(&scope).and_then(|f| f.scope.clone());
        let key = crate::hash::sha256(&[&crate::format::encode(&(&scope, who)).expect("who serializes")]);
        Some(Feed {
            who: who.clone(),
            holds,
            key,
        })
    }

    /// Which rows `who` reads, by table: every one, fed whole.
    pub fn reads(&self, who: &Ctx) -> impl Fn(&str, &[u8]) -> bool {
        let feed = self.feed_of(who);
        move |table, row| feed.as_ref().is_none_or(|f| f.holds(table, row).unwrap_or(false))
    }

    /// A replica's schema judged beside its own (§1.2), kept by its hash
    /// for its `Hello`s and pushes to name.
    fn judge(&mut self, theirs: crate::schema::Schema) {
        let ours = self.store.schema_arc().clone();
        let stance = match (crate::schema::compat(&theirs, &ours), crate::schema::compat(&ours, &theirs)) {
            (Ok(()), _) => Stance::Older,
            (_, Ok(())) => Stance::Newer,
            _ => Stance::Other,
        };
        if let Role::Authority { builds, .. } = &mut self.role {
            // A few builds are out at once; told of many, it starts afresh.
            if builds.len() >= 64 {
                builds.clear();
            }
            builds.insert(theirs.hash(), stance);
        }
    }

    /// What it makes of a replica's schema, by its hash: its own, or one it
    /// was told ([`Frame::Schema`]); one it was not told — its `Schema`
    /// lost — is taken for a newer build's, served, and what it names that
    /// is not here held.
    fn stance(&self, schema: SchemaHash) -> Stance {
        match &self.role {
            Role::Authority { builds, .. } if schema != self.store.schema_hash() => builds.get(&schema).copied().unwrap_or(Stance::Newer),
            _ => Stance::Same,
        }
    }

    /// A pushed intent judged, its device's pending beginning past `from`,
    /// its build's schema as `stance` says, and the verdict sent back:
    /// every push is answered.
    fn push(&mut self, who: &Ctx, from: i64, entry: Ctx, stance: Stance, o: &mut Out) {
        let id = entry.id;
        let verdict = self.verdict_on(who, from, entry, stance, o);
        o.send(Frame::Verdict {
            space: self.space,
            id,
            verdict,
        });
    }

    fn verdict_on(&mut self, who: &Ctx, from: i64, mut entry: Ctx, stance: Stance, o: &mut Out) -> Verdict {
        let head = self.log().head().0;
        let known = self.domain.fns.contains_key(&entry.func);
        let j = self.journal_mut();
        if let Some(why) = j.refused.get(&(entry.origin, entry.local)) {
            return Verdict::Refused { why: why.clone() };
        }
        if !entry.id_checks() || j.others(&entry.origin, &who.actor) {
            let why = "an id that is not its own, or from an origin of another actor".into();
            return Verdict::Refused { why };
        }
        // What its device no longer holds pending is nowhere: passed, and
        // its refusals forgotten.
        let held = from.min(entry.local - 1);
        j.pass(entry.origin, &who.actor, held);
        if j.forget(entry.origin, held) {
            o.persist(Record::Journal(j.clone()));
        }
        let last = j.last_of(&entry.origin);
        // Not the next of its origin, or held — of a build it cannot serve,
        // or naming a function a newer build has and it does not yet: where
        // its origin stands. At or below the last and not refused, it is in
        // the log at or below the head — every entry up to which this
        // connection is sent before the verdict; past it, not judged.
        let held = stance == Stance::Other || !known && stance == Stance::Newer;
        if entry.local != last + 1 || held {
            match entry.local > last {
                true if entry.local == last + 1 => said!(warn, "held", &entry, last = last),
                true => said!(warn, "gap", &entry, last = last),
                false => {}
            }
            return Verdict::At { seq: head, last };
        }
        (entry.actor, entry.session, entry.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        // A function it does not run, of a build not newer, is retired.
        let run = match known {
            true => self.attempt(&entry, o),
            false => Err(format!("{UPDATE}: this server runs no function {}", entry.func)),
        };
        let (origin, local) = (entry.origin, entry.local);
        self.judged(entry, run, o).map_or_else(
            |why| {
                let j = self.journal_mut();
                j.refused.insert((origin, local), why.clone());
                o.persist(Record::Journal(j.clone()));
                Verdict::Refused { why }
            },
            |seq| Verdict::At { seq, last: local },
        )
    }

    /// Confirmed entries, applied by their facts: a page is one rebase.
    /// An entry that is the oldest pending intent as it was previewed moves
    /// nothing. At the first that is not, the pending intents are undone,
    /// each entry's facts applied — read under this schema when another
    /// wrote them (§1.2), put as they are while filling — and the intents
    /// still pending run again once, after the page. Facts that do not
    /// apply are a divergence, healed by a fill.
    fn confirm(&mut self, ls: Vec<Logged>, o: &mut Out) {
        let mut undone = false;
        for l in ls {
            let n = self.log().head().0;
            if l.seq <= n {
                continue;
            }
            if l.seq > n + 1 {
                // A gap: say where this peer stands.
                if undone {
                    self.redo_pending(o);
                }
                return self.ask(o);
            }
            if let Some(f) = &mut self.fill {
                f.waiting.extend(l.facts.iter().filter(|c| !f.taken.contains(c.table())).cloned());
            }
            let previewed = !undone && self.pending.front().is_some_and(|p| p.entry.id == l.entry.id && p.facts == l.facts);
            if previewed {
                self.pending.pop_front();
                self.keeping.gone += 1;
            } else {
                if !undone {
                    self.undo_pending(o);
                    undone = true;
                }
                let moved = match self.fill {
                    Some(_) => self.store.force_all(&l.facts),
                    None => self.store.apply_facts(&l.facts),
                };
                let Ok(moved) = moved else {
                    self.redo_pending(o);
                    return self.diverge(l.seq, o);
                };
                o.tell(Some(l.entry.span()), moved);
                if let Some(i) = self.pending.iter().position(|p| p.entry.id == l.entry.id) {
                    self.pending.remove(i);
                }
            }
            let mine = l.entry.origin == self.origin;
            said!(debug, if mine { "confirmed" } else { "applied" }, &l.entry, seq = l.seq);
            if mine {
                o.out.push(Output::Settled(l.entry.id, Ok(())));
            }
            self.asked = None;
            self.logged(&l, o);
        }
        if undone {
            self.redo_pending(o);
        }
    }

    /// A `Hello` where this peer stands, and where its fill does, unless
    /// one is asked there already: to its server, or — its disk its
    /// upstream — kept for its disk to answer ([`Input::Read`]).
    fn ask(&mut self, o: &mut Out) {
        let (n, h) = self.log().head();
        let fill = self.fill.as_ref().map(|f| f.at.clone());
        let at = Some((n, fill.clone()));
        if self.asked != at {
            self.asked = at;
            if self.upstream.is_none() {
                o.send(Frame::Hello {
                    space: self.space,
                    cursor: n,
                    hash: h,
                    fill,
                    most: self.most(),
                    schema: self.store.schema_hash(),
                });
            }
        }
    }

    /// The most bytes of rows this replica would have a part carry.
    fn most(&self) -> u64 {
        self.part.unwrap_or(PART) as u64
    }

    /// Facts that do not apply here at `seq`: the fill begun again, from
    /// nothing — the store emptied, its log standing at `seq` on nothing the
    /// authority has, so that its `Hello` is sent a first part (§1.7). One
    /// filling from its disk asks its server.
    fn diverge(&mut self, seq: Seq, o: &mut Out) {
        tracing::warn!(target: "ark", seq, "diverged");
        self.diverged += 1;
        let read = self.upstream.take().is_some();
        self.undo_pending(o);
        self.stand((seq, EntryHash::default()), o);
        self.redo_pending(o);
        self.begin_fill();
        self.ask(o);
        if read {
            self.repush(o);
        }
    }

    /// A fill begun where it stands: nothing of it taken, its first part
    /// asked from where it would go on.
    fn begin_fill(&mut self) {
        let taken = BTreeSet::new();
        self.fill = self.next_table(&taken).map(|at| Filling { at, taken, waiting: vec![] });
    }

    /// Where a fill goes on: the start of the first table not taken whole
    /// yet, of those its views read and then of its schema's (§1.7).
    fn next_table(&self, taken: &BTreeSet<String>) -> Option<Cursor> {
        let schema = self.store.schema_arc();
        let tables = schema.tables.iter().filter(|t| t.space == self.space).map(|t| &t.name);
        let mut order = self.wants.iter().filter(|w| tables.clone().any(|t| t == *w)).chain(tables.clone());
        let table = order.find(|t| !taken.contains(*t))?.clone();
        Some(Cursor { table, key: vec![] })
    }

    /// Whether a fill under way is at a table its views read: they come
    /// first.
    pub fn wanting(&self) -> bool {
        self.fill.as_ref().is_some_and(|f| self.wants.contains(&f.at.table))
    }

    /// The store emptied, its log standing on `at`, told as what moved.
    fn stand(&mut self, at: (Seq, EntryHash), o: &mut Out) {
        let fresh = self.standing(at.0, at.1);
        o.tell(None, self.store.diff(&fresh));
        (self.store, self.asked) = (fresh, None);
    }

    /// A part of a fill, taken at `(seq, hash)`. The first (`from` none)
    /// begins one — unless this peer stands where it was taken, filled
    /// already — the store emptied, the log standing on where the part was
    /// taken: an authority is always right, one restarted behind its
    /// replicas too. Any other is the one from where the fill stands, at
    /// the head or before it, or it is passed by — past a gap, the head
    /// asked for again, and the part. Its rows are put as they are, judging nothing (a row
    /// may name one not sent yet: every row is the authority's, judged
    /// where it was written), and the facts confirmed since the fill began
    /// on their table put again over them: rows of any moment since are
    /// right then. A table done is taken whole, and the fill goes on at the
    /// next its views read, or its schema has; none left is the fill done,
    /// and the store made durable whole — until then nothing is, so a peer
    /// reopened mid-fill fills again. The pending intents run again on top
    /// of each. A fill from its disk done, it asks its server.
    fn take(&mut self, at: (Seq, EntryHash), from: Option<Cursor>, rows: Rows, next: Option<Cursor>, o: &mut Out) {
        let first = from.is_none();
        let standing = self.fill.as_ref().map(|f| &f.at);
        if first && standing.is_none() && at == self.log().head() || !first && (standing.is_none() || from.as_ref() != standing) {
            return;
        } else if !first && at.0 > self.log().head().0 {
            self.asked = None;
            return self.ask(o);
        }
        self.undo_pending(o);
        if first {
            self.stand(at, o);
            self.begin_fill();
        }
        for (table, rows) in &rows {
            let Some(f) = &mut self.fill else { break };
            let again: Vec<Change> = f.waiting.iter().filter(|c| c.table() == table).cloned().collect();
            if next.as_ref().is_none_or(|c| c.table != *table) {
                f.taken.insert(table.clone());
            }
            match self
                .store
                .put_rows(table, rows)
                .and_then(|m| Ok([m, self.store.force_all(&again)?].concat()))
            {
                Ok(moved) => o.tell(None, moved),
                Err(_) => {
                    self.redo_pending(o);
                    return self.diverge(at.0, o);
                }
            }
        }
        let next = next.or_else(|| self.next_table(&self.fill.as_ref()?.taken));
        match (next, &mut self.fill) {
            (Some(next), Some(f)) => {
                f.at = next;
                self.ask(o)
            }
            _ => {
                tracing::info!(target: "ark", seq = at.0, "filled");
                self.fill = None;
                o.persist(Record::Rows(self.store.kv_rows()))
            }
        }
        self.redo_pending(o);
        if self.fill.is_none() && self.upstream.take().is_some() {
            // Its disk read, it asks its server.
            self.connected(o);
        } else if first {
            self.repush(o);
        }
    }

    /// Its pending intents pushed again, in pages, each saying where they
    /// begin.
    fn repush(&mut self, o: &mut Out) {
        let most = self.most() as usize;
        let from = self.pending.front().map_or(0, |p| p.entry.local - 1);
        for page in pages(self.pending.make_contiguous(), most, Logged::weight) {
            o.send(Frame::push(
                self.space,
                self.store.schema_hash(),
                from,
                page.iter().map(|p| p.entry.clone()),
            ));
        }
    }

    // * Lifecycle

    fn connected(&mut self, o: &mut Out) {
        if self.is_authority() {
            return;
        }
        // What was asked on the connection before is not coming; what it
        // is, first (§1.2).
        self.asked = None;
        let schema = (**self.store.schema_arc()).clone();
        o.send(Frame::Schema { space: self.space, schema });
        self.ask(o);
        self.repush(o);
    }

    /// Its disk's answer to the `Hello` it would send its server, its disk
    /// its upstream ([`Source`]): the first time, the fill begun again at
    /// the tables its views read now.
    fn read(&mut self, o: &mut Out) {
        let Some(s) = &mut self.upstream else { return };
        if !std::mem::replace(&mut s.begun, true) {
            self.begin_fill();
            self.asked = None;
            self.ask(o);
        }
        let Some((cursor, Some(fill))) = self.asked.clone() else { return };
        let answer = self
            .upstream
            .as_ref()
            .map(|s| s.answer(self.space, cursor, fill, self.most()))
            .unwrap_or_default();
        for f in answer {
            self.frame(None, f, o);
        }
    }

    /// Moves the horizon to `n` now, whatever its [`Fold`] says: what a
    /// test or a measurement does by hand.
    pub fn compact(&mut self, n: Seq) -> Vec<Output> {
        let mut o = Out::default();
        self.compact_to(n, &mut o);
        o.out
    }

    /// Moves the horizon to `n`, at most the head: the log's rows below it
    /// deleted, and the
    /// store as confirmed made durable whole, which folds every write
    /// before. A replica may move its own: its pending intents stand on
    /// the head and are rebased by their own changes, so no entry below
    /// the head is read again; a `Hello` says the head.
    fn compact_to(&mut self, n: Seq, o: &mut Out) {
        let (head, base) = (self.log().head().0, self.log().base_seq());
        let n = n.clamp(base, head);
        if n == base || self.fill.is_some() {
            return;
        }
        self.store.log_horizon(n);
        if self.space == Space::Durable {
            o.persist(Record::Rows(self.confirmed_rows()));
            // An origin's last below the horizon is the journal's alone.
            o.out.extend(self.journal().map(|j| Output::Persist(Record::Journal(j.clone()))));
        }
    }

    /// Opens what was made durable under this peer's schema, or one it grows
    /// from or into (§1.2): the store's rows read under it, the log's checked
    /// as a chain, and the log's rows appended since, in order, each
    /// linking onto the head and its facts applied, all or none, up to the
    /// first that does not — the store is then made durable whole again,
    /// without it. A fill under way begins again from nothing. The pending
    /// intents' changes are put back on top when the log stands where they
    /// were made and apply; from the first that does not, they run again.
    /// An authority's journal is read beside them. One reading its disk
    /// stops, and asks its server.
    fn open(&mut self, records: Vec<Record>, o: &mut Out) {
        let (mut rows, mut logged, mut pending, mut kept) = (vec![], vec![], VecDeque::new(), (None, 0, 0));
        for r in records {
            match r {
                Record::Rows(r) => (rows, logged) = (r, vec![]),
                Record::Logged(l) => logged.push(l),
                Record::Pending(k) => {
                    if k.whole {
                        pending.clear();
                    }
                    pending.drain(..(k.gone as usize).min(pending.len()));
                    pending.extend(k.put);
                    kept = (k.at, k.local, kept.2 + 1);
                }
                Record::Journal(j) => {
                    if let Role::Authority { journal, .. } = &mut self.role {
                        *journal = j;
                    }
                }
            }
        }
        self.undo_pending(o);
        self.pending.clear();
        let (filling, read) = (self.fill.take().is_some(), self.upstream.take().is_some());
        let (log, rows): (Rows, Rows) = rows.into_iter().partition(|(t, _)| t == LOG);
        let log = log.into_iter().flat_map(|(_, rs)| rs);
        let opened = match filling {
            true => Ok(self.blank()),
            false => Store::from_rows(self.store.schema_arc().clone(), self.space, &rows).and_then(|mut s| s.start_log(log).map(|_| s)),
        };
        // An authority's log is its chain, checked as it is read (§1.6).
        let chain = self.is_authority();
        if let Some(mut store) = opened.ok().filter(|s| !chain || s.log().chained()) {
            // Every entry read once: those the rows hold, from their rows,
            // and those appended, as their records were read.
            let mut entries = store.log().entries();
            let (n, held) = (logged.len(), entries.len());
            for l in logged.into_iter().filter(|_| !filling) {
                if chain && !l.links(&store.log().head().1) || store.append(&l).is_err() {
                    break;
                }
                entries.push(l);
            }
            if entries.len() - held < n {
                o.persist(Record::Rows(store.kv_rows()));
            }
            o.tell(None, self.store.diff(&store));
            self.store = store;
            if let Role::Authority { journal, .. } = &mut self.role {
                for l in &entries {
                    journal.pass(l.entry.origin, &l.entry.actor, l.entry.local);
                }
                self.local = self.local.max(journal.last_of(&self.origin));
            }
        }
        let (at, local, records) = kept;
        self.local = self.local.max(local);
        let mut put_back = at.is_some() && at == Some(self.log().head());
        for p in pending {
            put_back = put_back && self.store.apply_all(&p.facts).is_ok();
            let changes = match put_back {
                true => {
                    o.tell(Some(p.entry.span()), p.facts.iter().cloned());
                    p.facts
                }
                false => self.attempt(&p.entry, o).unwrap_or_default(),
            };
            self.pending.push_back(Logged::intent(p.entry, changes));
        }
        self.keeping = Keeping {
            records,
            again: !put_back,
            ..Keeping::default()
        };
        if read {
            // What it was reading is no more: it asks its server.
            self.connected(o);
        }
    }

    /// Its disk the upstream it fills from first ([`Source`]), standing
    /// where its rows were written: it asks its server once it is done.
    pub fn read_from(&mut self, source: Source) {
        self.upstream = Some(source);
        self.begin_fill();
    }
}

/// §1.7 A replica's own disk as its first upstream: what it made durable —
/// its rows whole where its log stood, and the entries logged after —
/// answering its `Hello`s as its authority would, from those, one answer
/// an [`Input::Read`]. A replica opens standing where its rows did, filling
/// them from it as it fills from its server: each part as it is taken, the
/// tables its views read first; it asks its server once it is done. Its
/// parts' cursors are row indexes, not keys: a fill never moves to another
/// upstream inside a table, but begins again from nothing.
#[derive(Clone)]
pub struct Source {
    at: (Seq, EntryHash),
    rows: BTreeMap<String, Vec<Vec<u8>>>,
    tail: Vec<Logged>,
    /// Whether it was read yet: the first read begins the fill again, at
    /// the tables the views read by then.
    begun: bool,
}

impl Source {
    /// The rows and entries of a disk's records — its rows whole, first,
    /// then the entries after — taken out of them as a source, unless they
    /// hold no row but the log's: the records left stand where those rows
    /// did.
    pub fn of(records: &mut Vec<Record>) -> Option<Source> {
        let Some(Record::Rows(rows)) = records.first_mut() else { return None };
        let at = Logged::from_bytes(rows.iter().find(|(t, _)| t == LOG)?.1.last()?).ok()?;
        if rows.iter().all(|(t, rs)| t == LOG || rs.is_empty()) {
            return None;
        }
        let rows = std::mem::replace(rows, crate::log::standing(vec![], at.seq, at.hash));
        let rows = rows.into_iter().filter(|(t, _)| t != LOG).collect();
        let tail = records.drain(1..).filter_map(|r| if let Record::Logged(l) = r { Some(l) } else { None });
        Some(Source {
            at: (at.seq, at.hash),
            rows,
            tail: tail.collect(),
            begun: false,
        })
    }

    /// Its answer to a `Hello` at `cursor` of its replica, its fill at
    /// `from`: the entries after where that stands, and the part from there.
    fn answer(&self, space: Space, cursor: Seq, from: Cursor, most: u64) -> Vec<Frame> {
        let logged: Vec<Logged> = self.tail.iter().filter(|l| l.seq > cursor).cloned().collect();
        let page = (!logged.is_empty()).then_some(Frame::Page { space, from: cursor, logged });
        let n = from.key.as_slice().try_into().map_or(0, u64::from_be_bytes) as usize;
        let rows = self.rows.get(&from.table).into_iter().flatten().enumerate().skip(n);
        let rows = rows.map(|(i, r)| ((i as u64).to_be_bytes(), &r[..]));
        page.into_iter()
            .chain([part(space, self.at, Some(from), rows, (most as usize).clamp(1, PART))])
            .collect()
    }
}

/// A part of a fill, its rows as they stand at `(seq, hash)`: none, the
/// first, which only says where they stand — the replica asks for each
/// table it would have next — or, from where its fill stands, that table's
/// rows in key order (`rows`, from its key on, each with its own) until
/// about `most` bytes, and where the next begins, unless the table
/// is done. A table this peer lacks has none.
pub fn part<'a, K: AsRef<[u8]>>(
    space: Space,
    (seq, hash): (Seq, EntryHash),
    from: Option<Cursor>,
    rows: impl Iterator<Item = (K, &'a [u8])>,
    most: usize,
) -> Frame {
    let (mut held, mut size, mut next) = (vec![], 0, None);
    if let Some(c) = &from {
        for (k, r) in rows {
            if size >= most {
                next = Some(Cursor {
                    table: c.table.clone(),
                    key: k.as_ref().to_vec(),
                });
                break;
            }
            size += r.len();
            held.push(r.to_vec());
        }
    }
    let rows = from.as_ref().map(|c| vec![(c.table.clone(), held)]).unwrap_or_default();
    Frame::Part {
        space,
        seq,
        hash,
        from,
        rows,
        next,
    }
}

/// How a connection fed by a scope is fed (§1.8): who it is, its scope's
/// verdict on a row (none: its domain has no such scope, and nothing is
/// sent), and the key its chain is the log's under.
#[derive(Clone)]
pub struct Feed {
    who: Ctx,
    holds: Option<Holds>,
    key: [u8; 32],
}

impl Feed {
    fn holds(&self, table: &str, row: &[u8]) -> Result<bool, String> {
        self.holds.as_ref().ok_or_else(|| String::from("no such scope"))?(&self.who, table, row)
    }

    /// The log's hash at an entry, as this connection's chain has it.
    fn key(&self, h: EntryHash) -> EntryHash {
        EntryHash(crate::hash::sha256(&[&self.key, &h.0]))
    }

    /// A logged entry as this connection is sent it: its facts through the
    /// scope, its hash keyed.
    fn logged(&self, l: Logged) -> Result<Logged, String> {
        let facts = scoped(&l.facts, |t, r| self.holds(t, r))?;
        let hash = self.key(l.hash);
        Ok(Logged { facts, hash, ..l })
    }

    /// What this connection is sent of a frame sent to all ([`To::All`]):
    /// a page through its scope; none, when its scope cannot say.
    pub fn fed(&self, f: &Frame) -> Option<Frame> {
        let Frame::Page { space, from, logged } = f else { return None };
        let logged: Result<_, _> = logged.iter().map(|l| self.logged(l.clone())).collect();
        Some(Frame::Page {
            space: *space,
            from: *from,
            logged: logged.ok()?,
        })
    }
}

/// §1.8 A scope's filter over facts, in order: the changes they make to the
/// rows it holds — an edit across its line as the add or the remove it is.
/// `holds` is the scope's verdict on a row of a table; a scope is a
/// function of the row alone, so the filter keeps nothing.
fn scoped(facts: &[Change], mut holds: impl FnMut(&str, &[u8]) -> Result<bool, String>) -> Result<Vec<Change>, String> {
    let mut out = vec![];
    for c in facts {
        let t = || c.table().to_string();
        let kept = match c {
            Change::Add { row, .. } | Change::Remove { row, .. } => holds(c.table(), row)?.then(|| c.clone()),
            Change::Edit { old, new, .. } => match (holds(c.table(), old)?, holds(c.table(), new)?) {
                (true, true) => Some(c.clone()),
                (true, false) => Some(Change::Remove {
                    table: t(),
                    row: old.clone(),
                }),
                (false, true) => Some(Change::Add {
                    table: t(),
                    row: new.clone(),
                }),
                (false, false) => None,
            },
        };
        out.extend(kept);
    }
    Ok(out)
}
