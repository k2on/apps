//! §15 The simulation, as `Ark.Sim` defines it: a seeded fleet — one server,
//! any number of clients, and a network that reorders, duplicates and drops
//! — with no socket, no thread and no clock. The `rebase/fleet-*` vectors
//! are its transcripts: the same script, run here, must reach the same
//! hashes.
//!
//! What a deployment lives through beyond the network is here too, for the
//! fuzzer (`arkc fuzz`, `docs/plan-db.md` D2) and for the vectors it
//! writes: a server restarted from its journal, or over an emptied one; a
//! horizon moved, so that a peer comes back below it; a client re-opened
//! from what it had made durable; a peer that joins late, by facts or
//! without closures; one used before anyone signed in, and the sign-in.
//! Each is an [`Op`], a script is a list of them, and [`Sim::run`] is the
//! one place an op means something — so a session the fuzzer ran and the
//! same session read back from a vector are one code path. None of it is
//! on unless asked for ([`Sim::durable`], [`Sim::tapped`]): the scripted
//! fleet of `rebase/fleet-seed-7.json` runs exactly as it always did.

use std::collections::{BTreeMap, BTreeSet};

use crate::authoring::Procedure;
use crate::eval::{Args, Ctx};
use crate::hash::{state_hash, Closure, FnHash};
use crate::journal::{self, Journal, Keys, Layout};
use crate::live::{ConnId, Silent};
use crate::log::{Log, Seq};
use crate::peer::{Authority, Changes, Journal as Durable, Replica, Sequenced};
use crate::protocol::{open_access, trusting, Client, ClientMsg, Mode, Server, ServerMsg};
use crate::schema::Schema;
use crate::store::{MemoryStore, Store};
use crate::value::{hex, Id, Value};

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
    /// The module the fleet runs: what a client that joins, a server that
    /// restarts and a replica re-opened are opened with.
    pub schema: Schema,
    pub bodies: BTreeMap<FnHash, Closure>,
    /// Native procedures, and who runs them: the server when
    /// `native_server`, and every client in `native_peers`. The rest
    /// interpret — so that a fleet of both is a fleet in which the two
    /// must agree, or a replica's replay is caught diverging.
    pub natives: Vec<(FnHash, Procedure)>,
    pub native_server: bool,
    pub native_peers: BTreeSet<i64>,
    /// Every frame put on the wire, oldest first, while tapped
    /// ([`Sim::tapped`]): what the fuzzer holds to its encoding.
    pub tap: Option<Vec<Frame>>,
    /// The server's journal, once [`Sim::durable`]: written after every
    /// message the server answers, as a hub writes its log, and what
    /// [`Op::Restart`] opens the server from.
    pub disk: Option<Disk>,
    /// What each client has made durable, once [`Sim::durable`]: its
    /// confirmed store as `take_confirmed` moved it, its cursor and its
    /// log, which [`Op::Reopen`] opens it from.
    pub kept: BTreeMap<i64, Kept>,
    /// Clients used before anyone signed in on them: they author as
    /// [`Ctx::nobody`] until [`Op::SignIn`].
    pub nobody: BTreeSet<i64>,
    /// What each client's view was told since a reader last took it
    /// ([`Replica::take_changes`], taken here once durable, in order), and
    /// the store those changes reach from the last rebuild: which must be
    /// the view, or a screen kept by them would be wrong.
    pub told: BTreeMap<i64, Vec<Changes>>,
    pub told_store: BTreeMap<i64, MemoryStore>,
    /// Logs created by [`Op::Wipe`], numbering their identities.
    pub wiped: u8,
    /// What a restart or a re-open found not to be what it should: the
    /// journal read back as another log, or a client's durable store not
    /// its confirmed one. Empty on a conformant runtime.
    pub faults: Vec<String>,
}

/// One frame, as it went on the wire.
#[derive(Clone, Debug)]
pub enum Frame {
    ToServer(ClientMsg),
    ToClient(ServerMsg),
}

/// A key/value storage in memory, which is all a journal asks of one.
#[derive(Clone, Debug, Default)]
pub struct MemKeys(pub BTreeMap<String, Vec<u8>>);

impl Keys for MemKeys {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.get(key).cloned())
    }
    fn save(&mut self, key: &str, bytes: &[u8]) -> Result<(), String> {
        self.0.insert(key.into(), bytes.to_vec());
        Ok(())
    }
    fn remove(&mut self, key: &str) -> Result<(), String> {
        self.0.remove(key);
        Ok(())
    }
}

/// The server's durable log: the storage and the journal over it.
#[derive(Clone, Debug)]
pub struct Disk {
    pub keys: MemKeys,
    pub journal: Journal,
}

/// What a client has made durable (§11.1): the confirmed store, moved only
/// by what [`Replica::take_confirmed`] reported, the cursor and the log.
/// Its pending intents are durable too, and are the replica's own list.
#[derive(Clone, Debug)]
pub struct Kept {
    pub store: MemoryStore,
    pub cursor: Seq,
    pub log_id: Option<Id>,
}

/// One move of a scripted session (`rebase/fleet-fuzz-*` vectors, and the
/// fuzzer's sessions). A function is named by its name in the module,
/// which a verified module has once; the entry carries its hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// A client authors an intent; one its own view refuses is dropped.
    Mutate {
        peer: i64,
        function: String,
        eid: Id,
        autos: Args,
        args: Args,
    },
    Partition(i64),
    Heal(i64),
    /// One random delivery ([`Sim::step`]).
    Step,
    /// Everyone reconnects and everything is delivered ([`Sim::settle`]).
    Settle,
    /// The server stops and starts again from its journal; every
    /// connection drops and comes back.
    Restart,
    /// The server starts again over an emptied directory: a new log, of
    /// another name, at 0.
    Wipe,
    /// The authority moves its horizon to this sequence, clamped to the
    /// log it has; the journal writes a snapshot.
    Compact(Seq),
    /// A client joins at 0: by facts or by replay, with the module's
    /// closures or none, signed in or used by nobody until it signs in.
    Join {
        mode: Mode,
        closures: bool,
        nobody: bool,
    },
    /// A client used by nobody signs in, and connects.
    SignIn(i64),
    /// A client stops and is opened again from what it made durable.
    Reopen(i64),
    /// A client asks the authority whether it agrees ([`Client::verify_all`]).
    Verify(i64),
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
            schema: sch,
            bodies,
            natives: vec![],
            native_server: false,
            native_peers: BTreeSet::new(),
            tap: None,
            disk: None,
            kept: BTreeMap::new(),
            nobody: BTreeSet::new(),
            told: BTreeMap::new(),
            told_store: BTreeMap::new(),
            wiped: 0,
            faults: vec![],
        };
        for i in 0..n {
            sim.heal(i);
        }
        sim
    }

    /// Record every frame put on the wire from here on, in [`Sim::tap`].
    pub fn tapped(mut self) -> Sim {
        self.tap = Some(vec![]);
        self
    }

    /// Give the server a journal and a name for its log, and every client
    /// a durable copy of what it confirmed: what [`Op::Restart`] and
    /// [`Op::Reopen`] open from. The log is named as a server names the
    /// one it creates, so that a wiped log is told apart from it.
    pub fn durable(mut self) -> Sim {
        let mut name = [0u8; 16];
        name[..8].copy_from_slice(&self.seed.to_be_bytes());
        name[15] = 0xa0;
        self.server.authority.log.name_if_unnamed(name);
        let mut keys = MemKeys::default();
        let journal = Journal::create(&mut keys, Layout::server(), &self.server.authority.log).expect("a journal in memory");
        self.disk = Some(Disk { keys, journal });
        let ids: Vec<i64> = self.clients.keys().copied().collect();
        for i in ids {
            self.keep(i);
        }
        self
    }

    /// Hold native procedures on the server, when `server`, and on the
    /// clients named; the rest interpret.
    pub fn hold(&mut self, procs: Vec<(FnHash, Procedure)>, server: bool, peers: impl IntoIterator<Item = i64>) {
        self.native_server = server;
        self.native_peers = peers.into_iter().collect();
        if server {
            self.server.authority.hold(procs.clone());
        }
        for i in &self.native_peers {
            if let Some(c) = self.clients.get_mut(i) {
                c.hold(&procs);
            }
        }
        self.natives = procs;
    }

    /// A client authors an intent. A refusal by its own view is dropped.
    pub fn mutate(&mut self, i: i64, eid: Id, fh: &FnHash, autos: &Args, args: &Args) {
        let ctx = if self.nobody.contains(&i) { Ctx::nobody() } else { ctx_of(i) };
        let Some(c) = self.clients.get_mut(&i) else { return };
        if c.mutate(eid, &ctx, fh, autos, args).is_ok() {
            self.flush_client(i);
        }
        self.keep(i);
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
        if let Some(tap) = &mut self.tap {
            tap.extend(out.iter().cloned().map(Frame::ToServer));
        }
        self.to_server.entry(i).or_default().extend(out);
    }

    // Move what the server queued onto the wire, to whichever client each
    // connection belongs to; a frame for a connection nobody has is lost.
    fn flush_server(&mut self) {
        let out = self.server.take_outgoing();
        if let Some(tap) = &mut self.tap {
            tap.extend(out.iter().map(|(_, m)| Frame::ToClient(m.clone())));
        }
        self.write_log();
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
                self.settle_client(i);
            }
        }
    }

    fn deliver_to_server(&mut self, i: i64, m: ClientMsg) {
        let Some(conn) = self.conn.get(&i).copied() else { return };
        self.server.recv(conn, m);
        self.flush_server();
    }

    // A frame reaches a client's inbox; nothing is applied until the
    // client settles (R8 of `docs/plan-perf.md`).
    fn deliver_to_client(&mut self, i: i64, m: ServerMsg) {
        let Some(c) = self.clients.get_mut(&i) else { return };
        c.recv(m);
    }

    // The end of a client's pump: what its frames placed is applied once,
    // and what that makes it say goes on the wire. A step is one pump of
    // one frame (twice, when the network duplicated it); a drain is one
    // pump of every frame in flight to that client.
    fn settle_client(&mut self, i: i64) {
        let Some(c) = self.clients.get_mut(&i) else { return };
        c.settle();
        self.flush_client(i);
        self.keep(i);
    }

    /// §15.2 Reconnect everyone — every client, on a fresh connection — and
    /// deliver everything, perfectly, until nothing is in flight and nothing
    /// is pending. Bounded, so a fleet that cannot converge is a failure
    /// rather than a hang.
    pub fn settle(&mut self) {
        // A client nobody has signed in on does not connect: it has no one
        // to connect as ([`Op::SignIn`]).
        let peers: Vec<i64> = self.clients.keys().copied().filter(|i| !self.nobody.contains(i)).collect();
        for i in &peers {
            self.partition(*i);
        }
        for i in &peers {
            self.heal(*i);
        }
        if let Err(e) = self.deliver_all() {
            panic!("{e}");
        }
    }

    /// [`Sim::settle`], answering rather than panicking when the fleet does
    /// not converge: what a script's `settle` op is, so that a fleet that
    /// cannot settle is a finding that says why ([`Sim::stuck`]).
    pub fn try_settle(&mut self) -> Result<(), String> {
        let peers: Vec<i64> = self.clients.keys().copied().filter(|i| !self.nobody.contains(i)).collect();
        for i in &peers {
            self.partition(*i);
        }
        for i in &peers {
            self.heal(*i);
        }
        self.deliver_all()
    }

    fn deliver_all(&mut self) -> Result<(), String> {
        for _ in 0..10000 {
            if self.quiet() {
                return Ok(());
            }
            self.drain();
        }
        Err(format!("settle: the fleet did not converge in 10000 rounds: {}", self.stuck()))
    }

    /// Why a fleet does not settle, as far as it can be said: each intent
    /// still pending, and where the authority has it.
    pub fn stuck(&self) -> String {
        let log = &self.server.authority.log;
        let mut out = vec![];
        for (i, c) in &self.clients {
            if self.nobody.contains(i) {
                continue;
            }
            for e in &c.replica.pending {
                out.push(match log.seq_of(&e.id) {
                    Some(n) if n <= c.replica.cursor => format!(
                        "client {i}: an intent the authority sequenced at {n}, at or below the client's cursor {}, is still pending",
                        c.replica.cursor
                    ),
                    Some(n) => format!("client {i}: an intent sequenced at {n} is pending at cursor {}", c.replica.cursor),
                    None => format!("client {i}: an intent the authority never sequenced is pending"),
                });
            }
        }
        out.sort();
        out.dedup();
        if out.is_empty() {
            "nothing pending, something still in flight".into()
        } else {
            out.join("; ")
        }
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
            self.settle_client(i);
        }
    }

    /// Nothing in flight and nothing pending, but on a client nobody has
    /// signed in on.
    pub fn quiet(&self) -> bool {
        self.to_server.values().all(|ms| ms.is_empty())
            && self.to_client.values().all(|ms| ms.is_empty())
            && self.clients.iter().all(|(i, c)| c.replica.pending.is_empty() || self.nobody.contains(i))
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

    // The server's log, written as a hub writes it after answering: what
    // moved since the last write, or a snapshot where the horizon moved.
    fn write_log(&mut self) {
        let Some(d) = &mut self.disk else { return };
        if let Err(e) = d.journal.write(&mut d.keys, &self.server.authority.log) {
            self.faults.push(format!("the journal refused a write: {e}"));
        }
    }

    // What a client's confirmed store moved by, applied to its durable
    // copy, as a client's storage keeps it.
    fn keep(&mut self, i: i64) {
        if self.disk.is_none() {
            return;
        }
        let Some(c) = self.clients.get_mut(&i) else { return };
        let r = &mut c.replica;
        let k = self.kept.entry(i).or_insert_with(|| Kept {
            store: r.confirmed.clone(),
            cursor: r.cursor,
            log_id: r.log_id,
        });
        match r.take_confirmed() {
            Durable::Replaced => k.store = r.confirmed.clone(),
            Durable::Facts(moved) => {
                for (_, f) in moved {
                    k.store.apply_changes(&f);
                }
            }
        }
        k.cursor = r.cursor;
        k.log_id = r.log_id;
        let ch = r.take_changes();
        match &ch {
            Changes::Rebuilt => {
                self.told_store.insert(i, r.view.clone());
            }
            Changes::Applied(cs) => match self.told_store.get_mut(&i) {
                Some(st) => {
                    st.apply_changes(cs);
                    if !same_rows(st, &r.view) {
                        self.faults
                            .push(format!("client {i}: the changes its view was told do not reach its view"));
                    }
                }
                None => {
                    self.told_store.insert(i, r.view.clone());
                }
            },
        }
        self.told.entry(i).or_default().push(ch);
    }

    /// The one meaning of an [`Op`]. A function named that the module does
    /// not have is a script for another module, and an `Err`.
    pub fn run(&mut self, op: &Op) -> Result<(), String> {
        match op {
            Op::Mutate {
                peer,
                function,
                eid,
                autos,
                args,
            } => {
                let fh = self
                    .bodies
                    .iter()
                    .find(|(_, c)| c.function.name == *function)
                    .map(|(h, _)| h.clone())
                    .ok_or_else(|| format!("no function {function}"))?;
                self.mutate(*peer, *eid, &fh, autos, args);
            }
            Op::Partition(i) => self.partition(*i),
            Op::Heal(i) => self.heal(*i),
            Op::Step => self.step(),
            Op::Settle => self.try_settle()?,
            Op::Restart => self.restart(false),
            Op::Wipe => self.restart(true),
            Op::Compact(n) => {
                let log = &self.server.authority.log;
                let n = (*n).clamp(log.horizon(), log.head_seq());
                self.server.authority.compact(n);
                self.write_log();
            }
            Op::Join { mode, closures, nobody } => self.join(*mode, *closures, *nobody),
            Op::SignIn(i) => {
                if self.nobody.remove(i) {
                    if let Some(c) = self.clients.get_mut(i) {
                        c.sign_in(&ctx_of(*i), Some(name(*i)));
                    }
                    self.keep(*i);
                    self.heal(*i);
                }
            }
            Op::Reopen(i) => self.reopen(*i),
            Op::Verify(i) => {
                if let Some(c) = self.clients.get_mut(i) {
                    c.verify_all();
                }
                self.flush_client(*i);
            }
        }
        Ok(())
    }

    // The server stops: every connection drops and what was in flight is
    // lost. It starts again from its journal — or, wiped, from nothing,
    // under a new name — and everyone reconnects.
    fn restart(&mut self, wipe: bool) {
        let peers: Vec<i64> = self.clients.keys().copied().collect();
        for i in &peers {
            self.partition(*i);
        }
        let sch = self.schema.clone();
        let log = if wipe {
            self.wiped = self.wiped.wrapping_add(1);
            let mut name = [0u8; 16];
            name[..8].copy_from_slice(&self.seed.to_be_bytes());
            name[14] = self.wiped;
            name[15] = 0xa0;
            let mut log = Log::empty(sch.clone());
            log.name_if_unnamed(name);
            if let Some(d) = &mut self.disk {
                match Journal::create(&mut d.keys, Layout::server(), &log) {
                    Ok(j) => d.journal = j,
                    Err(e) => self.faults.push(format!("wipe: {e}")),
                }
            }
            log
        } else {
            match &mut self.disk {
                None => self.server.authority.log.clone(),
                Some(d) => {
                    let opened = Journal::open(&mut d.keys, Layout::server(), &sch, |_, _, _| {})
                        .and_then(|(j, _)| journal::load(&d.keys, &Layout::server(), &sch).map(|l| (j, l)));
                    match opened {
                        Ok((j, Some(log))) => {
                            d.journal = j;
                            if log != self.server.authority.log {
                                self.faults.push(format!(
                                    "restart: the journal reads back as another log (head {} horizon {}, was head {} horizon {})",
                                    log.head_seq(),
                                    log.horizon(),
                                    self.server.authority.log.head_seq(),
                                    self.server.authority.log.horizon()
                                ));
                            }
                            log
                        }
                        Ok((_, None)) => {
                            self.faults.push("restart: the journal holds no log".into());
                            self.server.authority.log.clone()
                        }
                        Err(e) => {
                            self.faults.push(format!("restart: {e}"));
                            self.server.authority.log.clone()
                        }
                    }
                }
            }
        };
        let mut a = Authority::new(sch, self.bodies.clone());
        if self.native_server {
            a.hold(self.natives.clone());
        }
        match log.state_at(log.head_seq()) {
            Some(st) => a.store = st,
            None => self.faults.push(format!("restart: no state at the head {}", log.head_seq())),
        }
        a.log = log;
        self.server = Server::open(trusting(), open_access(), Silent, a);
        for i in &peers {
            if !self.nobody.contains(i) {
                self.heal(*i);
            }
        }
    }

    // A client at 0, connected unless nobody is using it.
    fn join(&mut self, mode: Mode, closures: bool, nobody: bool) {
        let i = self.clients.keys().next_back().map_or(0, |n| n + 1);
        let bodies = if closures { self.bodies.clone() } else { BTreeMap::new() };
        let r = Replica::open(self.schema.clone(), bodies, MemoryStore::empty(self.schema.clone()), 0, vec![]);
        let token = if nobody { None } else { Some(name(i)) };
        let mut c = Client::open(r, mode, token);
        if closures && i % 2 == 0 && !self.natives.is_empty() && !self.native_peers.is_empty() {
            c.hold(&self.natives);
            self.native_peers.insert(i);
        }
        self.clients.insert(i, c);
        if self.disk.is_some() {
            self.keep(i);
        }
        if nobody {
            self.nobody.insert(i);
        } else {
            self.heal(i);
        }
    }

    // A client stops — its connection and what was in flight to it lost —
    // and is opened again from its durable store, cursor and log, with its
    // pending intents, as a client process starting does.
    fn reopen(&mut self, i: i64) {
        let Some(k) = self.kept.get(&i).cloned() else { return };
        let linked = self.conn.contains_key(&i);
        self.partition(i);
        let Some(old) = self.clients.get(&i) else { return };
        if !same_rows(&k.store, &old.replica.confirmed) || k.cursor != old.replica.cursor {
            self.faults.push(format!(
                "reopen {i}: the durable store at {} is not the confirmed store at {}",
                k.cursor, old.replica.cursor
            ));
        }
        let mut r = Replica::open(
            old.schema.clone(),
            old.replica.bodies.clone(),
            k.store,
            k.cursor,
            old.replica.pending.clone(),
        );
        r.log_id = k.log_id;
        let mut c = Client::open(r, old.mode, old.token.clone());
        if self.native_peers.contains(&i) {
            c.hold(&self.natives);
        }
        self.clients.insert(i, c);
        self.keep(i);
        if linked {
            self.heal(i);
        }
    }

    /// After [`Sim::settle`], what a converged fleet is: every client at the
    /// server's head and hash, nothing in flight or pending, no replay that
    /// disagreed with the authority, every view its confirmed store, every
    /// durable copy its replica's, and nothing a restart or a re-open found
    /// wrong. The first claim that does not hold, named.
    pub fn converged(&self) -> Result<(), String> {
        let (head, hash) = self.server_hash();
        for (i, c) in &self.clients {
            if self.nobody.contains(i) {
                continue;
            }
            let (n, h) = c.replica.verify_at();
            if (n, &h) != (head, &hash) {
                return Err(format!("client {i} at {n} {} and the server at {head} {}", hex(&h), hex(&hash)));
            }
            if !c.replica.diverged.is_empty() {
                return Err(format!("client {i} diverged at {:?}", c.replica.diverged));
            }
            if !same_rows(&c.replica.view, &c.replica.confirmed) {
                return Err(format!("client {i}: with nothing pending its view is not its confirmed store"));
            }
            if let Some(k) = self.kept.get(i) {
                if !same_rows(&k.store, &c.replica.confirmed) || k.cursor != c.replica.cursor {
                    return Err(format!("client {i}: its durable store is not its confirmed store"));
                }
            }
        }
        if !self.quiet() {
            return Err("something is in flight or pending after settle".into());
        }
        if let Some(f) = self.faults.first() {
            return Err(f.clone());
        }
        Ok(())
    }

    /// The authority's log replays: from its snapshot by the facts alone to
    /// the head state, and by running every retained intent again over the
    /// snapshot to the same facts.
    pub fn replays(&self) -> Result<(), String> {
        let a = &self.server.authority;
        let log = &a.log;
        let by_facts = log.state_at(log.head_seq()).ok_or("no state at the head")?;
        if state_hash(&by_facts) != state_hash(&a.store) {
            return Err(format!(
                "the log's facts reach {} and the authority holds {}",
                hex(&state_hash(&by_facts)),
                hex(&state_hash(&a.store))
            ));
        }
        let mut again = Authority::new(self.schema.clone(), a.bodies.clone());
        again.store = log.base.store.clone();
        again.log = Log {
            base: log.base.clone(),
            entries: BTreeMap::new(),
            ids: BTreeMap::new(),
        };
        for (n, (e, facts)) in &log.entries {
            match again.sequence_entry(e) {
                Sequenced::Appended(m, f) if m == *n && f == *facts => {}
                Sequenced::Appended(m, _) if m != *n => return Err(format!("entry {n} replays at {m}")),
                Sequenced::Appended(..) => return Err(format!("entry {n} replays to other facts")),
                other => return Err(format!("entry {n} replays as {other:?}")),
            }
        }
        if state_hash(&again.store) != state_hash(&a.store) {
            return Err("the intents replay to another state".into());
        }
        Ok(())
    }

    fn roll(&mut self) -> u64 {
        self.seed = lcg(self.seed);
        self.seed >> 11
    }
}

/// Whether two stores hold the same rows, a table never written and one
/// emptied being the same.
fn same_rows(a: &MemoryStore, b: &MemoryStore) -> bool {
    a.schema().tables().all(|t| a.scan(&t.name) == b.scan(&t.name))
}

impl Op {
    /// The op as a script writes it: `{"t": …}` and its fields, the
    /// spelling `rebase/fleet-*` vectors already use for theirs.
    pub fn value(&self) -> Value {
        let t = |s: &str| ("t", Value::text(s));
        let peer = |i: &i64| ("peer", Value::Int(*i));
        match self {
            Op::Mutate {
                peer: i,
                function,
                eid,
                autos,
                args,
            } => Value::record(vec![
                t("mutate"),
                peer(i),
                ("function", Value::text(function.clone())),
                ("eid", Value::Id(*eid)),
                ("autos", Value::Struct(autos.clone())),
                ("args", Value::Struct(args.clone())),
            ]),
            Op::Partition(i) => Value::record(vec![t("partition"), peer(i)]),
            Op::Heal(i) => Value::record(vec![t("heal"), peer(i)]),
            Op::Step => Value::record(vec![t("step")]),
            Op::Settle => Value::record(vec![t("settle")]),
            Op::Restart => Value::record(vec![t("restart")]),
            Op::Wipe => Value::record(vec![t("wipe")]),
            Op::Compact(n) => Value::record(vec![t("compact"), ("seq", Value::Int(*n))]),
            Op::Join { mode, closures, nobody } => Value::record(vec![
                t("join"),
                ("mode", Value::text(if *mode == Mode::Whole { "whole" } else { "facts" })),
                ("closures", Value::Bool(*closures)),
                ("nobody", Value::Bool(*nobody)),
            ]),
            Op::SignIn(i) => Value::record(vec![t("sign_in"), peer(i)]),
            Op::Reopen(i) => Value::record(vec![t("reopen"), peer(i)]),
            Op::Verify(i) => Value::record(vec![t("verify"), peer(i)]),
        }
    }

    /// An op read back; `Err` names what is not one.
    pub fn from_value(v: &Value) -> Result<Op, String> {
        let Value::Struct(m) = v else { return Err(format!("not an op: {v:?}")) };
        let get = |k: &str| m.get(k).ok_or_else(|| format!("an op without {k}: {v:?}"));
        let int = |k: &str| match get(k)? {
            Value::Int(n) => Ok(*n),
            other => Err(format!("{k} is not an int: {other:?}")),
        };
        let flag = |k: &str| match get(k)? {
            Value::Bool(b) => Ok(*b),
            other => Err(format!("{k} is not a bool: {other:?}")),
        };
        let args = |k: &str| match get(k)? {
            Value::Struct(a) => Ok(a.clone()),
            other => Err(format!("{k} is not a struct: {other:?}")),
        };
        let Value::Text(t) = get("t")? else {
            return Err(format!("an op whose t is not text: {v:?}"));
        };
        Ok(match t.as_str() {
            "mutate" => Op::Mutate {
                peer: int("peer")?,
                function: match get("function")? {
                    Value::Text(f) => f.clone(),
                    other => return Err(format!("function is not text: {other:?}")),
                },
                eid: match get("eid")? {
                    Value::Id(i) => *i,
                    other => return Err(format!("eid is not an id: {other:?}")),
                },
                autos: args("autos")?,
                args: args("args")?,
            },
            "partition" => Op::Partition(int("peer")?),
            "heal" => Op::Heal(int("peer")?),
            "step" => Op::Step,
            "settle" => Op::Settle,
            "restart" => Op::Restart,
            "wipe" => Op::Wipe,
            "compact" => Op::Compact(int("seq")?),
            "join" => Op::Join {
                mode: match get("mode")? {
                    Value::Text(m) if m == "whole" => Mode::Whole,
                    Value::Text(m) if m == "facts" => Mode::ByFacts,
                    other => return Err(format!("mode is neither whole nor facts: {other:?}")),
                },
                closures: flag("closures")?,
                nobody: flag("nobody")?,
            },
            "sign_in" => Op::SignIn(int("peer")?),
            "reopen" => Op::Reopen(int("peer")?),
            "verify" => Op::Verify(int("peer")?),
            other => return Err(format!("unknown op {other}")),
        })
    }
}

/// A scripted session run from the beginning (`rebase/fleet-fuzz-*`): a
/// fleet of `clients` over the module's closures — every peer interprets —
/// durable, then every op, then a settle; the fleet must have converged
/// ([`Sim::converged`]) and its log must replay ([`Sim::replays`]).
pub fn run_script(sch: Schema, bodies: BTreeMap<FnHash, Closure>, clients: i64, seed: u64, script: &[Value]) -> Result<Sim, String> {
    let mut sim = Sim::new(sch, bodies, clients, seed).durable();
    for (k, v) in script.iter().enumerate() {
        let op = Op::from_value(v)?;
        sim.run(&op).map_err(|e| format!("op {k}: {e}"))?;
        if let Some(f) = sim.faults.first() {
            return Err(f.clone());
        }
    }
    sim.try_settle()?;
    sim.converged()?;
    sim.replays()?;
    Ok(sim)
}

/// Knuth's MMIX constants: the whole of the simulation's randomness.
pub fn lcg(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}
