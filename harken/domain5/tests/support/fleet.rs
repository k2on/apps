//! The simulator's network (`rust/ark5/tests/support/sim.rs`, K1's port of
//! v4's `ark::sim`), with what harken's fleets need besides: each client's
//! verified identity is the driver's to say — which login a connection
//! is, roles and all — and a peer authors as whoever it believes it is. A
//! seeded scheduler moves one frame at a time, arriving twice one time in
//! eight and not at all one in sixteen, by Knuth's MMIX generator. D1
//! moves the simulator to `ark5-dev`; this goes with it.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ark5::peer::{Cas, Input, Journal, Output, Peer, Role, Who};
use ark5::schema::Schema;

pub struct Net {
    pub server: Peer,
    pub clients: BTreeMap<i64, Peer>,
    /// Who each client's connection was verified to be.
    pub logins: BTreeMap<i64, Who>,
    pub linked: BTreeSet<i64>,
    pub to_server: BTreeMap<i64, Vec<Vec<u8>>>,
    pub to_client: BTreeMap<i64, Vec<Vec<u8>>>,
    pub seed: u64,
    /// Every refusal a client was told, by its own view or the server.
    pub refused: Vec<(i64, String)>,
}

pub fn lcg(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

pub fn who(actor: &str, session: &str, roles: &[&str]) -> Who {
    Who { actor: actor.into(), session: session.into(), roles: roles.iter().map(|r| r.to_string()).collect() }
}

impl Net {
    /// A client per login, each authoring as its login and linked.
    pub fn new(schema: Arc<Schema>, cas: Cas, logins: Vec<Who>, seed: u64) -> Net {
        let role = Role::Authority { journal: Journal::default() };
        let n = logins.len() as i64;
        let mut net = Net {
            server: Peer::new(schema.clone(), cas.clone(), role, who("server", "server", &[])),
            clients: (0..n).map(|i| (i, Peer::new(schema.clone(), cas.clone(), Role::Replica, logins[i as usize].clone()))).collect(),
            logins: (0..n).zip(logins).collect(),
            linked: BTreeSet::new(),
            to_server: BTreeMap::new(),
            to_client: BTreeMap::new(),
            seed,
            refused: vec![],
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

    pub fn partition(&mut self, i: i64) {
        self.linked.remove(&i);
        self.to_server.remove(&i);
        self.to_client.remove(&i);
    }

    pub fn heal(&mut self, i: i64) {
        if self.linked.insert(i) {
            let o = self.clients.get_mut(&i).unwrap().step(Input::Connected);
            self.told_by_client(i, o);
        }
    }

    fn up(&mut self, i: i64, b: Vec<u8>) {
        if self.linked.contains(&i) {
            let o = self.server.step(Input::Frame { from: Some(self.logins[&i].clone()), bytes: b });
            self.told_by_server(i, o);
        }
    }

    fn down(&mut self, i: i64, b: Vec<u8>) {
        let o = self.clients.get_mut(&i).unwrap().step(Input::Frame { from: None, bytes: b });
        self.told_by_client(i, o);
    }

    /// One delivery: a random frame in flight, from a random direction.
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

    pub fn quiet(&self) -> bool {
        self.to_server.values().all(|m| m.is_empty()) && self.to_client.values().all(|m| m.is_empty()) && self.clients.values().all(|c| c.pending.is_empty())
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
    /// is in flight and nothing is pending.
    pub fn settle(&mut self) {
        let all: Vec<i64> = self.clients.keys().copied().collect();
        for i in &all {
            self.partition(*i);
        }
        for i in &all {
            self.heal(*i);
        }
        for _ in 0..10_000 {
            if self.quiet() {
                return;
            }
            self.drain();
        }
        panic!("the fleet did not settle in 10,000 rounds");
    }

    /// One state, one chain, nothing pending, on every peer.
    pub fn converged(&self) -> Result<(), String> {
        for (i, c) in &self.clients {
            if c.log != self.server.log || c.store != self.server.store || !c.pending.is_empty() {
                return Err(format!("client {i} at {:?} with {} pending; the server at {:?}", c.log.head(), c.pending.len(), self.server.log.head()));
            }
        }
        Ok(())
    }
}
