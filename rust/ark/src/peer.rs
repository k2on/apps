//! §1.7 One peer machine: a log, a store, the functions it can run, its
//! own intents pending, and a role.
//!
//! ```text
//! Role  = Replica | Authority { journal }
//! Input = Frame { from, bytes } | Mutate { fn, input, now, cause } | Open | Connected
//! Output= Send(Back | All | Disk) | Changes(by span) | Settled(id, Ok | Err(why))
//! ```
//!
//! Sans-io: [`Peer::step`] takes one input and says what to do — no
//! socket, clock or file is inside. A hub, a client, a simulator and a
//! trace player are drivers that feed it and route what it says: a frame
//! `Send` back where the input came from (a replica's upstream), or to
//! every connection ([`Peer::fed`]); and a record `Send` to the disk, written
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
//! `last` is confirmed. A push says where its device's pending intents
//! begin (its page's `from`, the `local` before the first): the ones of
//! its origin up to there it no longer holds — its disk lost them, or the
//! authority's log is older than the one that judged them — and they are
//! passed, so a device is never stuck behind an intent nobody holds, and
//! an intent's number, and the ids it draws, never change.
//!
//! **A schema grows** (§1.2). A peer's store reads the rows and facts of
//! any schema its own grows from or into ([`crate::schema::Table::fit`],
//! [`Store::apply_facts`]), so a replica of an older or a newer build than
//! its authority syncs with it. A schema
//! only grows: rows move within it by an export, a transform and an import
//! ([`crate::dynamic`]).
//!
//! **A fill** is how a replica the log cannot page is sent the rows: in
//! parts of about [`PART`] bytes, the tables a domain sends later
//! after the rest, each part asked for (a `Hello` saying where the fill
//! stands) once the one before is held, so what the authority logs
//! meanwhile is never behind more than a part. A part is no frozen
//! snapshot: it is the rows of its key ranges as they stand at the
//! authority's head when it is sent, and every entry before that head was
//! sent before it, so a replica at that head puts its rows and is right
//! there. Entries come as they are logged,
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
//! authority's feed ([`Peer::feed`]) gives a scope — a domain's function
//! of its own kind, asked of a table's row under the connection's identity
//! — is sent everything through it, and the authority keeps nothing of it:
//! each entry with the facts the scope's filter makes of its own
//! (`scoped`), and a fill of the rows the scope holds, its union, in one
//! part — sent only when the union holds the rows its rows reference. Its
//! chain is the log's, each hash keyed by who it is and its scope, so a
//! `Hello` stands in the log only for a replica fed as it is now: one fed
//! otherwise is filled afresh. What goes to all goes through each
//! connection's scope ([`Peer::fed`]). The replica is the same machine: it
//! applies facts like every replica, its own intents previewed over what
//! it holds and confirmed by the authority's facts, a difference being the
//! authority's answer. A replica takes the chain it is sent as it is: where
//! it stands is the authority's to judge, at its `Hello`.
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
//! chain its replicas lack, so each is filled from it, and passes what
//! their pushes say they no longer hold, as a durable one does.
//!
//! **The horizon** is moved by the machine as its [`Fold`] says, after a
//! step, in either role, up to the head: a replica's pending
//! intents are rebased by their own changes over the head, never by an
//! entry below it, so a replica folds as an authority does.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use crate::domain::{Domain, Holds};
use crate::format::Encode;
use crate::frames::{Frame, Verdict};
use crate::hash::EntryHash;
pub use crate::log::Ctx;
use crate::log::{Cursor, EntryId, Logged, Origin, Record, Rows, Seq, Span};
use crate::schema::Space;
use crate::store::{Change, Log, Store, LOG};
use crate::Refusal;

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
pub enum Input {
    Frame {
        from: Option<Ctx>,
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
/// upstream, or the connection a frame came on), or to every connection,
/// each through its feed ([`Peer::fed`]); and where a record goes, the
/// disk, before anything after it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum To {
    Back,
    All,
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
    pub me: Ctx,
    pub space: Space,
    /// Where this peer's intents come from (§1.6), and the last `local` it
    /// drew.
    pub origin: Origin,
    pub local: i64,
    /// Where a confirmed entry did not apply here: each time, the peer was
    /// filled again from nothing.
    pub diverged: Vec<Seq>,
    /// The cursor a `Hello` is outstanding at, and the fill's it asked.
    asked: Option<(Seq, Option<Cursor>)>,
    /// When it folds its log (none: never).
    pub fold: Option<Fold>,
    /// A replica filling: where its next part begins (the start: none
    /// taken yet). Nothing is made durable until the fill is done.
    pub fill: Option<Cursor>,
    /// How many bytes a replica would have each frame it is sent of rows or
    /// entries carry — a part of a fill, a page of the log — and each page
    /// of its own pushes, about, said in each `Hello`; none: [`PART`].
    pub part: Option<usize>,
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
            fold: None,
            fill: None,
            part: None,
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
        let row = l.to_bytes();
        if self.fill.is_none() {
            o.out.push(Output::Send(To::Disk, Record::logged(&row)));
        }
        self.store.log_push(l);
    }

    fn of_rows(&self, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, Refusal> {
        Store::from_rows(self.store.schema_arc().clone(), self.space, rows)
    }

    /// The id this peer's next intent will have (§1.6).
    pub fn next_id(&self) -> EntryId {
        EntryId::of(self.origin, self.local + 1)
    }

    pub fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority { .. })
    }

    /// An authority's journal.
    pub fn journal(&mut self) -> &mut Journal {
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
    fn attempt(&mut self, e: &Ctx, o: &mut Out) -> Result<Vec<Change>, Refusal> {
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
        let changes = run.in_scope(|| p(&mut self.store, e, &e.input))?;
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
    fn entry(&self, func: String, input: Vec<u8>, now: i64) -> Ctx {
        Ctx::new(&self.me, self.origin, self.local + 1, func, input, now)
    }

    fn mutate(&mut self, func: String, input: Vec<u8>, now: i64, cause: Option<Span>, o: &mut Out) {
        let mut entry = Ctx {
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
                said!(warn, "refused", &entry, why = why.0.as_str());
                o.out.push(Output::Settled(id, Err(why.0)))
            }
            run => {
                self.local = local;
                let from = self.pending.front().map_or(local, |p| p.entry.local) - 1;
                o.send(Frame::push(self.space, from, [entry.clone()]));
                self.pending.push_back(Logged::intent(entry, run.unwrap_or_default()));
            }
        }
    }

    /// An intent judged by what its run did: its origin's `local` passed,
    /// and it logged with its facts and told everyone, or its refusal kept.
    fn judged(&mut self, entry: Ctx, run: Result<Vec<Change>, Refusal>, o: &mut Out) -> Result<Seq, String> {
        let id = entry.id;
        self.journal().pass(entry.origin, &entry.actor, entry.local);
        let facts = match run {
            Ok(facts) => facts,
            Err(Refusal(why)) => {
                tracing::warn!(target: "ark", entry = %crate::hash::hex(&id.0), why = %why, "refused");
                self.journal().refused.insert(id, why.clone());
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
        o.out.push(Output::Send(To::All, page.enc()));
        Ok(n + 1)
    }

    // * Frames

    fn frame(&mut self, from: Option<Ctx>, f: Frame, o: &mut Out) {
        if f.space() != self.space {
            return;
        }
        match (f, self.is_authority(), from) {
            (
                Frame::Hello {
                    cursor, hash, fill, most, ..
                },
                true,
                who,
            ) => {
                // Never past [`PART`].
                let feed = who.and_then(|w| self.feed_of(w));
                self.serve(feed, (cursor, hash), fill, (most as usize).clamp(1, PART), o)
            }
            // A peer's intents, each a push.
            (Frame::Page { from, logged, .. }, true, Some(who)) => logged.into_iter().for_each(|l| self.push(&who, from, l.entry, o)),
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
    /// there; a fill's first part when it does not — of about `most` bytes
    /// of rows, or of the union whole.
    fn serve(&mut self, feed: Option<Feed>, at: (Seq, EntryHash), fill: Option<Cursor>, most: usize, o: &mut Out) {
        let key = |h: EntryHash| feed.as_ref().map_or(h, |f| f.key(h));
        let has = self.log().hash_at(at.0).is_some_and(|h| key(h) == at.1);
        if has {
            let after: Result<Vec<Logged>, Refusal> = self.log().after(at.0).map(|l| Feed::fed(feed.as_ref(), l)).collect();
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
            let (seq, hash) = self.log().head();
            let order = self.fill_order();
            let part = match &feed {
                None => part(self.space, &self.store, &order, (seq, hash), fill.filter(|_| has), most),
                Some(f) => {
                    let Ok(union) = f.union(self) else { return };
                    part(self.space, &union, &order, (seq, f.key(hash)), None, usize::MAX)
                }
            };
            o.send(part);
        }
    }

    /// The tables of this peer's space in the order a fill sends them: the
    /// schema's, those its domain sends later after the rest.
    fn fill_order(&self) -> Vec<String> {
        let tables = self.store.schema_arc().tables.iter().filter(|t| t.space == self.space);
        let (later, first): (Vec<_>, Vec<_>) = tables.map(|t| t.name.clone()).partition(|t| self.domain.later.contains(t));
        first.into_iter().chain(later).collect()
    }

    /// How `who` is fed (§1.8): none, the log whole; or by its scope.
    fn feed_of(&self, who: Ctx) -> Option<Feed> {
        let scope = self.feed.as_ref()?(&who)?;
        let holds = self.domain.fns.get(&scope).and_then(|f| f.scope.clone());
        let key = crate::hash::sha256(&[&crate::format::encode(&(&scope, &who)).expect("who serializes")]);
        Some(Feed { who, holds, key })
    }

    /// What `who` is sent of a frame sent to all ([`To::All`]): as it is,
    /// when fed whole; a page through its scope; none, when its scope
    /// cannot say.
    pub fn fed(&self, who: &Ctx, bytes: Vec<u8>) -> Option<Vec<u8>> {
        let Some(feed) = self.feed_of(who.clone()) else { return Some(bytes) };
        let Ok(Frame::Page { space, from, logged }) = Frame::dec(&bytes) else {
            return None;
        };
        let logged: Result<_, _> = logged.into_iter().map(|l| feed.logged(l)).collect();
        Some(
            Frame::Page {
                space,
                from,
                logged: logged.ok()?,
            }
            .enc(),
        )
    }

    /// Which rows `who` reads, by table: every one, fed whole.
    pub fn reads(&self, who: &Ctx) -> impl Fn(&str, &[u8]) -> bool {
        let feed = self.feed_of(who.clone());
        move |table, row| feed.as_ref().is_none_or(|f| f.holds(table, row).unwrap_or(false))
    }

    /// A pushed intent judged, its device's pending beginning past `from`,
    /// and the verdict sent back: every push is answered.
    fn push(&mut self, who: &Ctx, from: i64, entry: Ctx, o: &mut Out) {
        let id = entry.id;
        let verdict = self.verdict_on(who, from, entry, o);
        o.send(Frame::Verdict {
            space: self.space,
            id,
            verdict,
        });
    }

    fn verdict_on(&mut self, who: &Ctx, from: i64, mut entry: Ctx, o: &mut Out) -> Verdict {
        let head = self.log().head().0;
        let j = self.journal();
        if let Some(why) = j.refused.get(&entry.id) {
            return Verdict::Refused { why: why.clone() };
        }
        if !entry.id_checks() || j.others(&entry.origin, &who.actor) {
            let why = "an id that is not its own, or from an origin of another actor".into();
            return Verdict::Refused { why };
        }
        // What its device no longer holds pending is nowhere: passed.
        j.pass(entry.origin, &who.actor, from.min(entry.local - 1));
        let last = j.last_of(&entry.origin);
        // Not the next of its origin, or naming no function here: where its
        // origin stands. At or below the last and not refused, it is in the
        // log at or below the head — every entry up to which this connection
        // is sent before the verdict; past it, not judged.
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
            let previewed = !undone && self.pending.front().is_some_and(|p| p.entry.id == l.entry.id && p.facts == l.facts);
            if previewed {
                self.pending.pop_front();
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
    /// one is asked there already.
    fn ask(&mut self, o: &mut Out) {
        let (n, h) = self.log().head();
        let at = Some((n, self.fill.clone()));
        if self.asked != at {
            self.asked = at;
            o.send(Frame::Hello {
                space: self.space,
                cursor: n,
                hash: h,
                fill: self.fill.clone(),
                most: self.most(),
            });
        }
    }

    /// The most bytes of rows this replica would have a part carry.
    fn most(&self) -> u64 {
        self.part.unwrap_or(PART) as u64
    }

    /// Facts that do not apply here at `seq`: the fill begun again, from
    /// nothing — the store emptied, its log standing at `seq` on nothing the
    /// authority has, so that its `Hello` is sent a first part (§1.7).
    fn diverge(&mut self, seq: Seq, o: &mut Out) {
        tracing::warn!(target: "ark", seq, "diverged");
        self.diverged.push(seq);
        self.undo_pending(o);
        self.stand((seq, EntryHash::default()), o);
        self.redo_pending(o);
        self.fill = Some(Cursor::default());
        self.ask(o);
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
    /// replicas too. Any other is the one from where the fill stands, at the
    /// head — every entry the authority logged before it was sent before it
    /// — or it is passed by: past a gap, the head asked for again, and the
    /// part. Its rows are put as they are, judging nothing (a row may name
    /// one not sent yet: every row is the authority's, judged where it was
    /// written): what the store held of the rows' ranges is what the entries
    /// since the fill began wrote there, and the authority's rows at its
    /// head hold that too. The last is the fill done, and the store made
    /// durable whole; until then nothing is, so a peer reopened mid-fill
    /// fills again. The pending intents run again on top of each.
    fn take(&mut self, at: (Seq, EntryHash), from: Option<Cursor>, rows: Rows, next: Option<Cursor>, o: &mut Out) {
        let first = from.is_none();
        if first {
            if self.fill.is_none() && at == self.log().head() {
                return;
            }
        } else if self.fill.is_none() || from != self.fill {
            return;
        } else if at != self.log().head() {
            self.asked = None;
            return self.ask(o);
        }
        self.undo_pending(o);
        if first {
            self.stand(at, o);
        }
        for (table, rows) in &rows {
            match self.store.put_rows(table, rows) {
                Ok(moved) => o.tell(None, moved),
                Err(_) => {
                    self.redo_pending(o);
                    return self.diverge(at.0, o);
                }
            }
        }
        self.fill = next;
        match self.fill {
            Some(_) => self.ask(o),
            None => {
                tracing::info!(target: "ark", seq = at.0, "filled");
                o.persist(Record::Rows(self.store.kv_rows()))
            }
        }
        self.redo_pending(o);
        if first {
            self.repush(o);
        }
    }

    /// Its pending intents pushed again, in pages, each saying where they
    /// begin.
    fn repush(&mut self, o: &mut Out) {
        let most = self.most() as usize;
        let from = self.pending.front().map_or(0, |p| p.entry.local - 1);
        for page in pages(self.pending.make_contiguous(), most, Logged::weight) {
            let push = Frame::push(self.space, from, page.iter().map(|p| p.entry.clone()));
            o.out.push(Output::Send(To::Back, push.enc()));
        }
    }

    // * Lifecycle

    fn connected(&mut self, o: &mut Out) {
        if self.is_authority() {
            return;
        }
        // What was asked on the connection before is not coming.
        self.asked = None;
        self.ask(o);
        self.repush(o);
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
        let (log, rows): (Rows, Rows) = rows.into_iter().partition(|(t, _)| t == LOG);
        let log = log.into_iter().flat_map(|(_, rs)| rs);
        let opened = match filling {
            true => Ok(self.blank()),
            false => self.of_rows(&rows).and_then(|mut s| s.start_log(log).map(|_| s)),
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
/// rows' start), of its rows as they stand at `head`: tables in `order`,
/// each's rows in key order, until about `most` bytes of rows; and where
/// the next begins, unless none is left. A cursor at a table this peer
/// lacks begins the fill again.
fn part(space: Space, store: &Store, order: &[String], (seq, hash): (Seq, EntryHash), from: Option<Cursor>, most: usize) -> Frame {
    let start = from.as_ref().and_then(|c| order.iter().position(|t| *t == c.table));
    let from = from.filter(|_| start.is_some());
    let (mut rows, mut size, mut next) = (vec![], 0, None);
    for (i, table) in order.iter().enumerate().skip(start.unwrap_or(0)) {
        let key = from.as_ref().filter(|_| Some(i) == start).map_or(&[][..], |c| &c.key[..]);
        let mut held = vec![];
        for (k, r) in store.keyed_from(table, key) {
            if size >= most {
                next = Some(Cursor {
                    table: table.clone(),
                    key: k.to_vec(),
                });
                break;
            }
            size += r.len();
            held.push(r.to_vec());
        }
        rows.push((table.clone(), held));
        if next.is_none() && size >= most {
            next = order.get(i + 1).map(|t| Cursor {
                table: t.clone(),
                key: vec![],
            });
        }
        if next.is_some() {
            break;
        }
    }
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
struct Feed {
    who: Ctx,
    holds: Option<Holds>,
    key: [u8; 32],
}

impl Feed {
    fn holds(&self, table: &str, row: &[u8]) -> Result<bool, Refusal> {
        self.holds.as_ref().ok_or_else(|| Refusal::new("no such scope"))?(&self.who, table, row)
    }

    /// The log's hash at an entry, as this connection's chain has it.
    fn key(&self, h: EntryHash) -> EntryHash {
        EntryHash(crate::hash::sha256(&[&self.key, &h.0]))
    }

    /// A logged entry as this connection is sent it: its facts through the
    /// scope, its hash keyed.
    fn logged(&self, l: Logged) -> Result<Logged, Refusal> {
        let facts = scoped(&l.facts, |t, r| self.holds(t, r))?;
        let hash = self.key(l.hash);
        Ok(Logged { facts, hash, ..l })
    }

    /// `l` as a connection fed as `feed` says is sent it.
    fn fed(feed: Option<&Feed>, l: Logged) -> Result<Logged, Refusal> {
        feed.map_or(Ok(l.clone()), |f| f.logged(l))
    }

    /// The rows of `p`'s store the scope holds, as a store: refused unless
    /// it holds every row they reference.
    fn union(&self, p: &Peer) -> Result<Store, Refusal> {
        let mut rows = vec![];
        for (t, rs) in p.store.snapshot_rows() {
            let held: Result<_, Refusal> = rs
                .into_iter()
                .filter_map(|r| self.holds(&t, &r).map(|h| h.then_some(r)).transpose())
                .collect();
            rows.push((t, held?));
        }
        p.of_rows(&rows)
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
