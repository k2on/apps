//! The loop: a [`Node`] takes one entry at a time ([`Machine::step`]),
//! runs its handler in a transaction on its space's rows, and returns the
//! entries the handler made, which the host routes by their space
//! ([`Route`]).
//!
//! A handler is found by the number its body names ([`Body::handler`]) in
//! the node's table of them, written by the build. It reads and writes
//! through [`Cx::db`], an [`Overlay`] whose writes are kept whole when it
//! returns `Ok` and dropped when it refuses. A refusal is a code
//! ([`Refusal`]).
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
//!   confirmed rows at a `seq` ([`Down::Fill`], then [`Down::Rows`]), in
//!   place of the log up to it, then the log after it. Its pending entries
//!   are rebased once the fill ends; one interrupted is started again at
//!   the next connection, its first part emptying what the last left.
//! - An entry the server refuses is dropped, and its refusal kept in the
//!   node's journal of refusals for the host to show. It never blocks the
//!   entries after it: each is judged on its own.
//! - An entry is let go once it is confirmed: logged here, or answered by
//!   an `At` verdict (it, or one after it) whose `seq` this node has
//!   applied. The link is ordered, so each entry's own answer comes before
//!   those of the entries pushed after it.
//!
//! The entries a shared handler emits are taken from its first run here
//! only, never from a rebase.
//!
//! **What changed.** The node notes each row of the shared view that its
//! own writes, facts, fills and rebases touch, with what it read before;
//! [`Node::take_changes`] answers what changed since the host last asked,
//! for the queries over the view ([`crate::plan`]). The host takes them
//! after each step or receive; until then they gather.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::entry::{Actor, Entry, EntryId, Hlc, Origin, Seq, Space, Version};
use crate::plan::{Changes, Diff};
use crate::store::{Layer, Overlay, Range, Read, Store, TableId, View, Writes, EMPTY};
use crate::sync::{Down, Logged, Pending, Up, Verdict};

/// The machine every node is: one entry in, the entries it made out.
pub trait Machine {
    type Body;
    fn step(&mut self, e: Entry<Self::Body>) -> Vec<Entry<Self::Body>>;
}

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
}

/// What a handler runs with: the entry, its space's rows, and the entries
/// it makes.
pub struct Cx<'a, B> {
    pub entry: &'a Entry<B>,
    /// The entry's space's rows, as this run has written them.
    pub db: &'a mut dyn Store,
    made: Vec<(Space, B)>,
}

impl<B> Cx<'_, B> {
    /// Makes an entry in `space`, caused by this one, for the host to route
    /// once this run is kept.
    pub fn emit(&mut self, space: Space, body: B) {
        self.made.push((space, body));
    }
}

/// A handler: reads and writes `cx.db`, emits entries, or refuses.
pub type Handler<B> = fn(&mut Cx<'_, B>) -> Result<(), Refusal>;

/// What a handler's run made: its writes and the entries it emitted.
pub(crate) type Ran<B> = (Writes, Vec<(Space, B)>);

/// Runs `e`'s handler over `base`, writing nothing to it.
pub(crate) fn run<B: Body>(handlers: &[Handler<B>], base: &dyn Read, e: &Entry<B>) -> Result<Ran<B>, Refusal> {
    let handler = handlers.get(e.body.handler().0 as usize).ok_or(Refusal::NO_HANDLER)?;
    let mut db = Overlay::new(base);
    let mut cx = Cx {
        entry: e,
        db: &mut db,
        made: Vec::new(),
    };
    handler(&mut cx)?;
    let made = cx.made;
    Ok((db.into_writes(), made))
}

/// One store for each space.
pub struct Spaces<S> {
    pub local: S,
    pub device: S,
    pub session: S,
    /// The shared space's confirmed rows.
    pub shared: S,
}

impl<S> Spaces<S> {
    pub fn get(&self, space: Space) -> &S {
        match space {
            Space::Local => &self.local,
            Space::Device => &self.device,
            Space::Session => &self.session,
            Space::Shared => &self.shared,
        }
    }

    pub fn get_mut(&mut self, space: Space) -> &mut S {
        match space {
            Space::Local => &mut self.local,
            Space::Device => &mut self.device,
            Space::Session => &mut self.session,
            Space::Shared => &mut self.shared,
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
    /// `session`: sent upstream, to the user's other nodes; never logged.
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

/// One process running the loop.
pub struct Node<B: 'static, S> {
    origin: Origin,
    made: u64,
    actor: Actor,
    version: Version,
    clock: Hlc,
    handlers: &'static [Handler<B>],
    stores: Spaces<S>,
    /// The last `seq` whose facts the confirmed rows hold.
    cursor: Seq,
    /// This node's shared entries not yet confirmed, oldest first.
    pending: Vec<Pending<B>>,
    /// Every pending entry's writes, in order: what the shared space reads
    /// as over the confirmed rows.
    layer: Layer,
    refused: Vec<(EntryId, Refusal)>,
    /// A fill under way: the `seq` it reaches and the parts still to come.
    filling: Option<(Seq, u32)>,
    /// The shared view's rows touched since the changes were last taken,
    /// each with what it read before; or everything, after a fill or a
    /// resume.
    touched: Layer,
    reset: bool,
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
            stores,
            cursor: Seq(0),
            pending: Vec::new(),
            layer: Layer::new(),
            refused: Vec::new(),
            filling: None,
            touched: Layer::new(),
            reset: false,
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
        self.layer = Layer::new();
        for p in &pending {
            self.layer.apply(&p.writes);
        }
        self.pending = pending;
        self.reset = true;
    }

    /// What changed in the shared view since this was last called, its
    /// rows as the view reads them now.
    pub fn take_changes(&mut self) -> Changes {
        let touched = core::mem::take(&mut self.touched);
        let reset = core::mem::take(&mut self.reset);
        let view = View {
            base: &self.stores.shared,
            layer: &self.layer,
        };
        let rows = touched.into_writes().0.into_iter().filter(|_| !reset).filter_map(|w| {
            let new = view.get(w.table, &w.key);
            (w.value != new).then_some((
                w.table,
                Diff {
                    key: w.key,
                    old: w.value,
                    new,
                },
            ))
        });
        Changes { reset, rows: rows.collect() }
    }

    /// Who the entries this node makes are from.
    pub fn set_actor(&mut self, actor: Actor) {
        self.actor = actor;
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

    /// A space's store: for `shared`, its confirmed rows alone.
    pub fn store(&self, space: Space) -> &S {
        self.stores.get(space)
    }

    /// A space as this node reads it: for `shared`, the confirmed rows
    /// with the pending entries' writes over them.
    pub fn view(&self, space: Space) -> View<'_> {
        let layer = if space == Space::Shared { &self.layer } else { &EMPTY };
        View {
            base: self.stores.get(space),
            layer,
        }
    }

    /// The entries refused since the last call, oldest first.
    pub fn take_refused(&mut self) -> Vec<(EntryId, Refusal)> {
        core::mem::take(&mut self.refused)
    }

    /// A new entry of this node's, at wall time `now`: a tap, a timer.
    pub fn make(&mut self, space: Space, body: B, now: u64) -> Entry<B> {
        self.clock = self.clock.tick(now);
        self.made += 1;
        Entry {
            id: EntryId {
                origin: self.origin,
                n: self.made,
            },
            space,
            actor: self.actor,
            at: self.clock,
            cause: None,
            version: self.version,
            seq: None,
            body,
        }
    }

    /// An entry `cause`'s handler emitted, made this node's.
    fn made_by(&mut self, cause: &Entry<B>, (space, body): (Space, B)) -> Entry<B> {
        self.clock = self.clock.tick(self.clock.wall);
        self.made += 1;
        Entry {
            id: EntryId {
                origin: self.origin,
                n: self.made,
            },
            space,
            actor: cause.actor,
            at: self.clock,
            cause: Some(cause.id),
            version: self.version,
            seq: None,
            body,
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
        match run(self.handlers, self.stores.get(e.space), &e) {
            Ok((writes, made)) => {
                self.stores.get_mut(e.space).apply(&writes);
                made.into_iter().map(|m| self.made_by(&e, m)).collect()
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
        let view = View {
            base: &self.stores.shared,
            layer: &self.layer,
        };
        let (writes, made) = run(self.handlers, &view, &e).unwrap_or_default();
        note(&mut self.touched, &view, writes.0.iter().map(|w| (w.table, &w.key[..])));
        self.layer.apply(&writes);
        let made = made.into_iter().map(|m| self.made_by(&e, m)).collect();
        self.pending.push(Pending {
            entry: e,
            writes,
            judged: None,
        });
        made
    }

    /// Routes an entry this node made, and what stepping it makes, until
    /// nothing is left to step: in order, each `local` and `device` entry
    /// stepped here, each `shared` one stepped and pushed, each `session`
    /// one sent.
    pub fn route(&mut self, e: Entry<B>, link: &mut dyn Link<B>) {
        self.run(VecDeque::from([e]), link);
    }

    fn run(&mut self, mut queue: VecDeque<Entry<B>>, link: &mut dyn Link<B>) {
        while let Some(e) = queue.pop_front() {
            match Route::of(e.space) {
                Route::Step => queue.extend(self.step(e)),
                Route::Pending => {
                    let made = self.step(e.clone());
                    link.send(Up::Push(e));
                    queue.extend(made);
                }
                Route::Send => link.send(Up::Live(e)),
            }
        }
    }

    /// What this node says at each connection: its cursor, then every
    /// pending entry again, oldest first. The upstream judges each once.
    /// A fill the last connection left unfinished is let go.
    pub fn connect(&mut self, link: &mut dyn Link<B>) {
        self.filling = None;
        link.send(Up::Hello { cursor: self.cursor });
        for p in &self.pending {
            link.send(Up::Push(p.entry.clone()));
        }
    }

    /// Takes in what the upstream sent, and routes what it makes.
    pub fn receive(&mut self, down: Down<B>, link: &mut dyn Link<B>) {
        match down {
            Down::Logged(l) => self.confirm(l),
            Down::Verdict { id, verdict } => self.judge(id, verdict),
            Down::Live(e) => {
                let made = self.step(e);
                self.run(made.into(), link);
            }
            Down::Fill { seq, tables, parts } => {
                for table in tables {
                    let mut keys = Vec::new();
                    self.stores.shared.scan(table, Range::all(), &mut |k, _| {
                        keys.push(k.to_vec());
                        true
                    });
                    for k in keys {
                        self.stores.shared.delete(table, &k);
                    }
                }
                self.filling = Some((seq, parts));
                self.reset = true;
                self.filled();
            }
            Down::Rows(rows) => {
                if let Some((_, parts)) = &mut self.filling {
                    self.stores.shared.apply(&rows);
                    self.reset = true;
                    *parts = parts.saturating_sub(1);
                    self.filled();
                }
            }
        }
    }

    /// Ends a fill whose every part has come: the node is at its `seq`.
    fn filled(&mut self) {
        let Some((seq, 0)) = self.filling else { return };
        self.filling = None;
        self.cursor = seq;
        self.pending.retain(|p| p.judged.is_none_or(|j| j > seq));
        self.rebase();
    }

    /// Applies the log's next entry by its facts, lets go of what it
    /// confirms, and rebases. An entry this node has, or one past a gap
    /// (which the next connection's hello fills), is passed over.
    fn confirm(&mut self, l: Logged<B>) {
        let Some(seq) = l.entry.seq.filter(|s| s.0 == self.cursor.0 + 1 && self.filling.is_none()) else {
            return;
        };
        self.clock = self.clock.witness(l.entry.at, l.entry.at.wall);
        let view = View {
            base: &self.stores.shared,
            layer: &self.layer,
        };
        note(&mut self.touched, &view, l.facts.0.iter().map(|w| (w.table, &w.key[..])));
        self.stores.shared.apply(&l.facts);
        self.cursor = seq;
        let id = l.entry.id;
        let previewed = self.pending.first().is_some_and(|p| p.entry.id == id && p.writes == l.facts);
        let before = self.pending.len();
        self.pending.retain(|p| p.entry.id != id && p.judged.is_none_or(|j| j > seq));
        if previewed && self.pending.len() + 1 == before {
            // The server wrote what this node's run did, so the confirmed
            // rows are what the entries after it ran over: their writes
            // stand, and only the layer is made again without it, which
            // reads the same.
            self.layer = Layer::new();
            for p in &self.pending {
                self.layer.apply(&p.writes);
            }
            #[cfg(any(test, ark_selftest))]
            {
                self.skipped += 1;
            }
        } else {
            self.rebase();
        }
    }

    /// Takes the authority's answer to one of this node's pushes.
    fn judge(&mut self, id: EntryId, verdict: Verdict) {
        match verdict {
            Verdict::Refused(why) => {
                let Some(i) = self.pending.iter().position(|p| p.entry.id == id) else {
                    return;
                };
                self.pending.remove(i);
                self.refused.push((id, why));
            }
            // Covering the entries up to the one it answers: their own
            // answers came before it, and a refusal of one after it, under
            // `last`, is still to come.
            Verdict::At { seq, .. } => {
                for p in &mut self.pending {
                    if p.entry.id.origin == id.origin && p.entry.id.n <= id.n && p.judged.is_none() {
                        p.judged = Some(seq);
                    }
                }
                let cursor = self.cursor;
                let before = self.pending.len();
                self.pending.retain(|p| p.judged.is_none_or(|j| j > cursor));
                if self.pending.len() == before {
                    return;
                }
            }
        }
        self.rebase();
    }

    /// Runs every pending entry again, in order, over the confirmed rows.
    fn rebase(&mut self) {
        let view = View {
            base: &self.stores.shared,
            layer: &self.layer,
        };
        note(&mut self.touched, &view, self.layer.keys());
        let mut layer = Layer::new();
        for p in &mut self.pending {
            let view = View {
                base: &self.stores.shared,
                layer: &layer,
            };
            p.writes = run(self.handlers, &view, &p.entry).map(|(writes, _)| writes).unwrap_or_default();
            layer.apply(&p.writes);
        }
        // A row only the new layer writes was read from the confirmed rows
        // before, which the facts did not touch, or it would be noted.
        note(&mut self.touched, &self.stores.shared, layer.keys());
        self.layer = layer;
    }
}

/// Notes what each of `keys` reads in `view` now, unless it is noted.
fn note<'k>(touched: &mut Layer, view: &dyn Read, keys: impl Iterator<Item = (TableId, &'k [u8])>) {
    for (table, key) in keys {
        touched.keep(table, key, || view.get(table, key));
    }
}

impl<B: Body + Clone + 'static, S: Store> Machine for Node<B, S> {
    type Body = B;

    fn step(&mut self, e: Entry<B>) -> Vec<Entry<B>> {
        Node::step(self, e)
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

    /// A counter app: `Add` adds to a count, `Take` takes one and refuses
    /// at none, `Tap` asks for a shared `Add` and a local `Seen`, `Seen`
    /// counts taps, `Refuse` writes and refuses.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) enum Msg {
        Add { key: u8, by: u64 },
        Tap { key: u8 },
        Seen,
        Refuse,
        Take { key: u8 },
        Set { key: u8, value: u64 },
        Unknown,
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
                Msg::Unknown => 9,
            })
        }

        fn row(&self) -> Option<(TableId, Vec<u8>)> {
            match self {
                Msg::Set { key, .. } => Some((POSITIONS, alloc::vec![*key])),
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

    pub(crate) static HANDLERS: [Handler<Msg>; 6] = [
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
            add(cx, 0, 1);
            Ok(())
        },
        |cx| {
            add(cx, 0, 100);
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
            cx.db.put(POSITIONS, &[key], &value.to_le_bytes());
            Ok(())
        },
    ];

    pub(crate) fn node(origin: u64) -> Node<Msg, Memory> {
        let stores = Spaces {
            local: Memory::new(),
            device: Memory::new(),
            session: Memory::new(),
            shared: Memory::new(),
        };
        Node::new(Origin(origin), 0, Version(1), &HANDLERS, stores)
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
        assert_eq!(count(n.store(Space::Device), 4), 4);
        assert_eq!(count(n.store(Space::Local), 4), 0);
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
        assert_eq!(count(n.store(Space::Local), 0), 1);
        assert_eq!(count(&n.view(Space::Shared), 7), 1);
        assert_eq!(count(n.store(Space::Shared), 7), 0);
        let [Up::Push(add)] = &up[..] else { panic!("one push: {up:?}") };
        let add = add.clone();
        assert_eq!(add.body, Msg::Add { key: 7, by: 1 });
        assert_eq!((add.cause, add.actor, add.id.n, add.seq), (Some(tap.id), Actor(5), 2, None));
        assert!(add.at > tap.at);
        assert_eq!(n.pending().len(), 1);

        // Confirmed, it is the confirmed rows' and no longer pending.
        n.receive(logged(&add, 1, count_is(7, 1)), &mut up);
        assert_eq!(count(n.store(Space::Shared), 7), 1);
        assert_eq!(count(&n.view(Space::Shared), 7), 1);
        assert_eq!(n.pending(), []);
        assert_eq!(n.cursor(), Seq(1));
        // Again, or past a gap, it is passed over.
        n.receive(logged(&add, 1, count_is(7, 9)), &mut up);
        n.receive(logged(&add, 3, count_is(7, 9)), &mut up);
        assert_eq!(count(n.store(Space::Shared), 7), 1);
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
        a.receive(logged(&theirs, 1, count_is(1, 5)), &mut up);
        assert_eq!(count(a.store(Space::Shared), 1), 5);
        assert_eq!(count(&a.view(Space::Shared), 1), 7);
        assert_eq!(a.pending()[0].writes, count_is(1, 7));
        a.receive(logged(&mine, 2, count_is(1, 7)), &mut up);
        assert_eq!(a.pending(), []);
        assert_eq!(rows(&a.view(Space::Shared)), rows(a.store(Space::Shared)));
    }

    /// A refused entry is dropped and journaled; the one after it stays,
    /// rebased without it.
    #[cfg_attr(test, test)]
    fn a_refusal_drops_one_entry_only() {
        let mut n = node(1);
        let mut up = Vec::new();
        let first = n.make(Space::Shared, Msg::Add { key: 2, by: 1 }, 10);
        let second = n.make(Space::Shared, Msg::Add { key: 2, by: 10 }, 10);
        n.route(first.clone(), &mut up);
        n.route(second, &mut up);
        assert_eq!(count(&n.view(Space::Shared), 2), 11);
        n.receive(
            Down::Verdict {
                id: first.id,
                verdict: Verdict::Refused(Refusal(300)),
            },
            &mut up,
        );
        assert_eq!(n.take_refused(), [(first.id, Refusal(300))]);
        assert_eq!(n.pending().len(), 1);
        assert_eq!(count(&n.view(Space::Shared), 2), 10);
        // A refusal said again is not journaled twice.
        n.receive(
            Down::Verdict {
                id: first.id,
                verdict: Verdict::Refused(Refusal(300)),
            },
            &mut up,
        );
        assert_eq!(n.take_refused(), []);
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
        assert_eq!(rows(n.store(Space::Device)), []);
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

    /// An `At` verdict lets go of what it covers once the log is applied
    /// that far: a push judged before a connection was lost.
    #[cfg_attr(test, test)]
    fn an_at_verdict_lets_go_once_applied() {
        let mut n = node(1);
        let mut up = Vec::new();
        let e = n.make(Space::Shared, Msg::Add { key: 4, by: 1 }, 10);
        n.route(e.clone(), &mut up);
        n.receive(
            Down::Verdict {
                id: e.id,
                verdict: Verdict::At { seq: Seq(1), last: e.id.n },
            },
            &mut up,
        );
        assert_eq!(n.pending()[0].judged, Some(Seq(1)));
        // It is let go once the log is applied to 1, by facts that need
        // not carry it by name (as a fill's will not).
        let other = Entry {
            id: EntryId { origin: Origin(5), n: 1 },
            ..e.clone()
        };
        n.receive(logged(&other, 1, count_is(4, 1)), &mut up);
        assert_eq!(n.pending(), []);
        assert_eq!(count(&n.view(Space::Shared), 4), 1);
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
        again.connect(&mut up);
        assert_eq!(up, [Up::Hello { cursor: Seq(0) }, Up::Push(e)]);
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
        n.receive(logged(&first, 1, count_is(6, 1)), &mut up);
        assert_eq!((n.skipped, n.pending().len()), (1, 1));
        assert_eq!(n.pending()[0].writes, count_is(6, 40));
        assert_eq!(count(&n.view(Space::Shared), 6), 40);
        // Another node's facts: the rest is run again over them.
        let theirs = node(2).make(Space::Shared, Msg::Add { key: 6, by: 4 }, 10);
        n.receive(logged(&theirs, 2, count_is(6, 5)), &mut up);
        assert_eq!(n.skipped, 1);
        assert_eq!(n.pending()[0].writes, count_is(6, 7));
        // The node's own facts unlike its preview: run again too.
        let mut m = node(3);
        let a = m.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        let b = m.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        m.route(a.clone(), &mut up);
        m.route(b, &mut up);
        m.receive(logged(&a, 1, count_is(1, 9)), &mut up);
        assert_eq!((m.skipped, count(&m.view(Space::Shared), 1)), (0, 10));
    }

    /// A fill empties the tables it names, takes its rows in parts, and
    /// at its end puts the node at its `seq`, lets go of what that
    /// confirms, and rebases the rest. One interrupted is let go at the
    /// next connection.
    #[cfg_attr(test, test)]
    fn a_fill_replaces_the_confirmed_rows() {
        let mut n = node(1);
        let mut up = Vec::new();
        let theirs = node(2).make(Space::Shared, Msg::Add { key: 1, by: 3 }, 10);
        n.receive(logged(&theirs, 1, count_is(1, 3)), &mut up);
        let mine = n.make(Space::Shared, Msg::Add { key: 2, by: 1 }, 10);
        let late = n.make(Space::Shared, Msg::Add { key: 2, by: 10 }, 10);
        n.route(mine.clone(), &mut up);
        n.route(late.clone(), &mut up);
        // Judged at 5, in the part of the log the upstream let go.
        n.receive(
            Down::Verdict {
                id: mine.id,
                verdict: Verdict::At {
                    seq: Seq(5),
                    last: mine.id.n,
                },
            },
            &mut up,
        );
        n.connect(&mut up);
        n.receive(
            Down::Fill {
                seq: Seq(5),
                tables: vec![COUNTS],
                parts: 2,
            },
            &mut up,
        );
        assert_eq!((count(n.store(Space::Shared), 1), n.cursor()), (0, Seq(1)));
        // The log is not taken during a fill.
        n.receive(logged(&theirs, 2, count_is(1, 50)), &mut up);
        n.receive(Down::Rows(count_is(2, 1)), &mut up);
        assert_eq!(n.cursor(), Seq(1));
        n.receive(Down::Rows(count_is(3, 4)), &mut up);
        assert_eq!(n.cursor(), Seq(5));
        assert_eq!(
            rows(n.store(Space::Shared)),
            [(vec![2], 1u64.to_le_bytes().to_vec()), (vec![3], 4u64.to_le_bytes().to_vec())]
        );
        assert_eq!(n.pending().len(), 1);
        assert_eq!(count(&n.view(Space::Shared), 2), 11);
        n.receive(logged(&late, 6, count_is(2, 11)), &mut up);
        assert_eq!((n.pending(), n.cursor()), (&[][..], Seq(6)));

        // Interrupted: the rest of its parts are not taken after a new
        // connection, and a part with no fill is passed over.
        n.receive(
            Down::Fill {
                seq: Seq(9),
                tables: vec![COUNTS],
                parts: 2,
            },
            &mut up,
        );
        n.receive(Down::Rows(count_is(7, 1)), &mut up);
        n.connect(&mut up);
        n.receive(Down::Rows(count_is(8, 1)), &mut up);
        assert_eq!((count(n.store(Space::Shared), 8), n.cursor()), (0, Seq(6)));
        // An empty fill ends at once.
        n.receive(
            Down::Fill {
                seq: Seq(9),
                tables: vec![COUNTS],
                parts: 0,
            },
            &mut up,
        );
        assert_eq!((rows(n.store(Space::Shared)), n.cursor()), (vec![], Seq(9)));
    }

    #[cfg_attr(test, test)]
    fn session_entries_are_sent_and_taken() {
        let mut a = node(1);
        let mut up = Vec::new();
        let e = a.make(Space::Session, Msg::Add { key: 1, by: 3 }, 10);
        a.route(e.clone(), &mut up);
        assert_eq!(up, [Up::Live(e.clone())]);
        let mut b = node(2);
        b.receive(Down::Live(e), &mut up);
        assert_eq!(count(b.store(Space::Session), 1), 3);
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
        an_at_verdict_lets_go_once_applied,
        pending_records_are_put_back,
        a_confirmed_preview_is_not_run_again,
        a_fill_replaces_the_confirmed_rows,
        session_entries_are_sent_and_taken,
        routes,
    );
}
