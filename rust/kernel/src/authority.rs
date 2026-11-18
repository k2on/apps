//! The authority: the one node that appends to the master log, which a
//! device never links (the server's host holds it, and the watch's size
//! probe does not count it).
//!
//! It speaks to each connection the host has opened, as the actor the
//! host signed it in as ([`Authority::open`]), and answers what each says
//! ([`Authority::receive`]) with frames addressed to connections.
//!
//! **Pushes.** It judges each pushed entry once. Each origin's entries are
//! judged in the order of their count, as a node pushes them, so a count
//! at or below the last one judged from its origin is a repeat (a push sent
//! again at a connection): it is answered with the refusal the authority
//! journaled for it, or `At { seq, last }`, and not run again. Any other is
//! run on the confirmed rows: refused, it is journaled and answered so;
//! kept, its writes are its facts, and it is logged at the next `seq` and
//! sent to every connection. A refusal never holds back the entries after
//! it.
//!
//! **The actor is the connection's.** An entry made before its node knew
//! who it was for ([`Actor::NONE`]) is stamped with its connection's actor;
//! one that names another actor is refused ([`Refusal::NOT_YOURS`]), and a
//! session entry that does is dropped.
//!
//! **Session entries** are forwarded to the actor's other connections and
//! never logged, each through that connection's [`Feed`], at the rate it
//! asked for; [`Authority::tick`] sends what is due. Of an entry that sets
//! a row whole ([`Body::row`]) only the latest is kept, by its time, in
//! memory: one older than it is dropped, and a connection's hello is
//! answered with its actor's latest, so a node that connects again holds
//! the live state at once (a playback position it extrapolates, not one
//! it waits for).
//!
//! **Fills.** The host may let go of the log's start ([`Authority::trim`]).
//! A hello whose cursor is below what is kept is answered with the
//! confirmed rows at the head, in parts, in place of the log.
//!
//! **Subscriptions.** It runs the [`Query`]s the build gives it
//! ([`Authority::set_queries`]) for the connections that subscribe, each
//! a [`Live`] answer over its rows, and keeps per connection the rows it
//! holds with how many of its subscriptions read each, never their
//! contents. A connection that subscribes before its hello holds only
//! those rows: its hello is answered with a fill of them, and each entry
//! logged after with the facts of the rows it holds and the rows that
//! moved into or out of its queries, at the entry's `seq`. The record
//! lives as long as the connection, so a node that connects again is
//! filled again: keeping it across connections, as Zero's per-client
//! records are, is for later.
//!
//! It keeps the log, its rows, the last count judged per origin and the
//! refusals, all in memory here: storing them and trimming the journal are
//! the server host's. The entries a handler emits on the authority are not
//! taken yet: reactions are a later step.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::entry::{Actor, Entry, EntryId, Origin, Seq, Space};
use crate::node::{run, Body, Handler, Refusal};
use crate::plan::{Changes, Diff, Live, Plan};
use crate::store::{Range, Store, TableId, Write, Writes};
use crate::sync::{Down, Feed, Logged, QueryId, Up, Verdict};

/// A connection, by the host's number for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Conn(pub u32);

/// A frame for a connection.
pub type Out<B> = (Conn, Down<B>);

/// A query's plan for its arguments, over one table's rows, which it keeps
/// whole (a scan, its filters and its take): what a node that subscribes
/// holds. The node runs its own maps, sorts and groups over them.
pub type Query = fn(args: &[u8]) -> (TableId, Plan);

/// A row of the shared space.
type At = (TableId, Vec<u8>);

/// A subscription: its query and arguments, and its answer kept live.
struct Sub {
    query: QueryId,
    args: Vec<u8>,
    table: TableId,
    live: Live,
}

/// What the authority keeps of a connection.
struct Peer<B> {
    actor: Actor,
    feed: Feed<B>,
    hello: bool,
    /// The subscriptions it said before its hello, each told `Got` then.
    said: Vec<(QueryId, Vec<u8>)>,
    subs: Vec<Sub>,
    /// The rows it holds, with how many subscriptions read each; `None`
    /// for every row.
    held: Option<BTreeMap<At, u32>>,
}

pub struct Authority<B: 'static, S> {
    handlers: &'static [Handler<B>],
    store: S,
    /// The log after `base`: the entry at `seq` is `log[seq - base - 1]`.
    log: Vec<Logged<B>>,
    base: u64,
    /// Every table the log has written: what a fill empties.
    tables: BTreeSet<TableId>,
    last: BTreeMap<Origin, u64>,
    refused: BTreeMap<EntryId, Refusal>,
    conns: BTreeMap<Conn, Peer<B>>,
    /// The latest session entry for each row, by its actor.
    latest: BTreeMap<(Actor, At), Entry<B>>,
    queries: &'static [Query],
    /// The most rows a fill's part carries.
    part: usize,
}

impl<B: Body + Clone + 'static, S: Store> Authority<B, S> {
    /// An authority over an empty log, running `handlers` on `store`.
    pub fn new(handlers: &'static [Handler<B>], store: S) -> Authority<B, S> {
        Authority {
            handlers,
            store,
            log: Vec::new(),
            base: 0,
            tables: BTreeSet::new(),
            last: BTreeMap::new(),
            refused: BTreeMap::new(),
            conns: BTreeMap::new(),
            latest: BTreeMap::new(),
            queries: &[],
            part: 32,
        }
    }

    /// Sets the queries nodes subscribe to, by their numbers.
    pub fn set_queries(&mut self, queries: &'static [Query]) {
        self.queries = queries;
    }

    /// Sets the most rows a fill's part carries, so that each frame fits
    /// what the node reads at once.
    pub fn set_part(&mut self, rows: usize) {
        self.part = rows.max(1);
    }

    /// The last `seq` logged.
    pub fn head(&self) -> Seq {
        Seq(self.base + self.log.len() as u64)
    }

    /// The confirmed rows.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The log as kept: after [`Authority::trim`]'s `seq`.
    pub fn log(&self) -> &[Logged<B>] {
        &self.log
    }

    /// The journal of refusals.
    pub fn refused(&self) -> &BTreeMap<EntryId, Refusal> {
        &self.refused
    }

    /// Lets go of the log up to `seq`: a node behind it is sent a fill.
    pub fn trim(&mut self, seq: Seq) {
        let n = seq.0.saturating_sub(self.base).min(self.log.len() as u64);
        self.log.drain(..n as usize);
        self.base += n;
    }

    /// A connection opened, signed in as `actor`.
    pub fn open(&mut self, conn: Conn, actor: Actor) {
        let peer = Peer {
            actor,
            feed: Feed::default(),
            hello: false,
            said: Vec::new(),
            subs: Vec::new(),
            held: None,
        };
        self.conns.insert(conn, peer);
    }

    /// A connection closed: what was held for it is dropped.
    pub fn close(&mut self, conn: Conn) {
        self.conns.remove(&conn);
    }

    /// Takes what `conn` said at `now`, and answers with what goes where.
    /// A connection not open is not answered.
    pub fn receive(&mut self, conn: Conn, up: Up<B>, now: u64) -> Vec<Out<B>> {
        let Some(peer) = self.conns.get_mut(&conn) else {
            return Vec::new();
        };
        let actor = peer.actor;
        let out = match up {
            Up::Hello { cursor } => {
                peer.hello = true;
                let mut out = if peer.said.is_empty() {
                    peer.held = None;
                    self.hello(cursor)
                } else {
                    self.fill(conn)
                };
                let live = self.latest.iter().filter(|((a, _), _)| *a == actor);
                out.extend(live.map(|(_, e)| Down::Live(e.clone())));
                out
            }
            Up::Subscribe { query, args } => {
                if !peer.hello && !peer.said.iter().any(|s| s.0 == query && s.1 == args) {
                    peer.said.push((query, args.clone()));
                }
                self.subscribe(conn, query, args)
            }
            Up::Unsubscribe { query, args } => {
                peer.said.retain(|s| s.0 != query || s.1 != args);
                let Some(i) = peer.subs.iter().position(|s| s.query == query && s.args == args) else {
                    return Vec::new();
                };
                let sub = peer.subs.remove(i);
                let mut gone = Vec::new();
                if let Some(held) = &mut peer.held {
                    for (key, _) in sub.live.rows() {
                        if release(held, (sub.table, key.clone())) {
                            gone.push(Write {
                                table: sub.table,
                                key: key.clone(),
                                value: None,
                            });
                        }
                    }
                }
                self.parts(gone)
            }
            Up::Push(e) => {
                let (verdict, logged) = self.push(actor, e);
                let mut out = Vec::from([(conn, verdict)]);
                if let Some((l, changes)) = logged {
                    out.extend(self.feed(&l, &changes));
                }
                return out;
            }
            Up::Rate { table, every } => {
                peer.feed.rate(table, every);
                Vec::new()
            }
            Up::Live(e) => {
                let mut out = Vec::new();
                let e = stamped(actor, e).ok().filter(|e| e.space == Space::Session && actor != Actor::NONE);
                if let Some(e) = e.filter(|e| self.newest(actor, e)) {
                    for (&c, peer) in &mut self.conns {
                        if c != conn && peer.actor == actor {
                            out.extend(peer.feed.offer(e.clone(), now).into_iter().map(|e| (c, Down::Live(e))));
                        }
                    }
                }
                return out;
            }
        };
        out.into_iter().map(|d| (conn, d)).collect()
    }

    /// Rows that moved, in parts.
    fn parts(&self, rows: Vec<Write>) -> Vec<Down<B>> {
        rows.chunks(self.part).map(|c| Down::Moved(Writes(c.to_vec()))).collect()
    }

    /// Runs a query for `conn`, and sends what it holds now. One the build
    /// does not have is got at once, with no rows.
    fn subscribe(&mut self, conn: Conn, query: QueryId, args: Vec<u8>) -> Vec<Down<B>> {
        let Some(peer) = self.conns.get_mut(&conn) else {
            return Vec::new();
        };
        let Some(make) = self.queries.get(query.0 as usize) else {
            return if peer.hello {
                Vec::from([Down::Got { query, args }])
            } else {
                Vec::new()
            };
        };
        if peer.subs.iter().any(|s| s.query == query && s.args == args) {
            return Vec::new();
        }
        let (table, plan) = make(&args);
        let live = plan.live(&self.store);
        let mut came = Vec::new();
        if let (Some(held), true) = (&mut peer.held, peer.hello) {
            for (key, _) in live.rows() {
                let n = held.entry((table, key.clone())).or_insert(0);
                *n += 1;
                if *n == 1 {
                    came.push(Write {
                        table,
                        key: key.clone(),
                        value: self.store.get(table, key),
                    });
                }
            }
        }
        let hello = peer.hello;
        peer.subs.push(Sub {
            query,
            args: args.clone(),
            table,
            live,
        });
        if !hello {
            return Vec::new();
        }
        let mut out = self.parts(came);
        out.push(Down::Got { query, args });
        out
    }

    /// Fills a connection that subscribed before its hello with its
    /// queries' rows, at the head.
    fn fill(&mut self, conn: Conn) -> Vec<Down<B>> {
        let Some(peer) = self.conns.get_mut(&conn) else {
            return Vec::new();
        };
        let mut held = BTreeMap::new();
        let mut tables = BTreeSet::new();
        for sub in &peer.subs {
            tables.insert(sub.table);
            for (key, _) in sub.live.rows() {
                *held.entry((sub.table, key.clone())).or_insert(0) += 1;
            }
        }
        let rows: Vec<Write> = held
            .keys()
            .map(|(table, key)| Write {
                table: *table,
                key: key.clone(),
                value: self.store.get(*table, key),
            })
            .collect();
        let got: Vec<Down<B>> = peer
            .said
            .iter()
            .map(|(query, args)| Down::Got {
                query: *query,
                args: args.clone(),
            })
            .collect();
        peer.held = Some(held);
        let parts: Vec<Down<B>> = rows.chunks(self.part).map(|c| Down::Rows(Writes(c.to_vec()))).collect();
        let mut out = Vec::from([Down::Fill {
            seq: self.head(),
            tables: tables.into_iter().collect(),
            parts: parts.len() as u32,
        }]);
        out.extend(parts);
        out.extend(got);
        out
    }

    /// What a logged entry is to each connection: every fact, to one that
    /// holds every row; to one that holds its queries' rows, the facts of
    /// those it holds, and the rows that came into its queries or left
    /// them. A connection that has subscribed and not said hello is sent
    /// nothing: its hello fills it.
    fn feed(&mut self, l: &Logged<B>, changes: &Changes) -> Vec<Out<B>> {
        let mut out = Vec::new();
        for (&c, peer) in &mut self.conns {
            // Whether each row whose count of subscriptions changed was
            // held before.
            let mut was: BTreeMap<At, bool> = BTreeMap::new();
            for sub in &mut peer.subs {
                for Diff { key, old, new } in sub.live.diffs(&self.store, changes) {
                    let (Some(held), true) = (&mut peer.held, old.is_some() != new.is_some()) else {
                        continue;
                    };
                    let at = (sub.table, key);
                    was.entry(at.clone()).or_insert_with(|| held.contains_key(&at));
                    if new.is_some() {
                        *held.entry(at).or_insert(0) += 1;
                    } else {
                        release(held, at);
                    }
                }
            }
            let facts = match &peer.held {
                _ if !peer.hello && !peer.said.is_empty() => continue,
                None => l.facts.clone(),
                Some(held) => {
                    for w in &l.facts.0 {
                        let at = (w.table, w.key.clone());
                        was.entry(at).or_insert_with(|| held.contains_key(&(w.table, w.key.clone())));
                    }
                    // A row held now is sent whole; one held before only
                    // is a move-out.
                    let rows = was.into_iter().filter_map(|((table, key), before)| {
                        let now = held.contains_key(&(table, key.clone()));
                        let value = if now { self.store.get(table, &key) } else { None };
                        (now || before).then_some(Write { table, key, value })
                    });
                    Writes(rows.collect())
                }
            };
            out.push((
                c,
                Down::Logged(Logged {
                    entry: l.entry.clone(),
                    facts,
                }),
            ));
        }
        out
    }

    /// Whether `e` is newer than the latest session entry kept for its
    /// row, keeping it if so; one that names no row always is.
    fn newest(&mut self, actor: Actor, e: &Entry<B>) -> bool {
        let Some(row) = e.body.row() else { return true };
        let key = (actor, row);
        if self.latest.get(&key).is_some_and(|l| (l.at, l.id) >= (e.at, e.id)) {
            return false;
        }
        self.latest.insert(key, e.clone());
        true
    }

    /// The latest session entry of each row, as kept.
    pub fn latest(&self) -> impl Iterator<Item = &Entry<B>> {
        self.latest.values()
    }

    /// The session entries whose turn has come by `now`.
    pub fn tick(&mut self, now: u64) -> Vec<Out<B>> {
        let mut out = Vec::new();
        for (&c, peer) in &mut self.conns {
            out.extend(peer.feed.due(now).into_iter().map(|e| (c, Down::Live(e))));
        }
        out
    }

    /// When [`Authority::tick`] next has something to send, if ever.
    pub fn next_tick(&self) -> Option<u64> {
        self.conns.values().filter_map(|peer| peer.feed.next()).min()
    }

    /// The answer to a node's hello: the log after its cursor, or, when
    /// that is let go, a fill of the rows at the head.
    fn hello(&self, cursor: Seq) -> Vec<Down<B>> {
        if cursor.0 >= self.base {
            let from = ((cursor.0 - self.base) as usize).min(self.log.len());
            return self.log[from..].iter().cloned().map(Down::Logged).collect();
        }
        let mut rows = Vec::new();
        for &table in &self.tables {
            self.store.scan(table, Range::all(), &mut |k, v| {
                rows.push(Write {
                    table,
                    key: k.to_vec(),
                    value: Some(v.to_vec()),
                });
                true
            });
        }
        let parts: Vec<Down<B>> = rows.chunks(self.part).map(|c| Down::Rows(Writes(c.to_vec()))).collect();
        let mut out = Vec::from([Down::Fill {
            seq: self.head(),
            tables: self.tables.iter().copied().collect(),
            parts: parts.len() as u32,
        }]);
        out.extend(parts);
        out
    }

    /// Judges an entry `actor`'s connection pushed: the verdict for its
    /// node, and what it logged with what that changed, for every node.
    fn push(&mut self, actor: Actor, e: Entry<B>) -> (Down<B>, Option<(Logged<B>, Changes)>) {
        let id = e.id;
        let answer = |verdict| Down::Verdict { id, verdict };
        if e.space != Space::Shared || e.seq.is_some() {
            return (answer(Verdict::Refused(Refusal::NOT_PUSHABLE)), None);
        }
        let last = self.last.get(&id.origin).copied().unwrap_or(0);
        if id.n <= last {
            let verdict = match self.refused.get(&id) {
                Some(&why) => Verdict::Refused(why),
                None => Verdict::At { seq: self.head(), last },
            };
            return (answer(verdict), None);
        }
        self.last.insert(id.origin, id.n);
        match stamped(actor, e).and_then(|e| run(self.handlers, &self.store, &e).map(|(facts, _)| (e, facts))) {
            Err(why) => {
                self.refused.insert(id, why);
                (answer(Verdict::Refused(why)), None)
            }
            Ok((e, facts)) => {
                let changes = facts.0.iter().filter_map(|w| {
                    let old = self.store.get(w.table, &w.key);
                    let diff = Diff {
                        key: w.key.clone(),
                        old,
                        new: w.value.clone(),
                    };
                    (diff.old != diff.new).then_some((w.table, diff))
                });
                let changes = Changes {
                    reset: false,
                    rows: changes.collect(),
                };
                self.store.apply(&facts);
                self.tables.extend(facts.0.iter().map(|w| w.table));
                let seq = Seq(self.head().0 + 1);
                let logged = Logged {
                    entry: Entry { seq: Some(seq), ..e },
                    facts,
                };
                self.log.push(logged.clone());
                (answer(Verdict::At { seq, last: id.n }), Some((logged, changes)))
            }
        }
    }
}

/// Takes away one subscription's hold of a row: whether none holds it now.
fn release(held: &mut BTreeMap<At, u32>, at: At) -> bool {
    let Some(n) = held.get_mut(&at) else { return false };
    *n -= 1;
    if *n > 0 {
        return false;
    }
    held.remove(&at);
    true
}

/// `e` as from `actor`, its connection's: stamped if it names no one,
/// refused if it names another.
fn stamped<B>(actor: Actor, mut e: Entry<B>) -> Result<Entry<B>, Refusal> {
    if e.actor == Actor::NONE {
        e.actor = actor;
    }
    if e.actor == actor {
        Ok(e)
    } else {
        Err(Refusal::NOT_YOURS)
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::node::app::*;
    use crate::node::Node;
    use crate::plan::tests::{diff, splice};
    use crate::plan::{scan, Live, Row};
    use crate::store::{Memory, Read};
    use crate::sync::QueryId;
    use crate::Rng;
    use alloc::collections::VecDeque;
    use alloc::vec;
    use core::ops::Bound;

    fn authority() -> Authority<Msg, Memory> {
        Authority::new(&HANDLERS, Memory::new())
    }

    const ME: Actor = Actor(5);

    /// What `conn` is sent of `out`.
    fn to(conn: u32, out: &[Out<Msg>]) -> Vec<Down<Msg>> {
        out.iter().filter(|(c, _)| *c == Conn(conn)).map(|(_, d)| d.clone()).collect()
    }

    #[cfg_attr(test, test)]
    fn a_push_is_judged_once() {
        let mut a = authority();
        a.open(Conn(1), ME);
        a.open(Conn(2), Actor(6));
        let mut n = node(1);
        n.set_actor(ME);
        let e = n.make(Space::Shared, Msg::Add { key: 1, by: 2 }, 10);
        let out = a.receive(Conn(1), Up::Push(e.clone()), 0);
        let verdict = Down::Verdict {
            id: e.id,
            verdict: Verdict::At { seq: Seq(1), last: 1 },
        };
        let logged = Down::Logged(a.log()[0].clone());
        assert_eq!(to(1, &out), [verdict, logged.clone()]);
        assert_eq!(to(2, &out), [logged]);
        assert_eq!(a.log()[0].entry.seq, Some(Seq(1)));
        assert_eq!(count(a.store(), 1), 2);
        // Pushed again, it is answered where its origin stands, not run.
        let mut other = node(2);
        other.set_actor(Actor(6));
        a.receive(Conn(2), Up::Push(other.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10)), 0);
        assert_eq!(
            a.receive(Conn(1), Up::Push(e.clone()), 0),
            [(
                Conn(1),
                Down::Verdict {
                    id: e.id,
                    verdict: Verdict::At { seq: Seq(2), last: 1 }
                }
            )]
        );
        assert_eq!(count(a.store(), 1), 3);
        assert_eq!(
            a.receive(Conn(1), Up::Hello { cursor: Seq(1) }, 0),
            [(Conn(1), Down::Logged(a.log()[1].clone()))]
        );
        assert_eq!(a.receive(Conn(1), Up::Hello { cursor: Seq(9) }, 0), []);
        // A connection not open is not answered.
        assert_eq!(a.receive(Conn(3), Up::Hello { cursor: Seq(0) }, 0), []);
    }

    #[cfg_attr(test, test)]
    fn a_refusal_is_journaled_and_holds_nothing_back() {
        let mut a = authority();
        a.open(Conn(1), Actor::NONE);
        let mut n = node(1);
        let take = n.make(Space::Shared, Msg::Take { key: 1 }, 10);
        let add = n.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        let refused = (
            Conn(1),
            Down::Verdict {
                id: take.id,
                verdict: Verdict::Refused(NONE_LEFT),
            },
        );
        assert_eq!(a.receive(Conn(1), Up::Push(take.clone()), 0), core::slice::from_ref(&refused));
        assert_eq!(a.receive(Conn(1), Up::Push(add), 0).len(), 2);
        // The refusal is said again to a repeat.
        assert_eq!(a.receive(Conn(1), Up::Push(take), 0), [refused]);
        let local = n.make(Space::Local, Msg::Seen, 10);
        assert_eq!(
            a.receive(Conn(1), Up::Push(local.clone()), 0),
            [(
                Conn(1),
                Down::Verdict {
                    id: local.id,
                    verdict: Verdict::Refused(Refusal::NOT_PUSHABLE)
                }
            )]
        );
        assert_eq!(a.head(), Seq(1));
    }

    /// An entry that names no one is the connection's; one that names
    /// another actor is refused, or for a session entry, dropped.
    #[cfg_attr(test, test)]
    fn the_actor_is_the_connections() {
        let mut a = authority();
        a.open(Conn(1), ME);
        a.open(Conn(2), ME);
        let mut n = node(1);
        let unsigned = n.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        a.receive(Conn(1), Up::Push(unsigned), 0);
        assert_eq!(a.log()[0].entry.actor, ME);
        n.set_actor(Actor(6));
        let forged = n.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        assert_eq!(
            a.receive(Conn(1), Up::Push(forged.clone()), 0),
            [(
                Conn(1),
                Down::Verdict {
                    id: forged.id,
                    verdict: Verdict::Refused(Refusal::NOT_YOURS)
                }
            )]
        );
        assert_eq!(a.refused().get(&forged.id), Some(&Refusal::NOT_YOURS));
        let live = n.make(Space::Session, Msg::Set { key: 1, value: 1 }, 10);
        assert_eq!(a.receive(Conn(1), Up::Live(live), 0), []);
        assert_eq!(a.head(), Seq(1));
    }

    /// A session entry goes to its actor's other connections only, never
    /// back to its own or to another actor's, and is not logged; each is
    /// sent it at the rate it asked for.
    #[cfg_attr(test, test)]
    fn session_entries_go_to_the_owners_other_connections() {
        let mut a = authority();
        a.open(Conn(1), ME);
        a.open(Conn(2), ME);
        a.open(Conn(3), ME);
        a.open(Conn(4), Actor(6));
        a.receive(Conn(3), Up::Rate { table: POSITIONS, every: 50 }, 0);
        let mut n = node(1);
        n.set_actor(ME);
        let mut set = |value| n.make(Space::Session, Msg::Set { key: 1, value }, 0);
        let (first, second, third) = (set(1), set(2), set(3));
        let out = a.receive(Conn(1), Up::Live(first.clone()), 0);
        assert_eq!(out, [(Conn(2), Down::Live(first.clone())), (Conn(3), Down::Live(first.clone()))]);
        assert_eq!(a.receive(Conn(1), Up::Live(second.clone()), 10), [(Conn(2), Down::Live(second))]);
        assert_eq!(a.receive(Conn(1), Up::Live(third.clone()), 20), [(Conn(2), Down::Live(third.clone()))]);
        assert_eq!(a.next_tick(), Some(50));
        assert_eq!(a.tick(49), []);
        assert_eq!(a.tick(50), [(Conn(3), Down::Live(third))]);
        assert_eq!((a.next_tick(), a.head()), (None, Seq(0)));
        // What is held for a connection closed is dropped.
        a.receive(Conn(1), Up::Live(first.clone()), 60);
        a.receive(Conn(1), Up::Live(first), 70);
        a.close(Conn(3));
        assert_eq!(a.tick(200), []);
    }

    /// The latest session entry of each row is kept, by its time: an older
    /// one is dropped, and a hello is answered with its actor's latest.
    #[cfg_attr(test, test)]
    fn a_hello_is_told_the_latest_session_rows() {
        let mut a = authority();
        a.open(Conn(1), ME);
        a.open(Conn(2), ME);
        a.open(Conn(3), Actor(6));
        let mut n = node(1);
        n.set_actor(ME);
        let early = n.make(Space::Session, Msg::Set { key: 1, value: 1 }, 10);
        let late = n.make(Space::Session, Msg::Set { key: 1, value: 2 }, 20);
        let other = n.make(Space::Session, Msg::Set { key: 2, value: 3 }, 30);
        assert_eq!(a.receive(Conn(1), Up::Live(late.clone()), 0).len(), 1);
        assert_eq!(a.receive(Conn(1), Up::Live(early), 0), []);
        assert_eq!(a.receive(Conn(1), Up::Live(late.clone()), 0), []);
        a.receive(Conn(1), Up::Live(other.clone()), 0);
        let out = a.receive(Conn(2), Up::Hello { cursor: Seq(0) }, 0);
        assert_eq!(out, [(Conn(2), Down::Live(late)), (Conn(2), Down::Live(other))]);
        assert_eq!(a.receive(Conn(3), Up::Hello { cursor: Seq(0) }, 0), []);
        // A node that hears its own takes nothing again, and one that
        // has not heard them holds them now.
        let mut d = node(2);
        for (_, down) in out {
            d.receive(down, &mut Vec::new());
        }
        assert_eq!(d.store(Space::Session).len(POSITIONS), 2);
    }

    /// A hello below what the authority keeps of its log is answered with
    /// the rows at the head, in parts; one within it, with the log.
    #[cfg_attr(test, test)]
    fn a_cursor_below_the_log_is_filled() {
        let mut a = authority();
        a.set_part(2);
        a.open(Conn(1), ME);
        let mut n = node(1);
        for key in 0..5 {
            let e = n.make(Space::Shared, Msg::Add { key, by: 1 }, 10);
            a.receive(Conn(1), Up::Push(e), 0);
        }
        a.trim(Seq(3));
        assert_eq!((a.log().len(), a.head()), (2, Seq(5)));
        assert_eq!(a.receive(Conn(1), Up::Hello { cursor: Seq(3) }, 0).len(), 2);
        let fill = to(1, &a.receive(Conn(1), Up::Hello { cursor: Seq(2) }, 0));
        assert_eq!(
            fill[0],
            Down::Fill {
                seq: Seq(5),
                tables: vec![COUNTS],
                parts: 3
            }
        );
        let mut d = node(2);
        let mut up = Vec::new();
        for down in fill {
            d.receive(down, &mut up);
        }
        assert_eq!((rows(d.store(Space::Shared)), d.cursor()), (rows(a.store()), Seq(5)));
    }

    /// The tests' queries: the counts whose value is odd or even as the
    /// argument says, and the first two.
    static QUERIES: [Query; 2] = [
        |args| {
            let odd = args.first().copied().unwrap_or(0);
            (
                COUNTS,
                scan(COUNTS, Bound::Unbounded, Bound::Unbounded).filter(move |_, v| v[0] % 2 == odd),
            )
        },
        |_| (COUNTS, scan(COUNTS, Bound::Unbounded, Bound::Unbounded).take(2, false)),
    ];

    /// A node that subscribes holds its queries' rows: filled at its
    /// hello, sent the rows a later subscription adds, the facts of what
    /// it holds and the rows that move in or out, and move-outs when it
    /// lets a query go; never the rest.
    #[cfg_attr(test, test)]
    fn a_subscriber_holds_its_queries_rows() {
        let mut a = authority();
        a.set_queries(&QUERIES);
        a.set_part(2);
        a.open(Conn(1), ME);
        a.open(Conn(2), ME);
        let mut writer = node(1);
        let mut push = |a: &mut Authority<Msg, Memory>, key, by| {
            let e = writer.make(Space::Shared, Msg::Add { key, by }, 10);
            a.receive(Conn(1), Up::Push(e), 0)
        };
        for (key, by) in [(1, 1), (2, 2), (3, 3), (4, 5)] {
            push(&mut a, key, by);
        }
        let mut watch = node(2);
        let mut up = Vec::new();
        watch.subscribe(QueryId(0), vec![1], &mut up);
        watch.connect(&mut up);
        let odd = |db: &dyn Read| rows(db).into_iter().filter(|(_, v)| v[0] % 2 == 1).collect::<Vec<_>>();
        let take = |up: &mut Vec<Up<Msg>>, a: &mut Authority<Msg, Memory>, watch: &mut Node<Msg, Memory>| {
            for u in core::mem::take(up) {
                for (c, down) in a.receive(Conn(2), u, 0) {
                    assert_eq!(c, Conn(2));
                    watch.receive(down, &mut Vec::new());
                }
            }
        };
        take(&mut up, &mut a, &mut watch);
        assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        assert!(watch.complete(QueryId(0), &[1]));
        assert_eq!(watch.cursor(), a.head());
        // A write it holds, one it does not, and one that moves a row out.
        let deliver = |out: Vec<Out<Msg>>, watch: &mut Node<Msg, Memory>| {
            for (c, down) in out {
                if c == Conn(2) {
                    watch.receive(down, &mut Vec::new());
                }
            }
        };
        for (key, by) in [(1, 2), (2, 2), (3, 1), (5, 1)] {
            let out = push(&mut a, key, by);
            deliver(out, &mut watch);
            assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        }
        // A second query: what it adds comes, what both hold is not sent
        // twice, and letting the first go moves out only what the second
        // does not hold.
        watch.subscribe(QueryId(1), vec![], &mut up);
        let sent: Vec<Up<Msg>> = up.clone();
        let out = a.receive(Conn(2), sent[0].clone(), 0);
        up.clear();
        assert!(matches!(out.last(), Some((_, Down::Got { .. }))));
        deliver(out, &mut watch);
        let first_two: Vec<_> = rows(a.store()).into_iter().take(2).collect();
        let mut both: BTreeSet<_> = odd(a.store()).into_iter().collect();
        both.extend(first_two.iter().cloned());
        assert_eq!(rows(watch.store(Space::Shared)), both.into_iter().collect::<Vec<_>>());
        watch.unsubscribe(QueryId(0), vec![1], &mut up);
        take(&mut up, &mut a, &mut watch);
        assert_eq!(rows(watch.store(Space::Shared)), first_two);
        // A row that comes before them moves into the take; one moves out.
        deliver(push(&mut a, 0, 4), &mut watch);
        assert_eq!(rows(watch.store(Space::Shared)), rows(a.store()).into_iter().take(2).collect::<Vec<_>>());
        assert!(watch.complete(QueryId(1), &[]) && !watch.complete(QueryId(0), &[1]));
        // A query the build does not have is got at once, empty, before
        // the hello and after it.
        watch.subscribe(QueryId(7), vec![], &mut up);
        take(&mut up, &mut a, &mut watch);
        assert!(watch.complete(QueryId(7), &[]));
        let mut late = node(3);
        late.subscribe(QueryId(7), vec![1], &mut up);
        late.connect(&mut up);
        a.open(Conn(3), ME);
        for u in core::mem::take(&mut up) {
            for (_, down) in a.receive(Conn(3), u, 0) {
                late.receive(down, &mut Vec::new());
            }
        }
        assert!(late.complete(QueryId(7), &[1]));
        assert_eq!((late.store(Space::Shared).len(COUNTS), late.cursor()), (0, a.head()));
    }

    /// A device in the convergence test: its node, what is in flight on
    /// its connection each way, and whether it is connected.
    struct Device {
        node: Node<Msg, Memory>,
        /// Who it signs in as.
        actor: Actor,
        up: VecDeque<Up<Msg>>,
        down: VecDeque<Down<Msg>>,
        online: bool,
        made: BTreeSet<EntryId>,
        refused: BTreeSet<EntryId>,
        /// Queries over its shared view, each with the list its patches
        /// splice, and the view's rows when its changes were last taken.
        lives: Vec<(Live, Vec<Row>)>,
        seen: Memory,
        /// Its session space when its changes were last taken.
        seen_live: Memory,
        /// Its subscriptions, if it holds only what they read.
        subs: Vec<(QueryId, Vec<u8>)>,
    }

    /// The queries each device keeps over its shared view: a bounded take
    /// of a filter, and a group of every row.
    fn queries(last: bool) -> [crate::plan::Plan; 2] {
        let all = || scan(COUNTS, Bound::Unbounded, Bound::Unbounded);
        [
            all().filter(|_, v| v[0] % 3 != 0).take(2, last),
            all().group(|_, _| Vec::new(), |_, v| i64::from(v[0]), true, true),
        ]
    }

    impl Device {
        /// What the node said, sent if connected and lost if not.
        fn send(&mut self, said: Vec<Up<Msg>>) {
            if self.online {
                self.up.extend(said);
            }
        }

        /// Takes the node's changes: they are what its shared view did, and
        /// pushed through the queries, each answer is a fresh run's and its
        /// patches splice the list.
        fn watch(&mut self, seed: u64) {
            let live = self.node.take_changes(Space::Session);
            let session = self.node.view(Space::Session);
            assert_eq!(
                live,
                diff(&self.seen_live, &session, &[COUNTS, POSITIONS]),
                "seed {seed}: the session's changes"
            );
            self.seen_live = Memory::new();
            session.scan(POSITIONS, crate::store::Range::all(), &mut |k, v| {
                self.seen_live.put(POSITIONS, k, v);
                true
            });
            let changes = self.node.take_changes(Space::Shared);
            let view = self.node.view(Space::Shared);
            if !changes.reset {
                assert_eq!(
                    changes,
                    diff(&self.seen, &view, &[COUNTS, POSITIONS]),
                    "seed {seed}: the changes are the view's"
                );
            }
            let mut seen = Memory::new();
            for (k, v) in rows(&view) {
                seen.put(COUNTS, &k, &v);
            }
            self.seen = seen;
            for ((live, shown), fresh) in self.lives.iter_mut().zip(queries(seed.is_multiple_of(2))) {
                splice(shown, &live.push(&view, &changes));
                let fresh = fresh.live(&view);
                assert_eq!(live.rows(), fresh.rows(), "seed {seed}: a query over the view");
                assert_eq!(shown, fresh.rows(), "seed {seed}: its patches");
            }
        }
    }

    fn position(db: &dyn Read, key: u8) -> u64 {
        db.get(POSITIONS, &[key]).map_or(0, |v| u64::from_le_bytes(v.try_into().unwrap()))
    }

    /// What the convergence runs saw, summed: so that each path is known
    /// to have been taken.
    #[derive(Default)]
    struct Seen {
        skipped: u32,
        fills: u32,
        forged: u32,
        held: u32,
    }

    /// Three devices (two of one actor, and a third of another, which holds
    /// only what its queries read and subscribes and lets go at random)
    /// and a server, each
    /// device writing offline and online, disconnected and reconnected at
    /// random with what was in flight lost, and messages delivered in
    /// random interleavings; the server lets go of its log's start at
    /// random, so a device behind is filled, and its session entries go
    /// to its actor's other devices at the rate each asked. Once all are
    /// connected and quiet, every device's rows are the server's (for the
    /// third, those its queries read, each complete), nothing
    /// is pending, every entry made is logged once or refused, each device
    /// was told of its every refusal, and each holds the latest session
    /// values of its actor's other devices, never another actor's.
    fn converge(seed: u64, steps: usize, seen: &mut Seen) {
        let mut rng = Rng::new(seed);
        let mut server = authority();
        server.set_part(1 + rng.below(3) as usize);
        server.set_queries(&QUERIES);
        let actors = [ME, ME, Actor(6)];
        let mut devices: Vec<Device> = (0..3)
            .map(|d| Device {
                node: node(d as u64 + 1),
                actor: actors[d],
                up: VecDeque::new(),
                down: VecDeque::new(),
                online: false,
                made: BTreeSet::new(),
                refused: BTreeSet::new(),
                lives: queries(seed.is_multiple_of(2)).map(|q| (q.live(&Memory::new()), Vec::new())).into(),
                seen: Memory::new(),
                seen_live: Memory::new(),
                subs: Vec::new(),
            })
            .collect();
        for (query, args) in [(QueryId(1), vec![]), (QueryId(0), vec![1])] {
            devices[2].node.subscribe(query, args.clone(), &mut Vec::new());
            devices[2].subs.push((query, args));
        }
        // The second device knows who it is from the start; the others
        // learn it at their first connection, their writes before it
        // stamped by the server.
        devices[1].node.set_actor(ME);
        let mut log: BTreeMap<Seq, Logged<Msg>> = BTreeMap::new();
        let mut now = 0u64;
        let mut value = 0u64;
        let connect = |rng: &mut Rng, server: &mut Authority<Msg, Memory>, dev: &mut Device, d: usize| {
            dev.online = true;
            server.open(Conn(d as u32), dev.actor);
            dev.node.set_actor(dev.actor);
            let mut said = Vec::new();
            dev.node.connect(&mut said);
            dev.watch(seed);
            said.push(Up::Rate {
                table: POSITIONS,
                every: [0, 15, 40][rng.below(3) as usize],
            });
            dev.send(said);
        };
        let deliver = |out: Vec<Out<Msg>>, devices: &mut Vec<Device>| {
            for (c, down) in out {
                let dev = &mut devices[c.0 as usize];
                assert!(dev.online, "sent only to a connection open");
                dev.down.push_back(down);
            }
        };
        let mut step = |rng: &mut Rng,
                        server: &mut Authority<Msg, Memory>,
                        devices: &mut Vec<Device>,
                        log: &mut BTreeMap<Seq, Logged<Msg>>,
                        seen: &mut Seen,
                        settle: bool| {
            let d = rng.below(3) as usize;
            now += rng.below(6);
            let op = if settle { 1 + rng.below(2) } else { rng.below(15) };
            match op {
                // A write, connected or not; now and then one that says it
                // is from someone else.
                0 | 3 | 4 => {
                    let key = rng.below(3) as u8;
                    let body = if rng.below(2) == 0 {
                        Msg::Take { key }
                    } else {
                        Msg::Add { key, by: 1 + rng.below(2) }
                    };
                    let dev = &mut devices[d];
                    let mut e = dev.node.make(Space::Shared, body, rng.below(100));
                    if rng.below(12) == 0 {
                        e.actor = Actor(99);
                        seen.forged += 1;
                    }
                    dev.made.insert(e.id);
                    let mut said = Vec::new();
                    dev.node.route(e, &mut said);
                    dev.watch(seed);
                    dev.send(said);
                }
                // The server takes what the device sent.
                1 | 5 | 6 => {
                    let Some(up) = devices[d].up.pop_front() else { return };
                    let mut owners = 0;
                    if let Up::Live(_) = &up {
                        let actor = devices[d].actor;
                        owners = devices
                            .iter()
                            .enumerate()
                            .filter(|(o, dev)| *o != d && dev.online && dev.actor == actor)
                            .count();
                    }
                    let out = server.receive(Conn(d as u32), up, now);
                    if out.iter().filter(|(_, down)| matches!(down, Down::Live(_))).count() < owners {
                        seen.held += 1;
                    }
                    // The log as the server keeps it: a subscriber is sent
                    // only its part of the facts.
                    if let Some(l) = server.log().last() {
                        log.entry(l.entry.seq.unwrap()).or_insert_with(|| l.clone());
                    }
                    seen.fills += out.iter().filter(|(_, down)| matches!(down, Down::Fill { .. })).count() as u32;
                    deliver(out, devices);
                }
                // The device takes what the server sent.
                2 | 7 | 8 => {
                    let dev = &mut devices[d];
                    let Some(down) = dev.down.pop_front() else { return };
                    // A device's own entry, said back at its hello, is not
                    // taken again.
                    let live = match &down {
                        Down::Live(e) if e.id.origin != Origin(d as u64 + 1) => {
                            let Msg::Set { key, value } = e.body else { unreachable!() };
                            Some((key, value, position(dev.node.store(Space::Session), key)))
                        }
                        _ => None,
                    };
                    let mut said = Vec::new();
                    let skipped = dev.node.skipped;
                    dev.node.receive(down, &mut said);
                    dev.watch(seed);
                    seen.skipped += dev.node.skipped - skipped;
                    if let Some((key, value, before)) = live {
                        let writer = actors[usize::from(key / 10)];
                        assert!(
                            writer == dev.actor && usize::from(key / 10) != d,
                            "seed {seed}: a session entry to its owner's other devices only"
                        );
                        assert!(value >= before, "seed {seed}: the latest per row, in order");
                    }
                    dev.send(said);
                    dev.refused.extend(dev.node.take_refused().into_iter().map(|(id, _)| id));
                }
                // Session values, a few at once as a position streams: each
                // device sets its own keys.
                9 | 10 => {
                    let dev = &mut devices[d];
                    let mut said = Vec::new();
                    for _ in 0..1 + rng.below(3) {
                        value += 1;
                        let key = 10 * d as u8 + rng.below(2) as u8;
                        let e = dev.node.make(Space::Session, Msg::Set { key, value }, now);
                        dev.node.route(e, &mut said);
                    }
                    dev.watch(seed);
                    dev.send(said);
                }
                // The server's timer, or its log let go of.
                11 => {
                    let out = server.tick(now);
                    deliver(out, devices);
                    if rng.below(3) == 0 {
                        let head = server.head().0;
                        server.trim(Seq(rng.below(head + 1)));
                    }
                }
                // The third device lets go of a query, or takes one.
                12 if d == 2 => {
                    let dev = &mut devices[d];
                    let mut said = Vec::new();
                    match dev.subs.iter().position(|s| s.0 == QueryId(0)) {
                        Some(i) => {
                            let (query, args) = dev.subs.remove(i);
                            dev.node.unsubscribe(query, args, &mut said);
                        }
                        None => {
                            let args = vec![rng.below(2) as u8];
                            dev.node.subscribe(QueryId(0), args.clone(), &mut said);
                            dev.subs.push((QueryId(0), args));
                        }
                    }
                    dev.send(said);
                }
                // Disconnected, or connected again.
                _ => {
                    let dev = &mut devices[d];
                    if dev.online {
                        dev.online = false;
                        server.close(Conn(d as u32));
                        dev.up.clear();
                        dev.down.clear();
                    } else {
                        connect(rng, server, dev, d);
                    }
                }
            }
        };
        for _ in 0..steps {
            step(&mut rng, &mut server, &mut devices, &mut log, seen, false);
        }
        // Everyone connected, then quiet, the server's timer run out.
        for (d, dev) in devices.iter_mut().enumerate().filter(|(_, dev)| !dev.online) {
            connect(&mut rng, &mut server, dev, d);
        }
        loop {
            while devices.iter().any(|dev| !dev.up.is_empty() || !dev.down.is_empty()) {
                step(&mut rng, &mut server, &mut devices, &mut log, seen, true);
            }
            let Some(at) = server.next_tick() else { break };
            deliver(server.tick(at), &mut devices);
        }

        let logged: Vec<EntryId> = log.values().map(|l| l.entry.id).collect();
        let once: BTreeSet<EntryId> = logged.iter().copied().collect();
        assert_eq!(once.len(), logged.len(), "an entry logged twice");
        assert_eq!(log.len() as u64, server.head().0, "every seq logged");
        for (d, dev) in devices.iter().enumerate() {
            // What it holds: every row, or the rows its queries read now.
            let mut held = rows(server.store());
            if !dev.subs.is_empty() {
                let mut read = BTreeSet::new();
                for (query, args) in &dev.subs {
                    let (_, plan) = QUERIES[query.0 as usize](args);
                    read.extend(plan.live(server.store()).rows().iter().cloned());
                    assert!(dev.node.complete(*query, args), "seed {seed}: {query:?} complete");
                }
                held = read.into_iter().collect();
            }
            assert_eq!(rows(dev.node.store(Space::Shared)), held, "seed {seed}: device {d}");
            assert_eq!(rows(&dev.node.view(Space::Shared)), held, "seed {seed}: device {d}");
            assert_eq!(dev.node.pending(), [], "seed {seed}");
            assert_eq!(dev.node.cursor(), server.head());
            for id in &dev.made {
                assert!(
                    once.contains(id) != server.refused().contains_key(id),
                    "seed {seed}: {id:?} logged or refused"
                );
            }
            let theirs: BTreeSet<EntryId> = server.refused().keys().filter(|id| dev.made.contains(id)).copied().collect();
            assert_eq!(dev.refused, theirs, "seed {seed}: told of every refusal; made {:?}", dev.made);
            // Its session space: its own rows, and the latest the server
            // took of its actor's other devices' rows; never another
            // actor's.
            for e in server.latest() {
                let Msg::Set { key, value } = e.body else { unreachable!() };
                if e.actor == dev.actor && e.id.origin != Origin(d as u64 + 1) {
                    assert_eq!(
                        position(dev.node.store(Space::Session), key),
                        value,
                        "seed {seed}: device {d}'s latest of {key}"
                    );
                }
            }
            dev.node.store(Space::Session).scan(POSITIONS, crate::store::Range::all(), &mut |k, _| {
                assert_eq!(actors[usize::from(k[0] / 10)], dev.actor, "seed {seed}: another actor's session row");
                true
            });
        }
        for l in log.values() {
            assert_eq!(
                l.entry.actor,
                actors[l.entry.id.origin.0 as usize - 1],
                "seed {seed}: logged as its connection's"
            );
        }
        // The rows are the log's facts, applied in order.
        let mut replay = Memory::new();
        for l in log.values() {
            replay.apply(&l.facts);
        }
        assert_eq!(rows(&replay), rows(server.store()));
        assert!(rows(&replay).iter().all(|(_, v)| v.len() == 8));
    }

    #[cfg_attr(test, test)]
    fn devices_and_a_server_converge() {
        let mut seen = Seen::default();
        for seed in 0..60 {
            converge(seed, 160, &mut seen);
        }
        assert!(seen.skipped > 0 && seen.fills > 0 && seen.forged > 0 && seen.held > 0);
    }

    crate::listed!(
        a_push_is_judged_once,
        a_refusal_is_journaled_and_holds_nothing_back,
        the_actor_is_the_connections,
        session_entries_go_to_the_owners_other_connections,
        a_hello_is_told_the_latest_session_rows,
        a_cursor_below_the_log_is_filled,
        a_subscriber_holds_its_queries_rows,
        devices_and_a_server_converge
    );
}
