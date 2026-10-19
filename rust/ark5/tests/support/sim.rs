//! The simulator's network, ported from v4's `ark::sim`: one authority,
//! clients, and frames in flight that a seeded scheduler moves one at a
//! time — a random frame from a random direction, arriving twice one time
//! in eight and not at all one in sixteen, by Knuth's MMIX generator. No
//! socket, thread or clock: every peer is the one machine, and this is its
//! driver. D1 moves it to `ark5-dev`.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use ark5::peer::{Cas, Input, Journal, Output, Peer, Role, Who};
use ark5::schema::Schema;
use std::sync::Arc;

pub struct Net {
    pub server: Peer,
    pub clients: BTreeMap<i64, Peer>,
    /// The clients with a connection now.
    pub linked: BTreeSet<i64>,
    /// Frames in flight, oldest first.
    pub to_server: BTreeMap<i64, Vec<Vec<u8>>>,
    pub to_client: BTreeMap<i64, Vec<Vec<u8>>>,
    pub seed: u64,
    /// Every refusal a client was told.
    pub refused: Vec<(i64, String)>,
    /// How many frames the scheduler lost, and delivered twice.
    pub dropped: usize,
    pub doubled: usize,
}

/// Knuth's MMIX constants: the whole of the simulation's randomness.
pub fn lcg(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

pub fn who(i: i64) -> Who {
    Who {
        actor: format!("peer-{i}"),
        session: format!("peer-{i}-session"),
        roles: vec![],
    }
}

impl Net {
    /// `n` clients, each linked and said hello.
    pub fn new(schema: Arc<Schema>, cas: Cas, n: i64, seed: u64) -> Net {
        let role = Role::Authority { journal: Journal::default() };
        let mut net = Net {
            server: Peer::new(schema.clone(), cas.clone(), role, who(-1)),
            clients: (0..n)
                .map(|i| (i, Peer::new(schema.clone(), cas.clone(), Role::Replica, who(i))))
                .collect(),
            linked: BTreeSet::new(),
            to_server: BTreeMap::new(),
            to_client: BTreeMap::new(),
            seed,
            refused: vec![],
            dropped: 0,
            doubled: 0,
        };
        for i in 0..n {
            net.heal(i);
        }
        net
    }

    fn roll(&mut self) -> u64 {
        self.seed = lcg(self.seed);
        self.seed >> 11
    }

    fn told_by_client(&mut self, i: i64, outs: Vec<Output>) {
        for o in outs {
            match o {
                Output::Send(b) if self.linked.contains(&i) => self.to_server.entry(i).or_default().push(b),
                Output::Refused { why, .. } => self.refused.push((i, why)),
                _ => {}
            }
        }
    }

    fn told_by_server(&mut self, i: i64, outs: Vec<Output>) {
        for o in outs {
            match o {
                Output::Send(b) => self.to_client.entry(i).or_default().push(b),
                Output::Fanout(b) => {
                    for j in &self.linked {
                        self.to_client.entry(*j).or_default().push(b.clone());
                    }
                }
                _ => {}
            }
        }
    }

    /// A client authors an intent; one its own view refuses is dropped.
    pub fn mutate(&mut self, i: i64, func: ark5::hash::FnHash, input: Vec<u8>, now: i64) {
        let o = self.clients.get_mut(&i).unwrap().step(Input::Mutate { func, input, now });
        self.told_by_client(i, o);
    }

    /// A client goes dark: its connection closes, frames in flight are lost.
    pub fn partition(&mut self, i: i64) {
        self.linked.remove(&i);
        self.to_server.remove(&i);
        self.to_client.remove(&i);
    }

    /// A client comes back on a fresh connection and says hello.
    pub fn heal(&mut self, i: i64) {
        if self.linked.insert(i) {
            let o = self.clients.get_mut(&i).unwrap().step(Input::Connected);
            self.told_by_client(i, o);
        }
    }

    fn up(&mut self, i: i64, b: Vec<u8>) {
        if self.linked.contains(&i) {
            let o = self.server.step(Input::Frame {
                from: Some(who(i)),
                bytes: b,
            });
            self.told_by_server(i, o);
        }
    }

    fn down(&mut self, i: i64, b: Vec<u8>) {
        let o = self.clients.get_mut(&i).unwrap().step(Input::Frame { from: None, bytes: b });
        self.told_by_client(i, o);
    }

    /// One delivery: a random frame in flight, from a random direction,
    /// with a one-in-eight chance of arriving twice and one in sixteen of
    /// not arriving at all.
    pub fn step(&mut self) {
        let r = self.roll();
        let mut candidates: Vec<Result<i64, i64>> = self.to_server.iter().filter(|(_, ms)| !ms.is_empty()).map(|(i, _)| Ok(*i)).collect();
        candidates.extend(self.to_client.iter().filter(|(_, ms)| !ms.is_empty()).map(|(i, _)| Err(*i)));
        if candidates.is_empty() {
            return;
        }
        let pick = candidates[(r % candidates.len() as u64) as usize];
        let times = match self.roll() % 16 {
            0 => 0,
            1 | 2 => 2,
            _ => 1,
        };
        self.dropped += (times == 0) as usize;
        self.doubled += (times == 2) as usize;
        match pick {
            Ok(i) => {
                let m = self.to_server.get_mut(&i).unwrap().remove(0);
                for _ in 0..times {
                    self.up(i, m.clone());
                }
            }
            Err(i) => {
                let m = self.to_client.get_mut(&i).unwrap().remove(0);
                for _ in 0..times {
                    self.down(i, m.clone());
                }
            }
        }
    }

    /// Nothing in flight and nothing pending.
    pub fn quiet(&self) -> bool {
        self.to_server.values().all(|m| m.is_empty())
            && self.to_client.values().all(|m| m.is_empty())
            && self.clients.values().all(|c| c.pending.is_empty())
    }

    fn drain(&mut self) {
        for (i, ms) in std::mem::take(&mut self.to_server) {
            for m in ms {
                self.up(i, m);
            }
        }
        for (i, ms) in std::mem::take(&mut self.to_client) {
            for m in ms {
                self.down(i, m);
            }
        }
    }

    /// Reconnect everyone and deliver everything, perfectly, until nothing
    /// is in flight and nothing is pending; bounded, so a fleet that cannot
    /// converge is an error rather than a hang.
    pub fn settle(&mut self) -> Result<(), String> {
        let all: Vec<i64> = self.clients.keys().copied().collect();
        for i in &all {
            self.partition(*i);
        }
        for i in &all {
            self.heal(*i);
        }
        for _ in 0..10_000 {
            if self.quiet() {
                return Ok(());
            }
            self.drain();
        }
        Err("the fleet did not settle in 10,000 rounds".into())
    }

    /// One state, one chain, nothing pending, on every peer.
    pub fn converged(&self) -> Result<(), String> {
        for (i, c) in &self.clients {
            if c.log != self.server.log || c.store != self.server.store || !c.pending.is_empty() {
                return Err(format!(
                    "client {i} at {:?} with {} pending; the server at {:?}",
                    c.log.head(),
                    c.pending.len(),
                    self.server.log.head()
                ));
            }
        }
        Ok(())
    }
}
