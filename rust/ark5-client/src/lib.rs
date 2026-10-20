//! The ArkDB v5 client: a driver of the one peer machine (§1.7), twice —
//! the log's and, when the module has live state, the volatile space's
//! (§1.9) — over one connection.
//!
//! A [`Client`] is sans-io like the machines it drives: a UI or a test
//! feeds it what happens (`mutate`, `call`, a frame received, a connection
//! up or down, the clock) and it does three things with what the machines
//! say. A frame goes out while the client is linked and is dropped while it
//! is not. What the log's machine makes durable goes to its [`Disk`]
//! (`storage`), and so do its pending intents whenever they change; a
//! client opens from what its disk holds. Every change a machine reports
//! is pushed into the maintained views watching its space (§1.10), whose
//! patches, verdicts and confirmations are the client's [`Event`]s.
//!
//! **The relay** of ephemeral calls is the driver's policy, as §1.9 says:
//! a call to a function the module names ephemeral goes to the volatile
//! machine at most once every `every_ms`, coalesced to the newest call per
//! function; the rest go at once.
//!
//! **The origin** (§1.6) is sixteen bytes the client draws at its first
//! open and keeps on its disk (`origin`); every intent's id is its origin's
//! and its `local`'s, whoever is signed in. **Sign-in** gives the client
//! another login: its pending intents are the signer's from then on, their
//! ids and the rows they drew kept. A login of another actor than the one
//! the client authored as gets an origin of its own, named by the old one
//! and its session, and the pending intents are authored again from its
//! first `local` — an authority binds an origin to the actor whose intents
//! first came from it. [`link`] keeps a connection up and
//! [`Client::pump`] joins the two; [`auth`] signs in through `ark-auth`.
//!
//! Every input either machine is given is recorded, when asked
//! (`record`), as an [`Act`]: a client is a pure function of these, which
//! the trace player (`ark5-dev`) holds it to.

pub mod auth;
pub mod link;
pub mod storage;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ark5::frames::Frame;
use ark5::hash::{EntryHash, FnHash};
use ark5::ir::Node;
use ark5::log::{Durable, Entry, EntryId, Logged, Origin, Seq};
use ark5::module::Lib;
use ark5::peer::{Cas, Ctx, Input, Journal, Output, Peer, Role, Who};
use ark5::schema::{Schema, Space};
use ark5::store::Change;
use ark5::view::{Patch, View};
use ark5::Refusal;

pub use storage::{Disk, Files, Memory};

/// One space's module: its schema, the functions a peer runs, and the
/// library its views are evaluated by.
#[derive(Clone)]
pub struct Module {
    pub schema: Arc<Schema>,
    pub cas: Cas,
    pub lib: Lib,
}

/// What a client is made of: the log's module, and the live one with the
/// device's declaration, the functions relayed as ephemeral and how often.
#[derive(Clone)]
pub struct Config {
    pub log: Module,
    pub live: Option<Live>,
}

#[derive(Clone)]
pub struct Live {
    pub module: Module,
    pub device: Option<Vec<u8>>,
    pub ephemeral: BTreeSet<FnHash>,
    pub every_ms: i64,
}

/// What a joined peer hands over again until the server answers it.
pub type Joining = (Seq, EntryHash, Vec<Logged>);

/// One input to one machine; or both machines opened afresh as a login,
/// as at [`Client::open`] — the log's alone from its fork when it was
/// alone, handing its chain over again when it had joined unanswered; or a
/// sign-in: the log's machine authoring as the login from its first
/// `local`, the live one afresh.
#[derive(Clone, Debug)]
pub enum Act {
    Open(Who, Origin, Option<(Seq, EntryHash)>, Option<Joining>),
    SignIn(Who, Origin),
    Step(Space, Input),
}

/// Where a machine stands after a step: its head, its state, its pending
/// intents — what a trace holds a replay to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct At {
    pub head: (ark5::log::Seq, ark5::hash::EntryHash),
    pub state: ark5::hash::StateHash,
    pub pending: Vec<EntryId>,
}

impl At {
    pub fn of(p: &Peer) -> At {
        At {
            head: p.log.head(),
            state: p.store.state_hash(),
            pending: p.pending.iter().map(|x| x.entry.id).collect(),
        }
    }
}

/// One recorded act, what it said, and where its machine stood after.
pub type Recorded = (Act, Vec<Output>, Option<At>);

/// What a UI is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Refused {
        space: Space,
        id: EntryId,
        why: String,
    },
    Confirmed {
        space: Space,
        id: EntryId,
    },
    Patches {
        view: usize,
        patches: Vec<Patch>,
    },
    /// A view that could not be kept up and is held no more.
    Dropped {
        view: usize,
        why: String,
    },
}

struct Watch {
    space: Space,
    view: Option<View>,
}

pub struct Client {
    pub config: Config,
    pub log: Peer,
    pub live: Option<Peer>,
    pub disk: Box<dyn Disk>,
    pub linked: bool,
    /// Every input given, when recording.
    pub record: Option<Vec<Recorded>>,
    /// The last write the disk refused.
    pub disk_error: Option<String>,
    views: Vec<Watch>,
    relay: BTreeMap<FnHash, (Vec<u8>, i64)>,
    relayed: Option<i64>,
    frames: Vec<Vec<u8>>,
    events: Vec<Event>,
    saved: (i64, Vec<EntryId>),
    /// How much the `pending` journal holds, in entries and records; `None`
    /// until it is written whole, as it is first after an open.
    journal: Option<usize>,
    joined: bool,
}

/// The role of a device alone since `fork`.
pub fn alone_at(fork: Option<(Seq, EntryHash)>) -> Role {
    Role::Authority {
        journal: Journal { fork, ..Journal::default() },
    }
}

fn fresh(m: &Module, me: &Who, origin: Origin, space: Space) -> Peer {
    let mut p = Peer::new(m.schema.clone(), m.cas.clone(), Role::Replica, me.clone()).in_space(space);
    p.origin = origin;
    p
}

/// Sixteen bytes no other replica draws: the process's hasher keys (which
/// the platform seeds), the clock and the process, through sha256.
pub fn draw_origin() -> Origin {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut b = nanos.to_be_bytes().to_vec();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        b.extend(h.finish().to_be_bytes());
    }
    b.extend(std::process::id().to_be_bytes());
    Origin::named(b)
}

impl Client {
    /// A client opened as `me` from what `disk` holds.
    pub fn open(config: Config, me: Who, disk: Box<dyn Disk>, record: bool) -> Client {
        Client::opened(config, me, disk, record.then(Vec::new))
    }

    fn opened(config: Config, me: Who, mut disk: Box<dyn Disk>, mut record: Option<Vec<Recorded>>) -> Client {
        let fork = disk.get("fork").ok().flatten().and_then(|b| <(Seq, EntryHash)>::from_bytes(&b).ok());
        let joining = disk
            .get("joining")
            .ok()
            .flatten()
            .and_then(|b| <((Seq, EntryHash), Vec<Logged>)>::from_bytes(&b).ok());
        let joining = joining.map(|((n, h), c)| (n, h, c));
        let origin = match disk.get("origin").ok().flatten().and_then(|b| Origin::from_bytes(&b).ok()) {
            Some(o) => o,
            None => {
                let o = draw_origin();
                let _ = disk.put("origin", &o.to_bytes());
                o
            }
        };
        if let Some(r) = &mut record {
            r.push((Act::Open(me.clone(), origin, fork, joining.clone()), vec![], None));
        }
        let mut log = fresh(&config.log, &me, origin, Space::Durable);
        if fork.is_some() {
            log.role = alone_at(fork);
        }
        log.joining = joining;
        let live = config.live.as_ref().map(|l| {
            let mut p = fresh(&l.module, &me, origin, Space::Volatile);
            p.device = l.device.clone();
            p
        });
        let read = disk.load().and_then(|s| Ok((s, disk.get("pending")?)));
        let ((stored, pending), error) = match read {
            Ok(s) => (s, None),
            Err(e) => (Default::default(), Some(e.to_string())),
        };
        let (local, pending) = pending.map(|b| storage::pending(&b)).unwrap_or_default();
        let durable = Durable {
            snapshot: stored.snapshot.unwrap_or_else(|| log.log.base.clone()),
            chain: stored.chain,
            pending,
            local,
        };
        log.local = 0;
        let mut c = Client {
            config,
            log,
            live,
            disk,
            linked: false,
            record,
            disk_error: error,
            views: vec![],
            relay: BTreeMap::new(),
            relayed: None,
            frames: vec![],
            events: vec![],
            saved: (0, vec![]),
            journal: None,
            joined: false,
        };
        c.joined = c.log.joining.is_some();
        c.step(Space::Durable, Input::Open(durable));
        c
    }

    /// This client stopped and opened again from its disk, unlinked.
    pub fn reopen(self) -> Client {
        let me = self.log.me.clone();
        Client::opened(self.config, me, self.disk, self.record)
    }

    pub fn peer(&self, space: Space) -> Option<&Peer> {
        match space {
            Space::Durable => Some(&self.log),
            Space::Volatile => self.live.as_ref(),
        }
    }

    /// One input to one machine, and what it says, routed.
    pub fn step(&mut self, space: Space, input: Input) {
        let rec = self.record.is_some().then(|| input.clone());
        let Some(peer) = (match space {
            Space::Durable => Some(&mut self.log),
            Space::Volatile => self.live.as_mut(),
        }) else {
            return;
        };
        let out = peer.step(input);
        for o in &out {
            match o {
                Output::Send(b) if self.linked => self.frames.push(b.clone()),
                Output::Persist(b) => self.disk_error = self.disk.record(b).err().map(|e| e.to_string()).or(self.disk_error.take()),
                Output::Changes(cs) => self.changed(space, cs),
                Output::Refused { id, why } => self.events.push(Event::Refused {
                    space,
                    id: *id,
                    why: why.clone(),
                }),
                Output::Confirmed(id) => self.events.push(Event::Confirmed { space, id: *id }),
                _ => {}
            }
        }
        if let Some(i) = rec {
            let at = At::of(self.peer(space).expect("a machine that spoke"));
            self.record.as_mut().expect("recording").push((Act::Step(space, i), out, Some(at)));
        }
        if space == Space::Durable && self.log.joining.is_some() != self.joined {
            self.joined = self.log.joining.is_some();
            let b = self.log.joining.clone().map_or(vec![], |(n, h, c)| ((n, h), c).to_bytes());
            self.blob("joining", &b);
        }
        let now = (self.log.local, self.log.pending.iter().map(|p| p.entry.id).collect());
        if space == Space::Durable && now != self.saved {
            self.save_pending(now);
        }
    }

    /// The pending intents made durable as a change to what was saved: one
    /// record appended — the `local`, how many left the front, the intents
    /// new at the back — so a mutate writes its one entry; written whole
    /// when the journal holds more than twice what is pending.
    fn save_pending(&mut self, now: (i64, Vec<EntryId>)) {
        let (was, ids) = (&self.saved.1, &now.1);
        let gone = (0..=was.len()).find(|d| ids.starts_with(&was[*d..])).unwrap_or(was.len());
        let kept = was.len() - gone;
        let held = self.journal.map(|j| j + ids.len() - kept + 1);
        let (record, whole) = match held {
            Some(j) if j <= 2 * (ids.len() + 1) + 64 => {
                let new: Vec<Entry> = self.log.pending[kept..].iter().map(|p| p.entry.clone()).collect();
                self.journal = Some(j);
                (storage::pending_record(now.0, gone, new), false)
            }
            _ => {
                let all: Vec<Entry> = self.log.pending.iter().map(|p| p.entry.clone()).collect();
                self.journal = Some(ids.len() + 1);
                (storage::pending_record(now.0, 0, all), true)
            }
        };
        let wrote = if whole {
            self.disk.put("pending", &record)
        } else {
            self.disk.append("pending", &record)
        };
        if let Err(e) = wrote {
            self.disk_error = Some(e.to_string());
            self.journal = None;
        }
        self.saved = now;
    }

    fn changed(&mut self, space: Space, cs: &[Change]) {
        let store = match space {
            Space::Durable => &self.log.store,
            Space::Volatile => &self.live.as_ref().expect("a machine that spoke").store,
        };
        for (i, w) in self.views.iter_mut().enumerate().filter(|(_, w)| w.space == space) {
            let Some(v) = &mut w.view else { continue };
            match v.push(cs, store) {
                Ok(patches) => self.events.push(Event::Patches { view: i, patches }),
                Err(why) => {
                    w.view = None;
                    self.events.push(Event::Dropped { view: i, why: why.0 });
                }
            }
        }
    }

    /// An intent on the log.
    pub fn mutate(&mut self, func: FnHash, input: Vec<u8>, now: i64) {
        self.step(Space::Durable, Input::Mutate { func, input, now });
    }

    /// A live call: an ephemeral one through the relay, any other at once.
    pub fn call(&mut self, func: FnHash, input: Vec<u8>, now: i64) {
        let ephemeral = self.config.live.as_ref().is_some_and(|l| l.ephemeral.contains(&func));
        if !ephemeral {
            return self.step(Space::Volatile, Input::Mutate { func, input, now });
        }
        self.relay.insert(func, (input, now));
        self.tick(now);
    }

    /// The clock: the relay goes when its interval has passed.
    pub fn tick(&mut self, now: i64) {
        let every = self.config.live.as_ref().map_or(0, |l| l.every_ms);
        if self.relay.is_empty() || self.relayed.is_some_and(|t| now - t < every) {
            return;
        }
        self.relayed = Some(now);
        for (func, (input, now)) in std::mem::take(&mut self.relay) {
            self.step(Space::Volatile, Input::Mutate { func, input, now });
        }
    }

    pub fn connected(&mut self) {
        self.linked = true;
        self.frames.clear();
        self.step(Space::Durable, Input::Connected);
        self.step(Space::Volatile, Input::Connected);
    }

    pub fn disconnected(&mut self) {
        self.linked = false;
        self.frames.clear();
    }

    /// A frame from the hub, to the machine of its space.
    pub fn receive(&mut self, bytes: Vec<u8>) {
        let space = Frame::dec(&bytes).map_or(Space::Durable, |f| f.space());
        self.step(space, Input::Frame { from: None, bytes });
    }

    /// Alone, its fork kept on its disk so that a reopen is alone still.
    pub fn alone(&mut self) {
        self.step(Space::Durable, Input::Alone);
        if let Role::Authority {
            journal: Journal { fork: Some(f), .. },
        } = &self.log.role
        {
            let b = f.to_bytes();
            self.blob("fork", &b);
        }
    }

    pub fn join(&mut self) {
        self.step(Space::Durable, Input::Join);
        self.blob("fork", &[]);
    }

    fn blob(&mut self, name: &str, b: &[u8]) {
        if let Err(e) = self.disk.put(name, b) {
            self.disk_error = Some(e.to_string());
        }
    }

    /// Another login: every pending intent the signer's, its id kept — or,
    /// for another actor than the one this client authored as, authored
    /// again from a new origin's first `local` — and the live machine
    /// started over. A chain handed over as another actor is not handed
    /// over again: its facts are not the signer's, and its intents are
    /// pushed as the signer's instead.
    pub fn sign_in(&mut self, who: Who) {
        let was = &self.log.me;
        let other = !was.actor.is_empty() && was.actor != who.actor;
        if (&was.actor, &was.roles) != (&who.actor, &who.roles) {
            self.log.joining = None;
        }
        if other {
            self.log.origin = Origin::named([&self.log.origin.0[..], who.session.as_bytes()].concat());
            self.log.local = 0;
            let _ = self.disk.put("origin", &self.log.origin.to_bytes());
        }
        let origin = self.log.origin;
        let mut pending: Vec<Entry> = self.log.pending.iter().map(|p| p.entry.clone()).collect();
        for (n, e) in pending.iter_mut().enumerate() {
            if other {
                (e.origin, e.local) = (origin, n as i64 + 1);
                e.id = EntryId::of(origin, e.local);
            }
            (e.actor, e.session, e.roles) = (who.actor.clone(), who.session.clone(), who.roles.clone());
        }
        if let Some(r) = &mut self.record {
            r.push((Act::SignIn(who.clone(), origin), vec![], None));
        }
        self.log.me = who.clone();
        if let Some(l) = &self.config.live {
            let mut p = fresh(&l.module, &who, origin, Space::Volatile);
            p.device = l.device.clone();
            // The origin goes on counting in the live space too, unless new.
            p.local = self.live.as_ref().filter(|_| !other).map_or(0, |x| x.local);
            self.live = Some(p);
        }
        let durable = Durable {
            snapshot: self.log.log.base.clone(),
            chain: self.log.log.entries.clone(),
            local: if other { pending.len() as i64 } else { self.log.local },
            pending,
        };
        self.step(Space::Durable, Input::Open(durable));
        if self.linked {
            self.connected();
        }
    }

    /// A maintained view of query `f` on `input` over `space`'s optimistic
    /// state, as `ctx` reads it; its patches come as events.
    pub fn watch(&mut self, space: Space, f: &FnHash, input: &[u8], ctx: Ctx) -> Result<usize, Refusal> {
        let lib = match space {
            Space::Durable => &self.config.log.lib,
            Space::Volatile => &self.config.live.as_ref().ok_or_else(|| Refusal::new("no live module"))?.module.lib,
        };
        let store = &self.peer(space).expect("a module of that space").store;
        let view = View::new(lib, f, input, ctx, store)?;
        self.views.push(Watch { space, view: Some(view) });
        Ok(self.views.len() - 1)
    }

    /// A view's answer as it stands.
    pub fn rows(&self, view: usize) -> Option<Vec<ark5::interp::V>> {
        self.views.get(view)?.view.as_ref().map(View::rows)
    }

    /// A view held no more: no change is pushed into it again, and its
    /// number is not given to another.
    pub fn unwatch(&mut self, view: usize) {
        if let Some(w) = self.views.get_mut(view) {
            w.view = None;
        }
    }

    pub fn take_frames(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.frames)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// What the link said, given to the client; the clock; and the frames
    /// the client has to send, sent.
    pub fn pump(&mut self, link: &link::Link, events: Vec<link::Event>, now: i64) {
        for e in events {
            match e {
                link::Event::Open => self.connected(),
                link::Event::Frame(b) => self.receive(b),
                link::Event::Closed(_) => self.disconnected(),
            }
        }
        self.tick(now);
        for f in self.take_frames() {
            link.send(f);
        }
    }
}
