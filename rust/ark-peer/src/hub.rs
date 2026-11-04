//! The server role: a peer that is the authority of each space (§1.7,
//! §1.9) and the connections it serves, routed — sans-io like the
//! machines, so the simulator drives the same server a socket does.
//!
//! **Routing.** A frame from a connection goes to the machine of its space
//! with the connection's verified identity. What the machine says goes:
//! a frame `Send` back to that connection, to all — every connection fed
//! the log whole, or, for the volatile space, every connection that said
//! `Hello` in it — or to the connections of a session; and `Persist` to the
//! disk **first** — nothing a step says is delivered until its records are
//! written, and while a write fails every delivery waits with it, in
//! order, for a write that succeeds ([`Peer::failure`]).
//!
//! **Feeds.** Each connection is fed as the app says of its login
//! (`Config::feed`): whole, or by a scope (§1.8), which the authority asks
//! of the same function (`Functions::feed`) at the connection's `Hello`.
//!
//! **Devices** (§1.9). A connection's volatile `Hello` may say what its
//! device is; the app turns that and the login into the input of its
//! `on_join` (`Live::arrive`), and the server authors `on_join` when a
//! session's first such connection says it and `on_leave` when its last
//! closes, so a device on two connections is one device. On a restart the
//! volatile space is what the server wrote of it when it last closed
//! (its rows whole, one record beside its local instance: `local.volatile`,
//! and its journal, `local.live`), and `on_restore` is authored over it.
//!
//! **The journal** beside the log keeps what the log cannot, as one record
//! beside the peer's local instance (`local.journal`): each origin's last
//! `local` and actor and the intents refused, written whole when a refusal
//! or the horizon moves, so that after a restart a push past a refusal is not
//! taken for a gap, a refused intent pushed again is answered again, and
//! one sequenced below the horizon is acknowledged there. **The horizon**
//! moves as `keep` says: the log keeps that many entries above its
//! snapshot.

use std::collections::BTreeMap;

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::{Durable, EntryId, Origin, Record};
use ark::peer::{Input, Journal, Peer as Machine, Role, Who};
use ark::schema::Space;

use crate::{machine, Config, Disk, Fold, Instance, Peer};

/// What a server keeps beside its local instance, each one record on its
/// disk: its volatile space as it last closed, the store's rows whole, and
/// the live journal then; and its journal, each origin's last `local` and
/// actor and what it refused.
const VOLATILE: &str = "volatile";
const LIVE: &str = "live";
const JOURNAL: &str = "journal";

/// Each origin's last `local`, and the actor it is bound to.
type Lasts = BTreeMap<Origin, (String, i64)>;

/// A record kept beside `local`, read.
fn kept<T: Encode>(local: &mut Instance, name: &str) -> Option<T> {
    T::from_bytes(&local.disk.as_mut()?.get(name).ok()??).ok()
}

pub type ConnId = u64;

/// A frame for a connection.
pub type Deliver = (ConnId, Vec<u8>);

struct Conn {
    who: Who,
    hello: [bool; 2],
    whole: bool,
    device: Option<Vec<u8>>,
}

/// The connections a server serves.
#[derive(Default)]
pub(crate) struct Hub {
    pub serving: bool,
    conns: BTreeMap<ConnId, Conn>,
    devices: BTreeMap<String, (usize, Vec<u8>)>,
    next: ConnId,
    /// Deliveries waiting for what is unwritten.
    pub held: Vec<Deliver>,
    /// The refusals and the horizon the journal was last written at.
    mark: (usize, i64),
    /// What the log's entries changed since the hub's watch last looked,
    /// kept while one watches (`serve`).
    pub watched: bool,
    pub seen: Vec<ark::store::Change>,
    /// The calls connections made, not yet run.
    called: Vec<Called>,
}

/// A call of one of the server's functions ([`crate::Config::calls`]) a
/// connection made: who, verified as the connection is, and what.
#[derive(Clone, Debug)]
pub struct Called {
    pub conn: ConnId,
    pub who: Who,
    pub id: u64,
    pub name: String,
    pub input: Vec<u8>,
}

impl Hub {
    pub fn fanout(&mut self, space: Space, b: &[u8]) {
        let to = self
            .conns
            .iter()
            .filter(|(_, c)| if space == Space::Durable { c.whole } else { c.hello[1] });
        self.held.extend(to.map(|(i, _)| (*i, b.to_vec())).collect::<Vec<_>>());
    }

    pub fn to(&mut self, session: &str, b: &[u8]) {
        let to = self.conns.iter().filter(|(_, c)| c.hello[0] && c.who.session == session);
        self.held.extend(to.map(|(i, _)| (*i, b.to_vec())).collect::<Vec<_>>());
    }
}

fn journal(p: &mut Machine) -> &mut Journal {
    match &mut p.role {
        Role::Authority { journal } => journal,
        Role::Replica => unreachable!("a server's machines are authorities"),
    }
}

/// Who a server's authorities author as.
pub fn authority() -> Who {
    Who {
        actor: "authority".into(),
        session: "authority".into(),
        roles: vec![],
    }
}

impl Peer {
    /// A server opened from what `disk` holds: the authority of each space.
    pub fn serve(config: Config, disk: Box<dyn Disk>, now: i64) -> Peer {
        let files = disk.files();
        let (disk, mut local) = crate::local::beside(disk);
        let me = authority();
        let auth = || Role::Authority { journal: Journal::default() };
        let mut log = machine(&config.log, auth(), &me, None, Space::Durable);
        (log.disk, log.fold, log.functions.feed) = (Some(disk), Fold::keep(config.keep), Some(config.feed.clone()));
        let stored = log.stored();
        if let Some(e) = &log.failure {
            panic!("the log on disk does not read: {e}");
        }
        log.step(Input::Open(stored));
        let origin = log.origin;
        let mine = log.log().entries().into_iter().filter(|l| l.entry.origin == origin);
        log.local = mine.map(|l| l.entry.local).max().unwrap_or(0);
        let (last, refused): (Lasts, Vec<(EntryId, String)>) = kept(&mut local, JOURNAL).unwrap_or_default();
        let j = journal(&mut log);
        last.into_iter().for_each(|(origin, (actor, l))| j.pass(origin, &actor, l));
        j.refused.extend(refused);
        log.local = log.local.max(journal(&mut log).last_of(&origin));
        // The live authority draws an origin at every open, as a device's
        // live machine does: under the session's, the one the log authors
        // from, its first `local` and the log's had one id.
        let live = config.live.as_ref().map(|l| {
            let mut m = machine(&l.domain, auth(), &me, Some((l.origin)()), Space::Volatile);
            m.fold = Fold::keep(l.keep);
            m
        });
        let mut p = Peer::with(config, log, live, local);
        p.keep_files(files);
        (p.hub.serving, p.hub.next) = (true, 1);
        if let (Some(live), Some(j)) = (&mut p.live, kept::<Journal>(&mut p.local, LIVE)) {
            *journal(live) = j;
        }
        if let (Some(Record::Rows(rows)), Some(live)) = (kept(&mut p.local, VOLATILE), &mut p.live) {
            live.step(Input::Open(Durable { rows, ..Durable::default() }));
            if let Some(f) = p.config.live.as_ref().and_then(|l| l.on_restore.clone()) {
                let _ = p.author(Space::Volatile, &f, vec![], now);
            }
        }
        p.hub.mark = p.mark();
        p
    }

    /// Writes the volatile space as it stands, whole, for the next open:
    /// its rows, its log's and its journal, so that the live authority
    /// opened again goes on judging each origin from where it was.
    pub fn close(&mut self) {
        let Some(live) = &mut self.live else { return };
        let (seq, hash) = live.log().head();
        let rows = Record::Rows(ark::log::standing(live.store.snapshot_rows(), seq, hash));
        let j = journal(live).to_bytes();
        self.put_beside(VOLATILE, &rows.enc());
        self.put_beside(LIVE, &j);
    }

    /// A record kept beside the local instance, on its disk; a failure kept
    /// as its failure.
    fn put_beside(&mut self, name: &str, bytes: &[u8]) {
        if let Some(Err(e)) = self.local.disk.as_mut().map(|d| d.put(name, bytes)) {
            self.local.failure = Some(e.to_string());
        }
    }

    /// A connection verified as `who`.
    pub fn accept(&mut self, who: Who) -> ConnId {
        let c = self.hub.next;
        self.accept_as(c, who);
        c
    }

    /// A connection verified as `who`, numbered by the caller.
    pub fn accept_as(&mut self, c: ConnId, who: Who) {
        self.hub.next = self.hub.next.max(c + 1);
        let conn = Conn {
            whole: (self.config.feed)(&who).is_none(),
            who,
            hello: [false; 2],
            device: None,
        };
        self.hub.conns.insert(c, conn);
    }

    /// Every connection open, and who it is.
    pub fn connections(&self) -> impl Iterator<Item = (ConnId, &Who)> {
        self.hub.conns.iter().map(|(c, conn)| (*c, &conn.who))
    }

    /// Who a connection is.
    pub(crate) fn who(&self, c: ConnId) -> Option<Who> {
        self.hub.conns.get(&c).map(|conn| conn.who.clone())
    }

    /// The connections of a session, to close when it is revoked.
    pub fn of_session(&self, session: &str) -> Vec<ConnId> {
        self.connections().filter(|(_, w)| w.session == session).map(|(c, _)| c).collect()
    }

    /// A frame from a connection: what it is answered with, and what
    /// everyone else is told of it.
    pub fn frame(&mut self, c: ConnId, bytes: Vec<u8>, now: i64) -> Vec<Deliver> {
        let Some(conn) = self.hub.conns.get_mut(&c) else { return vec![] };
        let Ok(f) = Frame::dec(&bytes) else { return vec![] };
        if let Frame::Call { name, input, .. } = &f {
            if name == ark::frames::EVENTS {
                // What a device says of itself, kept with what this server
                // says, each named by the login that sent it: a device says
                // which of its peers, the connection whose.
                let (actor, session) = (conn.who.actor.clone(), conn.who.session.clone());
                let events = Vec::<ark::frames::Event>::from_bytes(input).unwrap_or_default();
                let events = events.into_iter().map(|mut e| {
                    e.fields.push(("from".into(), format!("{actor} \u{b7} {session}")));
                    e
                });
                crate::observe::collect(events.collect());
                return vec![];
            }
        }
        if let Frame::Call { id, name, input } = f {
            // No machine's: whoever drives the server runs it
            // ([`Peer::take_calls`]) and answers this connection alone.
            let who = conn.who.clone();
            self.hub.called.push(Called {
                conn: c,
                who,
                id,
                name,
                input,
            });
            return vec![];
        }
        if let Frame::Reply { .. } = f {
            return vec![];
        }
        if f.is_file() {
            return self.file_frame(c, f);
        }
        let (space, device) = match &f {
            Frame::Hello { space, device, .. } => {
                conn.hello[*space as usize] = true;
                (*space, device.clone())
            }
            f => (f.space(), None),
        };
        let from = Some(conn.who.clone());
        self.route(space, Some(c), Input::Frame { from, bytes });
        if let Some(d) = device.filter(|_| space == Space::Volatile) {
            self.arrive(c, &d, now);
        }
        self.release()
    }

    /// The calls connections made since, taken, each with the function
    /// it names, none when the server has no such function: for whoever
    /// drives the server to run and answer ([`Peer::reply`]).
    pub fn take_calls(&mut self) -> Vec<(Called, Option<crate::Call>)> {
        let called = std::mem::take(&mut self.hub.called);
        called
            .into_iter()
            .map(|c| {
                let f = self.config.calls.get(&c.name).cloned();
                (c, f)
            })
            .collect()
    }

    /// A call's answer, for the connection that made it if it is open still.
    pub fn reply(&mut self, c: ConnId, id: u64, answer: Result<Vec<u8>, String>) -> Vec<Deliver> {
        if !self.hub.conns.contains_key(&c) {
            return vec![];
        }
        let reply = match answer {
            Ok(output) => ark::frames::Reply::Ok { output },
            Err(why) => ark::frames::Reply::Refused { why },
        };
        vec![(c, Frame::Reply { id, reply }.enc())]
    }

    /// A connection gone; the device's `on_leave` when it was its last.
    pub fn leave(&mut self, c: ConnId, now: i64) -> Vec<Deliver> {
        self.files_left(c);
        if let Some(Conn { who, device: Some(_), .. }) = self.hub.conns.remove(&c) {
            let (n, input) = self.hub.devices.get_mut(&who.session).expect("a device counted");
            *n -= 1;
            if *n == 0 {
                let input = input.clone();
                self.hub.devices.remove(&who.session);
                if let Some(f) = self.config.live.as_ref().and_then(|l| l.on_leave.clone()) {
                    let _ = self.author(Space::Volatile, &f, input, now);
                }
            }
        }
        self.release()
    }

    fn arrive(&mut self, c: ConnId, said: &[u8], now: i64) {
        let (Some(live), Some(conn)) = (self.config.live.clone(), self.hub.conns.get_mut(&c)) else {
            return;
        };
        let Some(input) = conn.device.is_none().then(|| (live.arrive)(&conn.who, said)).flatten() else {
            return;
        };
        conn.device = Some(input.clone());
        let n = &mut self.hub.devices.entry(conn.who.session.clone()).or_insert((0, input.clone())).0;
        *n += 1;
        if let (1, Some(f)) = (*n, live.on_join) {
            let _ = self.author(Space::Volatile, &f, input, now);
        }
    }

    /// After a server's step on its log: the journal written when a
    /// refusal or the horizon moved.
    pub(crate) fn kept(&mut self) {
        let mark = self.mark();
        if mark != self.hub.mark {
            self.hub.mark = mark;
            self.keep_journal();
        }
    }

    /// What the journal moves with: the refusals, the horizon.
    fn mark(&mut self) -> (usize, i64) {
        let base = self.log.log().base_seq();
        let j = journal(&mut self.log);
        (j.refused.len(), base)
    }

    /// The journal whole: each origin's last, and what it refused.
    fn keep_journal(&mut self) {
        let j = journal(&mut self.log);
        let refused: Vec<(EntryId, String)> = j.refused.iter().map(|(id, why)| (*id, why.clone())).collect();
        let bytes = ark::format::encode(&(&j.last, refused)).expect("a journal serializes");
        self.put_beside(JOURNAL, &bytes);
    }

    /// Writes what is unwritten; then everything waiting is delivered, or,
    /// while a write fails, waits on with what comes after it.
    pub fn release(&mut self) -> Vec<Deliver> {
        match self.log.flush() {
            true => std::mem::take(&mut self.hub.held),
            false => vec![],
        }
    }
}
