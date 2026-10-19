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
//! function; the rest go at once. **Sign-in** gives the client another
//! login: its pending intents are authored again under the new session,
//! from that session's first `local`. [`link`] keeps a connection up and
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
use ark5::hash::FnHash;
use ark5::ir::Node;
use ark5::log::{Durable, Entry, EntryId};
use ark5::module::Lib;
use ark5::peer::{Cas, Ctx, Input, Output, Peer, Role, Who};
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

/// One input to one machine, or the login it authors as changing.
#[derive(Clone, Debug)]
pub enum Act {
    Step(Space, Input),
    Who(Space, Who),
}

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
    pub record: Option<Vec<(Act, Vec<Output>)>>,
    /// The last write the disk refused.
    pub disk_error: Option<String>,
    views: Vec<Watch>,
    relay: BTreeMap<FnHash, (Vec<u8>, i64)>,
    relayed: Option<i64>,
    frames: Vec<Vec<u8>>,
    events: Vec<Event>,
    saved: (i64, Vec<EntryId>),
}

fn fresh(m: &Module, me: &Who, space: Space) -> Peer {
    Peer::new(m.schema.clone(), m.cas.clone(), Role::Replica, me.clone()).in_space(space)
}

impl Client {
    /// A client opened as `me` from what `disk` holds.
    pub fn open(config: Config, me: Who, mut disk: Box<dyn Disk>, record: bool) -> Client {
        let mut log = fresh(&config.log, &me, Space::Durable);
        let live = config.live.as_ref().map(|l| {
            let mut p = fresh(&l.module, &me, Space::Volatile);
            p.device = l.device.clone();
            p
        });
        let read = disk.load().and_then(|s| Ok((s, disk.get("pending")?)));
        let ((stored, pending), error) = match read {
            Ok(s) => (s, None),
            Err(e) => (Default::default(), Some(e.to_string())),
        };
        let (local, pending) = pending.and_then(|b| <(i64, Vec<Entry>)>::from_bytes(&b).ok()).unwrap_or_default();
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
            record: record.then(Vec::new),
            disk_error: error,
            views: vec![],
            relay: BTreeMap::new(),
            relayed: None,
            frames: vec![],
            events: vec![],
            saved: (0, vec![]),
        };
        c.step(Space::Durable, Input::Open(durable));
        c
    }

    /// This client stopped and opened again from its disk, unlinked.
    pub fn reopen(self) -> Client {
        let me = self.log.me.clone();
        Client::open(self.config, me, self.disk, self.record.is_some())
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
        if let (Some(r), Some(i)) = (&mut self.record, rec) {
            r.push((Act::Step(space, i), out));
        }
        let now = (self.log.local, self.log.pending.iter().map(|p| p.entry.id).collect());
        if space == Space::Durable && now != self.saved {
            let entries: Vec<Entry> = self.log.pending.iter().map(|p| p.entry.clone()).collect();
            if let Err(e) = self.disk.put("pending", &(self.log.local, entries).to_bytes()) {
                self.disk_error = Some(e.to_string());
            }
            self.saved = now;
        }
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
        let space = match Frame::dec(&bytes) {
            Ok(Frame::Hello { space, .. } | Frame::Push { space, .. } | Frame::Join { space, .. }) => space,
            Ok(Frame::Ack { space, .. } | Frame::Reject { space, .. } | Frame::Held { space, .. }) => space,
            Ok(Frame::Batch { space, .. } | Frame::Snapshot { space, .. }) => space,
            _ => Space::Durable,
        };
        self.step(space, Input::Frame { from: None, bytes });
    }

    pub fn alone(&mut self) {
        self.step(Space::Durable, Input::Alone);
    }

    pub fn join(&mut self) {
        self.step(Space::Durable, Input::Join);
    }

    /// Another login: every pending intent authored again under it, from
    /// its session's first `local`, and the live machine started over.
    pub fn sign_in(&mut self, who: Who) {
        let mut pending: Vec<Entry> = self.log.pending.iter().map(|p| p.entry.clone()).collect();
        for (n, e) in pending.iter_mut().enumerate() {
            let local = n as i64 + 1;
            (e.id, e.actor, e.session, e.local, e.roles) = (
                EntryId::of(&who.session, local),
                who.actor.clone(),
                who.session.clone(),
                local,
                who.roles.clone(),
            );
        }
        for space in [Space::Durable, Space::Volatile] {
            if let Some(r) = &mut self.record {
                r.push((Act::Who(space, who.clone()), vec![]));
            }
        }
        self.log.me = who.clone();
        self.log.local = 0;
        if let Some(l) = &self.config.live {
            let mut p = fresh(&l.module, &who, Space::Volatile);
            p.device = l.device.clone();
            self.live = Some(p);
        }
        let durable = Durable {
            snapshot: self.log.log.base.clone(),
            chain: self.log.log.entries.clone(),
            local: pending.len() as i64,
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
