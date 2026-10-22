//! The server role: a peer that is the authority of each space (§1.7,
//! §1.9) and the connections it serves, routed — sans-io like the
//! machines, so the simulator drives the same server a socket does.
//!
//! **Routing.** A frame from a connection goes to the machine of its space
//! with the connection's verified identity. What the machine says goes:
//! `Send` back to that connection; `Fanout` to every connection fed the log
//! whole, or, for the volatile space, every connection that said `Hello`
//! in it; `To` to the connections of that session; and `Persist` to the
//! disk **first** — nothing a step says is delivered until its records are
//! written, and while a write fails every delivery waits with it, in
//! order, for a write that succeeds ([`Peer::failure`]).
//!
//! **Feeds.** Each connection is fed as the app says of its login
//! (`Config::feed`): whole, or by a scope (§1.8), told to the authority by
//! `Input::Feed` before the connection's first frame.
//!
//! **Devices** (§1.9). A connection's volatile `Hello` may say what its
//! device is; the app turns that and the login into the input of its
//! `on_join` (`Live::arrive`), and the server authors `on_join` when a
//! session's first such connection says it and `on_leave` when its last
//! closes, so a device on two connections is one device. On a restart the
//! volatile space is what the server wrote of it when it last closed
//! (its local instance's `volatile` rows), and `on_restore` is authored
//! over it.
//!
//! **The journal** beside the log keeps what the log cannot, as rows of the
//! peer's local instance (`origin`, `verdict`): each origin's last `local`
//! and actor and the intents refused, written when a refusal or the
//! horizon moves, so that after a restart a push past a refusal is not
//! taken for a gap, a refused intent pushed again is answered again, and
//! one sequenced below the horizon is acknowledged there. **The horizon**
//! moves as `keep` says: the log keeps that many entries above its
//! snapshot.

use std::collections::BTreeMap;

use ark::frames::{Frame, Verdict};
use ark::log::{Durable, Snapshot};
use ark::peer::{Feed, Input, Journal, Peer as Machine, Role, Who};
use ark::row::Bytes;
use ark::schema::Space;

use crate::local::{Last, LiveRow, Refused};
use crate::{machine, Config, Disk, Fold, Instance, Peer};

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
        let (disk, local) = crate::local::beside(disk);
        let me = authority();
        let auth = || Role::Authority { journal: Journal::default() };
        let mut log = machine(&config.log, auth(), &me, None, Space::Durable);
        (log.disk, log.fold) = (Some(disk), Fold::keep(config.keep));
        let stored = log.stored();
        if let Some(e) = &log.failure {
            panic!("the log on disk does not read: {e}");
        }
        log.step(Input::Open(stored));
        let origin = log.origin;
        let mine = log.log().entries().into_iter().filter(|l| l.entry.origin == origin);
        log.local = mine.map(|l| l.entry.local).max().unwrap_or(0);
        let (last, refused, kept) = local.read::<crate::Local, _>(|db| (db.origin.scan(), db.verdict.scan(), db.volatile.scan()));
        let j = journal(&mut log);
        last.into_iter().for_each(|l| j.pass(l.origin, &l.actor, l.last));
        j.verdicts.extend(refused.into_iter().map(|r| (r.entry, Verdict::Refused { why: r.why })));
        log.local = log.local.max(journal(&mut log).last_of(&origin));
        let live = config.live.as_ref().map(|l| Instance {
            fold: Fold::keep(l.keep),
            ..machine(&l.module, auth(), &me, None, Space::Volatile)
        });
        let mut p = Peer::with(config, log, live, local);
        (p.hub.serving, p.hub.next) = (true, 1);
        if let (false, Some(live)) = (kept.is_empty(), &mut p.live) {
            let mut rows: Vec<(String, Vec<Vec<u8>>)> = vec![];
            for LiveRow { table, row } in kept {
                match rows.last_mut() {
                    Some((t, rs)) if *t == table => rs.push(row.0),
                    _ => rows.push((table, vec![row.0])),
                }
            }
            live.step(Input::Open(Durable { rows, ..Durable::default() }));
            if let Some(f) = p.config.live.as_ref().and_then(|l| l.on_restore.clone()) {
                let _ = p.author(Space::Volatile, &f, vec![], now);
            }
        }
        p.hub.mark = p.mark();
        p
    }

    /// Writes the volatile space as it stands, whole, for the next open:
    /// its rows, and its log's.
    pub fn close(&mut self) {
        let Some(live) = &self.live else { return };
        let (seq, hash) = live.log().head();
        let (state, rows) = (live.store.state_hash(), live.store.snapshot_rows());
        let rows = Snapshot { seq, hash, state, rows }.kv_rows();
        self.keep(|db| {
            db.volatile.scan().iter().try_for_each(|k| db.volatile.remove(k))?;
            let rows = rows.into_iter().flat_map(|(table, rs)| rs.into_iter().map(move |r| (table.clone(), r)));
            rows.into_iter()
                .try_for_each(|(table, row)| db.volatile.put(LiveRow { table, row: Bytes(row) }))
        });
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
        let feed = (self.config.feed)(&who);
        if let Feed::Scoped(_) = feed {
            self.log.step(Input::Feed {
                who: who.clone(),
                feed: feed.clone(),
            });
        }
        let conn = Conn {
            who,
            hello: [false; 2],
            whole: feed == Feed::Whole,
            device: None,
        };
        self.hub.conns.insert(c, conn);
    }

    pub fn who(&self, c: ConnId) -> Option<&Who> {
        self.hub.conns.get(&c).map(|c| &c.who)
    }

    /// Every connection open, and who it is.
    pub fn connections(&self) -> impl Iterator<Item = (ConnId, &Who)> {
        self.hub.conns.iter().map(|(c, conn)| (*c, &conn.who))
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

    /// A connection gone; the device's `on_leave` when it was its last.
    pub fn leave(&mut self, c: ConnId, now: i64) -> Vec<Deliver> {
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
        let refused = j.verdicts.values().filter(|v| matches!(v, Verdict::Refused { .. })).count();
        (refused, base)
    }

    /// The journal as rows: each origin's last and what it refused put.
    fn keep_journal(&mut self) {
        let j = journal(&mut self.log);
        let refused: Vec<Refused> = (j.verdicts.iter())
            .filter_map(|(id, v)| match v {
                Verdict::Refused { why } => Some(Refused {
                    entry: *id,
                    why: why.clone(),
                }),
                _ => None,
            })
            .collect();
        let last = j.last.clone();
        self.keep(|db| {
            for (origin, (actor, last)) in last {
                db.origin.put(Last { origin, actor, last })?;
            }
            refused.into_iter().try_for_each(|r| db.verdict.put(r))
        });
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
