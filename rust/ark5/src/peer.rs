//! §1.7 One peer machine: a log, a store, the functions it can run, its
//! own intents pending, and a role.
//!
//! ```text
//! Role  = Replica | Authority { journal }   -- "alone" is an authority whose journal has a fork
//! Input = Frame { from, bytes } | Mutate { fn, input, now } | Tick | Persisted | Open
//!       | Connected | Alone | Join | Compact
//! Output= Send | Fanout | Changes | Persist | Refused { id, why } | Confirmed(id)
//! ```
//!
//! Sans-io: [`Peer::step`] takes one input and says what to do — no
//! socket, clock or file is inside. A hub, a client, a simulator and a
//! trace player are drivers that feed it and route what it says: `Send`
//! goes back where the frame came from (a replica's upstream), `Fanout` to
//! every connection, `Persist` to the journal before anything after it is
//! sent. `from` on a frame is the identity the driver verified for its
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
//! **The authority** judges each pushed intent once — dedupe by `(session,
//! local)`, a gap in a session's pushes waits for them in order, an unknown
//! function is `Held` until its body comes — stamps it with the actor and
//! roles it judged it under, runs it, and logs it with its facts.
//!
//! **Partial sync is a view** (§1.8). A connection the driver says is fed
//! by a scope (`Input::Feed`) is served its union: the rows a `Pure`
//! function of a row — `Sum` over the schema's tables in order, each
//! variant that table's row — holds, under the connection's identity. At
//! its `Hello` the authority takes the union from its store and sends it as
//! a snapshot at the head; every entry after is sent to it alone
//! (`Output::To`) with the facts the scope's filter makes of the entry's
//! ([`crate::view::scoped`]), the union's state hash after them, and a
//! chain of its own from that snapshot. `Fanout` is for connections fed
//! whole. The replica is the same machine: it applies facts like every
//! replica, its own intents previewed over what it holds and confirmed by
//! the authority's facts, a difference being the authority's answer. A
//! union must hold the rows its rows reference; a `Hello` from a
//! connection whose union does not is answered with nothing.
//!
//! **Going alone and joining.** `Alone` makes a replica an authority over
//! its own log from where it is (the fork), sequencing its pending intents
//! and every one after. `Join` hands the chain above the fork to a server
//! and turns back into a replica, the chain its pending intents: a server
//! still at the fork adopts the chain whole when its own run of every
//! entry gives the entry's facts, and refuses every entry of it when one
//! does not; a server that moved on sequences them anew as pushes, a gap
//! in the chain's `local`s being one the device refused itself, and the
//! peer rebases onto what it answers. Until the server has answered any of
//! the chain, a new connection hands it over again rather than pushing its
//! entries one by one, which a gap would hold.
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
//! volatile authority forgets on restart what each session last pushed, so
//! there a push past a gap is taken, and one that comes after it is
//! refused as overtaken: what was live then is gone.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::frames::Frame;
use crate::hash::{EntryHash, FnHash};
use crate::log::{Durable, Entry, EntryId, Log, Logged, Record, Seq, Snapshot};
use crate::schema::{Schema, Space};
use crate::store::{Change, Read, Store};
use crate::Refusal;

/// How many logged entries a `Batch` carries at most.
pub const PAGE: usize = 256;

/// What a function knows of the intent it runs for: all frozen in the
/// entry, so every run of it, anywhere, is the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ctx {
    pub id: EntryId,
    pub actor: String,
    pub session: String,
    pub roles: Vec<String>,
    pub now: i64,
}

impl Ctx {
    pub fn of(e: &Entry) -> Ctx {
        Ctx {
            id: e.id,
            actor: e.actor.clone(),
            session: e.session.clone(),
            roles: e.roles.clone(),
            now: e.now,
        }
    }

    /// The `n`-th row id this intent draws (§1.6).
    pub fn new_id(&self, n: i64) -> [u8; 16] {
        self.id.draw(n)
    }
}

/// A function, opaque to the kernel: its changes, or its refusal.
pub trait Procedure: Send + Sync {
    fn run(&self, store: &dyn Read, ctx: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal>;

    /// A scope's verdict on its input, a row as its table's variant
    /// (§1.8): whether the union holds it. Only a `Pure` function gives one.
    fn holds(&self, _ctx: &Ctx, _input: &[u8]) -> Result<bool, Refusal> {
        Err(Refusal::new("not a scope"))
    }
}

/// What turns a function's bytes into something that runs: the language's.
pub type Loader = Arc<dyn Fn(&[u8]) -> Option<Arc<dyn Procedure>> + Send + Sync>;

/// The content store (§1.5): functions by hash, and the bytes of those it
/// can send.
#[derive(Clone, Default)]
pub struct Cas {
    procs: BTreeMap<FnHash, Arc<dyn Procedure>>,
    bodies: BTreeMap<FnHash, Vec<u8>>,
    loader: Option<Loader>,
}

impl fmt::Debug for Cas {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_set().entries(self.procs.keys()).finish()
    }
}

impl Cas {
    pub fn insert(&mut self, h: FnHash, p: Arc<dyn Procedure>) {
        self.procs.insert(h, p);
    }

    pub fn get(&self, h: &FnHash) -> Option<&Arc<dyn Procedure>> {
        self.procs.get(h)
    }

    pub fn with_loader(mut self, l: Loader) -> Cas {
        self.loader = Some(l);
        self
    }

    /// Keeps a function's bytes under their hash and loads them, when there
    /// is a loader and it can.
    pub fn add_body(&mut self, bytes: Vec<u8>) -> FnHash {
        let h = FnHash::of(&bytes);
        if let Some(p) = self.loader.as_ref().and_then(|l| l(&bytes)) {
            self.procs.insert(h, p);
        }
        self.bodies.insert(h, bytes);
        h
    }

    pub fn body(&self, h: &FnHash) -> Option<&Vec<u8>> {
        self.bodies.get(h)
    }

    /// Every body it holds, in hash order.
    pub fn bodies(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.bodies.values()
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Seq(Seq),
    Refused(String),
}

/// What an authority keeps beside its log: every intent it judged and how,
/// and each session's last `local`; and, for a device alone, where it
/// forked from the log it will join again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Journal {
    pub fork: Option<(Seq, EntryHash)>,
    pub verdicts: BTreeMap<EntryId, Verdict>,
    pub last: BTreeMap<String, i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Replica,
    Authority { journal: Journal },
}

/// What a replica is fed: every entry whole, or a scope's union (§1.8),
/// the scope by hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feed {
    Whole,
    Scoped(FnHash),
}

/// A connection an authority serves by a scope: who it is, the scope, the
/// union it holds, and the head of the chain it was sent.
#[derive(Clone, Debug)]
pub struct Served {
    pub who: Who,
    pub scope: FnHash,
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
        func: FnHash,
        input: Vec<u8>,
        now: i64,
    },
    Tick(i64),
    Persisted(Seq),
    Open(Durable),
    Connected,
    Alone,
    Join,
    Compact(Seq),
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
    Changes(Vec<Change>),
    Persist(Vec<u8>),
    Refused {
        id: EntryId,
        why: String,
    },
    Confirmed(EntryId),
}

#[derive(Default)]
struct Out {
    out: Vec<Output>,
    told: Vec<Change>,
}

impl Out {
    fn send(&mut self, f: Frame) {
        self.out.push(Output::Send(f.enc()));
    }
    fn persist(&mut self, r: Record) {
        self.out.push(Output::Persist(r.enc()));
    }
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub log: Log,
    /// The confirmed state with the pending intents' changes applied.
    pub store: Store,
    pub cas: Cas,
    pub pending: Vec<Pending>,
    pub role: Role,
    pub feed: Feed,
    pub me: Who,
    pub space: Space,
    /// The last `local` this peer's session drew.
    pub local: i64,
    /// Where a confirmed entry's state was not this peer's own.
    pub diverged: Vec<Seq>,
    /// The cursor a `Hello` is outstanding at.
    asked: Option<Seq>,
    /// An authority's connections fed by a scope, by session.
    pub served: BTreeMap<String, Served>,
    /// What this device says it is, in every `Hello` (§1.9).
    pub device: Option<Vec<u8>>,
    /// The chain a join handed over, until the server has answered any of
    /// it: a connection made before then hands it over again.
    pub joining: Option<(Seq, EntryHash, Vec<Logged>)>,
}

fn empty_snapshot(store: &Store) -> Snapshot {
    Snapshot {
        seq: 0,
        hash: EntryHash::default(),
        state: store.state_hash(),
        rows: store.snapshot_rows(),
    }
}

impl Peer {
    pub fn new(schema: Arc<Schema>, cas: Cas, role: Role, me: Who) -> Peer {
        let store = Store::new(schema);
        Peer {
            log: Log::new(empty_snapshot(&store)),
            store,
            cas,
            pending: vec![],
            role,
            feed: Feed::Whole,
            me,
            space: Space::Durable,
            local: 0,
            diverged: vec![],
            asked: None,
            served: BTreeMap::new(),
            device: None,
            joining: None,
        }
    }

    /// This peer over `space`'s tables (§1.9), from empty.
    pub fn in_space(mut self, space: Space) -> Peer {
        self.space = space;
        self.store = self.blank();
        self.log = Log::new(empty_snapshot(&self.store));
        self
    }

    fn blank(&self) -> Store {
        Store::new_in(self.store.schema_arc().clone(), self.space)
    }

    fn of_rows(&self, rows: &[(String, Vec<Vec<u8>>)]) -> Result<Store, Refusal> {
        Store::from_rows_in(self.store.schema_arc().clone(), self.space, rows)
    }

    pub fn is_authority(&self) -> bool {
        matches!(self.role, Role::Authority { .. })
    }

    /// The confirmed state's hash: the head's.
    pub fn confirmed_hash(&self) -> crate::hash::StateHash {
        self.log.head_state()
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
            Input::Mutate { func, input, now } => self.mutate(func, input, now, &mut o),
            Input::Tick(_) | Input::Persisted(_) => {}
            Input::Open(d) => self.open(d, &mut o),
            Input::Connected => self.connected(&mut o),
            Input::Alone => self.alone(&mut o),
            Input::Join => self.join(&mut o),
            Input::Compact(n) => self.compact(n, &mut o),
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
        let p = self.cas.get(&e.func).cloned().ok_or_else(|| Refusal(format!("no function {}", e.func)))?;
        let changes = p.run(&self.store, &Ctx::of(e), &e.input)?;
        let schema = self.store.schema_arc().clone();
        if let Some(c) = changes.iter().find(|c| schema.table(c.table()).is_some_and(|t| t.1.space != self.space)) {
            return Err(Refusal(format!("{} is not in this space", c.table())));
        }
        self.store.apply_all(&changes)?;
        o.told.extend(changes.iter().cloned());
        Ok(changes)
    }

    fn undo_pending(&mut self, o: &mut Out) {
        for c in self.pending.iter().rev().flat_map(|p| p.changes.iter().rev()) {
            let c = c.inverse();
            self.store.apply(&c).expect("a pending change undoes");
            o.told.push(c);
        }
    }

    /// Runs the pending intents again, in order, over the store as it is;
    /// one that now refuses stays pending with no changes, the authority's
    /// to judge.
    fn redo_pending(&mut self, o: &mut Out) {
        for mut p in std::mem::take(&mut self.pending) {
            p.changes = self.attempt(&p.entry, o).unwrap_or_default();
            self.pending.push(p);
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

    fn mutate(&mut self, func: FnHash, input: Vec<u8>, now: i64, o: &mut Out) {
        let local = self.local + 1;
        let entry = Entry {
            id: EntryId::of(&self.me.session, local),
            actor: self.me.actor.clone(),
            session: self.me.session.clone(),
            local,
            roles: self.me.roles.clone(),
            func,
            input,
            now,
        };
        let id = entry.id;
        if self.is_authority() {
            // One it refuses draws no `local`: a chain made alone has no gaps.
            self.decide(entry, o);
            if let Some(Output::Confirmed(_)) = o.out.last() {
                self.local = local;
            }
            return;
        }
        match self.attempt(&entry, o) {
            Err(why) => o.out.push(Output::Refused { id, why: why.0 }),
            Ok(changes) => {
                self.local = local;
                o.send(Frame::Push {
                    space: self.space,
                    entry: entry.clone(),
                    body: None,
                });
                self.pending.push(Pending { entry, changes });
            }
        }
    }

    /// An authority's own intent, sequenced, and the verdict told.
    fn decide(&mut self, entry: Entry, o: &mut Out) {
        let id = entry.id;
        let v = self.sequence(entry, None, o);
        o.out.push(v.map_or_else(|why| Output::Refused { id, why }, |_| Output::Confirmed(id)));
    }

    /// The authority's judgment of one intent: run, log, tell everyone. With
    /// `expect`, the facts must be those or it is refused.
    fn sequence(&mut self, entry: Entry, expect: Option<&[Change]>, o: &mut Out) -> Result<Seq, String> {
        let mut r = self.attempt(&entry, o);
        if let (Ok(facts), Some(x)) = (&r, expect) {
            if facts != x {
                self.store
                    .apply_all(&facts.iter().rev().map(Change::inverse).collect::<Vec<_>>())
                    .expect("its inverse applies");
                r = Err(Refusal::new("its facts are not what this authority's run of it gives"));
            }
        }
        let (id, session, local) = (entry.id, entry.session.clone(), entry.local);
        self.journal().last.insert(session, local);
        let v = match r {
            Err(why) => Err(why.0),
            Ok(facts) => {
                let state = self.store.state_hash();
                let l = self.log.append(entry, facts, state).clone();
                o.persist(Record::Logged(l.clone()));
                o.out.push(Output::Fanout(
                    Frame::Batch {
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
        let verdict = v.clone().map_or_else(Verdict::Refused, Verdict::Seq);
        self.journal().verdicts.insert(id, verdict);
        v
    }

    // * Frames

    fn frame(&mut self, from: Option<Who>, f: Frame, o: &mut Out) {
        let space = match &f {
            Frame::Hello { space, .. } | Frame::Push { space, .. } | Frame::Batch { space, .. } => Some(*space),
            Frame::Snapshot { space, .. } | Frame::Join { space, .. } => Some(*space),
            Frame::Ack { space, .. } | Frame::Reject { space, .. } | Frame::Held { space, .. } => Some(*space),
            _ => None,
        };
        if space.is_some_and(|s| s != self.space) {
            return;
        }
        let scoped = from.as_ref().filter(|w| self.served.contains_key(&w.session)).map(|w| w.session.clone());
        match (f, self.is_authority(), from) {
            (Frame::Hello { cursor, hash, .. }, true, _) if scoped.is_some() => self.serve_scoped(&scoped.unwrap(), cursor, hash, o),
            (Frame::Hello { cursor, hash, .. }, true, _) => self.serve(cursor, hash, o),
            (Frame::Push { entry, body, .. }, true, Some(who)) => self.push(&who, entry, body, o),
            (Frame::Join { from, hash, logged, .. }, true, Some(who)) => self.adopt(&who, (from, hash), logged, o),
            (Frame::Fetch { hashes }, true, _) => {
                let bodies = hashes.iter().filter_map(|h| Some((*h, self.cas.body(h)?.clone()))).collect();
                o.send(Frame::Bodies { bodies });
            }
            (Frame::Bodies { bodies }, _, _) => bodies.into_iter().for_each(|(_, b)| {
                self.cas.add_body(b);
            }),
            (Frame::Batch { logged, .. }, false, _) => self.confirm(logged, o),
            (Frame::Snapshot { snapshot, .. }, false, _) => self.reset(snapshot, o),
            (Frame::Ack { id, seq, .. }, false, _) => {
                // At or below where this peer stands, an intent still pending
                // here is in the state it holds (a snapshot carried it).
                if seq <= self.log.head().0 && self.settle(&id, o) {
                    o.out.push(Output::Confirmed(id));
                }
            }
            (Frame::Reject { id, why, .. }, false, _) => {
                if self.settle(&id, o) {
                    o.out.push(Output::Refused { id, why });
                }
            }
            (Frame::Held { id, func, .. }, false, _) => {
                if let (Some(p), Some(b)) = (self.pending.iter().find(|p| p.entry.id == id), self.cas.body(&func)) {
                    o.send(Frame::Push {
                        space: self.space,
                        entry: p.entry.clone(),
                        body: Some(b.clone()),
                    });
                }
            }
            _ => {}
        }
    }

    /// A `Hello` at `(cursor, hash)`: the pages from there when this log has
    /// that hash there, the snapshot at the head when it does not.
    fn serve(&mut self, cursor: Seq, hash: Option<EntryHash>, o: &mut Out) {
        let has = match hash {
            Some(h) => self.log.has(cursor, &h),
            None => cursor == 0 && self.log.base.seq == 0,
        };
        if !has {
            let (seq, hash) = self.log.head();
            let snapshot = Snapshot {
                seq,
                hash,
                state: self.store.state_hash(),
                rows: self.store.snapshot_rows(),
            };
            return o.send(Frame::Snapshot { space: self.space, snapshot });
        }
        for page in self.log.after(cursor).chunks(PAGE) {
            o.send(Frame::Batch {
                space: self.space,
                from: page[0].seq - 1,
                logged: page.to_vec(),
            });
        }
    }

    /// A scope's verdict on a row of `table`, for the connection it serves.
    fn holder(&self, session: &str) -> impl FnMut(&str, &[u8]) -> Result<bool, Refusal> {
        let s = &self.served[session];
        let ctx = Ctx {
            id: EntryId::default(),
            actor: s.who.actor.clone(),
            session: s.who.session.clone(),
            roles: s.who.roles.clone(),
            now: 0,
        };
        let (scope, schema) = (self.cas.get(&s.scope).cloned(), self.store.schema_arc().clone());
        move |table, row| {
            let scope = scope.as_ref().ok_or_else(|| Refusal::new("no such scope"))?;
            let i = schema.table(table).ok_or_else(|| Refusal(format!("no table {table}")))?.0;
            scope.holds(&ctx, &[&[i as u8][..], row].concat())
        }
    }

    /// A `Hello` from a connection fed by a scope: nothing when it stands at
    /// the head of what it was sent, and otherwise its union, afresh, as a
    /// snapshot at the head.
    fn serve_scoped(&mut self, session: &str, cursor: Seq, hash: Option<EntryHash>, o: &mut Out) {
        let head = self.log.head();
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
            let facts = crate::view::scoped(&l.facts, &mut self.holder(&session));
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
            let bytes = Frame::Batch {
                space: self.space,
                from: at,
                logged,
            }
            .enc();
            o.out.push(Output::To { session, bytes });
        }
    }

    fn push(&mut self, who: &Who, mut entry: Entry, body: Option<Vec<u8>>, o: &mut Out) {
        let (id, space) = (entry.id, self.space);
        let asked = body.is_some();
        if let Some(b) = body {
            self.cas.add_body(b);
        }
        let j = self.journal();
        match j.verdicts.get(&id) {
            Some(Verdict::Seq(seq)) => {
                return o.send(Frame::Ack {
                    space,
                    id,
                    seq: *seq,
                    facts: None,
                })
            }
            Some(Verdict::Refused(why)) => return o.send(Frame::Reject { space, id, why: why.clone() }),
            None => {}
        }
        if !entry.id_checks() || entry.session != who.session {
            let why = "an entry of another session, or an id that is not its own".into();
            return o.send(Frame::Reject { space, id, why });
        }
        // A gap: the ones before it come first, pushed again after a Hello.
        let last = j.last.get(&entry.session).copied().unwrap_or(0);
        // At or below the last and judged by no verdict here: in the log's
        // space, sequenced below the horizon by a log opened since (a
        // refusal's verdict is journaled); in the volatile, overtaken.
        if entry.local <= last {
            return o.send(match space {
                Space::Durable => Frame::Ack {
                    space,
                    id,
                    seq: self.log.base.seq,
                    facts: None,
                },
                Space::Volatile => Frame::Reject {
                    space,
                    id,
                    why: "overtaken by a later one of its session".into(),
                },
            });
        }
        if entry.local != last + 1 && !(space == Space::Volatile && entry.local > last) {
            return;
        }
        if self.cas.get(&entry.func).is_none() {
            return o.send(match asked {
                false => Frame::Held { space, id, func: entry.func },
                true => Frame::Reject {
                    space,
                    id,
                    why: format!("no function {}", entry.func),
                },
            });
        }
        entry.actor = who.actor.clone();
        entry.roles = who.roles.clone();
        match self.sequence(entry, None, o) {
            Ok(seq) => o.send(Frame::Ack { space, id, seq, facts: None }),
            Err(why) => o.send(Frame::Reject { space, id, why }),
        }
    }

    /// A chain made alone, handed over from `at`.
    fn adopt(&mut self, who: &Who, at: (Seq, EntryHash), chain: Vec<Logged>, o: &mut Out) {
        let fresh = !chain.iter().any(|l| self.journal().verdicts.contains_key(&l.entry.id));
        if !fresh || self.log.head() != at {
            // A chain is its session's whole history above the fork: a gap
            // in it is an intent the device refused itself, never to come.
            for l in chain {
                let last = self.journal().last.entry(l.entry.session.clone()).or_insert(0);
                if l.entry.session == who.session && l.entry.local > *last + 1 {
                    *last = l.entry.local - 1;
                }
                self.push(who, l.entry, None, o);
            }
            return;
        }
        let (store, log, role) = (self.store.clone(), self.log.clone(), self.role.clone());
        let mut done = Out::default();
        for l in &chain {
            let mut e = l.entry.clone();
            (e.actor, e.roles) = (who.actor.clone(), who.roles.clone());
            let owned = e.id_checks() && e.session == who.session;
            if !owned || self.sequence(e, Some(&l.facts), &mut done).is_err() {
                (self.store, self.log, self.role) = (store, log, role);
                let why = format!("the chain's facts at {} are not what this authority's run gives", l.seq);
                for l in &chain {
                    let (id, s, n) = (l.entry.id, l.entry.session.clone(), l.entry.local);
                    let last = self.journal().last.entry(s).or_insert(0);
                    *last = (*last).max(n);
                    self.journal().verdicts.insert(id, Verdict::Refused(why.clone()));
                    o.send(Frame::Reject {
                        space: self.space,
                        id,
                        why: why.clone(),
                    });
                }
                return;
            }
        }
        o.told.extend(done.told);
        o.out.extend(done.out);
        let (n, _) = at;
        for (k, l) in chain.iter().enumerate() {
            o.send(Frame::Ack {
                space: self.space,
                id: l.entry.id,
                seq: n + 1 + k as Seq,
                facts: None,
            });
        }
    }

    /// Confirmed entries, applied by their facts: a page is one rebase.
    /// The pending intents are undone at the first entry that is not the
    /// oldest of them as previewed, every entry's facts are applied and
    /// its state and chain checked, and the intents still pending run
    /// again once, after the page.
    fn confirm(&mut self, ls: Vec<Logged>, o: &mut Out) {
        let mut undone = false;
        for l in ls {
            let (n, prev) = self.log.head();
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
            let own = self.pending.first().is_some_and(|p| p.entry.id == l.entry.id);
            if !undone && own && self.pending[0].changes == l.facts {
                if self.store.hash_beside(self.pending[1..].iter().flat_map(|p| &p.changes)) != l.state {
                    return self.diverge(l.seq, o);
                }
                self.pending.remove(0);
            } else {
                if !undone {
                    self.undo_pending(o);
                    undone = true;
                }
                if self.store.apply_all(&l.facts).is_err() || self.store.state_hash() != l.state {
                    self.redo_pending(o);
                    return self.diverge(l.seq, o);
                }
                o.told.extend(l.facts.iter().cloned());
                if let Some(i) = self.pending.iter().position(|p| p.entry.id == l.entry.id) {
                    self.pending.remove(i);
                }
            }
            if l.entry.session == self.me.session {
                o.out.push(Output::Confirmed(l.entry.id));
            }
            self.asked = None;
            o.persist(Record::Logged(l.clone()));
            self.log.push(l).expect("it links");
        }
        if undone {
            self.redo_pending(o);
        }
    }

    fn ask(&mut self, o: &mut Out) {
        let (n, h) = self.log.head();
        if self.asked != Some(n) {
            self.asked = Some(n);
            o.send(Frame::Hello {
                space: self.space,
                cursor: n,
                hash: Some(h),
                device: self.device.clone(),
            });
        }
    }

    /// The state an entry says is not this peer's: start again from nothing,
    /// which the authority answers with what it has.
    fn diverge(&mut self, seq: Seq, o: &mut Out) {
        self.diverged.push(seq);
        self.undo_pending(o);
        let fresh = self.blank();
        o.told.extend(self.store.diff(&fresh));
        self.log = Log::new(empty_snapshot(&fresh));
        self.store = fresh;
        self.redo_pending(o);
        self.asked = Some(0);
        o.send(Frame::Hello {
            space: self.space,
            cursor: 0,
            hash: None,
            device: self.device.clone(),
        });
    }

    /// A snapshot: the store becomes its rows, the log stands on it, the
    /// pending intents run again on top and are pushed again.
    fn reset(&mut self, s: Snapshot, o: &mut Out) {
        // One this history passed is a late copy, but a volatile authority
        // that restarted is behind its replicas: they go back to it.
        let (n, _) = self.log.head();
        if (s.seq == n || s.seq < n && self.space == Space::Durable) && self.log.has(s.seq, &s.hash) {
            return;
        }
        let Ok(new) = self.of_rows(&s.rows) else {
            return;
        };
        if new.state_hash() != s.state {
            return;
        }
        self.undo_pending(o);
        o.told.extend(self.store.diff(&new));
        self.store = new;
        o.persist(Record::Snapshot(s.clone()));
        self.log = Log::new(s);
        self.asked = None;
        self.redo_pending(o);
        self.repush(o);
    }

    fn repush(&mut self, o: &mut Out) {
        for p in &self.pending {
            o.send(Frame::Push {
                space: self.space,
                entry: p.entry.clone(),
                body: None,
            });
        }
    }

    // * Lifecycle

    fn connected(&mut self, o: &mut Out) {
        if self.is_authority() {
            return;
        }
        self.asked = None;
        self.ask(o);
        match self.joining.take() {
            Some(j) if self.pending.first().map(|p| p.entry.id) == j.2.first().map(|l| l.entry.id) => {
                let (from, hash, logged) = j.clone();
                let all = self.pending.clone();
                self.pending.retain(|p| !logged.iter().any(|l| l.entry.id == p.entry.id));
                o.send(Frame::Join {
                    space: self.space,
                    from,
                    hash,
                    logged,
                });
                self.repush(o);
                (self.pending, self.joining) = (all, Some(j));
            }
            _ => self.repush(o),
        }
    }

    fn alone(&mut self, o: &mut Out) {
        if self.is_authority() {
            return;
        }
        self.undo_pending(o);
        let fork = Some(self.log.head());
        self.role = Role::Authority {
            journal: Journal { fork, ..Journal::default() },
        };
        for p in std::mem::take(&mut self.pending) {
            self.decide(p.entry, o);
        }
    }

    fn join(&mut self, o: &mut Out) {
        let Role::Authority {
            journal: Journal { fork: Some((at, hash)), .. },
        } = self.role
        else {
            return;
        };
        let chain = self.log.after(at).to_vec();
        self.log.truncate(at);
        self.pending = chain
            .iter()
            .map(|l| Pending {
                entry: l.entry.clone(),
                changes: l.facts.clone(),
            })
            .collect();
        self.role = Role::Replica;
        self.asked = Some(at);
        o.send(Frame::Hello {
            space: self.space,
            cursor: at,
            hash: Some(hash),
            device: self.device.clone(),
        });
        self.joining = Some((at, hash, chain.clone()));
        o.send(Frame::Join {
            space: self.space,
            from: at,
            hash,
            logged: chain,
        });
    }

    /// Moves the horizon to `n` (an authority's; a device alone keeps what
    /// is above its fork): the log stands on the snapshot there.
    fn compact(&mut self, n: Seq, o: &mut Out) {
        let Role::Authority { journal } = &self.role else { return };
        let (head, _) = self.log.head();
        let n = n.clamp(self.log.base.seq, journal.fork.map_or(head, |(f, _)| f.min(head)));
        let Some(at) = self.log.at(n) else { return };
        let mut s = self.store.clone();
        for c in self.log.after(n).iter().rev().flat_map(|l| l.facts.iter().rev()) {
            s.apply(&c.inverse()).expect("a logged fact undoes");
        }
        let base = Snapshot {
            seq: n,
            hash: at.hash,
            state: at.state,
            rows: s.snapshot_rows(),
        };
        let rest = self.log.after(n).to_vec();
        o.persist(Record::Snapshot(base.clone()));
        for l in &rest {
            o.persist(Record::Logged(l.clone()));
        }
        self.log = Log { base, entries: rest };
    }

    /// Opens what was made durable: the snapshot, the chain on it as far as
    /// each entry's facts give its state, and the pending intents run again.
    fn open(&mut self, d: Durable, o: &mut Out) {
        let Ok(mut store) = self.of_rows(&d.snapshot.rows) else {
            return;
        };
        if store.state_hash() != d.snapshot.state {
            return;
        }
        let mut log = Log::new(d.snapshot);
        for l in d.chain {
            if store.apply_all(&l.facts).is_err() {
                break;
            }
            if store.state_hash() != l.state || log.push(l.clone()).is_err() {
                store
                    .apply_all(&l.facts.iter().rev().map(Change::inverse).collect::<Vec<_>>())
                    .expect("its inverse applies");
                break;
            }
        }
        self.undo_pending(o);
        o.told.extend(self.store.diff(&store));
        (self.store, self.log) = (store, log);
        if let Role::Authority { journal } = &mut self.role {
            for l in &self.log.entries {
                journal.verdicts.insert(l.entry.id, Verdict::Seq(l.seq));
                let last = journal.last.entry(l.entry.session.clone()).or_insert(0);
                *last = (*last).max(l.entry.local);
            }
        }
        self.local = self.local.max(d.local);
        self.pending = d.pending.into_iter().map(|entry| Pending { entry, changes: vec![] }).collect();
        self.redo_pending(o);
    }
}
