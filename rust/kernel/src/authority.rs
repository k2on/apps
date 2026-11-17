//! The authority: the one node that appends to the master log, which a
//! device never links (the server's host holds it).
//!
//! It judges each pushed entry once. Each origin's entries are judged in
//! the order of their count, as a node pushes them, so a count at or below
//! the last one judged from its origin is a repeat (a push sent again at a
//! connection): it is answered with the refusal the authority journaled
//! for it, or `At { seq, last }`, and not run again. Any other is run on
//! the confirmed rows: refused, it is journaled and answered so; kept, its
//! writes are its facts, and it is logged at the next `seq`. A refusal
//! never holds back the entries after it.
//!
//! The authority keeps the log, its rows, the last count judged per origin
//! and the refusals, all in memory here: storing them and trimming the
//! journal are the server host's. The entries a handler emits on the
//! authority are not taken yet: reactions are a later step.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::entry::{Entry, EntryId, Origin, Seq, Space};
use crate::node::{run, Body, Handler, Refusal};
use crate::store::Store;
use crate::sync::{Down, Logged, Verdict};

pub struct Authority<B: 'static, S> {
    handlers: &'static [Handler<B>],
    store: S,
    log: Vec<Logged<B>>,
    last: BTreeMap<Origin, u64>,
    refused: BTreeMap<EntryId, Refusal>,
}

impl<B: Body + Clone + 'static, S: Store> Authority<B, S> {
    /// An authority over an empty log, running `handlers` on `store`.
    pub fn new(handlers: &'static [Handler<B>], store: S) -> Authority<B, S> {
        Authority {
            handlers,
            store,
            log: Vec::new(),
            last: BTreeMap::new(),
            refused: BTreeMap::new(),
        }
    }

    /// The last `seq` logged.
    pub fn head(&self) -> Seq {
        Seq(self.log.len() as u64)
    }

    /// The confirmed rows.
    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn log(&self) -> &[Logged<B>] {
        &self.log
    }

    /// The journal of refusals.
    pub fn refused(&self) -> &BTreeMap<EntryId, Refusal> {
        &self.refused
    }

    /// The answer to a node's hello: the log after its cursor.
    pub fn hello(&self, cursor: Seq) -> Vec<Down<B>> {
        let from = (cursor.0 as usize).min(self.log.len());
        self.log[from..].iter().cloned().map(Down::Logged).collect()
    }

    /// Judges a pushed entry: the verdict for its node, and what it logged,
    /// for every node.
    pub fn push(&mut self, e: Entry<B>) -> (Down<B>, Option<Logged<B>>) {
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
        match run(self.handlers, &self.store, &e) {
            Err(why) => {
                self.refused.insert(id, why);
                (answer(Verdict::Refused(why)), None)
            }
            Ok((facts, _)) => {
                self.store.apply(&facts);
                let seq = Seq(self.log.len() as u64 + 1);
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

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::node::app::*;
    use crate::node::Node;
    use crate::store::Memory;
    use crate::sync::Up;
    use crate::Rng;
    use alloc::collections::{BTreeSet, VecDeque};

    fn authority() -> Authority<Msg, Memory> {
        Authority::new(&HANDLERS, Memory::new())
    }

    #[cfg_attr(test, test)]
    fn a_push_is_judged_once() {
        let mut a = authority();
        let mut n = node(1);
        let e = n.make(Space::Shared, Msg::Add { key: 1, by: 2 }, 10);
        let (verdict, logged) = a.push(e.clone());
        assert_eq!(
            verdict,
            Down::Verdict {
                id: e.id,
                verdict: Verdict::At { seq: Seq(1), last: 1 }
            }
        );
        let logged = logged.unwrap();
        assert_eq!(logged.entry.seq, Some(Seq(1)));
        assert_eq!(count(a.store(), 1), 2);
        // Pushed again, it is answered where its origin stands, not run.
        let other = node(2).make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        a.push(other);
        assert_eq!(
            a.push(e.clone()),
            (
                Down::Verdict {
                    id: e.id,
                    verdict: Verdict::At { seq: Seq(2), last: 1 }
                },
                None
            )
        );
        assert_eq!(count(a.store(), 1), 3);
        assert_eq!(a.hello(Seq(1)), [Down::Logged(a.log()[1].clone())]);
        assert_eq!(a.hello(Seq(9)), []);
    }

    #[cfg_attr(test, test)]
    fn a_refusal_is_journaled_and_holds_nothing_back() {
        let mut a = authority();
        let mut n = node(1);
        let take = n.make(Space::Shared, Msg::Take { key: 1 }, 10);
        let add = n.make(Space::Shared, Msg::Add { key: 1, by: 1 }, 10);
        let refused = Down::Verdict {
            id: take.id,
            verdict: Verdict::Refused(NONE_LEFT),
        };
        assert_eq!(a.push(take.clone()), (refused.clone(), None));
        assert!(a.push(add).1.is_some());
        // The refusal is said again to a repeat.
        assert_eq!(a.push(take), (refused, None));
        let local = n.make(Space::Local, Msg::Seen, 10);
        assert_eq!(
            a.push(local.clone()).0,
            Down::Verdict {
                id: local.id,
                verdict: Verdict::Refused(Refusal::NOT_PUSHABLE)
            }
        );
        assert_eq!(a.head(), Seq(1));
    }

    /// A device in the convergence test: its node, what is in flight on
    /// its connection each way, and whether it is connected.
    struct Device {
        node: Node<Msg, Memory>,
        up: VecDeque<Up<Msg>>,
        down: VecDeque<Down<Msg>>,
        online: bool,
        made: BTreeSet<EntryId>,
        refused: BTreeSet<EntryId>,
    }

    impl Device {
        /// What the node said, sent if connected and lost if not.
        fn send(&mut self, said: Vec<Up<Msg>>) {
            if self.online {
                self.up.extend(said);
            }
        }
    }

    /// Two devices and a server, each device writing offline and online,
    /// disconnected and reconnected at random with what was in flight
    /// lost, and messages delivered in random interleavings: once all are
    /// connected and quiet, every device's rows are the server's, nothing
    /// is pending, every entry made is logged once or refused, and each
    /// device was told of its every refusal.
    fn converge(seed: u64, steps: usize) {
        let mut rng = Rng::new(seed);
        let mut server = authority();
        let mut devices: Vec<Device> = (1..=2)
            .map(|o| Device {
                node: node(o),
                up: VecDeque::new(),
                down: VecDeque::new(),
                online: false,
                made: BTreeSet::new(),
                refused: BTreeSet::new(),
            })
            .collect();
        let step = |rng: &mut Rng, server: &mut Authority<Msg, Memory>, devices: &mut Vec<Device>, settle: bool| {
            let d = rng.below(2) as usize;
            let op = if settle { 1 + rng.below(2) } else { rng.below(10) };
            match op {
                // A write, connected or not.
                0 | 3 | 4 => {
                    let key = rng.below(3) as u8;
                    let body = if rng.below(2) == 0 {
                        Msg::Take { key }
                    } else {
                        Msg::Add { key, by: 1 + rng.below(2) }
                    };
                    let dev = &mut devices[d];
                    let e = dev.node.make(Space::Shared, body, rng.below(100));
                    dev.made.insert(e.id);
                    let mut said = Vec::new();
                    dev.node.route(e, &mut said);
                    dev.send(said);
                }
                // The server takes what the device sent.
                1 | 5 | 6 => {
                    let Some(up) = devices[d].up.pop_front() else { return };
                    match up {
                        Up::Hello { cursor } => devices[d].down.extend(server.hello(cursor)),
                        Up::Push(e) => {
                            let (verdict, logged) = server.push(e);
                            if let Some(l) = logged {
                                for dev in devices.iter_mut().filter(|dev| dev.online) {
                                    dev.down.push_back(Down::Logged(l.clone()));
                                }
                            }
                            devices[d].down.push_back(verdict);
                        }
                        Up::Live(_) => {}
                    }
                }
                // The device takes what the server sent.
                2 | 7 | 8 => {
                    let dev = &mut devices[d];
                    let Some(down) = dev.down.pop_front() else { return };
                    let mut said = Vec::new();
                    dev.node.receive(down, &mut said);
                    dev.send(said);
                    dev.refused.extend(dev.node.take_refused().into_iter().map(|(id, _)| id));
                }
                // Disconnected, or connected again.
                _ => {
                    let dev = &mut devices[d];
                    if dev.online {
                        dev.online = false;
                        dev.up.clear();
                        dev.down.clear();
                    } else {
                        dev.online = true;
                        let mut said = Vec::new();
                        dev.node.connect(&mut said);
                        dev.send(said);
                    }
                }
            }
        };
        for _ in 0..steps {
            step(&mut rng, &mut server, &mut devices, false);
        }
        // Everyone connected, then quiet.
        for dev in devices.iter_mut().filter(|dev| !dev.online) {
            dev.online = true;
            let mut said = Vec::new();
            dev.node.connect(&mut said);
            dev.send(said);
        }
        while devices.iter().any(|dev| !dev.up.is_empty() || !dev.down.is_empty()) {
            step(&mut rng, &mut server, &mut devices, true);
        }

        let logged: Vec<EntryId> = server.log().iter().map(|l| l.entry.id).collect();
        let once: BTreeSet<EntryId> = logged.iter().copied().collect();
        assert_eq!(once.len(), logged.len(), "an entry logged twice");
        for dev in &devices {
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
        }
        // The rows are the log's facts, applied in order.
        let mut replay = Memory::new();
        for l in server.log() {
            replay.apply(&l.facts);
        }
        assert_eq!(rows(&replay), rows(server.store()));
        assert!(rows(&replay).iter().all(|(_, v)| v.len() == 8));
    }

    #[cfg_attr(test, test)]
    fn devices_and_a_server_converge() {
        for seed in 0..60 {
            converge(seed, 120);
        }
    }

    crate::listed!(
        a_push_is_judged_once,
        a_refusal_is_journaled_and_holds_nothing_back,
        devices_and_a_server_converge
    );
}
