//! §1.7 One peer machine: a log, a store, the functions it can run, its
//! own intents pending, and a role.
//!
//! ```text
//! Role  = Replica | Authority { journal }   -- "alone" is an authority whose journal has a fork
//! Input = Frame { from, bytes } | Mutate { fn, input, now, cause } | Tick | Persisted | Open
//!       | Connected | Alone | Join | Compact
//! Output= Send | Fanout | Changes(by span) | Persist | Refused { id, why } | Confirmed(id)
//! ```
//!
//! Sans-io: [`Peer::step`] takes one input and says what to do — no
//! socket, clock or file is inside. A hub, a client, a simulator and a
//! trace player are drivers that feed it and route what it says: `Send`
//! goes back where the frame came from (a replica's upstream), `Fanout` to
//! every connection, `Persist` to the disk before anything after it is
//! sent: a row appended to the store's `_log` table, which is the log
//! (§1.6) and holds the entry's facts, or the store's rows whole. `from` on a frame is the identity the driver verified for its
//! connection; an authority judges and stamps by it.
//!
//! **Effects are values.** A [`Procedure`] reads the store and returns its
//! changes; the machine applies them. A replica runs code for its own
//! pending intents only (that is the optimistic view) and applies every
//! confirmed entry by its facts, checking the state hash the entry carries
//! against its own: a difference is a divergence, healed from a snapshot.
//!
//! **The rebase.** Confirmed entries arriving under pending intents: undo
//! the pending changes newest-first, apply each entry's facts and check
//! its state, then re-run the intents still pending over the new store,
//! once for the page, and record their new changes. When an entry is the
//! oldest pending intent itself and its facts are what the preview did,
//! nothing moves: its state is checked beside the rest by the digests'
//! arithmetic ([`Store::hash_beside`]). A view is told every change made,
//! rebases included, as `Output::Changes`.
//!
//! **The authority** judges each pushed intent once — dedupe by `(origin,
//! local)`, a gap in a replica's pushes waits for them in order — stamps it
//! with the actor, session and roles of the connection it judged it on,
//! runs it, and logs it with its facts. An origin is bound to the actor
//! whose intents first came from it; another's pushed from it is refused. It runs exactly the functions its program
//! was built with, by name (§1.16): nothing it is sent is code. An intent naming any other is `Held` — the device keeps
//! it pending and pushes it again on its next connection. The authority
//! keeps nothing of a held intent: its `local` is not passed, so one
//! pushed after it from its origin is a gap, answered by nothing and
//! pushed again, after it, on the next connection.
//!
//! **A schema grows** (§1.2). Facts and snapshot rows another schema of a
//! table wrote are read under this one ([`crate::schema::Table::fit`]). A
//! log opened under a wider schema keeps its chain as recorded, checked by
//! its hashes, and its store's digests are the wider schema's
//! ([`Peer::foreign`] marks what it did not derive; a `Hello` below that is
//! answered with the snapshot). A replica whose authority runs another
//! schema takes each confirmed state as the entry says ([`Peer::faith`]).
//!
//! **A fill** is how a replica the log cannot page is sent the rows: in
//! parts of about [`PART`] bytes, the tables a domain sends [`Peer::later`]
//! after the rest, each part asked for (`More`) once the one before is
//! held, so what the authority logs meanwhile is never behind more than a
//! part. A part is no frozen snapshot: it is the rows of its key ranges as
//! they stand at the authority's head when it is sent, and every entry
//! before that head was sent before it, so a replica at that head takes
//! the ranges whole and is right there. Entries come as they are logged,
//! their facts put as they are over rows not all sent yet; nothing is
//! judged until the last part, when the store is judged whole and its
//! state checked, as a snapshot's is. A filling replica makes each part
//! durable as it is taken ([`Record::Part`]) and its place beside its rows
//! ([`FILL`]), so one reopened goes on where it was; its own intents are
//! the authority's to judge, one its store refuses pending unrun.
//!
//! **Partial sync is a view** (§1.8). A connection the driver says is fed
//! by a scope (`Input::Feed`) is served its union: the rows its scope —
//! a [`Scope`], its own kind of function, asked of a table's row — holds,
//! under the connection's identity. At
//! its `Hello` the authority takes the union from its store and sends it as
//! a snapshot at the head; every entry after is sent to it alone
//! (`Output::To`) with the facts the scope's filter makes of the entry's
//! ([`scoped`]), the union's state hash after them, and a
//! chain of its own from that snapshot. `Fanout` is for connections fed
//! whole. The replica is the same machine: it applies facts like every
//! replica, its own intents previewed over what it holds and confirmed by
//! the authority's facts, a difference being the authority's answer. A
//! union must hold the rows its rows reference; a `Hello` from a
//! connection whose union does not is answered with nothing.
//!
//! **Going alone and joining.** `Alone` makes a replica an authority over
//! its own log from where it is (the fork), sequencing its pending intents
//! and every one after; a pending intent that refuses then stays pending,
//! unrun, the server's to judge. `Join` turns back into a replica, the
//! chain above the fork its pending intents beside those, in the order
//! they were authored, and pushes them as after any connection: the server
//! judges each as the push it is, and the peer rebases onto what it
//! answers. A chain is never taken whole: an entry refused loses that
//! entry, not the chain.
//!
//! **Live state is a second machine** (§1.9): a peer [`Peer::in_space`]
//! `Volatile` is this machine over the volatile tables, sharing the
//! connection, every frame of its log naming its space. Its store hashes and
//! snapshots those tables only; it makes nothing durable; an intent whose
//! changes touch a table of the other space is refused, in either space.
//! A device says what it is in its `Hello` ([`Peer::device`]); its
//! `on_join` and `on_leave` are entries the authority authors through
//! `Mutate`, as the driver sees connections come and go. `resync` is a
//! `Hello` with a hash the authority lacks, answered with a snapshot. A
//! volatile authority forgets on restart what each origin last pushed, so
//! there a push past a gap is taken, and one that comes after it is
//! refused as overtaken: what was live then is gone. It keeps the verdicts
//! of each origin's last [`WINDOW`] `local`s only: one pushed again inside
//! them is answered as it was, one below them refused as overtaken.
//!
//! **The horizon** (`Compact`) is moved by its driver, in either role, up
//! to the head (a device alone, up to its fork): a replica's pending
//! intents are rebased by their own changes over the head, never by an
//! entry below it, so a replica folds as an authority does.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::format::Encode;
use crate::frames::{Frame, Verdict};
use crate::hash::EntryHash;
use crate::log::{Cursor, Durable, Entry, EntryId, Logged, Origin, Part, Range, Record, Rows, Seq, Snapshot, Span};
use crate::schema::{Schema, Space};
use crate::store::{Change, Log, Store, Undo, LOG};
use crate::Refusal;

/// How many logged entries a `Page` carries at most.
pub const PAGE: usize = 256;

/// How many of an origin's last `local`s a volatile authority keeps the
/// verdicts of: one pushed again inside them is answered as it was, one
/// below them is refused as overtaken.
pub const WINDOW: i64 = 64;

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

/// A function, opaque to the kernel: run over the store and its changes
/// left applied, all or none — or its refusal. A function reads its own
/// writes as it makes them (§1.16).
pub trait Procedure: Send + Sync {
    fn apply(&self, store: &mut Store, ctx: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal>;
}

/// A scope (§1.8): whether the union of who reads holds a row of the table
/// named, by the row's bytes. Never run as an intent.
pub type Scope = Arc<dyn Fn(&Ctx, &str, &[u8]) -> Result<bool, Refusal> + Send + Sync>;

/// One function a peer has by name: a procedure it runs, or a scope it
/// asks of rows.
#[derive(Clone)]
pub enum Runs {
    Procedure(Arc<dyn Procedure>),
    Scope(Scope),
}

/// The functions a peer runs, by the names entries call them by (§1.16):
/// its domain's, registered when the program was built. Nothing a peer is
/// sent is ever run but by a name it already has.
#[derive(Clone, Default)]
pub struct Functions {
    fns: BTreeMap<String, Runs>,
}

impl fmt::Debug for Functions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_set().entries(self.fns.keys()).finish()
    }
}

impl Functions {
    pub fn insert(&mut self, name: impl Into<String>, p: Arc<dyn Procedure>) {
        self.put(name, Runs::Procedure(p));
    }

    pub fn put(&mut self, name: impl Into<String>, r: Runs) {
        self.fns.insert(name.into(), r);
    }

    /// What `name` is here, of either kind.
    pub fn of(&self, name: &str) -> Option<&Runs> {
        self.fns.get(name)
    }

    /// The procedure `name` is.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Procedure>> {
        match self.fns.get(name)? {
            Runs::Procedure(p) => Some(p),
            Runs::Scope(_) => None,
        }
    }

    /// The scope `name` is.
    pub fn scope(&self, name: &str) -> Option<&Scope> {
        match self.fns.get(name)? {
            Runs::Scope(s) => Some(s),
            Runs::Procedure(_) => None,
        }
    }

    /// Every name, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.fns.keys().map(String::as_str)
    }
}

/// Who a peer authors as, and who a connection was verified to be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Who {
    pub actor: String,
    pub session: String,
    pub roles: Vec<String>,
}

/// One of this peer's own intents, with what its last run changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub entry: Entry,
    pub changes: Vec<Change>,
    /// What undoing `changes` does to the digests, worked out once.
    undo: Memo<Undo>,
}

impl Pending {
    pub fn new(entry: Entry, changes: Vec<Change>) -> Pending {
        Pending {
            entry,
            changes,
            undo: Memo::default(),
        }
    }
}

/// A value worked out when first asked for, not part of what holds it.
#[derive(Clone, Debug, Default)]
struct Memo<T>(std::sync::OnceLock<T>);

impl<T> PartialEq for Memo<T> {
    fn eq(&self, _: &Memo<T>) -> bool {
        true
    }
}

impl<T> Eq for Memo<T> {}

/// What an authority keeps beside its log: every intent it judged and how;
/// each origin's last `local` and the actor it is bound to; and, for a
/// device alone, where it forked from the log it will join again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Journal {
    pub fork: Option<(Seq, EntryHash)>,
    pub verdicts: BTreeMap<EntryId, Verdict>,
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

/// What a replica is fed: every entry whole, or a scope's union (§1.8),
/// the scope by hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feed {
    Whole,
    Scoped(String),
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
    Tick(i64),
    Persisted(Seq),
    Open(Durable),
    Connected,
    Alone,
    Join,
    Compact(Seq),
    /// An authority's own facts, made by its driver (a closure over an
    /// unwired instance's tables): logged as an entry naming no function,
    /// which nothing runs again — a replay applies the facts.
    Write {
        facts: Vec<Change>,
        now: i64,
    },
    /// How the connection verified as `who` is fed (an authority's).
    Feed {
        who: Who,
        feed: Feed,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Send(Vec<u8>),
    Fanout(Vec<u8>),
    /// A frame for one connection fed by a scope, by its session.
    To {
        session: String,
        bytes: Vec<u8>,
    },
    /// What changed, each change with the span of the entry that made it.
    Changes(Vec<Told>),
    Persist(Vec<u8>),
    Refused {
        id: EntryId,
        why: String,
    },
    Confirmed(EntryId),
}

/// A change as a machine tells it: with the span of the entry that made it
/// (the one run, the one confirmed, the pending one undone), and none for
/// a whole state's difference (a snapshot, a divergence, an open).
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
        self.out.push(Output::Send(f.enc()));
    }
    fn tell(&mut self, by: Option<Span>, changes: impl IntoIterator<Item = Change>) {
        self.told.extend(changes.into_iter().map(|change| Told { by, change }));
    }
    fn persist(&mut self, r: Record) {
        self.out.push(Output::Persist(r.enc()));
    }
}

#[derive(Clone, Debug)]
pub struct Peer {
    /// The confirmed state with the pending intents' changes applied, and
    /// the log as its table (§1.6).
    pub store: Store,
    pub functions: Functions,
    pub pending: Vec<Pending>,
    pub role: Role,
    pub feed: Feed,
    pub me: Who,
    pub space: Space,
    /// Where this peer's intents come from (§1.6), and the last `local` it
    /// drew.
    pub origin: Origin,
    pub local: i64,
    /// Where a confirmed entry's state was not this peer's own.
    pub diverged: Vec<Seq>,
    /// The cursor a `Hello` is outstanding at.
    asked: Option<Seq>,
    /// An authority's connections fed by a scope, by session.
    pub served: BTreeMap<String, Served>,
    /// What this device says it is, in every `Hello` (§1.9).
    pub device: Option<Vec<u8>>,
    /// The version of the domain this peer runs, in every `Hello` beside
    /// its schema's hash: what an operator reads a fleet's spread by.
    pub version: String,
    /// The last entry whose state is not this peer's own derivation: one
    /// logged under another schema (§1.2). An authority answers a `Hello`
    /// below it with the snapshot at its head.
    pub foreign: Option<Seq>,
    /// A replica whose authority runs another schema: the state a
    /// confirmed entry carries is taken as it says.
    pub faith: bool,
    /// The tables an authority sends last in a fill, after every other:
    /// what a replica can be without for a while.
    pub later: Vec<String>,
    /// A replica filling: where its next part begins.
    pub fill: Option<Fill>,
    /// The part a replica filling has asked for and not been sent.
    more: Option<Cursor>,
}

/// A replica's fill as it stands (§1.7): where the next part begins, and
/// whether a row it was sent was another schema's. Kept beside the rows,
/// as the one row of [`FILL`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fill {
    pub next: Cursor,
    pub other: bool,
}

/// The table a replica's [`Fill`] is kept in, beside the log's: not the
/// schema's, and never sent.
pub const FILL: &str = "_fill";

/// How many bytes of rows a fill's part carries, about: past it, the
/// part ends at the row after; a row is never cut.
pub const PART: usize = 1 << 20;

/// Facts as some schema wrote them, read under `schema`: rows completed or
/// cut to their prefix, changes to tables it lacks dropped; and whether any
/// was another schema's.
fn fit(schema: &Schema, facts: &[Change]) -> (Vec<Change>, bool) {
    let other = std::cell::Cell::new(false);
    let row = |t: &crate::schema::Table, r: &Vec<u8>| match t.fit(r) {
        Ok((r, o)) => {
            other.set(other.get() | o);
            r
        }
        Err(_) => r.clone(),
    };
    let mut out = vec![];
    for c in facts {
        let Some((_, t)) = schema.table(c.table()) else {
            match c.table() == LOG {
                true => out.push(c.clone()),
                false => other.set(true),
            }
            continue;
        };
        out.push(match c {
            Change::Add { table, row: r } => Change::Add {
                table: table.clone(),
                row: row(t, r),
            },
            Change::Remove { table, row: r } => Change::Remove {
                table: table.clone(),
                row: row(t, r),
            },
            Change::Edit { table, old, new } => Change::Edit {
                table: table.clone(),
                old: row(t, old),
                new: row(t, new),
            },
        });
    }
    (out, other.get())
}

/// A store with an empty log: one standing on the empty chain.
fn started(mut store: Store) -> Store {
    let base = Snapshot {
        seq: 0,
        hash: EntryHash::default(),
        state: store.state_hash(),
        rows: vec![],
    };
    store.start_log([base.base().to_bytes()]).expect("a first row starts a log");
    store
}

impl Peer {
    /// A peer from empty; its origin named by its session's bytes until
    /// its driver gives it the one it drew ([`Origin`]).
    pub fn new(schema: Arc<Schema>, functions: Functions, role: Role, me: Who) -> Peer {
        let store = started(Store::new(schema, Space::Durable));
        let origin = Origin::named(&me.session);
        Peer {
            store,
            functions,
            pending: vec![],
            role,
            feed: Feed::Whole,
            me,
            space: Space::Durable,
            origin,
            local: 0,
            diverged: vec![],
            asked: None,
            served: BTreeMap::new(),
            device: None,
            version: String::new(),
            foreign: None,
            faith: false,
            later: vec![],
            fill: None,
            more: None,
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
            return self.kv(&self.store);
        }
        let mut s = self.store.clone();
        s.undo(self.pending.iter().flat_map(|p| &p.changes)).expect("a pending change undoes");
        self.kv(&s)
    }

    /// `store`'s rows and its log's, and this peer's fill's.
    fn kv(&self, store: &Store) -> Rows {
        let mut rows = store.kv_rows();
        rows.extend(self.fill.as_ref().map(|f| (FILL.to_string(), vec![f.to_bytes()])));
        rows
    }

    /// A logged entry made part of the log, its facts already applied, and
    /// durable as its row of the log's table alone: the facts are in it.
    /// The entry links onto the head: this peer chained it, or checked it.
    fn logged(&mut self, l: &Logged, o: &mut Out) {
        debug_assert!(l.seq == self.log().head().0 + 1 && l.links(&self.log().head().1));
        let row = l.to_bytes();
        o.out.push(Output::Persist(Record::logged(&row)));
        self.store.log_push(l, row);
    }

    fn of_rows(&self, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, Refusal> {
        Store::from_rows(self.store.schema_arc().clone(), self.space, rows).map(|(s, _)| s)
    }

    /// A snapshot's rows as some schema wrote them, read under this
    /// peer's (§1.2), and a log standing on `log`; and whether the rows were
    /// another schema's ([`Store::from_rows`]).
    fn on_fit(&self, rows: &[(String, Vec<Vec<u8>>)], log: impl IntoIterator<Item = Vec<u8>>) -> Result<(Store, bool), Refusal> {
        let (mut s, other) = Store::from_rows(self.store.schema_arc().clone(), self.space, rows)?;
        s.start_log(log)?;
        Ok((s, other))
    }

    /// The id this peer's next intent will have (§1.6).
    pub fn next_id(&self) -> EntryId {
        EntryId::of(self.origin, self.local + 1)
    }

    pub fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority { .. })
    }

    /// The confirmed state's hash: the head's.
    pub fn confirmed_hash(&self) -> crate::hash::StateHash {
        self.log().head_state()
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
            Input::Tick(_) | Input::Persisted(_) => {}
            Input::Open(d) => self.open(d, &mut o),
            Input::Connected => self.connected(&mut o),
            Input::Alone => self.alone(&mut o),
            Input::Join => self.join(&mut o),
            Input::Compact(n) => self.compact(n, &mut o),
            Input::Write { facts, now } if self.is_authority() => {
                let entry = self.entry(String::new(), vec![], now);
                let given = self.store.apply_all(&facts).map(|_| facts);
                o.tell(Some(entry.span()), given.iter().flatten().cloned());
                self.decide(entry, given, &mut o);
            }
            Input::Write { .. } => {}
            Input::Feed { who, feed } => match feed {
                Feed::Whole => drop(self.served.remove(&who.session)),
                Feed::Scoped(scope) => {
                    let union = self.blank();
                    let s = Served {
                        who: who.clone(),
                        scope,
                        union,
                        head: None,
                    };
                    self.served.insert(who.session, s);
                }
            },
        }
        if !o.told.is_empty() {
            o.out.insert(0, Output::Changes(o.told));
        }
        if self.space == Space::Volatile {
            o.out.retain(|x| !matches!(x, Output::Persist(_)));
        }
        o.out
    }

    // * Running

    /// Runs an intent over the store and applies what it did, all or none.
    fn attempt(&mut self, e: &Entry, o: &mut Out) -> Result<Vec<Change>, Refusal> {
        let p = match self.functions.of(&e.func) {
            Some(Runs::Procedure(p)) => p.clone(),
            Some(Runs::Scope(_)) => return Err(Refusal::new("a scope writes nothing")),
            None => return Err(Refusal(format!("no function {}", e.func))),
        };
        let changes = p.apply(&mut self.store, &Ctx::of(e), &e.input)?;
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
            let undone = self.store.undo(&p.changes).expect("a pending change undoes");
            o.tell(Some(p.entry.span()), undone);
        }
    }

    /// Runs the pending intents again, in order, over the store as it is;
    /// one that now refuses stays pending with no changes, the authority's
    /// to judge. A device alone keeps its own unrun: they are its server's.
    fn redo_pending(&mut self, o: &mut Out) {
        for p in std::mem::take(&mut self.pending) {
            let changes = match self.is_authority() {
                true => vec![],
                false => self.attempt(&p.entry, o).unwrap_or_default(),
            };
            self.pending.push(Pending::new(p.entry, changes));
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
        let entry = Entry {
            cause,
            ..self.entry(func, input, now)
        };
        let (id, local) = (entry.id, entry.local);
        if self.is_authority() {
            let run = self.attempt(&entry, o);
            return self.decide(entry, run, o);
        }
        // Filling, a refusal may be of rows not sent yet: the authority's to
        // judge, the intent pending unrun.
        match self.attempt(&entry, o) {
            Err(why) if self.fill.is_none() => o.out.push(Output::Refused { id, why: why.0 }),
            run => {
                self.local = local;
                o.send(Frame::push(self.space, [entry.clone()]));
                self.pending.push(Pending::new(entry, run.unwrap_or_default()));
            }
        }
    }

    /// An authority's own intent judged by what its run did — a function's
    /// or its driver's facts — and the verdict told: logged, its `local`
    /// drawn, or refused, drawing none, so a chain made alone has no gaps.
    fn decide(&mut self, entry: Entry, run: Result<Vec<Change>, Refusal>, o: &mut Out) {
        let (id, local) = (entry.id, entry.local);
        match self.judged(entry, run, o) {
            Ok(_) => {
                self.local = self.local.max(local);
                o.out.push(Output::Confirmed(id));
            }
            Err(why) => o.out.push(Output::Refused { id, why }),
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
                let state = self.store.state_hash();
                let hash = Logged::chain(&prev, n + 1, &entry, &facts, &state);
                let l = Logged {
                    seq: n + 1,
                    entry,
                    facts,
                    state,
                    hash,
                };
                self.logged(&l, o);
                o.out.push(Output::Fanout(
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
        let verdict = v.clone().map_or_else(|why| Verdict::Refused { why }, |seq| Verdict::Ok { seq });
        self.verdict(origin, &actor, local, id, verdict);
        v
    }

    /// A verdict kept, its origin's `local` passed. In the volatile space
    /// only an origin's last [`WINDOW`] are kept: as the window moves, the
    /// verdicts it leaves (at most a window's) are forgotten.
    fn verdict(&mut self, origin: Origin, actor: &str, local: i64, id: EntryId, verdict: Verdict) {
        let space = self.space;
        let j = self.journal();
        let was = j.last_of(&origin);
        j.pass(origin, actor, local);
        j.verdicts.insert(id, verdict);
        if space == Space::Volatile {
            for k in (was - WINDOW).max(0) + 1..=(local - WINDOW).min(was) {
                j.verdicts.remove(&EntryId::of(origin, k));
            }
        }
    }

    // * Frames

    fn frame(&mut self, from: Option<Who>, f: Frame, o: &mut Out) {
        if f.space() != self.space {
            return;
        }
        let scoped = from.as_ref().filter(|w| self.served.contains_key(&w.session)).map(|w| w.session.clone());
        match (f, self.is_authority(), from) {
            (Frame::Hello { cursor, hash, .. }, true, _) if scoped.is_some() => self.serve_scoped(&scoped.unwrap(), cursor, hash, o),
            (Frame::Hello { cursor, hash, .. }, true, _) => self.serve(cursor, hash, o),
            // A peer's intents, each a push.
            (Frame::Page { logged, .. }, true, Some(who)) => logged.into_iter().for_each(|l| self.push(&who, l.entry, o)),
            (Frame::Page { logged, .. }, false, _) => self.confirm(logged, o),
            (Frame::Snapshot { snapshot, .. }, false, _) => self.reset(snapshot, o),
            (Frame::More { from, .. }, true, _) if scoped.is_none() => o.send(Frame::Part {
                space: self.space,
                part: self.part(Some(from)),
            }),
            (Frame::Part { part, .. }, false, _) => self.take(part, o),
            (Frame::Verdict { id, verdict, .. }, false, _) => match verdict {
                // At or below where this peer stands, an intent still pending
                // here is in the state it holds (a snapshot carried it).
                Verdict::Ok { seq } if seq <= self.log().head().0 && self.settle(&id, o) => o.out.push(Output::Confirmed(id)),
                Verdict::Refused { why } if self.settle(&id, o) => o.out.push(Output::Refused { id, why }),
                _ => {}
            },
            _ => {}
        }
    }

    /// A `Hello` at `(cursor, hash)`: the pages from there when this log has
    /// that hash there, the snapshot at the head when it does not.
    fn serve(&mut self, cursor: Seq, hash: Option<EntryHash>, o: &mut Out) {
        let has = self.foreign.is_none_or(|f| cursor >= f)
            && match hash {
                Some(h) => self.log().has(cursor, &h),
                None => cursor == 0 && self.log().base_seq() == 0,
            };
        if !has && self.space == Space::Durable {
            return o.send(Frame::Part {
                space: self.space,
                part: self.part(None),
            });
        }
        if !has {
            let (seq, hash) = self.log().head();
            let snapshot = Snapshot {
                seq,
                hash,
                state: self.store.state_hash(),
                rows: self.store.snapshot_rows(),
            };
            return o.send(Frame::Snapshot { space: self.space, snapshot });
        }
        for page in self.log().after(cursor).collect::<Vec<_>>().chunks(PAGE) {
            o.send(Frame::Page {
                space: self.space,
                from: page[0].seq - 1,
                logged: page.to_vec(),
            });
        }
    }

    /// The tables of this peer's space in the order a fill sends them: the
    /// schema's, those it sends [`Peer::later`] after the rest.
    fn fill_order(&self) -> Vec<String> {
        let tables = self.store.schema_arc().tables.iter().filter(|t| t.space == self.space);
        let (later, first): (Vec<_>, Vec<_>) = tables.map(|t| t.name.clone()).partition(|t| self.later.contains(t));
        first.into_iter().chain(later).collect()
    }

    /// The part of a fill from `from` (none: the first, from the rows'
    /// start), of the rows as they stand at the head: ranges in
    /// [`Peer::fill_order`], each table's in key order, until about
    /// [`PART`] bytes of rows; and where the next begins, unless none is
    /// left. A cursor at a table this peer lacks begins the fill again.
    fn part(&self, from: Option<Cursor>) -> Part {
        let order = self.fill_order();
        let at = from.as_ref().and_then(|c| order.iter().position(|t| *t == c.table));
        let (start, key) = match (at, from) {
            (Some(i), Some(c)) => (i, c.key),
            _ => (0, vec![]),
        };
        let (mut ranges, mut size, mut next) = (vec![], 0, None);
        for (i, table) in order.iter().enumerate().skip(start) {
            let from = if i == start { key.clone() } else { vec![] };
            let (mut rows, mut to) = (vec![], None);
            for (k, r) in self.store.keyed_from(table, &from) {
                if size >= PART {
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
                (None, Some(t)) if size >= PART => Some(Cursor {
                    table: t.clone(),
                    key: vec![],
                }),
                _ => None,
            };
            if next.is_some() {
                break;
            }
        }
        let (seq, hash) = self.log().head();
        Part {
            seq,
            hash,
            state: self.store.state_hash(),
            first: at.is_none(),
            ranges,
            next,
        }
    }

    /// A scope's verdict on a row of `table`, for the connection it serves.
    fn holder(&self, session: &str) -> impl FnMut(&str, &[u8]) -> Result<bool, Refusal> {
        let s = &self.served[session];
        let (scope, ctx) = (self.functions.scope(&s.scope).cloned(), Ctx::new(&s.who, 0));
        move |table, row| scope.as_ref().ok_or_else(|| Refusal::new("no such scope"))?(&ctx, table, row)
    }

    /// A `Hello` from a connection fed by a scope: nothing when it stands at
    /// the head of what it was sent, and otherwise its union, afresh, as a
    /// snapshot at the head.
    fn serve_scoped(&mut self, session: &str, cursor: Seq, hash: Option<EntryHash>, o: &mut Out) {
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
        let (seq, hash) = head;
        let snapshot = Snapshot {
            seq,
            hash,
            state: union.state_hash(),
            rows: union.snapshot_rows(),
        };
        let s = self.served.get_mut(session).unwrap();
        (s.union, s.head) = (union, Some(head));
        let bytes = Frame::Snapshot { space: self.space, snapshot }.enc();
        o.out.push(Output::To {
            session: session.into(),
            bytes,
        });
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
            let state = s.union.state_hash();
            let hash = Logged::chain(&prev, l.seq, &l.entry, &facts, &state);
            s.head = Some((l.seq, hash));
            let logged = vec![Logged {
                seq: l.seq,
                entry: l.entry.clone(),
                facts,
                state,
                hash,
            }];
            let bytes = Frame::Page {
                space: self.space,
                from: at,
                logged,
            }
            .enc();
            o.out.push(Output::To { session, bytes });
        }
    }

    /// A pushed intent judged, and the verdict sent back; nothing for one
    /// past a gap.
    fn push(&mut self, who: &Who, entry: Entry, o: &mut Out) {
        let id = entry.id;
        if let Some(verdict) = self.verdict_on(who, entry, o) {
            o.send(Frame::Verdict {
                space: self.space,
                id,
                verdict,
            });
        }
    }

    fn verdict_on(&mut self, who: &Who, mut entry: Entry, o: &mut Out) -> Option<Verdict> {
        let (space, j) = (self.space, self.journal());
        if let Some(verdict) = j.verdicts.get(&entry.id) {
            return Some(verdict.clone());
        }
        if !entry.id_checks() || j.others(&entry.origin, &who.actor) {
            let why = "an id that is not its own, or from an origin of another actor".into();
            return Some(Verdict::Refused { why });
        }
        let last = j.last_of(&entry.origin);
        // At or below the last and judged by no verdict here: in the log's
        // space, sequenced below the horizon by a log opened since (a
        // refusal's verdict is journaled); in the volatile, overtaken.
        if entry.local <= last {
            return Some(match space {
                Space::Durable => Verdict::Ok { seq: self.log().base_seq() },
                Space::Volatile => Verdict::Refused {
                    why: "overtaken by a later one of its origin".into(),
                },
            });
        }
        // Past a gap (behind a held one, too): nothing, until it is pushed
        // again after the ones before it, after a `Hello`.
        if entry.local != last + 1 && !(space == Space::Volatile && entry.local > last) {
            return None;
        }
        if self.functions.of(&entry.func).is_none() {
            return Some(Verdict::Held { func: entry.func });
        }
        (entry.actor, entry.session, entry.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        let run = self.attempt(&entry, o);
        Some(
            self.judged(entry, run, o)
                .map_or_else(|why| Verdict::Refused { why }, |seq| Verdict::Ok { seq }),
        )
    }

    /// Confirmed entries, applied by their facts: a page is one rebase.
    /// The pending intents are undone at the first entry that is not the
    /// oldest of them as previewed, every entry's facts are applied and
    /// its state and chain checked, and the intents still pending run
    /// again once, after the page.
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
            // Filling, an entry's facts are put as they are over rows not
            // all sent yet, and its state is checked once the fill is done.
            if self.fill.is_some() {
                if !undone {
                    self.undo_pending(o);
                    undone = true;
                }
                let (facts, other) = fit(self.store.schema_arc(), &l.facts);
                let Ok(moved) = self.store.force_all(&facts) else {
                    self.redo_pending(o);
                    return self.diverge(l.seq, o);
                };
                o.tell(Some(l.entry.span()), moved);
                self.fill.as_mut().expect("filling").other |= other;
                if let Some(i) = self.pending.iter().position(|p| p.entry.id == l.entry.id) {
                    self.pending.remove(i);
                }
                if l.entry.origin == self.origin {
                    o.out.push(Output::Confirmed(l.entry.id));
                }
                self.asked = None;
                self.logged(&l, o);
                continue;
            }
            // Facts of another schema are read under this one, and the
            // state the entry says is taken as it says (§1.2). Facts are
            // fitted only when they are not this schema's as they are,
            // which comparing them to the preview or applying them says: a
            // row is read once.
            let schema = self.store.schema_arc().clone();
            let own = self.pending.first().is_some_and(|p| p.entry.id == l.entry.id);
            let mut fitted = None;
            let previewed = !undone
                && own
                && (self.pending[0].changes == l.facts || {
                    let f = fit(&schema, &l.facts);
                    let same = self.pending[0].changes == f.0;
                    fitted = Some(f);
                    same
                });
            if previewed {
                let other = fitted.is_some_and(|f| f.1);
                let trust = self.faith || other;
                let store = &self.store;
                let undone = self.pending[1..].iter().map(|p| p.undo.0.get_or_init(|| store.undo_of(&p.changes)));
                let same = store.hash_beside(undone) == l.state;
                if !same && !trust {
                    return self.diverge(l.seq, o);
                }
                self.faith = other || self.faith && !same;
                self.pending.remove(0);
            } else {
                if !undone {
                    self.undo_pending(o);
                    undone = true;
                }
                let apply = |store: &mut Store, (f, o): (Vec<Change>, bool)| {
                    let a = store.apply_all(&f).is_ok();
                    (f, o, a)
                };
                let (facts, other, applied) = match fitted {
                    Some(f) => apply(&mut self.store, f),
                    None if self.store.apply_all(&l.facts).is_ok() => (l.facts.clone(), false, true),
                    None => apply(&mut self.store, fit(&schema, &l.facts)),
                };
                let trust = self.faith || other;
                let same = applied && self.store.state_hash() == l.state;
                if !applied || !same && !trust {
                    self.redo_pending(o);
                    return self.diverge(l.seq, o);
                }
                self.faith = other || self.faith && !same;
                o.tell(Some(l.entry.span()), facts);
                if let Some(i) = self.pending.iter().position(|p| p.entry.id == l.entry.id) {
                    self.pending.remove(i);
                }
            }
            if l.entry.origin == self.origin {
                o.out.push(Output::Confirmed(l.entry.id));
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
            device: self.device.clone(),
            schema: self.store.schema_hash(),
            version: self.version.clone(),
        }
    }

    /// The state an entry says is not this peer's: start again from nothing,
    /// which the authority answers with what it has — the snapshot at its
    /// head (a cursor below every log's) when this entry diverged before,
    /// as one logged under a schema this one only shares tables with does.
    fn diverge(&mut self, seq: Seq, o: &mut Out) {
        let cursor = if self.diverged.contains(&seq) { -1 } else { 0 };
        (self.fill, self.more) = (None, None);
        self.diverged.push(seq);
        self.undo_pending(o);
        let fresh = self.blank();
        o.tell(None, self.store.diff(&fresh));
        self.store = fresh;
        self.redo_pending(o);
        self.asked = Some(cursor);
        o.send(self.hello(cursor, None));
    }

    /// A snapshot: the store becomes its rows, the log stands on it, the
    /// pending intents run again on top and are pushed again.
    fn reset(&mut self, s: Snapshot, o: &mut Out) {
        // One this history passed is a late copy, but a volatile authority
        // that restarted is behind its replicas: they go back to it.
        let (n, _) = self.log().head();
        if (s.seq == n || s.seq < n && self.space == Space::Durable) && self.log().has(s.seq, &s.hash) {
            return;
        }
        let new = self.on_fit(&s.rows, [s.base().to_bytes()]);
        let Ok(new) = new else {
            return;
        };
        let (new, other) = new;
        let same = new.state_hash() == s.state;
        if !same && !other {
            return;
        }
        (self.faith, self.foreign) = (!same, (!same).then_some(s.seq));
        (self.fill, self.more) = (None, None);
        self.undo_pending(o);
        o.tell(None, self.store.diff(&new));
        self.store = new;
        o.persist(Record::Rows(self.store.kv_rows()));
        self.asked = None;
        self.redo_pending(o);
        self.repush(o);
    }

    /// A part of a fill. The first begins one: the store emptied, the log
    /// standing on where the part was taken. Any other is the one asked
    /// for, at the head — every entry the authority logged before it was
    /// sent before it — or it is passed by: past a gap, the head asked for
    /// again, and the part. Its ranges replace what the store held in
    /// them, judging nothing (a row may name one not sent yet); it is made
    /// durable, appended, and the next asked for. The last is the fill
    /// done: every row the authority's at the head, the store judged whole
    /// and its state checked, as a snapshot's is. The pending intents run
    /// again on top of each.
    fn take(&mut self, p: Part, o: &mut Out) {
        let (n, h) = self.log().head();
        let asked = self.more.take();
        if p.first {
            if self.fill.is_none() && self.log().has(p.seq, &p.hash) && (p.seq == n || self.space == Space::Durable) {
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
            let base = Snapshot {
                seq: p.seq,
                hash: p.hash,
                state: p.state,
                rows: vec![],
            };
            fresh.start_log([base.base().to_bytes()]).expect("a first row starts a log");
            o.tell(None, self.store.diff(&fresh));
            self.store = fresh;
            (self.faith, self.foreign) = (false, None);
            self.fill = Some(Fill {
                next: Cursor::default(),
                other: false,
            });
        }
        let mut other = false;
        for r in &p.ranges {
            match self.store.replace(&r.table, &r.from, r.to.as_deref(), &r.rows) {
                Ok((moved, o2)) => {
                    o.tell(None, moved);
                    other |= o2;
                }
                Err(_) => {
                    self.redo_pending(o);
                    return self.diverge(p.seq, o);
                }
            }
        }
        let fill = self.fill.as_mut().expect("filling");
        fill.other |= other;
        let done = match p.next.clone() {
            Some(next) => {
                fill.next = next;
                match p.first {
                    true => o.persist(Record::Rows(self.kv(&self.store))),
                    false => o.persist(Record::Part(p.clone())),
                }
                self.ask_more(o);
                Ok(())
            }
            None => self.filled(p.seq, p.state, o),
        };
        self.redo_pending(o);
        if p.first {
            self.asked = None;
            self.repush(o);
        }
        if done.is_err() {
            self.diverge(p.seq, o);
        }
    }

    /// A fill's last part taken, the pending intents undone: the store
    /// judged whole, which a part does not, and its state the authority's
    /// at `seq` — or, its rows another schema's, taken as the part says it
    /// (§1.2) — and made durable whole. Or not: the fill begun again.
    fn filled(&mut self, seq: Seq, state: crate::hash::StateHash, o: &mut Out) -> Result<(), ()> {
        let other = self.fill.take().is_some_and(|f| f.other);
        let (log, rows): (Rows, Rows) = self.store.kv_rows().into_iter().partition(|(t, _)| t == LOG);
        let judged = self.on_fit(&rows, log.into_iter().flat_map(|(_, rs)| rs));
        let Ok((store, _)) = judged else { return Err(()) };
        let same = store.state_hash() == state;
        if !same && !other {
            return Err(());
        }
        (self.faith, self.foreign) = (!same, (!same).then_some(seq));
        self.store = store;
        o.persist(Record::Rows(self.store.kv_rows()));
        Ok(())
    }

    /// The next part of a fill asked for, unless one is asked already.
    fn ask_more(&mut self, o: &mut Out) {
        let Some(f) = &self.fill else { return };
        if self.more.is_none() {
            self.more = Some(f.next.clone());
            o.send(Frame::More {
                space: self.space,
                from: f.next.clone(),
            });
        }
    }

    fn repush(&mut self, o: &mut Out) {
        for page in self.pending.chunks(PAGE) {
            o.send(Frame::push(self.space, page.iter().map(|p| p.entry.clone())));
        }
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

    fn alone(&mut self, o: &mut Out) {
        // A replica filling holds too little to judge by: it stays one.
        if self.is_authority() || self.fill.is_some() {
            return;
        }
        self.undo_pending(o);
        let fork = Some(self.log().head());
        self.role = Role::Authority {
            journal: Journal { fork, ..Journal::default() },
        };
        for p in std::mem::take(&mut self.pending) {
            match self.attempt(&p.entry, o) {
                Ok(facts) => self.decide(p.entry, Ok(facts), o),
                // Kept for the server, whose `local`s it would otherwise skip.
                Err(_) => self.pending.push(Pending::new(p.entry, vec![])),
            }
        }
    }

    /// The chain above the fork made pending intents, its facts their
    /// changes, with those kept unrun as it went alone among them in the
    /// order they were authored (one already in the chain is not two), the
    /// log made durable at the fork; then a `Hello` there and every pending
    /// intent pushed, as on any connection.
    fn join(&mut self, o: &mut Out) {
        let Role::Authority {
            journal: Journal { fork: Some((at, _)), .. },
        } = self.role
        else {
            return;
        };
        let chain: Vec<Logged> = self.log().after(at).collect();
        self.store.log_truncate(at);
        let mut pending: Vec<Pending> = chain.into_iter().map(|l| Pending::new(l.entry, l.facts)).collect();
        for p in std::mem::take(&mut self.pending) {
            let e = &p.entry;
            if pending.iter().all(|q| q.entry.id != e.id) {
                let at = pending.iter().position(|q| (q.entry.origin, e.local < q.entry.local) == (e.origin, true));
                pending.insert(at.unwrap_or(pending.len()), p);
            }
        }
        (self.pending, self.role) = (pending, Role::Replica);
        o.persist(Record::Rows(self.confirmed_rows()));
        self.connected(o);
    }

    /// Moves the horizon to `n`, at most the head (a device alone keeps
    /// what is above its fork): the log's rows below it deleted, and the
    /// store as confirmed made durable whole, which folds every write
    /// before. A replica may move its own: its pending intents stand on
    /// the head and are rebased by their own changes, so no entry below
    /// the head is read again; a `Hello` says the head.
    fn compact(&mut self, n: Seq, o: &mut Out) {
        let (head, base) = (self.log().head().0, self.log().base_seq());
        let fork = match &self.role {
            Role::Authority { journal } => journal.fork,
            Role::Replica => None,
        };
        let n = n.clamp(base, fork.map_or(head, |(f, _)| f.min(head)));
        if n == base {
            return;
        }
        self.store.log_horizon(n);
        if self.space == Space::Durable {
            o.persist(Record::Rows(self.confirmed_rows()));
        }
    }

    /// Opens what was made durable, under this peer's schema, which may be a
    /// prefix-extension of the one it was written under (§1.2), or, for a
    /// replica behind its authority, a prefix of it: the store's rows read
    /// under it (completed, or cut to their prefix), the log's table's
    /// checked as a chain, and the log's rows appended since, in order, each
    /// linking onto the head and its facts applied, all or none, up to the
    /// first that does not — the store is then made durable whole again,
    /// without it. A state is never
    /// derived again here, and the store's digests are this schema's. An
    /// entry whose state is not what its facts give here was logged under
    /// another schema ([`Peer::foreign`]). The pending intents run again on
    /// top.
    fn open(&mut self, d: Durable, o: &mut Out) {
        self.undo_pending(o);
        self.pending.clear();
        let (log, rows): (Rows, Rows) = d.rows.into_iter().partition(|(t, _)| t == LOG);
        let (fill, rows): (Rows, Rows) = rows.into_iter().partition(|(t, _)| t == FILL);
        let log = log.into_iter().flat_map(|(_, rs)| rs);
        (self.fill, self.more) = (None, None);
        let filling = fill.into_iter().flat_map(|(_, rs)| rs).next().map(|f| Fill::from_bytes(&f));
        if let Some(fill) = filling {
            if let Ok(fill) = fill {
                self.refill(fill, &rows, log, d.logged, d.parts, o);
            }
        } else if let Ok((mut store, _)) = self.on_fit(&rows, log) {
            // Every entry read once: those the rows hold, from their rows,
            // and those appended, as their records were read; facts fitted
            // only when they are not this schema's as they are.
            let schema = store.schema_arc().clone();
            let mut entries = store.log().entries();
            let (n, held) = (d.logged.len(), entries.len());
            for l in d.logged {
                if store.append(&l, &l.facts).is_err() && store.append(&l, &fit(&schema, &l.facts).0).is_err() {
                    break;
                }
                entries.push(l);
            }
            if entries.len() - held < n {
                o.persist(Record::Rows(store.kv_rows()));
            }
            let foreign = foreign(&store, &entries);
            (self.foreign, self.faith) = (foreign, foreign.is_some_and(|f| f == store.log().head().0));
            o.tell(None, self.store.diff(&store));
            self.store = store;
            if let Role::Authority { journal } = &mut self.role {
                for l in &entries {
                    journal.verdicts.insert(l.entry.id, Verdict::Ok { seq: l.seq });
                    journal.pass(l.entry.origin, &l.entry.actor, l.entry.local);
                }
            }
        }
        self.local = self.local.max(d.local);
        self.pending = d.pending.into_iter().map(|entry| Pending::new(entry, vec![])).collect();
        self.redo_pending(o);
    }

    /// A replica's fill opened as it last stood: its rows, judging nothing,
    /// and what was appended since in the order it was taken — each entry
    /// put as it is, each part at its sequence, after the entries at or
    /// below it — up to the first that does not read; the fill on from the
    /// last part's next.
    fn refill(&mut self, mut fill: Fill, rows: &Rows, log: impl IntoIterator<Item = Vec<u8>>, logged: Vec<Logged>, parts: Vec<Part>, o: &mut Out) {
        let Ok((mut store, _)) = Store::of_rows(self.store.schema_arc().clone(), self.space, rows, false) else {
            return;
        };
        if store.start_log(log).is_err() {
            return;
        }
        fn take(store: &mut Store, p: Part, fill: &mut Fill) -> Result<(), Refusal> {
            if p.seq != store.log().head().0 {
                return Err(Refusal::new("a part not at the head"));
            }
            for r in &p.ranges {
                fill.other |= store.replace(&r.table, &r.from, r.to.as_deref(), &r.rows)?.1;
            }
            fill.next = p.next.unwrap_or_default();
            Ok(())
        }
        let (all, mut read) = (logged.len() + parts.len(), 0);
        let mut parts = parts.into_iter().peekable();
        'read: for l in logged {
            while let Some(p) = parts.next_if(|p| p.seq < l.seq) {
                if take(&mut store, p, &mut fill).is_err() {
                    break 'read;
                }
                read += 1;
            }
            let (n, h) = store.log().head();
            let (facts, other) = fit(store.schema_arc(), &l.facts);
            if l.seq != n + 1 || !l.links(&h) || store.force_all(&facts).is_err() {
                break;
            }
            fill.other |= other;
            store.log_push(&l, l.to_bytes());
            read += 1;
        }
        if read == all - parts.len() {
            for p in parts.by_ref() {
                if take(&mut store, p, &mut fill).is_err() {
                    break;
                }
                read += 1;
            }
        }
        o.tell(None, self.store.diff(&store));
        (self.store, self.fill) = (store, Some(fill));
        // What did not read is cut, so nothing is appended after it.
        if read < all {
            o.persist(Record::Rows(self.kv(&self.store)));
        }
    }
}

/// The last entry of a store's log whose state is not what its rows give
/// under this schema, read from the head down by undoing each entry's
/// facts from the tables' digests (§1.3's arithmetic, no row read): one
/// logged under another schema (§1.2); the row the log stands on when only
/// it is. `entries` are the log's, read.
fn foreign(store: &Store, entries: &[Logged]) -> Option<Seq> {
    let mut ds = store.digests();
    for l in entries.iter().rev() {
        if store.hash_with(&ds) != l.state {
            return Some(l.seq);
        }
        store.undo_of(&l.facts).apply(&mut ds);
    }
    let base = store.log().base();
    (store.hash_with(&ds) != base.state).then_some(base.seq)
}

/// A scope's verdict on a row of a table.
pub type Holds<'a> = dyn FnMut(&str, &[u8]) -> Result<bool, Refusal> + 'a;

/// §1.8 A scope's filter over facts, in order: the changes they make to the
/// rows it holds — an edit across its line as the add or the remove it is.
/// `holds` is the scope's verdict on a row of a table; a scope is a
/// function of the row alone, so the filter keeps nothing.
pub fn scoped(facts: &[Change], holds: &mut Holds) -> Result<Vec<Change>, Refusal> {
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
