//! The simulator: a real server and real devices (`ark_peer::Peer` in
//! each role, their disks in memory) over a network a seeded
//! scheduler moves one frame at a time — a random frame from a random
//! direction, arriving twice one time in eight and not at all one in
//! sixteen, by Knuth's MMIX generator (v4's `ark::sim`, ported by K1). No
//! socket, thread or clock: every step is a function of the seed and the
//! script.
//!
//! A script is [`Op`]s: authoring on the log or the live space, a
//! partition and its heal, the server restarted from its disk, a client
//! reopened from its own, a sign-in, going alone and joining, a delivery.
//! [`Net::settle`] reconnects everyone and delivers perfectly until nothing
//! is in flight or pending. Every frame moved is checked to decode to the
//! bytes that were sent, and every view a client keeps is mirrored by its
//! patches ([`Net::watch`], [`Net::views_hold`]).

use std::collections::{BTreeMap, BTreeSet};

use ark::frames::Frame;
use ark::peer::{Ctx, Who};
use ark::plan::splice;
use ark::schema::Space;
use ark_peer::{Config, ConnId, Deliver, Memory, Peer, Update};

/// Knuth's MMIX constants: the whole of the simulation's randomness.
pub fn lcg(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// Client `i`'s first login, as K1's simulator named it.
pub fn who(i: i64) -> Who {
    Who {
        actor: format!("peer-{i}"),
        session: format!("peer-{i}-session"),
        roles: vec![],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Mutate(i64, String, Vec<u8>),
    Call(i64, String, Vec<u8>),
    Partition(i64),
    Heal(i64),
    Restart,
    Reopen(i64),
    SignIn(i64, Who),
    Alone(i64),
    Join(i64),
    /// The clock moves on this many milliseconds.
    Wait(i64),
    Step,
}

pub struct Net {
    pub hub: Peer,
    pub hub_config: Config,
    pub clients: BTreeMap<i64, Peer>,
    /// The linked clients' connections.
    pub conns: BTreeMap<i64, ConnId>,
    pub to_server: BTreeMap<i64, Vec<Vec<u8>>>,
    pub to_client: BTreeMap<i64, Vec<Vec<u8>>>,
    pub seed: u64,
    pub now: i64,
    /// Every refusal a client was told; how many frames the network
    /// dropped and how many it delivered twice.
    pub refused: Vec<(i64, String)>,
    pub dropped: usize,
    pub doubled: usize,
    /// Frames whose bytes did not decode to what was sent.
    pub misread: Vec<Vec<u8>>,
    /// Every login each client held, by session: what its entries' roles
    /// must be.
    pub logins: BTreeMap<String, Who>,
    /// The clients alone.
    pub alone: BTreeSet<i64>,
    /// Each view kept, by client and number: what it is of, and the list
    /// its patches made.
    pub views: BTreeMap<(i64, usize), Watched>,
}

/// A view a client keeps: its space, its view's name and input, who
/// reads it, and the list its patches made.
pub type Watched = (Space, String, Vec<u8>, Ctx, Vec<Vec<u8>>);

impl Net {
    /// A hub and `n` clients, each opened as [`who`] with `config`, linked.
    pub fn new(hub: Config, config: impl Fn(i64) -> Config, n: i64, seed: u64) -> Net {
        Net::of(hub, (0..n).map(|i| (who(i), config(i))).collect(), seed)
    }

    /// A hub and a client per login, each linked.
    pub fn of(hub: Config, clients: Vec<(Who, Config)>, seed: u64) -> Net {
        let mut net = Net {
            hub: Peer::serve(hub.clone(), Box::new(Memory::default()), 0),
            hub_config: hub,
            clients: BTreeMap::new(),
            conns: BTreeMap::new(),
            to_server: BTreeMap::new(),
            to_client: BTreeMap::new(),
            seed,
            now: 0,
            refused: vec![],
            dropped: 0,
            doubled: 0,
            misread: vec![],
            logins: BTreeMap::new(),
            alone: BTreeSet::new(),
            views: BTreeMap::new(),
        };
        for (w, c) in clients {
            net.add(w, c);
        }
        net
    }

    /// One more client, linked: its number.
    pub fn add(&mut self, who: Who, config: Config) -> i64 {
        let i = self.clients.keys().last().map_or(0, |n| n + 1);
        self.logins.insert(who.session.clone(), who.clone());
        // The origin a device draws once, seeded: its first session's.
        let mut disk = Memory::default();
        let origin = ark::log::Origin::named(&who.session);
        let _ = ark_peer::local::preset(&mut disk, |r| r.origin = origin);
        self.clients.insert(i, Peer::open(config, who, Box::new(disk), true));
        self.heal(i);
        i
    }

    /// Everything in flight delivered, perfectly, until nothing is.
    pub fn deliver(&mut self) {
        while !(self.to_server.values().all(|m| m.is_empty()) && self.to_client.values().all(|m| m.is_empty())) {
            self.drain();
        }
    }

    fn roll(&mut self) -> u64 {
        self.seed = lcg(self.seed);
        self.seed >> 11
    }

    fn client(&mut self, i: i64) -> &mut Peer {
        self.clients.get_mut(&i).expect("a client")
    }

    /// What a client queued, in flight; what it was told, noted.
    fn told_by_client(&mut self, i: i64) {
        let c = self.clients.get_mut(&i).unwrap();
        let frames = c.take_frames();
        let refused = c.take_rejections().into_iter().map(|(_, why)| why).chain(c.take_live_refusals());
        self.refused.extend(refused.map(|why| (i, why)));
        let mine: Vec<(i64, usize)> = self.views.range((i, 0)..=(i, usize::MAX)).map(|(k, _)| *k).collect();
        for k in mine {
            match c.update(k.1) {
                Update::Patched(patches) => self.views.entry(k).and_modify(|v| v.4 = splice(&patches, &v.4)),
                Update::Reset => {
                    let rows = c.values(k.1).unwrap_or_default();
                    self.views.entry(k).and_modify(|v| v.4 = rows)
                }
                Update::Unchanged => continue,
            };
        }
        if self.conns.contains_key(&i) {
            self.to_server.entry(i).or_default().extend(frames);
        }
    }

    fn told_by_server(&mut self, out: Vec<Deliver>) {
        for (c, b) in out {
            if let Some((i, _)) = self.conns.iter().find(|(_, x)| **x == c) {
                self.to_client.entry(*i).or_default().push(b);
            }
        }
    }

    pub fn run(&mut self, op: &Op) {
        self.now += 1;
        match op.clone() {
            Op::Mutate(i, f, input) => {
                let now = self.now;
                let _ = self.client(i).author(Space::Durable, &f, input, now);
                self.told_by_client(i);
            }
            Op::Call(i, f, input) => {
                let now = self.now;
                let _ = self.client(i).author(Space::Volatile, &f, input, now);
                self.told_by_client(i);
            }
            Op::Partition(i) => self.partition(i),
            Op::Heal(i) => self.heal(i),
            Op::Restart => self.restart(),
            Op::Reopen(i) => {
                self.partition(i);
                self.views.retain(|k, _| k.0 != i);
                let c = self.clients.remove(&i).unwrap().reopen();
                if !c.log.is_authority() {
                    self.alone.remove(&i);
                }
                self.clients.insert(i, c);
                self.heal(i);
            }
            Op::SignIn(i, w) => {
                self.logins.insert(w.session.clone(), w.clone());
                self.partition(i);
                self.client(i).sign_in(w);
                self.heal(i);
            }
            Op::Alone(i) => {
                self.partition(i);
                self.client(i).alone();
                self.alone.insert(i);
            }
            Op::Join(i) => {
                if self.alone.remove(&i) {
                    self.heal(i);
                    self.client(i).join();
                    self.told_by_client(i);
                }
            }
            Op::Wait(ms) => {
                self.now += ms;
                for i in self.clients.keys().copied().collect::<Vec<_>>() {
                    let now = self.now;
                    self.client(i).tick(now);
                    self.told_by_client(i);
                }
            }
            Op::Step => self.step(),
        }
    }

    /// A client goes dark: its connection closes, frames in flight are lost.
    pub fn partition(&mut self, i: i64) {
        if let Some(c) = self.conns.remove(&i) {
            let now = self.now;
            let out = self.hub.leave(c, now);
            self.told_by_server(out);
            self.client(i).disconnected();
        }
        self.to_server.remove(&i);
        self.to_client.remove(&i);
    }

    /// A client comes back on a fresh connection; a client alone stays so.
    pub fn heal(&mut self, i: i64) {
        if self.conns.contains_key(&i) || self.alone.contains(&i) {
            return;
        }
        // The connection is who the client last signed in as, whatever it
        // believes since.
        let me = &self.clients[&i].log.me;
        let who = self.logins.get(&me.session).unwrap_or(me).clone();
        let c = self.hub.accept(who);
        self.conns.insert(i, c);
        self.client(i).connected();
        self.told_by_client(i);
    }

    /// The server stops and opens again from its disk; every connection
    /// with it is lost.
    pub fn restart(&mut self) {
        for i in self.conns.keys().copied().collect::<Vec<_>>() {
            self.partition(i);
        }
        self.hub.close();
        let disk = self.hub.log.disk.take().expect("a server's disk");
        self.hub = Peer::serve(self.hub_config.clone(), disk, self.now);
    }

    fn check(&mut self, b: &[u8]) {
        if Frame::dec(b).map(|f| f.enc()).as_deref() != Ok(b) {
            self.misread.push(b.to_vec());
        }
    }

    fn up(&mut self, i: i64, b: Vec<u8>) {
        self.check(&b);
        if let Some(c) = self.conns.get(&i).copied() {
            let now = self.now;
            let out = self.hub.frame(c, b, now);
            self.told_by_server(out);
        }
    }

    fn down(&mut self, i: i64, b: Vec<u8>) {
        self.check(&b);
        self.client(i).receive(b);
        self.told_by_client(i);
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

    /// A view client `i` keeps of its domain's `name`, mirrored here by
    /// its patches.
    pub fn watch(&mut self, i: i64, space: Space, name: &str, input: Vec<u8>, ctx: Ctx) -> Result<usize, String> {
        let c = self.client(i);
        let v = c.watch(space, name, &input, ctx.clone()).map_err(|r| r.0)?;
        let rows = c.values(v).unwrap_or_default();
        self.views.insert((i, v), (space, name.into(), input, ctx, rows));
        Ok(v)
    }

    /// Every view kept is its patches spliced and a fresh evaluation over
    /// what its client holds now.
    pub fn views_hold(&self) -> Result<(), String> {
        for ((i, v), (space, f, input, ctx, mirror)) in &self.views {
            let c = &self.clients[i];
            let d = match space {
                Space::Durable => c.config.log.domain.as_ref(),
                Space::Volatile => c.config.live.as_ref().and_then(|l| l.module.domain.as_ref()),
            };
            let store = &c.machine(*space).unwrap().store;
            let fresh = d.expect("a domain").watch(store, ctx, f, input).map_err(|r| r.0)?.rows();
            if c.values(*v).as_ref() != Some(&fresh) || *mirror != fresh {
                return Err(format!("client {i}'s view {v} is not a fresh evaluation"));
            }
        }
        Ok(())
    }

    /// Nothing in flight and nothing pending, in either space.
    pub fn quiet(&self) -> bool {
        let pending = |c: &Peer| c.log.pending.is_empty() && c.live.as_ref().is_none_or(|l| l.pending.is_empty());
        self.to_server.values().all(|m| m.is_empty()) && self.to_client.values().all(|m| m.is_empty()) && self.clients.values().all(pending)
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

    /// Everyone back from alone, reconnected, the relay run out, and
    /// everything delivered perfectly until nothing is in flight and nothing
    /// is pending; bounded, so a fleet that cannot converge is an error
    /// rather than a hang.
    pub fn settle(&mut self) -> Result<(), String> {
        let all: Vec<i64> = self.clients.keys().copied().collect();
        for i in &all {
            self.run(&Op::Join(*i));
        }
        for i in &all {
            self.partition(*i);
        }
        for i in &all {
            self.heal(*i);
        }
        self.run(&Op::Wait(1_000_000));
        for _ in 0..1_000 {
            if self.quiet() {
                return Ok(());
            }
            self.drain();
        }
        Err("the fleet did not settle in 1,000 rounds".into())
    }

    /// One head, one state, nothing pending, and one chain where both hold
    /// it (a horizon moves a log's base), on every whole client, in each
    /// space.
    pub fn converged(&self) -> Result<(), String> {
        for (i, c) in &self.clients {
            let pairs = [(Some(&c.log), Some(&self.hub.log)), (c.live.as_ref(), self.hub.live.as_ref())];
            for (space, (p, a)) in [Space::Durable, Space::Volatile].into_iter().zip(pairs) {
                let (Some(p), Some(a)) = (p, a) else { continue };
                if p.feed != ark::peer::Feed::Whole {
                    continue;
                }
                let from = p.log().base_seq().max(a.log().base_seq());
                let chain = p.log().head().0 < from || p.log().after(from).eq(a.log().after(from));
                if p.log().head() != a.log().head() || !chain || p.store != a.store || !p.pending.is_empty() {
                    return Err(format!(
                        "client {i} {space:?} at {:?} with {} pending; the server at {:?}",
                        p.log().head(),
                        p.pending.len(),
                        a.log().head()
                    ));
                }
            }
        }
        Ok(())
    }
}
