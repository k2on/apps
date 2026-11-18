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
//! never stored, each through that connection's [`Feed`], at the rate it
//! asked for; [`Authority::tick`] sends what is due.
//!
//! **Fills.** The host may let go of the log's start ([`Authority::trim`]).
//! A hello whose cursor is below what is kept is answered with the
//! confirmed rows at the head, in parts, in place of the log.
//!
//! It keeps the log, its rows, the last count judged per origin and the
//! refusals, all in memory here: storing them and trimming the journal are
//! the server host's. The entries a handler emits on the authority are not
//! taken yet: reactions are a later step.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::entry::{Actor, Entry, EntryId, Origin, Seq, Space};
use crate::node::{run, Body, Handler, Refusal};
use crate::store::{Range, Store, TableId, Write, Writes};
use crate::sync::{Down, Feed, Logged, Up, Verdict};

/// A connection, by the host's number for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Conn(pub u32);

/// A frame for a connection.
pub type Out<B> = (Conn, Down<B>);

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
    conns: BTreeMap<Conn, (Actor, Feed<B>)>,
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
            part: 32,
        }
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
        self.conns.insert(conn, (actor, Feed::default()));
    }

    /// A connection closed: what was held for it is dropped.
    pub fn close(&mut self, conn: Conn) {
        self.conns.remove(&conn);
    }

    /// Takes what `conn` said at `now`, and answers with what goes where.
    /// A connection not open is not answered.
    pub fn receive(&mut self, conn: Conn, up: Up<B>, now: u64) -> Vec<Out<B>> {
        let Some((actor, feed)) = self.conns.get_mut(&conn) else {
            return Vec::new();
        };
        let actor = *actor;
        match up {
            Up::Hello { cursor } => self.hello(cursor).into_iter().map(|d| (conn, d)).collect(),
            Up::Push(e) => {
                let (verdict, logged) = self.push(actor, e);
                let mut out = Vec::from([(conn, verdict)]);
                if let Some(l) = logged {
                    out.extend(self.conns.keys().map(|&c| (c, Down::Logged(l.clone()))));
                }
                out
            }
            Up::Rate { table, every } => {
                feed.rate(table, every);
                Vec::new()
            }
            Up::Live(e) => match stamped(actor, e) {
                Ok(e) if e.space == Space::Session && actor != Actor::NONE => {
                    let mut out = Vec::new();
                    for (&c, (a, feed)) in &mut self.conns {
                        if c != conn && *a == actor {
                            out.extend(feed.offer(e.clone(), now).into_iter().map(|e| (c, Down::Live(e))));
                        }
                    }
                    out
                }
                _ => Vec::new(),
            },
        }
    }

    /// The session entries whose turn has come by `now`.
    pub fn tick(&mut self, now: u64) -> Vec<Out<B>> {
        let mut out = Vec::new();
        for (&c, (_, feed)) in &mut self.conns {
            out.extend(feed.due(now).into_iter().map(|e| (c, Down::Live(e))));
        }
        out
    }

    /// When [`Authority::tick`] next has something to send, if ever.
    pub fn next_tick(&self) -> Option<u64> {
        self.conns.values().filter_map(|(_, feed)| feed.next()).min()
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
    /// node, and what it logged, for every node.
    fn push(&mut self, actor: Actor, e: Entry<B>) -> (Down<B>, Option<Logged<B>>) {
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
                self.store.apply(&facts);
                self.tables.extend(facts.0.iter().map(|w| w.table));
                let seq = Seq(self.head().0 + 1);
                let logged = Logged {
                    entry: Entry { seq: Some(seq), ..e },
                    facts,
                };
                self.log.push(logged.clone());
                (answer(Verdict::At { seq, last: id.n }), Some(logged))
            }
        }
    }
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
    use crate::store::{Memory, Read};
    use crate::Rng;
    use alloc::collections::VecDeque;
    use alloc::vec;

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
        /// The session values it must hold once quiet: the latest the
        /// server took from its actor's other devices while it was
        /// connected.
        expect: BTreeMap<u8, u64>,
    }

    impl Device {
        /// What the node said, sent if connected and lost if not.
        fn send(&mut self, said: Vec<Up<Msg>>) {
            if self.online {
                self.up.extend(said);
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

    /// Three devices (two of one actor, one of another) and a server, each
    /// device writing offline and online, disconnected and reconnected at
    /// random with what was in flight lost, and messages delivered in
    /// random interleavings; the server lets go of its log's start at
    /// random, so a device behind is filled, and its session entries go
    /// to its actor's other devices at the rate each asked. Once all are
    /// connected and quiet, every device's rows are the server's, nothing
    /// is pending, every entry made is logged once or refused, each device
    /// was told of its every refusal, and each holds the latest session
    /// values of its actor's other devices, never another actor's.
    fn converge(seed: u64, steps: usize, seen: &mut Seen) {
        let mut rng = Rng::new(seed);
        let mut server = authority();
        server.set_part(1 + rng.below(3) as usize);
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
                expect: BTreeMap::new(),
            })
            .collect();
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
            said.push(Up::Rate {
                table: POSITIONS,
                every: [0, 0, 15][rng.below(3) as usize],
            });
            dev.send(said);
        };
        let deliver = |out: Vec<Out<Msg>>, devices: &mut Vec<Device>, log: &mut BTreeMap<Seq, Logged<Msg>>| {
            for (c, down) in out {
                if let Down::Logged(l) = &down {
                    log.insert(l.entry.seq.unwrap(), l.clone());
                }
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
            let op = if settle { 1 + rng.below(2) } else { rng.below(14) };
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
                    dev.send(said);
                }
                // The server takes what the device sent.
                1 | 5 | 6 => {
                    let Some(up) = devices[d].up.pop_front() else { return };
                    if let Up::Live(e) = &up {
                        let Msg::Set { key, value } = e.body else { unreachable!() };
                        let actor = devices[d].actor;
                        for (o, dev) in devices.iter_mut().enumerate() {
                            if o != d && dev.online && dev.actor == actor {
                                dev.expect.insert(key, value);
                            }
                        }
                    }
                    let out = server.receive(Conn(d as u32), up, now);
                    seen.fills += out.iter().filter(|(_, down)| matches!(down, Down::Fill { .. })).count() as u32;
                    deliver(out, devices, log);
                }
                // The device takes what the server sent.
                2 | 7 | 8 => {
                    let dev = &mut devices[d];
                    let Some(down) = dev.down.pop_front() else { return };
                    let live = match &down {
                        Down::Live(e) => {
                            let Msg::Set { key, value } = e.body else { unreachable!() };
                            Some((key, value, position(dev.node.store(Space::Session), key)))
                        }
                        _ => None,
                    };
                    let mut said = Vec::new();
                    let skipped = dev.node.skipped;
                    dev.node.receive(down, &mut said);
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
                // A session value: each device sets its own keys.
                9 | 10 => {
                    let dev = &mut devices[d];
                    value += 1;
                    let key = 10 * d as u8 + rng.below(2) as u8;
                    let e = dev.node.make(Space::Session, Msg::Set { key, value }, now);
                    let mut said = Vec::new();
                    dev.node.route(e, &mut said);
                    dev.send(said);
                }
                // The server's timer, or its log let go of.
                11 => {
                    let out = server.tick(now);
                    deliver(out, devices, log);
                    if rng.below(3) == 0 {
                        let head = server.head().0;
                        server.trim(Seq(rng.below(head + 1)));
                    }
                }
                // Disconnected, or connected again.
                _ => {
                    let dev = &mut devices[d];
                    if dev.online {
                        dev.online = false;
                        server.close(Conn(d as u32));
                        dev.up.clear();
                        dev.down.clear();
                        dev.expect.clear();
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
            seen.held += 1;
            deliver(server.tick(at), &mut devices, &mut log);
        }

        let logged: Vec<EntryId> = log.values().map(|l| l.entry.id).collect();
        let once: BTreeSet<EntryId> = logged.iter().copied().collect();
        assert_eq!(once.len(), logged.len(), "an entry logged twice");
        assert_eq!(log.len() as u64, server.head().0, "every seq logged");
        for (d, dev) in devices.iter().enumerate() {
            assert_eq!(rows(dev.node.store(Space::Shared)), rows(server.store()), "seed {seed}");
            assert_eq!(rows(&dev.node.view(Space::Shared)), rows(server.store()), "seed {seed}");
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
            for (&key, &value) in &dev.expect {
                assert_eq!(
                    position(dev.node.store(Space::Session), key),
                    value,
                    "seed {seed}: device {d}'s latest of {key}"
                );
            }
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
        a_cursor_below_the_log_is_filled,
        devices_and_a_server_converge
    );
}
