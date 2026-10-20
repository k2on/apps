//! The hub: one authority per space (§1.7, §1.9), and the connections it
//! serves, routed — sans-io like the machines, so the simulator drives the
//! same hub a socket does.
//!
//! **Routing.** A frame from a connection goes to the machine of its space
//! (`Fetch` and `Bodies` to the log's) with the connection's verified
//! identity. What the machine says goes: `Send` back to that connection;
//! `Fanout` to every connection fed the log whole, or, for the volatile
//! space, every connection that said `Hello` in it; `To` to the connections of that session; and
//! `Persist` to the disk **first** — nothing a step says is delivered until
//! its records are written, and while a write fails every delivery waits
//! with it, in order, for a write that succeeds ([`Hub::failure`]).
//!
//! **Feeds.** Each connection is fed as the app says of its login
//! (`Config::feed`): whole, or by a scope (§1.8), told to the authority by
//! `Input::Feed` before the connection's first frame.
//!
//! **Devices** (§1.9). A connection's volatile `Hello` may say what its
//! device is; the app turns that and the login into the input of its
//! `on_join` (`Live::device`), and the hub authors `on_join` when a
//! session's first such connection says it and `on_leave` when its last
//! closes, so a device on two connections is one device. On a restart the
//! volatile space is what the hub wrote of it when it last closed
//! (`live`), and `on_restore` is authored over it.
//!
//! **The journal** beside the log keeps what the log cannot: each session's
//! last `local` and the intents refused, written when either a refusal or
//! the horizon moves, so that after a restart a push past a refusal is not
//! taken for a gap, a refused intent pushed again is answered again, and
//! one sequenced below the horizon is acknowledged there. **The horizon** moves as `keep` says: the log keeps
//! that many entries above its snapshot.

use std::collections::BTreeMap;
use std::sync::Arc;

use ark5::frames::Frame;
use ark5::hash::FnHash;
use ark5::ir::Node;
use ark5::log::{Durable, EntryId, Snapshot};
use ark5::peer::{Feed, Input, Journal, Output, Peer, Role, Verdict, Who};
use ark5::schema::Space;
use ark5_client::{Disk, Module};

pub type ConnId = u64;

/// A frame for a connection.
pub type Deliver = (ConnId, Vec<u8>);

/// What `on_join` and `on_leave` are authored with: from who a connection
/// is and what its device said, the function's input, or none.
pub type Device = Arc<dyn Fn(&Who, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

#[derive(Clone)]
pub struct Live {
    pub module: Module,
    pub on_join: Option<FnHash>,
    pub on_leave: Option<FnHash>,
    pub on_restore: Option<FnHash>,
    pub device: Device,
    /// Volatile entries kept above the horizon.
    pub keep: usize,
}

#[derive(Clone)]
pub struct Config {
    pub log: Module,
    pub live: Option<Live>,
    pub feed: Arc<dyn Fn(&Who) -> Feed + Send + Sync>,
    /// Logged entries kept above the horizon; all of them when none.
    pub keep: Option<usize>,
    /// Who the authorities author as.
    pub me: Who,
}

impl Config {
    pub fn new(log: Module) -> Config {
        Config {
            log,
            live: None,
            feed: Arc::new(|_| Feed::Whole),
            keep: None,
            me: Who {
                actor: "authority".into(),
                session: "authority".into(),
                roles: vec![],
            },
        }
    }
}

struct Conn {
    who: Who,
    hello: [bool; 2],
    whole: bool,
    device: Option<Vec<u8>>,
}

pub struct Hub {
    pub config: Config,
    pub log: Peer,
    pub live: Option<Peer>,
    pub disk: Box<dyn Disk>,
    conns: BTreeMap<ConnId, Conn>,
    devices: BTreeMap<String, (usize, Vec<u8>)>,
    next: ConnId,
    /// Records not yet written, and what waits for them.
    unwritten: Vec<Vec<u8>>,
    held: Vec<Deliver>,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
    /// The refusals and the horizon the journal was last written at.
    refused: (usize, i64),
}

fn ix(s: Space) -> usize {
    s as usize
}

fn authority(m: &Module, me: &Who, space: Space) -> Peer {
    Peer::new(
        m.schema.clone(),
        m.cas.clone(),
        Role::Authority { journal: Journal::default() },
        me.clone(),
    )
    .in_space(space)
}

fn journal(p: &mut Peer) -> &mut Journal {
    match &mut p.role {
        Role::Authority { journal } => journal,
        Role::Replica => unreachable!("a hub's machines are authorities"),
    }
}

impl Hub {
    /// The hub opened from what `disk` holds.
    pub fn open(config: Config, mut disk: Box<dyn Disk>, now: i64) -> Hub {
        let mut log = authority(&config.log, &config.me, Space::Durable);
        let stored = disk.load().unwrap_or_default();
        let durable = Durable {
            snapshot: stored.snapshot.unwrap_or_else(|| log.log.base.clone()),
            chain: stored.chain,
            pending: vec![],
            local: 0,
        };
        log.step(Input::Open(durable));
        log.local = log
            .log
            .entries
            .iter()
            .filter(|l| l.entry.session == config.me.session)
            .map(|l| l.entry.local)
            .max()
            .unwrap_or(0);
        type Kept = (Vec<(String, i64)>, Vec<(EntryId, String)>);
        if let Some(Ok((last, refused))) = disk.get("journal").ok().flatten().map(|b| Kept::from_bytes(&b)) {
            let j = journal(&mut log);
            for (s, n) in last {
                let l = j.last.entry(s).or_insert(0);
                *l = (*l).max(n);
            }
            j.verdicts.extend(refused.into_iter().map(|(id, why)| (id, Verdict::Refused(why))));
        }
        log.local = log.local.max(journal(&mut log).last.get(&config.me.session).copied().unwrap_or(0));
        let live = config.live.as_ref().map(|l| authority(&l.module, &config.me, Space::Volatile));
        let mut hub = Hub {
            config,
            log,
            live,
            disk,
            conns: BTreeMap::new(),
            devices: BTreeMap::new(),
            next: 1,
            unwritten: vec![],
            held: vec![],
            failure: None,
            refused: (0, 0),
        };
        let kept = hub.disk.get("live").ok().flatten().and_then(|b| Snapshot::from_bytes(&b).ok());
        if let (Some(snapshot), Some(live)) = (kept, &mut hub.live) {
            live.step(Input::Open(Durable {
                snapshot,
                chain: vec![],
                pending: vec![],
                local: 0,
            }));
            if let Some(f) = hub.config.live.as_ref().and_then(|l| l.on_restore) {
                let _ = hub.author(Space::Volatile, f, vec![], now);
            }
        }
        hub
    }

    /// Writes the volatile space as it stands, for the next open.
    pub fn close(&mut self) {
        if let Some(live) = &self.live {
            let (seq, hash) = live.log.head();
            let s = Snapshot {
                seq,
                hash,
                state: live.store.state_hash(),
                rows: live.store.snapshot_rows(),
            };
            let _ = self.disk.put("live", &s.to_bytes());
        }
    }

    /// A connection verified as `who`.
    pub fn connect(&mut self, who: Who) -> ConnId {
        let c = self.next;
        self.connect_as(c, who);
        c
    }

    /// A connection verified as `who`, numbered by the caller.
    pub fn connect_as(&mut self, c: ConnId, who: Who) {
        self.next = self.next.max(c + 1);
        let feed = (self.config.feed)(&who);
        if let Feed::Scoped(_) = feed {
            self.log.step(Input::Feed { who: who.clone(), feed });
        }
        let whole = feed == Feed::Whole;
        let conn = Conn {
            who,
            hello: [false; 2],
            whole,
            device: None,
        };
        self.conns.insert(c, conn);
    }

    pub fn who(&self, c: ConnId) -> Option<&Who> {
        self.conns.get(&c).map(|c| &c.who)
    }

    /// Every connection open, and who it is.
    pub fn connections(&self) -> impl Iterator<Item = (ConnId, &Who)> {
        self.conns.iter().map(|(c, conn)| (*c, &conn.who))
    }

    /// The connections of a session, to close when it is revoked.
    pub fn of_session(&self, session: &str) -> Vec<ConnId> {
        self.conns.iter().filter(|(_, c)| c.who.session == session).map(|(i, _)| *i).collect()
    }

    /// A frame from a connection: what it is answered with, and what everyone
    /// else is told of it.
    pub fn frame(&mut self, c: ConnId, bytes: Vec<u8>, now: i64) -> Vec<Deliver> {
        let Some(conn) = self.conns.get_mut(&c) else { return vec![] };
        let Ok(f) = Frame::dec(&bytes) else { return vec![] };
        let (space, device) = match &f {
            Frame::Hello { space, device, .. } => {
                conn.hello[ix(*space)] = true;
                (*space, device.clone())
            }
            Frame::Push { space, .. } | Frame::Join { space, .. } => (*space, None),
            _ => (Space::Durable, None),
        };
        let from = Some(conn.who.clone());
        let mut out = vec![];
        self.step(space, Some(c), Input::Frame { from, bytes }, &mut out);
        if let Some(d) = device.filter(|_| space == Space::Volatile) {
            self.arrive(c, &d, now, &mut out);
        }
        self.release(out)
    }

    /// A connection gone; the device's `on_leave` when it was its last.
    pub fn disconnect(&mut self, c: ConnId, now: i64) -> Vec<Deliver> {
        let mut out = vec![];
        if let Some(Conn { who, device: Some(_), .. }) = self.conns.remove(&c) {
            let (n, input) = self.devices.get_mut(&who.session).expect("a device counted");
            *n -= 1;
            if *n == 0 {
                let input = input.clone();
                self.devices.remove(&who.session);
                if let Some(f) = self.config.live.as_ref().and_then(|l| l.on_leave) {
                    let _ = self.mutate(Space::Volatile, f, input, now, &mut out);
                }
            }
        }
        self.release(out)
    }

    fn arrive(&mut self, c: ConnId, said: &[u8], now: i64, out: &mut Vec<Deliver>) {
        let (Some(live), Some(conn)) = (self.config.live.clone(), self.conns.get_mut(&c)) else {
            return;
        };
        let Some(input) = conn.device.is_none().then(|| (live.device)(&conn.who, said)).flatten() else {
            return;
        };
        conn.device = Some(input.clone());
        let n = &mut self.devices.entry(conn.who.session.clone()).or_insert((0, input.clone())).0;
        *n += 1;
        if let (1, Some(f)) = (*n, live.on_join) {
            let _ = self.mutate(Space::Volatile, f, input, now, out);
        }
    }

    /// An entry the authority of `space` authors itself — the server half of
    /// a mutator, a raw write, a device's arrival — and its verdict.
    pub fn author(&mut self, space: Space, func: FnHash, input: Vec<u8>, now: i64) -> (Result<(), String>, Vec<Deliver>) {
        let mut out = vec![];
        let r = self.mutate(space, func, input, now, &mut out);
        (r, self.release(out))
    }

    fn mutate(&mut self, space: Space, func: FnHash, input: Vec<u8>, now: i64, out: &mut Vec<Deliver>) -> Result<(), String> {
        let said = self.step(space, None, Input::Mutate { func, input, now }, out);
        said.into_iter()
            .find_map(|o| match o {
                Output::Refused { why, .. } => Some(Err(why)),
                Output::Confirmed(_) => Some(Ok(())),
                _ => None,
            })
            .unwrap_or_else(|| Err("no verdict".into()))
    }

    /// One input to one machine, its outputs routed into `out`.
    fn step(&mut self, space: Space, from: Option<ConnId>, input: Input, out: &mut Vec<Deliver>) -> Vec<Output> {
        let Some(peer) = (match space {
            Space::Durable => Some(&mut self.log),
            Space::Volatile => self.live.as_mut(),
        }) else {
            return vec![];
        };
        let said = peer.step(input);
        for o in &said {
            match o {
                Output::Send(b) => out.extend(from.map(|c| (c, b.clone()))),
                Output::Fanout(b) => {
                    let to = self
                        .conns
                        .iter()
                        .filter(|(_, c)| if space == Space::Durable { c.whole } else { c.hello[1] });
                    out.extend(to.map(|(i, _)| (*i, b.clone())));
                }
                Output::To { session, bytes } => {
                    let to = self.conns.iter().filter(|(_, c)| c.hello[0] && c.who.session == *session);
                    out.extend(to.map(|(i, _)| (*i, bytes.clone())));
                }
                Output::Persist(b) => self.unwritten.push(b.clone()),
                _ => {}
            }
        }
        self.horizon(space);
        let refused = journal(&mut self.log)
            .verdicts
            .values()
            .filter(|v| matches!(v, Verdict::Refused(_)))
            .count();
        let mark = (refused, self.log.log.base.seq);
        if space == Space::Durable && mark != self.refused {
            self.refused = mark;
            let j = journal(&mut self.log);
            let last: Vec<(String, i64)> = j.last.clone().into_iter().collect();
            let why = |v: &Verdict| match v {
                Verdict::Refused(w) => Some(w.clone()),
                Verdict::Seq(_) => None,
            };
            let refused: Vec<(EntryId, String)> = j.verdicts.iter().filter_map(|(id, v)| Some((*id, why(v)?))).collect();
            if let Err(e) = self.disk.put("journal", &(last, refused).to_bytes()) {
                self.failure = Some(e.to_string());
            }
        }
        said
    }

    fn horizon(&mut self, space: Space) {
        let (keep, peer) = match space {
            Space::Durable => (self.config.keep, &mut self.log),
            Space::Volatile => (self.config.live.as_ref().map(|l| l.keep), self.live.as_mut().expect("live")),
        };
        let (head, base) = (peer.log.head().0, peer.log.base.seq);
        if let Some(k) = keep.filter(|k| head - base > 2 * *k as i64) {
            for o in peer.step(Input::Compact(head - k as i64)) {
                if let Output::Persist(b) = o {
                    self.unwritten.push(b);
                }
            }
        }
    }

    /// Writes what is unwritten; then everything waiting is delivered, or,
    /// while a write fails, waits on with what comes after it.
    fn release(&mut self, out: Vec<Deliver>) -> Vec<Deliver> {
        self.held.extend(out);
        while let Some(r) = self.unwritten.first() {
            if let Err(e) = self.disk.record(r) {
                self.failure = Some(format!("the log cannot be written: {e}"));
                return vec![];
            }
            self.unwritten.remove(0);
        }
        self.failure = None;
        std::mem::take(&mut self.held)
    }

    /// Whether the log's records are all written and nothing waits.
    pub fn flushed(&mut self) -> Vec<Deliver> {
        self.release(vec![])
    }
}
