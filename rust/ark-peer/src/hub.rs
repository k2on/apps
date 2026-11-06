//! The server role: a peer that is the authority of each space (§1.7,
//! §1.9) and the connections it serves, routed — sans-io like the
//! machines, so the simulator drives the same server a socket does.
//!
//! **Routing.** A frame from a connection goes to the machine of its space
//! with the connection's verified identity. What the machine says goes:
//! a frame `Send` back to that connection, or to all — every connection
//! fed the log, each through its feed, or, for the volatile space, every
//! connection that said `Hello` in it, each encoded once for those fed
//! whole and once for each fed by a scope; and what it says to keep to the
//! disk **first** — nothing a step says is delivered until its records are
//! written, and while a write fails every delivery waits with it, in
//! order, for a write that succeeds ([`Peer::failure`]).
//!
//! **Feeds.** Each connection is fed as the app says of its login
//! (`Config::feed`): whole, or by a scope (§1.8) — the authority of each
//! space asks the same function (`Peer::feed`) at the connection's
//! `Hello`, and every page sent to all goes to it through its scope
//! (`ark::peer::Peer::fed`), once it has said `Hello`: the live space is
//! fed by the log's rules, its scope named in the live domain.
//!
//! **Devices** (§1.9). A connection may say what its device is, in a call
//! before its `Hello`s (`ark::frames::DEVICE`); the server authors the live domain's `on_join` on what it
//! said, as the connection's login, when a session's first such connection
//! says it, and `on_leave` when its last closes, so a device on two
//! connections is one device. On a restart the
//! volatile space is what the server wrote of it when it last closed
//! (its rows whole, one record beside its log: `local.volatile`,
//! and its journal, `local.live`), and `on_restore` is authored over it.
//!
//! **The journal** beside the log keeps what the log cannot, as one record
//! beside the log (`local.journal`): each origin's last
//! `local` and actor and the intents refused, written whole when the
//! machine says it moved — a refusal, or the horizon — so that after a
//! restart a push past a refusal is not
//! taken for a gap, a refused intent pushed again is answered again, and
//! one sequenced below the horizon is acknowledged there. **The horizon**
//! moves as `keep` says: the log keeps that many entries above its
//! snapshot.

use std::collections::BTreeMap;

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::{EntryId, Origin, Record};
use ark::peer::{Ctx, Feed, Input, Journal, Role};
use ark::schema::Space;

use crate::{machine, Config, Disk, Fold, Peer};

/// What a server keeps beside its log, each one record on its
/// disk: its volatile space as it last closed, the store's rows whole, and
/// the live journal then; and its journal, each origin's last `local` and
/// actor and what it refused.
const VOLATILE: &str = "volatile";
const LIVE: &str = "live";
pub(crate) const JOURNAL: &str = "journal";

/// Each origin's last `local`, and the actor it is bound to.
type Lasts = BTreeMap<Origin, (String, i64)>;

/// A record kept beside the log, read.
fn kept<T: Encode>(local: &mut Box<dyn Disk>, name: &str) -> Option<T> {
    T::from_bytes(&local.get(name).ok()??).ok()
}

/// What an authority refused, by id, as an older build kept it, and where
/// each stands, by origin and `local`, which it lacks.
type Ids = Vec<(EntryId, String)>;
type At = Vec<(Origin, i64)>;

/// A journal as it is kept beside its log (`local.journal`, or the live
/// space's, `local.live`): as an older build kept it — each origin's last
/// and what it refused, by id, in one order or the other — and, after, where
/// each refusal stands.
pub(crate) fn journal_bytes(j: &Journal, live: bool) -> Vec<u8> {
    let (ids, at): (Ids, At) = j.refused.iter().map(|(&(o, l), why)| ((EntryId::of(o, l), why.clone()), (o, l))).unzip();
    match live {
        false => ark::format::encode(&(&j.last, ids, at)),
        true => ark::format::encode(&(ids, &j.last, at)),
    }
    .expect("a journal serializes")
}

/// A journal as it was kept ([`journal_bytes`]): by an older build, its
/// refusals found among each origin's `local`s, up to its last.
fn journal(local: &mut Box<dyn Disk>, name: &str) -> Option<Journal> {
    let b = local.get(name).ok()??;
    let (last, ids, at): (Lasts, Ids, Option<At>) = match name == LIVE {
        false => <(Lasts, Ids, At)>::from_bytes(&b).map(|(l, i, a)| (l, i, Some(a))),
        true => <(Ids, Lasts, At)>::from_bytes(&b).map(|(i, l, a)| (l, i, Some(a))),
    }
    .or_else(|_| match name == LIVE {
        false => <(Lasts, Ids)>::from_bytes(&b).map(|(l, i)| (l, i, None)),
        true => <(Ids, Lasts)>::from_bytes(&b).map(|(i, l)| (l, i, None)),
    })
    .ok()?;
    let mut ids: BTreeMap<EntryId, String> = ids.into_iter().collect();
    let at = at.unwrap_or_else(|| {
        let each = last.iter().flat_map(|(o, (_, n))| (1..=*n).map(move |l| (*o, l)));
        each.filter(|(o, l)| ids.contains_key(&EntryId::of(o, *l))).collect()
    });
    let refused = at
        .into_iter()
        .filter_map(|(o, l)| Some(((o, l), ids.remove(&EntryId::of(o, l))?)))
        .collect();
    Some(Journal { last, refused })
}

pub type ConnId = u64;

/// A frame for a connection.
pub type Deliver = (ConnId, Vec<u8>);

struct Conn {
    who: Ctx,
    hello: [bool; 2],
    /// How it is fed in each space: by a scope, or whole (§1.8).
    feed: [Option<Feed>; 2],
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
    pub who: Ctx,
    pub id: u64,
    pub name: String,
    pub input: Vec<u8>,
}

/// Who a server's authorities author as.
pub fn authority() -> Ctx {
    Ctx::who("authority", "authority", vec![])
}

impl Peer {
    /// A server opened from what `disk` holds: the authority of each space.
    pub fn serve(config: Config, disk: Box<dyn Disk>, now: i64) -> Peer {
        let files = disk.files();
        let (disk, mut local) = crate::local::beside(disk);
        let me = authority();
        let mut log = machine(&config.log, Role::authority(), &me, None, Space::Durable);
        (log.disk, log.fold) = (Some(disk), Fold::keep(config.keep));
        log.feed(config.feed.clone());
        let mut stored = log.stored();
        if let Some(e) = &log.failure {
            panic!("the log on disk does not read: {e}");
        }
        stored.extend(journal(&mut local, JOURNAL).map(Record::Journal));
        log.step(Input::Open(stored));
        let origin = log.origin;
        // The live authority's origin is another at every open, as a
        // device's live machine's is: under the log's, its first `local` and
        // the log's had one id.
        let opens = crate::local::Replica::read(&mut *local).opens + 1;
        let r = crate::local::Replica { opens, ..Default::default() };
        let _ = local.put(crate::local::REPLICA, &r.to_bytes());
        let live = config.live.as_ref().map(|l| {
            let mut m = machine(l, Role::authority(), &me, Some(crate::live_origin(origin, opens)), Space::Volatile);
            m.fold = Fold::keep(config.live_keep);
            m.feed(config.feed.clone());
            m
        });
        let mut p = Peer::with(config, log, live, local);
        p.keep_files(files);
        (p.hub.serving, p.hub.next) = (true, 1);
        if let (Some(Record::Rows(rows)), Some(live)) = (kept(&mut p.local, VOLATILE), &mut p.live) {
            let j = journal(&mut p.local, LIVE).map(Record::Journal);
            live.step(Input::Open([Record::Rows(rows)].into_iter().chain(j).collect()));
            if let Some(f) = p.config.hook("on_restore") {
                let _ = p.author(Space::Volatile, &f, vec![], now);
            }
        }
        p
    }

    /// Writes the volatile space as it stands, whole, for the next open:
    /// its rows, its log's and its journal, so that the live authority
    /// opened again goes on judging each origin from where it was.
    pub fn close(&mut self) {
        let Some(live) = &mut self.live else { return };
        let (seq, hash) = live.log().head();
        let rows = Record::Rows(ark::log::standing(live.store.snapshot_rows(), seq, hash));
        let j = journal_bytes(live.journal().expect("the live authority's"), true);
        self.put_beside(VOLATILE, &rows.to_bytes());
        self.put_beside(LIVE, &j);
    }

    /// A connection verified as `who`.
    pub fn accept(&mut self, who: Ctx) -> ConnId {
        let c = self.hub.next;
        self.accept_as(c, who);
        c
    }

    /// A connection verified as `who`, numbered by the caller.
    pub fn accept_as(&mut self, c: ConnId, who: Ctx) {
        self.hub.next = self.hub.next.max(c + 1);
        let feed = [Space::Durable, Space::Volatile].map(|s| self.machine(s).and_then(|m| m.feed_of(&who)));
        let conn = Conn {
            who,
            hello: [false; 2],
            feed,
            device: None,
        };
        self.hub.conns.insert(c, conn);
    }

    /// Every connection open, and who it is.
    pub fn connections(&self) -> impl Iterator<Item = (ConnId, &Ctx)> {
        self.hub.conns.iter().map(|(c, conn)| (*c, &conn.who))
    }

    /// Who a connection is.
    pub(crate) fn who(&self, c: ConnId) -> Option<Ctx> {
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
            if name == ark::frames::DEVICE {
                let input = input.clone();
                self.arrive(c, &input, now);
                return self.release();
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
        let space = f.space();
        if let Frame::Hello { .. } = f {
            conn.hello[space as usize] = true;
        }
        let from = Some(conn.who.clone());
        self.route(space, Some(c), Input::Frame { from, frame: f });
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
                if let Some(f) = self.config.hook("on_leave") {
                    self.author_as(&who, &f, input, now);
                }
            }
        }
        self.release()
    }

    fn arrive(&mut self, c: ConnId, said: &[u8], now: i64) {
        let Some(conn) = self.hub.conns.get_mut(&c).filter(|c| c.device.is_none()) else {
            return;
        };
        conn.device = Some(said.to_vec());
        let who = conn.who.clone();
        let n = &mut self.hub.devices.entry(who.session.clone()).or_insert((0, said.to_vec())).0;
        *n += 1;
        if let (1, Some(f)) = (*n, self.config.hook("on_join")) {
            self.author_as(&who, &f, said.to_vec(), now);
        }
    }

    /// The live space's `f` on `input`, authored by the authority as `who`:
    /// a device's arrival or leaving is its login's.
    fn author_as(&mut self, who: &Ctx, f: &str, input: Vec<u8>, now: i64) {
        let Some(live) = &mut self.live else { return };
        let was = std::mem::replace(&mut live.me, who.clone());
        let _ = self.author(Space::Volatile, f, input, now);
        if let Some(live) = &mut self.live {
            live.me = was;
        }
    }

    /// A frame sent to all, for every connection of its space: the log's
    /// fed whole; and those that said `Hello` in it, each through its scope
    /// (§1.8) — the live space follows the log's rules.
    pub(crate) fn fanout(&mut self, space: Space, f: &Frame) {
        let whole = f.enc();
        let to = self.hub.conns.iter().filter_map(|(c, conn)| match &conn.feed[space as usize] {
            None if space == Space::Durable || conn.hello[1] => Some((*c, whole.clone())),
            Some(feed) if conn.hello[space as usize] => feed.fed(f).map(|f| (*c, f.enc())),
            _ => None,
        });
        let to: Vec<Deliver> = to.collect();
        self.hub.held.extend(to);
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
