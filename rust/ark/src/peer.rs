//! §1.7 One peer machine: a log, a store, the functions it can run, its
//! own intents pending, and a role.
//!
//! ```text
//! Role  = Replica | Authority { journal }
//! Input = Frame { from, bytes } | Mutate { fn, input, now, cause } | Open | Connected
//! Output= Send(Back | All | Session | Disk) | Changes(by span) | Settled(id, Ok | Err(why))
//! ```
//!
//! Sans-io: [`Peer::step`] takes one input and says what to do — no
//! socket, clock or file is inside. A hub, a client, a simulator and a
//! trace player are drivers that feed it and route what it says: a frame
//! `Send` back where the input came from (a replica's upstream), to every
//! connection, or to one session's; and a record `Send` to the disk, written
//! before anything after it is sent: a row appended to the store's `_log` table, which is the log
//! (§1.6) and holds the entry's facts, or the store's rows whole. `from` on a frame is the identity the driver verified for its
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
//! the next of its origin, naming a function the authority runs, is judged
//! first; one at or below `last` is in the log, and one past a gap or
//! naming no function here is not judged — its `local` not passed, so one
//! pushed after it is not judged either. A device's intent at or below
//! `last` is confirmed; a device whose oldest pending intent is
//! `last + 1` waits, and pushes it again on its next connection; one past
//! it no longer holds the ones between — its disk lost them, or the
//! authority's log is older than the one that judged them — and numbers its
//! pending intents on from `last` and pushes them again
//! ([`Peer::close_gap`]). A device is never stuck
//! behind an intent nobody holds.
//!
//! **A schema grows** (§1.2). A peer reads the rows and facts of any schema
//! its own grows from or into ([`crate::schema::Table::fit`]), so a replica
//! of an older or a newer build than its authority syncs with it. A schema
//! only grows: rows move within it by an export, a transform and an import
//! ([`crate::dynamic`]).
//!
//! **A fill** is how a replica the log cannot page is sent the rows: in
//! parts of about [`PART`] bytes, the tables a domain sends later
//! after the rest, each part asked for (`More`) once the one before is
//! held, so what the authority logs meanwhile is never behind more than a
//! part. A part is no frozen snapshot: it is the rows of its key ranges as
//! they stand at the authority's head when it is sent, and every entry
//! before that head was sent before it, so a replica at that head takes
//! the ranges whole and is right there. Entries come as they are logged,
//! their facts put as they are over rows not all sent yet; nothing is
//! judged. A replica makes nothing durable while filling, and the store
//! whole when the last part is taken: one reopened mid-fill fills again.
//! Its own intents are the authority's to judge, one its store refuses
//! pending unrun. A union fed by a scope, and the volatile space, are sent
//! as fills too.
//!
//! **An authority's own facts** are an intent naming no function, its input
//! their encoding: run, they are applied as they are, judged by the store as
//! any write is, and logged with no input — the facts are the entry, and
//! nothing runs it again. Only an authority's own: one pushed is not judged.
//!
//! **Partial sync is a view** (§1.8). A connection whose identity the
//! authority's feed ([`Peer::feed`]) gives a scope is served its union: the rows its scope —
//! a domain's function of its own kind, asked of a table's row — holds,
//! under the connection's identity. At
//! its `Hello` the authority takes the union from its store and sends it as
//! a fill of one part at the head; every entry after is sent to it alone
//! (`To::Session`) with the facts the scope's filter makes of the entry's
//! (`scoped`), the union's state hash after them, and a
//! chain of its own from that fill. `To::All` is for connections fed
//! whole. The replica is the same machine: it applies facts like every
//! replica, its own intents previewed over what it holds and confirmed by
//! the authority's facts, a difference being the authority's answer. A
//! union must hold the rows its rows reference; a `Hello` from a
//! connection whose union does not is answered with nothing.
//!
//! **Offline is not a mode.** A replica with no server, or one not reached
//! for a while, is a replica not connected: its intents stay pending, the
//! store their preview, and are pushed when it connects. What a driver
//! keeps of them on its disk — each intent and its preview's changes, and
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
//! chain its replicas lack, so each is filled from it, and judges none of
//! their pushes past what it judged until they number them on from its
//! `last`.
//!
//! **The horizon** is moved by the machine as its [`Fold`] says, after a
//! step, in either role, up to the head: a replica's pending
//! intents are rebased by their own changes over the head, never by an
//! entry below it, so a replica folds as an authority does.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use crate::domain::Domain;
use crate::format::Encode;
use crate::frames::{Frame, Verdict};
use crate::hash::EntryHash;
use crate::log::{Cursor, Entry, EntryId, Logged, Origin, Part, Range, Record, Rows, Seq, Span};
use crate::schema::{Schema, Space};
use crate::store::{Change, Log, Store, LOG};
use crate::Refusal;

/// An event of the machine's, `tracing`'s at target `ark`, said where what
/// it says happens: an entry's with its id, its trace and span (an entry's
/// span is its id) and the span it was a child of, so that one tap's trace
/// is read across every peer that saw it. The driver's span around a step
/// names the peer and the space.
macro_rules! said {
    ($level:ident, $what:expr, $e:expr $(, $k:ident = $v:expr)*) => {{
        let e: &Entry = $e;
        let s = e.span();
        let parent = e.cause.map(|c| crate::hash::hex(&c.id)).unwrap_or_default();
        tracing::$level!(target: "ark", entry = %crate::hash::hex(&e.id.0), trace = %crate::hash::hex(&s.trace.0),
            span = %crate::hash::hex(&s.id), parent = %parent, func = %e.func, actor = %e.actor,
            session = %e.session, local = e.local $(, $k = $v)*, "{}", $what)
    }};
}

/// What a function knows of the intent it runs for: all frozen in the
/// entry, so every run of it, anywhere, is the same.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ctx {
    pub id: EntryId,
    pub actor: String,
    pub session: String,
    pub roles: Vec<String>,
    pub now: i64,
    /// The entry's own span (none outside an entry): what a function, a job
    /// or a view may say it acted under.
    pub span: Option<Span>,
    /// How many row ids this run has drawn ([`Ctx::new_id`]).
    pub draws: std::cell::Cell<i64>,
}

impl Ctx {
    pub fn of(e: &Entry) -> Ctx {
        Ctx {
            id: e.id,
            actor: e.actor.clone(),
            session: e.session.clone(),
            roles: e.roles.clone(),
            now: e.now,
            span: Some(e.span()),
            ..Ctx::default()
        }
    }

    /// `who`'s at `now`, in no entry: a query's or a view's context.
    pub fn new(who: &Who, now: i64) -> Ctx {
        Ctx {
            actor: who.actor.clone(),
            session: who.session.clone(),
            roles: who.roles.clone(),
            now,
            ..Ctx::default()
        }
    }

    /// The next row id this run draws: the entry's `n`-th, from 0 (§1.6).
    /// With `now`, a function's only non-determinism, frozen in the entry.
    pub fn new_id<R>(&self) -> crate::row::Id<R> {
        let n = self.draws.get();
        self.draws.set(n + 1);
        crate::row::Id::new(self.id.draw(n))
    }

    /// Who the intent is from.
    pub fn user(&self) -> &str {
        &self.actor
    }

    /// The device the intent came from: its session.
    pub fn device(&self) -> &str {
        &self.session
    }

    /// Whether the intent was judged under `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Who a peer authors as, and who a connection was verified to be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Who {
    pub actor: String,
    pub session: String,
    pub roles: Vec<String>,
}

/// What an authority keeps beside its log: each origin's last `local` and
/// the actor it is bound to; the intents it refused, and why — an intent
/// at or below its origin's last and not refused is in the log, or folded
/// below its horizon.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Journal {
    pub refused: BTreeMap<EntryId, String>,
    pub last: BTreeMap<Origin, (String, i64)>,
}

impl Journal {
    /// The last `local` judged from `origin`.
    pub fn last_of(&self, origin: &Origin) -> i64 {
        self.last.get(origin).map_or(0, |l| l.1)
    }

    /// `origin`'s last `local` moved up to `local`; bound to `actor` when it
    /// was bound to nobody.
    pub fn pass(&mut self, origin: Origin, actor: &str, local: i64) {
        let l = self.last.entry(origin).or_insert_with(|| (actor.into(), 0));
        l.1 = l.1.max(local);
    }

    /// Whether `origin` is bound to an actor other than `actor`.
    fn others(&self, origin: &Origin, actor: &str) -> bool {
        self.last.get(origin).is_some_and(|l| l.0 != actor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Replica,
    Authority { journal: Journal },
}

/// A connection an authority serves by a scope: who it is, the scope, the
/// union it holds, and the head of the chain it was sent.
#[derive(Clone, Debug)]
pub struct Served {
    pub who: Who,
    pub scope: String,
    pub union: Store,
    pub head: Option<(Seq, EntryHash)>,
}

/// The scope a connection is fed by, by who it is (§1.8): its union, by
/// the scope's name; none, the log whole.
pub type Scoping = Arc<dyn Fn(&Who) -> Option<String> + Send + Sync>;

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
pub enum Input {
    Frame {
        from: Option<Who>,
        bytes: Vec<u8>,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// A frame, or a record ([`Record`]), and where it goes.
    Send(To, Vec<u8>),
    /// What changed, each change with the span of the entry that made it.
    Changes(Vec<Told>),
    /// What became of an intent of this peer's own: confirmed, or refused
    /// and why.
    Settled(EntryId, Result<(), String>),
}

/// Where a frame goes: back where the input came from (a replica's
/// upstream, or the connection a frame came on), to every connection fed
/// the log whole, or to the connections of one session fed by a scope; and
/// where a record goes, the disk, before anything after it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum To {
    Back,
    All,
    Session(String),
    Disk,
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
        self.out.push(Output::Send(To::Back, f.enc()));
    }
    fn tell(&mut self, by: Option<Span>, changes: impl IntoIterator<Item = Change>) {
        self.told.extend(changes.into_iter().map(|change| Told { by, change }));
    }
    fn persist(&mut self, r: Record) {
        self.out.push(Output::Send(To::Disk, r.enc()));
    }
}

#[derive(Clone)]
pub struct Peer {
    /// The confirmed state with the pending intents' changes applied, and
    /// the log as its table (§1.6).
    pub store: Store,
    /// What it runs, by name, its schema and which tables a fill sends
    /// last (§1.16): its domain's.
    pub domain: Arc<Domain>,
    /// An authority's: which scope feeds each connection, by who it is
    /// (none: every one the log whole).
    pub feed: Option<Scoping>,
    /// This peer's own intents not yet sequenced, each at sequence 0 with
    /// what its last run changed as its facts ([`Logged::intent`]).
    pub pending: VecDeque<Logged>,
    /// How many times the pending intents were run again: what a driver
    /// keeping them compares, to know when to write them whole rather than
    /// the ones taken off the front and put on the back.
    pub reruns: u64,
    pub role: Role,
    pub me: Who,
    pub space: Space,
    /// Where this peer's intents come from (§1.6), and the last `local` it
    /// drew.
    pub origin: Origin,
    pub local: i64,
    /// Where a confirmed entry did not apply here: each time, the peer was
    /// filled again from nothing.
    pub diverged: Vec<Seq>,
    /// The cursor a `Hello` is outstanding at.
    asked: Option<Seq>,
    /// The connections an authority serves by a scope, by session.
    pub served: BTreeMap<String, Served>,
    /// When it folds its log (none: never).
    pub fold: Option<Fold>,
    /// A replica filling: where its next part begins. Nothing is made
    /// durable until the fill is done.
    pub fill: Option<Cursor>,
    /// The part a replica filling has asked for and not been sent.
    more: Option<Cursor>,
    /// How many bytes a replica would have each frame it is sent of rows or
    /// entries carry — a part of a fill, a page of the log — and each page
    /// of its own pushes, about, said in each `Hello` and `More`; none:
    /// [`PART`].
    pub part: Option<usize>,
}

/// How many bytes of rows a fill's part, or of entries a page, carries at
/// most, about: past it, the frame ends at the row or the entry after; none
/// is cut, and each frame holds one. What a device taking frames on a
/// thread of its own is sent; one taking them on the thread it draws on
/// asks for less ([`Peer::part`]): a `Hello` and a `More` say.
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

/// Facts as some schema wrote them, read under `schema` (§1.2): rows
/// completed or cut to their prefix, changes to tables it lacks dropped.
fn fit(schema: &Schema, facts: &[Change]) -> Vec<Change> {
    let fitted = |c: &Change| match schema.table(c.table()) {
        Some((_, t)) => Some(c.map(|r| t.fit(r).unwrap_or_else(|_| r.to_vec()))),
        None => (c.table() == LOG).then(|| c.clone()),
    };
    facts.iter().filter_map(fitted).collect()
}

/// A store with an empty log: one standing on the empty chain.
fn started(mut store: Store) -> Store {
    let base = Logged::base(0, EntryHash::default());
    store.start_log([base.to_bytes()]).expect("a first row starts a log");
    store
}

impl Peer {
    /// A peer from empty; its origin named by its session's bytes until
    /// its driver gives it the one it drew ([`Origin`]).
    pub fn new(domain: Arc<Domain>, role: Role, me: Who) -> Peer {
        let store = started(Store::new(domain.schema.clone(), Space::Durable));
        let origin = Origin::named(&me.session);
        Peer {
            store,
            domain,
            feed: None,
            pending: VecDeque::new(),
            reruns: 0,
            role,
            me,
            space: Space::Durable,
            origin,
            local: 0,
            diverged: vec![],
            asked: None,
            served: BTreeMap::new(),
            fold: None,
            fill: None,
            more: None,
            part: None,
        }
    }

    /// This peer over `space`'s tables (§1.9), from empty.
    pub fn in_space(mut self, space: Space) -> Peer {
        self.space = space;
        self.store = self.blank();
        self
    }

    /// An empty store of this peer's space, its log empty.
    fn blank(&self) -> Store {
        started(Store::new(self.store.schema_arc().clone(), self.space))
    }

    /// The log, as the store's table holds it (§1.6).
    pub fn log(&self) -> Log<'_> {
        self.store.log()
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
    /// The entry links onto the head: this peer chained it, or checked it.
    fn logged(&mut self, l: &Logged, o: &mut Out) {
        debug_assert!(l.seq == self.log().head().0 + 1 && l.links(&self.log().head().1));
        let row = l.to_bytes();
        if self.fill.is_none() {
            o.out.push(Output::Send(To::Disk, Record::logged(&row)));
        }
        self.store.log_push(l);
    }

    fn of_rows(&self, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, Refusal> {
        Store::from_rows(self.store.schema_arc().clone(), self.space, rows)
    }

    /// A store of these rows, and a log standing on `log`.
    fn on_fit(&self, rows: &[(String, Vec<Vec<u8>>)], log: impl IntoIterator<Item = Vec<u8>>) -> Result<Store, Refusal> {
        let mut s = self.of_rows(rows)?;
        s.start_log(log)?;
        Ok(s)
    }

    /// The id this peer's next intent will have (§1.6).
    pub fn next_id(&self) -> EntryId {
        EntryId::of(self.origin, self.local + 1)
    }

    pub fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority { .. })
    }

    fn journal(&mut self) -> &mut Journal {
        match &mut self.role {
            Role::Authority { journal } => journal,
            Role::Replica => panic!("a replica keeps no journal"),
        }
    }

    pub fn step(&mut self, input: Input) -> Vec<Output> {
        let mut o = Out::default();
        match input {
            Input::Frame { from, bytes } => {
                if let Ok(f) = Frame::dec(&bytes) {
                    self.frame(from, f, &mut o)
                }
            }
            Input::Mutate { func, input, now, cause } => self.mutate(func, input, now, cause, &mut o),
            Input::Open(d) => self.open(d, &mut o),
            Input::Connected => self.connected(&mut o),
        }
        let (head, base, rows) = (self.log().head().0, self.log().base_seq(), self.store.held());
        if let Some(f) = self.fold.filter(|f| head - base > (f.keep + f.slack + f.rows as usize * rows) as i64) {
            self.compact_to(head - f.keep as i64, &mut o);
        }
        if !o.told.is_empty() {
            o.out.insert(0, Output::Changes(o.told));
        }
        if self.space == Space::Volatile {
            o.out.retain(|x| !matches!(x, Output::Send(To::Disk, _)));
        }
        o.out
    }

    // * Running

    /// Runs an intent over the store and applies what it did, all or none.
    fn attempt(&mut self, e: &Entry, o: &mut Out) -> Result<Vec<Change>, Refusal> {
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
            None if f.is_some() => return Err(Refusal(format!("{} writes nothing", e.func))),
            None => return Err(Refusal(format!("no function {}", e.func))),
        };
        let s = e.span();
        let run = tracing::debug_span!(target: "ark", "run", func = %e.func, trace = %crate::hash::hex(&s.trace.0), span = %crate::hash::hex(&s.id));
        let changes = run.in_scope(|| p(&mut self.store, &Ctx::of(e), &e.input))?;
        let schema = self.store.schema_arc().clone();
        if let Some(c) = changes.iter().find(|c| schema.table(c.table()).is_some_and(|t| t.1.space != self.space)) {
            self.store.undo(&changes).expect("an inverse applies");
            return Err(Refusal(format!("{} is not in this space", c.table())));
        }
        o.tell(Some(e.span()), changes.iter().cloned());
        Ok(changes)
    }

    fn undo_pending(&mut self, o: &mut Out) {
        for p in self.pending.iter().rev() {
            let undone = self.store.undo(&p.facts).expect("a pending change undoes");
            o.tell(Some(p.entry.span()), undone);
        }
    }

    /// Runs the pending intents again, in order, over the store as it is;
    /// one that now refuses stays pending with no changes, the authority's
    /// to judge.
    fn redo_pending(&mut self, o: &mut Out) {
        self.reruns += 1;
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

    /// The next intent of this peer's own.
    fn entry(&self, func: String, input: Vec<u8>, now: i64) -> Entry {
        Entry::new(&self.me, self.origin, self.local + 1, func, input, now)
    }

    fn mutate(&mut self, func: String, input: Vec<u8>, now: i64, cause: Option<Span>, o: &mut Out) {
        let mut entry = Entry {
            cause,
            ..self.entry(func, input, now)
        };
        let (id, local) = (entry.id, entry.local);
        said!(debug, "entry", &entry);
        if self.is_authority() {
            let run = self.attempt(&entry, o);
            if entry.func.is_empty() {
                // Its facts are the entry.
                entry.input.clear();
            }
            return self.decide(entry, run, o);
        }
        // Filling, a refusal may be of rows not sent yet: the authority's to
        // judge, the intent pending unrun.
        match self.attempt(&entry, o) {
            Err(why) if self.fill.is_none() => {
                said!(warn, "refused", &entry, why = why.0.as_str());
                o.out.push(Output::Settled(id, Err(why.0)))
            }
            run => {
                self.local = local;
                o.send(Frame::push(self.space, [entry.clone()]));
                self.pending.push_back(Logged::intent(entry, run.unwrap_or_default()));
            }
        }
    }

    /// An authority's own intent judged by what its run did — a function's
    /// or its driver's facts — and the verdict told: logged, its `local`
    /// drawn, or refused, drawing none.
    fn decide(&mut self, entry: Entry, run: Result<Vec<Change>, Refusal>, o: &mut Out) {
        let (id, local) = (entry.id, entry.local);
        match self.judged(entry, run, o) {
            Ok(_) => {
                self.local = self.local.max(local);
                o.out.push(Output::Settled(id, Ok(())));
            }
            Err(why) => o.out.push(Output::Settled(id, Err(why))),
        }
    }

    /// An intent judged: logged with its facts and told everyone, or
    /// refused; the verdict kept.
    fn judged(&mut self, entry: Entry, r: Result<Vec<Change>, Refusal>, o: &mut Out) -> Result<Seq, String> {
        let (id, origin, local, actor) = (entry.id, entry.origin, entry.local, entry.actor.clone());
        let v = match r {
            Err(why) => Err(why.0),
            Ok(facts) => {
                let (n, prev) = self.log().head();
                let hash = Logged::chain(&prev, n + 1, &entry, &facts);
                let l = Logged {
                    seq: n + 1,
                    entry,
                    facts,
                    hash,
                };
                self.logged(&l, o);
                o.out.push(Output::Send(
                    To::All,
                    Frame::Page {
                        space: self.space,
                        from: l.seq - 1,
                        logged: vec![l.clone()],
                    }
                    .enc(),
                ));
                self.feed_scoped(&l, o);
                Ok(l.seq)
            }
        };
        match &v {
            Ok(seq) => tracing::debug!(target: "ark", entry = %crate::hash::hex(&id.0), seq, "ok"),
            Err(why) => tracing::warn!(target: "ark", entry = %crate::hash::hex(&id.0), why = %why, "refused"),
        }
        self.verdict(origin, &actor, local, id, v.as_ref().err());
        v
    }

    /// An intent judged: its origin's `local` passed, and a refusal kept.
    fn verdict(&mut self, origin: Origin, actor: &str, local: i64, id: EntryId, refused: Option<&String>) {
        let j = self.journal();
        j.pass(origin, actor, local);
        if let Some(why) = refused {
            j.refused.insert(id, why.clone());
        }
    }

    // * Frames

    fn frame(&mut self, from: Option<Who>, f: Frame, o: &mut Out) {
        if f.space() != self.space {
            return;
        }
        let scope = from.as_ref().zip(self.feed.as_ref()).and_then(|(w, feed)| feed(w));
        let scoped = scope.is_some();
        // Never past [`PART`].
        let most = |most: u64| (most as usize).clamp(1, PART);
        match (f, self.is_authority(), from) {
            (Frame::Hello { cursor, hash, .. }, true, Some(who)) if scoped => self.serve_scoped(who, scope.unwrap(), cursor, hash, o),
            (Frame::Hello { cursor, hash, most: m, .. }, true, from) => {
                if let Some(w) = from {
                    self.served.remove(&w.session);
                }
                self.serve(cursor, hash, most(m), o)
            }
            // A peer's intents, each a push.
            (Frame::Page { logged, .. }, true, Some(who)) => logged.into_iter().for_each(|l| self.push(&who, l.entry, o)),
            (Frame::Page { logged, .. }, false, _) => self.confirm(logged, o),
            // A union fed by a scope is one part: asked for again, it is
            // sent afresh.
            (Frame::More { .. }, true, Some(who)) if scoped => {
                self.served.remove(&who.session);
                self.serve_scoped(who, scope.unwrap(), 0, None, o)
            }
            (Frame::More { from, most: m, .. }, true, _) => o.send(Frame::Part {
                space: self.space,
                part: part(&self.store, &self.fill_order(), self.log().head(), Some(from), most(m)),
            }),
            (Frame::Part { part, .. }, false, _) => self.take(part, o),
            (Frame::Verdict { id, verdict, .. }, false, _) => match verdict {
                // In the log at or below where this peer stands, an intent
                // still pending here is in the state it holds (a fill carried
                // it); and past where its origin stands, a gap is closed.
                Verdict::At { seq, last } => {
                    let taken = seq <= self.log().head().0 && self.pending.iter().any(|p| p.entry.id == id && p.entry.local <= last);
                    if taken && self.settle(&id, o) {
                        o.out.push(Output::Settled(id, Ok(())));
                    }
                    self.close_gap(last, o)
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

    /// A `Hello` at `(cursor, hash)`: the pages from there, each of about
    /// `most` bytes, when this log has that hash there, and a fill's first
    /// part, of about `most` bytes, when it does not.
    fn serve(&mut self, cursor: Seq, hash: Option<EntryHash>, most: usize, o: &mut Out) {
        let has = match hash {
            Some(h) => self.log().has(cursor, &h),
            None => cursor == 0 && self.log().base_seq() == 0,
        };
        if !has {
            let part = part(&self.store, &self.fill_order(), self.log().head(), None, most);
            return o.send(Frame::Part { space: self.space, part });
        }
        let after: Vec<Logged> = self.log().after(cursor).collect();
        for page in pages(&after, most, Logged::weight) {
            o.send(Frame::Page {
                space: self.space,
                from: page[0].seq - 1,
                logged: page.to_vec(),
            });
        }
    }

    /// The tables of this peer's space in the order a fill sends them: the
    /// schema's, those its domain sends later after the rest.
    fn fill_order(&self) -> Vec<String> {
        let tables = self.store.schema_arc().tables.iter().filter(|t| t.space == self.space);
        let (later, first): (Vec<_>, Vec<_>) = tables.map(|t| t.name.clone()).partition(|t| self.domain.later.contains(t));
        first.into_iter().chain(later).collect()
    }

    /// A scope's verdict on a row of `table`, for the connection it serves.
    fn holder(&self, session: &str) -> impl FnMut(&str, &[u8]) -> Result<bool, Refusal> {
        let s = &self.served[session];
        let scope = self.domain.fns.get(&s.scope).and_then(|f| f.scope.clone());
        let ctx = Ctx::new(&s.who, 0);
        move |table, row| scope.as_ref().ok_or_else(|| Refusal::new("no such scope"))?(&ctx, table, row)
    }

    /// A `Hello` from a connection fed by a scope: nothing when it stands at
    /// the head of what it was sent, and otherwise its union, afresh, as a
    /// fill of one part at the head.
    fn serve_scoped(&mut self, who: Who, scope: String, cursor: Seq, hash: Option<EntryHash>, o: &mut Out) {
        let session = &who.session.clone();
        if self.served.get(session).is_none_or(|s| (&s.who, &s.scope) != (&who, &scope)) {
            let union = self.blank();
            let s = Served {
                who,
                scope,
                union,
                head: None,
            };
            self.served.insert(session.clone(), s);
        }
        let head = self.log().head();
        if self.served[session].head == Some(head) && hash == Some(head.1) && cursor == head.0 {
            return;
        }
        let mut holds = self.holder(session);
        let mut rows = vec![];
        for (t, rs) in self.store.snapshot_rows() {
            let mut held = vec![];
            for r in rs {
                match holds(&t, &r) {
                    Ok(true) => held.push(r),
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
            rows.push((t, held));
        }
        let Ok(union) = self.of_rows(&rows) else {
            return;
        };
        let part = part(&union, &self.fill_order(), head, None, usize::MAX);
        let s = self.served.get_mut(session).unwrap();
        (s.union, s.head) = (union, Some(head));
        let bytes = Frame::Part { space: self.space, part }.enc();
        o.out.push(Output::Send(To::Session(session.into()), bytes));
    }

    /// A logged entry to every connection fed by a scope that stands just
    /// below it: its facts through the scope, the union's state after them,
    /// chained from what the connection was sent.
    fn feed_scoped(&mut self, l: &Logged, o: &mut Out) {
        let sessions: Vec<String> = self.served.keys().cloned().collect();
        for session in sessions {
            let Some((at, prev)) = self.served[&session].head.filter(|h| h.0 + 1 == l.seq) else {
                continue;
            };
            let facts = scoped(&l.facts, &mut self.holder(&session));
            let s = self.served.get_mut(&session).unwrap();
            let Some(facts) = facts.ok().filter(|fs| s.union.apply_all(fs).is_ok()) else {
                s.head = None;
                continue;
            };
            let hash = Logged::chain(&prev, l.seq, &l.entry, &facts);
            s.head = Some((l.seq, hash));
            let logged = vec![Logged {
                seq: l.seq,
                entry: l.entry.clone(),
                facts,
                hash,
            }];
            let bytes = Frame::Page {
                space: self.space,
                from: at,
                logged,
            }
            .enc();
            o.out.push(Output::Send(To::Session(session), bytes));
        }
    }

    /// A pushed intent judged, and the verdict sent back: every push is
    /// answered.
    fn push(&mut self, who: &Who, entry: Entry, o: &mut Out) {
        let id = entry.id;
        let verdict = self.verdict_on(who, entry, o);
        o.send(Frame::Verdict {
            space: self.space,
            id,
            verdict,
        });
    }

    fn verdict_on(&mut self, who: &Who, mut entry: Entry, o: &mut Out) -> Verdict {
        let head = self.log().head().0;
        let j = self.journal();
        if let Some(why) = j.refused.get(&entry.id) {
            return Verdict::Refused { why: why.clone() };
        }
        if !entry.id_checks() || j.others(&entry.origin, &who.actor) {
            let why = "an id that is not its own, or from an origin of another actor".into();
            return Verdict::Refused { why };
        }
        let last = j.last_of(&entry.origin);
        // Not the next of its origin, or naming no function here: where its
        // origin stands. At or below the last and not refused, it is in the
        // log at or below the head — every entry up to which this connection
        // is sent before the verdict; past it, not judged, for a device that
        // lost the ones between to go on from.
        let known = self.domain.fns.contains_key(&entry.func);
        if entry.local != last + 1 || !known {
            match entry.local > last {
                true if entry.local == last + 1 => said!(warn, "held", &entry, last = last),
                true => said!(warn, "gap", &entry, last = last),
                false => {}
            }
            return Verdict::At { seq: head, last };
        }
        (entry.actor, entry.session, entry.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        let (local, run) = (entry.local, self.attempt(&entry, o));
        self.judged(entry, run, o)
            .map_or_else(|why| Verdict::Refused { why }, |seq| Verdict::At { seq, last: local })
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
            let (n, prev) = self.log().head();
            if l.seq <= n {
                continue;
            }
            if l.seq > n + 1 || !l.links(&prev) {
                // A gap, or another history: say where this peer stands.
                if undone {
                    self.redo_pending(o);
                }
                return self.ask(o);
            }
            let previewed = !undone && self.pending.front().is_some_and(|p| p.entry.id == l.entry.id && p.facts == l.facts);
            if previewed {
                self.pending.pop_front();
            } else {
                if !undone {
                    self.undo_pending(o);
                    undone = true;
                }
                let schema = self.store.schema_arc().clone();
                let moved = match self.fill {
                    Some(_) => self.store.force_all(&fit(&schema, &l.facts)).ok(),
                    None => match self.store.apply_all(&l.facts) {
                        Ok(()) => Some(l.facts.clone()),
                        Err(_) => Some(fit(&schema, &l.facts)).filter(|f| self.store.apply_all(f).is_ok()),
                    },
                };
                let Some(moved) = moved else {
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

    fn ask(&mut self, o: &mut Out) {
        let (n, h) = self.log().head();
        if self.asked != Some(n) {
            self.asked = Some(n);
            o.send(self.hello(n, Some(h)));
        }
    }

    /// A `Hello` at `cursor`: where this peer stands, and what it runs.
    fn hello(&self, cursor: Seq, hash: Option<EntryHash>) -> Frame {
        Frame::Hello {
            space: self.space,
            cursor,
            hash,
            schema: self.store.schema_hash(),
            most: self.most(),
        }
    }

    /// The most bytes of rows this replica would have a part carry.
    fn most(&self) -> u64 {
        self.part.unwrap_or(PART) as u64
    }

    /// Facts that do not apply here: the fill begun again, from nothing —
    /// its first part asked for (§1.7).
    fn diverge(&mut self, seq: Seq, o: &mut Out) {
        tracing::warn!(target: "ark", seq, "diverged");
        self.diverged.push(seq);
        self.undo_pending(o);
        let fresh = self.blank();
        o.tell(None, self.store.diff(&fresh));
        self.store = fresh;
        self.redo_pending(o);
        (self.fill, self.more, self.asked) = (Some(Cursor::default()), None, None);
        self.ask_more(o);
    }

    /// A part of a fill. The first begins one — unless this peer stands
    /// where it was taken, filled already — the store emptied, the log
    /// standing on where the part was taken: an authority is always right,
    /// one restarted behind its replicas too. Any other is the one asked
    /// for, at the head — every entry the authority logged before it was
    /// sent before it — or it is passed by: past a gap, the head asked for
    /// again, and the part. Its ranges replace what the store held in
    /// them, judging nothing (a row may name one not sent yet: every row is
    /// the authority's, judged where it was written). The last is the fill
    /// done, and the store made durable whole; until then nothing is, so a
    /// peer reopened mid-fill fills again. The pending intents run again on
    /// top of each.
    fn take(&mut self, p: Part, o: &mut Out) {
        let (n, h) = self.log().head();
        let asked = self.more.take();
        if p.first {
            if self.fill.is_none() && (p.seq, p.hash) == (n, h) {
                return;
            }
        } else if self.fill.is_none() || asked.is_none() || asked != p.from() {
            self.more = asked;
            return;
        } else if (n, h) != (p.seq, p.hash) {
            self.asked = None;
            self.ask(o);
            return self.ask_more(o);
        }
        self.undo_pending(o);
        if p.first {
            let mut fresh = Store::new(self.store.schema_arc().clone(), self.space);
            fresh
                .start_log([Logged::base(p.seq, p.hash).to_bytes()])
                .expect("a first row starts a log");
            o.tell(None, self.store.diff(&fresh));
            self.store = fresh;
        }
        for r in &p.ranges {
            match self.store.replace(&r.table, &r.from, r.to.as_deref(), &r.rows) {
                Ok(moved) => o.tell(None, moved),
                Err(_) => {
                    self.redo_pending(o);
                    return self.diverge(p.seq, o);
                }
            }
        }
        self.fill = p.next.clone();
        if self.fill.is_none() {
            tracing::info!(target: "ark", seq = p.seq, "filled");
        }
        match self.fill {
            Some(_) => self.ask_more(o),
            None => o.persist(Record::Rows(self.store.kv_rows())),
        }
        self.redo_pending(o);
        if p.first {
            self.asked = None;
            self.repush(o);
        }
    }

    /// The next part of a fill asked for, unless one is asked already.
    fn ask_more(&mut self, o: &mut Out) {
        let Some(next) = &self.fill else { return };
        if self.more.is_none() {
            self.more = Some(next.clone());
            o.send(Frame::More {
                space: self.space,
                from: next.clone(),
                most: self.most(),
            });
        }
    }

    fn repush(&mut self, o: &mut Out) {
        let most = self.most() as usize;
        for page in pages(self.pending.make_contiguous(), most, Logged::weight) {
            o.out.push(Output::Send(
                To::Back,
                Frame::push(self.space, page.iter().map(|p| p.entry.clone())).enc(),
            ));
        }
    }

    /// The authority has judged this origin's intents up to `last` and no
    /// further. Pending from `last + 1` on, this device is pushing them in
    /// order (or the authority holds the first): it waits. Pending from
    /// past it, the ones between are nowhere — lost from this device's
    /// disk, or judged by a log the authority no longer is — and nothing
    /// would ever fill them: the intents pending are this origin's from
    /// `last + 1` on, run again (an intent's ids are drawn from its own),
    /// and pushed again. None of them has been judged: the authority's last
    /// is below them all.
    fn close_gap(&mut self, last: i64, o: &mut Out) {
        if self.pending.front().is_none_or(|p| p.entry.local <= last + 1) {
            return;
        }
        self.undo_pending(o);
        for (n, p) in self.pending.iter_mut().enumerate() {
            p.entry.local = last + 1 + n as i64;
            p.entry.id = EntryId::of(self.origin, p.entry.local);
        }
        self.local = last + self.pending.len() as i64;
        self.redo_pending(o);
        self.repush(o);
    }

    // * Lifecycle

    fn connected(&mut self, o: &mut Out) {
        if self.is_authority() {
            return;
        }
        self.asked = None;
        self.ask(o);
        self.repush(o);
        // What was asked on the connection before is not coming.
        self.more = None;
        self.ask_more(o);
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
    fn open(&mut self, records: Vec<Record>, o: &mut Out) {
        let (mut rows, mut logged, mut pending, mut at, mut local) = (vec![], vec![], vec![], None, 0);
        for r in records {
            match r {
                Record::Rows(r) => (rows, logged) = (r, vec![]),
                Record::Logged(l) => logged.push(l),
                Record::Pending { pending: p, at: a, local: n } => (pending, at, local) = (p, a, n),
            }
        }
        self.undo_pending(o);
        self.pending.clear();
        let filling = self.fill.take().is_some();
        self.more = None;
        let (log, rows): (Rows, Rows) = rows.into_iter().partition(|(t, _)| t == LOG);
        let log = log.into_iter().flat_map(|(_, rs)| rs);
        let opened = match filling {
            true => Ok(self.blank()),
            false => self.on_fit(&rows, log),
        };
        if let Ok(mut store) = opened {
            // Every entry read once: those the rows hold, from their rows,
            // and those appended, as their records were read.
            let mut entries = store.log().entries();
            let (n, held) = (logged.len(), entries.len());
            let schema = store.schema_arc().clone();
            for l in logged.into_iter().filter(|_| !filling) {
                if store.append(&l, &l.facts).is_err() && store.append(&l, &fit(&schema, &l.facts)).is_err() {
                    break;
                }
                entries.push(l);
            }
            if entries.len() - held < n {
                o.persist(Record::Rows(store.kv_rows()));
            }
            o.tell(None, self.store.diff(&store));
            self.store = store;
            if let Role::Authority { journal } = &mut self.role {
                for l in &entries {
                    journal.pass(l.entry.origin, &l.entry.actor, l.entry.local);
                }
            }
        }
        self.local = self.local.max(local);
        let mut kept = at.is_some() && at == Some(self.log().head());
        for p in pending {
            kept = kept && self.store.apply_all(&p.facts).is_ok();
            let changes = match kept {
                true => {
                    o.tell(Some(p.entry.span()), p.facts.iter().cloned());
                    p.facts
                }
                false => self.attempt(&p.entry, o).unwrap_or_default(),
            };
            self.pending.push_back(Logged::intent(p.entry, changes));
        }
        if !kept && !self.pending.is_empty() {
            self.reruns += 1;
        }
    }
}

/// The part of `store`'s fill from `from` (none: the first, from the
/// rows' start), of its rows as they stand at `head`: ranges in `order`, each table's in key order, until about `most`
/// bytes of rows; and where the next begins, unless none is left. A
/// cursor at a table this peer lacks begins the fill again.
fn part(store: &Store, order: &[String], head: (Seq, EntryHash), from: Option<Cursor>, most: usize) -> Part {
    let at = from.as_ref().and_then(|c| order.iter().position(|t| *t == c.table));
    let (start, key) = match (at, from) {
        (Some(i), Some(c)) => (i, c.key),
        _ => (0, vec![]),
    };
    let (mut ranges, mut size, mut next) = (vec![], 0, None);
    for (i, table) in order.iter().enumerate().skip(start) {
        let from = if i == start { key.clone() } else { vec![] };
        let (mut rows, mut to) = (vec![], None);
        for (k, r) in store.keyed_from(table, &from) {
            if size >= most {
                to = Some(k.to_vec());
                break;
            }
            size += r.len();
            rows.push(r.to_vec());
        }
        ranges.push(Range {
            table: table.clone(),
            from,
            to: to.clone(),
            rows,
        });
        next = match (to, order.get(i + 1)) {
            (Some(key), _) => Some(Cursor { table: table.clone(), key }),
            (None, Some(t)) if size >= most => Some(Cursor {
                table: t.clone(),
                key: vec![],
            }),
            _ => None,
        };
        if next.is_some() {
            break;
        }
    }
    let (seq, hash) = head;
    Part {
        seq,
        hash,
        first: at.is_none(),
        ranges,
        next,
    }
}

/// §1.8 A scope's filter over facts, in order: the changes they make to the
/// rows it holds — an edit across its line as the add or the remove it is.
/// `holds` is the scope's verdict on a row of a table; a scope is a
/// function of the row alone, so the filter keeps nothing.
fn scoped(facts: &[Change], mut holds: impl FnMut(&str, &[u8]) -> Result<bool, Refusal>) -> Result<Vec<Change>, Refusal> {
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
