//! §15 The simulation, as `Ark.Sim` defines it: a seeded fleet — one server,
//! any number of clients, and a network that reorders, duplicates and drops
//! — with no socket, no thread and no clock. The `rebase/fleet-*` vectors
//! are its transcripts: the same script, run here, must reach the same
//! hashes.

use std::collections::BTreeMap;

use crate::eval::{Args, Ctx};
use crate::hash::{state_hash, Closure, FnHash};
use crate::live::{ConnId, Silent};
use crate::log::Seq;
use crate::peer::{Authority, Replica};
use crate::protocol::{open_access, trusting, Client, ClientMsg, Mode, Server, ServerMsg};
use crate::schema::Schema;
use crate::store::MemoryStore;
use crate::value::Id;

pub struct Sim {
    pub server: Server<Silent>,
    pub clients: BTreeMap<i64, Client>,
    /// The connection each client currently has, if linked.
    pub conn: BTreeMap<i64, ConnId>,
    pub next_conn: ConnId,
    /// Frames in flight, oldest first.
    pub to_server: BTreeMap<i64, Vec<ClientMsg>>,
    pub to_client: BTreeMap<i64, Vec<ServerMsg>>,
    pub seed: u64,
}

fn name(i: i64) -> String {
    format!("peer-{i}")
}

// Under dev auth the server names every login "dev", and an entry is held
// to the login that pushed it, so this is what a client authors under.
fn ctx_of(i: i64) -> Ctx {
    Ctx {
        user: name(i),
        session: "dev".into(),
    }
}

impl Sim {
    /// A fleet: a trusting server that is the log's authority, and `n`
    /// clients each holding the log whole, all connected.
    pub fn new(sch: Schema, bodies: BTreeMap<FnHash, Closure>, n: i64, seed: u64) -> Sim {
        let server = Server::open(trusting(), open_access(), Silent, Authority::new(sch.clone(), bodies.clone()));
        let clients = (0..n)
            .map(|i| {
                let r = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
                (i, Client::open(r, Mode::Whole, Some(name(i))))
            })
            .collect();
        let mut sim = Sim {
            server,
            clients,
            conn: BTreeMap::new(),
            next_conn: 1,
            to_server: BTreeMap::new(),
            to_client: BTreeMap::new(),
            seed,
        };
        for i in 0..n {
            sim.heal(i);
        }
        sim
    }

    /// A client authors an intent. A refusal by its own view is dropped.
    pub fn mutate(&mut self, i: i64, eid: Id, fh: &FnHash, autos: &Args, args: &Args) {
        let Some(c) = self.clients.get_mut(&i) else { return };
        if c.mutate(eid, &ctx_of(i), fh, autos, args).is_ok() {
            self.flush_client(i);
        }
    }

    /// A client goes dark: its connection closes, frames in flight are lost.
    pub fn partition(&mut self, i: i64) {
        let Some(conn) = self.conn.remove(&i) else { return };
        self.server.disconnect(conn);
        if let Some(c) = self.clients.get_mut(&i) {
            c.disconnected();
        }
        self.to_server.remove(&i);
        self.to_client.remove(&i);
        self.flush_server();
    }

    /// A client comes back on a fresh connection and says hello.
    pub fn heal(&mut self, i: i64) {
        if self.conn.contains_key(&i) {
            return;
        }
        let conn = self.next_conn;
        self.conn.insert(i, conn);
        self.next_conn += 1;
        if let Some(c) = self.clients.get_mut(&i) {
            c.connected();
        }
        self.flush_client(i);
    }

    // Move what a client queued onto the wire.
    fn flush_client(&mut self, i: i64) {
        let Some(c) = self.clients.get_mut(&i) else { return };
        let out = c.take_outgoing();
        self.to_server.entry(i).or_default().extend(out);
    }

    // Move what the server queued onto the wire, to whichever client each
    // connection belongs to; a frame for a connection nobody has is lost.
    fn flush_server(&mut self) {
        let out = self.server.take_outgoing();
        let by_client: BTreeMap<ConnId, i64> = self.conn.iter().map(|(i, c)| (*c, *i)).collect();
        for (conn, m) in out {
            if let Some(i) = by_client.get(&conn) {
                self.to_client.entry(*i).or_default().push(m);
            }
        }
    }

    /// §15.1 One delivery: a random frame in flight, from a random
    /// direction, with a one-in-eight chance of arriving twice and one in
    /// sixteen of not arriving at all.
    pub fn step(&mut self) {
        let r = self.roll();
        let mut candidates: Vec<Result<i64, i64>> = self.to_server.iter().filter(|(_, ms)| !ms.is_empty()).map(|(i, _)| Ok(*i)).collect();
        candidates.extend(self.to_client.iter().filter(|(_, ms)| !ms.is_empty()).map(|(i, _)| Err(*i)));
        if candidates.is_empty() {
            return;
        }
        let pick = candidates[(r % candidates.len() as u64) as usize];
        let r2 = self.roll();
        let fate = r2 % 16; // 0: drop; 1,2: duplicate; else once
        let times = if fate == 0 {
            0
        } else if fate <= 2 {
            2
        } else {
            1
        };
        match pick {
            Ok(i) => {
                let queue = self.to_server.entry(i).or_default();
                if queue.is_empty() {
                    return;
                }
                let m = queue.remove(0);
                for _ in 0..times {
                    self.deliver_to_server(i, m.clone());
                }
            }
            Err(i) => {
                let queue = self.to_client.entry(i).or_default();
                if queue.is_empty() {
                    return;
                }
                let m = queue.remove(0);
                for _ in 0..times {
                    self.deliver_to_client(i, m.clone());
                }
            }
        }
    }

    fn deliver_to_server(&mut self, i: i64, m: ClientMsg) {
        let Some(conn) = self.conn.get(&i).copied() else { return };
        self.server.recv(conn, m);
        self.flush_server();
    }

    fn deliver_to_client(&mut self, i: i64, m: ServerMsg) {
        let Some(c) = self.clients.get_mut(&i) else { return };
        c.recv(m);
        self.flush_client(i);
    }

    /// §15.2 Reconnect everyone — every client, on a fresh connection — and
    /// deliver everything, perfectly, until nothing is in flight and nothing
    /// is pending. Bounded, so a fleet that cannot converge is a failure
    /// rather than a hang.
    pub fn settle(&mut self) {
        let peers: Vec<i64> = self.clients.keys().copied().collect();
        for i in &peers {
            self.partition(*i);
        }
        for i in &peers {
            self.heal(*i);
        }
        for _ in 0..10000 {
            if self.quiet() {
                return;
            }
            self.drain();
        }
        panic!("settle: the fleet did not converge in 10000 rounds");
    }

    fn drain(&mut self) {
        let to_server: Vec<(i64, Vec<ClientMsg>)> = self.to_server.iter().map(|(i, ms)| (*i, ms.clone())).collect();
        for (i, ms) in to_server {
            self.to_server.insert(i, vec![]);
            for m in ms {
                self.deliver_to_server(i, m);
            }
        }
        let to_client: Vec<(i64, Vec<ServerMsg>)> = self.to_client.iter().map(|(i, ms)| (*i, ms.clone())).collect();
        for (i, ms) in to_client {
            self.to_client.insert(i, vec![]);
            for m in ms {
                self.deliver_to_client(i, m);
            }
        }
    }

    /// Nothing in flight and nothing pending.
    pub fn quiet(&self) -> bool {
        self.to_server.values().all(|ms| ms.is_empty())
            && self.to_client.values().all(|ms| ms.is_empty())
            && self.clients.values().all(|c| c.replica.pending.is_empty())
    }

    /// Each client's confirmed sequence and hash.
    pub fn client_hashes(&self) -> Vec<(i64, Seq, Vec<u8>)> {
        self.clients
            .iter()
            .map(|(i, c)| {
                let (n, h) = c.replica.verify_at();
                (*i, n, h)
            })
            .collect()
    }

    /// The server's hash, at the head.
    pub fn server_hash(&self) -> (Seq, Vec<u8>) {
        let a = &self.server.authority;
        (a.log.head_seq(), state_hash(&a.store))
    }

    fn roll(&mut self) -> u64 {
        self.seed = lcg(self.seed);
        self.seed >> 11
    }
}

/// Knuth's MMIX constants: the whole of the simulation's randomness.
pub fn lcg(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}
