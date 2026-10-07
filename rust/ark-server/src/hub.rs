//! The one `ark::protocol::Server`, and the thread it lives on.
//!
//! The machine is sans-io and single-owner, and a mutex held across a disk
//! write would stall every socket waiting on it; so one thread owns it and
//! everything — a WebSocket handler, an in-process peer, a standing device,
//! `/healthz` — talks to it through a [`HubHandle`]. That thread is also
//! where the log and the rooms' kept snapshots are written, after the
//! message that moved them.
//!
//! **An acknowledgement follows the write.** What a message moved in the log
//! is appended to the journal and synced ([`crate::persist::LogFile`])
//! before anything the machine queued in answer to it is sent: the `Ack` to
//! the author, the `Batch` to everyone else. So an entry any peer has been
//! told about is on the disk, and a server killed at any instant restarts
//! with every sequence a peer holds — a torn tail can only be an entry
//! nobody was told of, which its author still has pending and pushes again
//! (`docs/plan-perf.md` §R3; `harken/server/tests/fleet.rs`, scenario 3b,
//! is the witness it answers). Where the write fails, everything queued is
//! held, in order, until a write succeeds: a server that cannot keep its
//! log promises nothing about it.
//!
//! **…and holds it for [`GIVE_UP_AFTER`], not for ever.** A stretch of
//! failed writes is retried every [`RETRY_EVERY`] whether or not anything
//! arrives, and said once when it starts and once when it ends. Once it has
//! lasted [`GIVE_UP_AFTER`], every connection is closed with the reason
//! (`the log cannot be written: <error>`, the close frame's text on a
//! socket), the held queue is dropped, and so is whatever a connection says
//! while the disk stays unwritable. The log in memory is not rolled back:
//! its unwritten entries go to the disk with the first write that
//! succeeds. A peer sees a closed link, keeps its pending intents on its
//! own disk, dials again with its backoff, and re-pushes them — the path a
//! restart already takes — and an intent the log holds is answered as the
//! duplicate it is, once the disk holds it. A standing device is not a
//! replica and hears no log, so it stays.
//!
//! **A revoked session's connections are closed at once.** The token is
//! asked at `Hello` and not again, so a session revoked while its socket is
//! open used to go on syncing until the socket happened to drop. The sign-in
//! routes tell the hub which session went ([`HubHandle::revoker`], which
//! `ark_auth::server::Auth::on_revoke` is given), and every replica
//! connection identified as it is sent `Denied` with [`REVOKED`] — the last
//! frame a socket gets, which the peer keeps as its reason and stops
//! dialling on — and forgotten by the machine at once, so nothing it says
//! after is taken (`docs/plan-perf.md` R6). The token check at `Hello` is
//! unchanged: it is what turns the peer away when it dials again.
//!
//! **The log is kept to what somebody may still ask for** (`ark::retention`,
//! `docs/plan-alone.md` §3). Every session's place in the log and when it
//! was last heard are recorded as it says `Hello` and as pages reach it
//! ([`crate::retain`], `cursors.cbor` beside `live.cbor`), and after every
//! message — so after each batch of appends — the rule is asked whether to
//! move the horizon. When it says so the authority compacts in memory and
//! the write that follows is the journal's own compaction: a snapshot at the
//! new horizon with the entries above it (`persist` module docs), written,
//! like everything else, before anything queued is sent. A peer below the
//! horizon is then served that snapshot and rebases its pending onto it,
//! which is what the machine already did for one (§12.4).
//!
//! Three kinds of connection, one numbering: a **socket** (frames go to its
//! writer task), a **local** peer (an `ark_client::Peer` in this process,
//! through [`HubHandle::dial`]: the scanner's shape), and a **standing**
//! peer — a device the server stands in for, a speaker, which is in a room
//! and not in the log and receives only live frames ([`HubHandle::stand`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use ark::live::{self, ConnId, Peer};
use ark::log::Seq;
use ark::peer::Authority;
use ark::protocol::{ClientMsg, Identity, Server, ServerMsg};
use ark::retention::{self, Retention};
use ark::store::{Row, Store};
use tokio::sync::{mpsc, oneshot};

use crate::hooks::Hooks;
use crate::live::{decode_kept, encode_kept, Relay};
use crate::persist::LogFile;
use crate::retain::{self, Cursors, Heard};

/// Where a connection's frames go.
enum Sink {
    Socket(mpsc::UnboundedSender<ServerMsg>),
    Local(std::sync::mpsc::Sender<ServerMsg>),
    /// Live frames only, raw.
    Standing(Box<dyn Fn(Vec<u8>) + Send>),
}

/// The machine and everything around it, on its thread. What
/// [`HubHandle::read`] hands a closure.
pub struct Hub {
    server: Server<Relay>,
    relay: Relay,
    sinks: BTreeMap<ConnId, Sink>,
    data: Option<PathBuf>,
    /// The log's files, where there is a data directory.
    log: Option<LogFile>,
    /// What the machine queued and nothing has been sent of, because the
    /// log it speaks of is not yet on the disk (the module docs).
    held: Vec<(ConnId, ServerMsg)>,
    /// Since when, and why, the log has not been written; `None` while it
    /// is.
    failing: Option<Failing>,
    /// How long a stretch of failed writes holds the queue before every
    /// connection is closed: [`GIVE_UP_AFTER`], or shorter in a test.
    give_up: Duration,
    /// What a connection closed for the disk is told, where the
    /// [`HubHandle`] can read it without the hub's thread.
    failure: Arc<Mutex<Option<String>>>,
    /// The rooms' kept snapshots as last written.
    kept: BTreeMap<String, Vec<u8>>,
    /// How much of the log is kept (the module docs).
    pub(crate) retention: Retention,
    /// Every session's place in the log and when it was heard, as
    /// `cursors.cbor` holds it once written ([`crate::retain`]).
    cursors: Cursors,
    /// Each open replica connection's place, so that a session open on two
    /// connections is recorded at the lower of the two.
    at: BTreeMap<ConnId, Seq>,
    /// The cursors moved since they were last written; whether what moved
    /// is worth writing now (a session arrived or left, the horizon moved);
    /// and when they were last written.
    cursors_due: bool,
    cursors_now: bool,
    cursors_written: Instant,
    /// `docs/plan-guards.md` D3 The app's hooks ([`crate::hooks`]), the last
    /// sequence handed to them or passed over, and the entries waiting for
    /// the write that makes them durable.
    hooks: Option<Hooks>,
    hooked: Seq,
    due: Vec<(Seq, String, ark::log::Entry, ark::log::Facts)>,
}

/// What `/healthz` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub connections: usize,
    /// The head of the log.
    pub head: Seq,
    /// The log's horizon: the snapshot it stands on (`ark::retention`).
    pub horizon: Seq,
    /// Open rooms and how many peers each has.
    pub rooms: Vec<(String, usize)>,
    /// The hash of every module this server has run, in hash order
    /// (`crate::modules`), and the one it runs now.
    pub modules: Vec<Vec<u8>>,
    pub module: Option<Vec<u8>>,
    /// The log's identity (`ark::log`, Round 4): what a peer's `Hello`
    /// names, and what a restore draws afresh (`arkc restore`).
    pub log_id: Option<ark::value::Id>,
    /// Every session the hub has heard from, as `cursors.cbor` holds it,
    /// with how many replica connections it has open now
    /// (`docs/plan-db.md` D6).
    pub sessions: Vec<SessionHealth>,
}

/// One session in `/healthz`: who, where in the log, when last heard, and
/// how many connections it has open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionHealth {
    pub user: String,
    pub session: String,
    /// Its place in the log as last recorded ([`crate::retain`]).
    pub cursor: Seq,
    /// When it was last heard, in milliseconds since the Unix epoch.
    pub heard_ms: i64,
    /// Replica connections identified as it, open now.
    pub open: usize,
}

const KEPT: &str = "live.cbor";

/// How long the hub holds what it would say while its log cannot be
/// written, before it closes every connection. Long enough for a
/// transient failure — a disk freed by a log rotation, a network
/// filesystem's hiccup — to pass unnoticed by any peer: five seconds is
/// under the keepalive's twenty, so a quiet peer never sees it. Short
/// enough that nobody waits on an `Ack` for longer than a peer's own
/// backoff would take to dial again, which is what it is better off doing:
/// its intents are on its disk, and the re-push is how the server learns
/// its disk is back.
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(5);

/// What a connection whose session was revoked is told, as a `Denied`.
pub const REVOKED: &str = "signed out: this login was revoked";

/// How often a failed write is tried again while nothing arrives: what
/// makes [`GIVE_UP_AFTER`] a duration rather than a count of messages that
/// may never come.
pub const RETRY_EVERY: Duration = Duration::from_millis(250);

/// A stretch of failed writes.
struct Failing {
    since: Instant,
    /// The connections have been closed for it.
    closed: bool,
}

impl Hub {
    pub(crate) fn new(mut server: Server<Relay>, relay: Relay, data: Option<PathBuf>, log: Option<LogFile>) -> Result<Hub> {
        let mut kept = BTreeMap::new();
        if let Some(dir) = &data {
            match std::fs::read(dir.join(KEPT)) {
                Ok(b) => kept = decode_kept(&b).map_err(|e| anyhow!("{}: {e}", dir.join(KEPT).display()))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("reading {}", dir.join(KEPT).display())),
            }
        }
        server.rooms.kept = kept.clone();
        let cursors = data.as_deref().map(retain::load).unwrap_or_default();
        // A log is named once, when it is created: here, for a new one or
        // one loaded from a directory written before logs had names — the
        // engine has no randomness to draw it with. One loaded named keeps
        // its name; `LogFile::write` puts a new one on the disk at the
        // first write (`persist` module docs, `docs/plan-perf.md` Round 4).
        server.authority.log.name_if_unnamed(ark_client::Autos::system().new_id());
        // Ids below the horizon by their keys (`ark::log::Below`,
        // `docs/plan-db.md` D6): a log read back holds them as written, and
        // one written before they were folded holds them whole.
        server.authority.log.fold_ids();
        // What the log held at start was committed by an earlier start, and
        // is no hook's (`crate::hooks`).
        let hooked = server.authority.log.head_seq();
        Ok(Hub {
            server,
            relay,
            sinks: BTreeMap::new(),
            data,
            log,
            held: vec![],
            failing: None,
            give_up: GIVE_UP_AFTER,
            failure: Arc::default(),
            kept,
            retention: Retention::default(),
            cursors,
            at: BTreeMap::new(),
            cursors_due: false,
            cursors_now: false,
            cursors_written: Instant::now(),
            hooks: None,
            hooked,
            due: vec![],
        })
    }

    /// `docs/plan-guards.md` D3 Hand every entry of these functions,
    /// durable, to its hook ([`crate::hooks`]).
    pub(crate) fn hook(&mut self, hooks: Vec<(String, crate::hooks::Hook)>) {
        self.hooks = Hooks::start(hooks);
    }

    // The entries appended since the last look whose function has a hook,
    // to be handed over once the write that follows has made them durable.
    // Asked before the horizon moves, so a compaction cannot take one away
    // first.
    fn collect_committed(&mut self) {
        let Some(hooks) = &self.hooks else { return };
        let a = &self.server.authority;
        let head = a.log.head_seq();
        if head <= self.hooked {
            return;
        }
        for (n, (e, f)) in a.log.entries.range(self.hooked + 1..) {
            if let Some(c) = a.bodies.get(&e.fn_hash).filter(|c| hooks.wants(&c.function.name)) {
                self.due.push((*n, c.function.name.clone(), e.clone(), f.clone()));
            }
        }
        self.hooked = head;
    }

    // -- reading: what a `read` closure may ask -------------------------------

    /// The authority: the log and the state at its head.
    pub fn authority(&self) -> &Authority {
        &self.server.authority
    }

    /// The rows of a table at the head of the log.
    pub fn rows(&self, table: &str) -> Vec<Row> {
        self.server.authority.store.scan(table)
    }

    /// Who a connection was identified as, if it has said hello.
    pub fn identity(&self, c: ConnId) -> Option<Identity> {
        self.server.identity(c).cloned()
    }

    /// Every session's place in the log and when it was last heard
    /// ([`crate::retain`]).
    pub fn cursors(&self) -> &Cursors {
        &self.cursors
    }

    /// How much of the log this hub keeps.
    pub fn retention(&self) -> Retention {
        self.retention
    }

    /// Every open room and who is in it.
    pub fn rooms(&self) -> BTreeMap<String, Vec<Peer>> {
        self.server.rooms.open.iter().map(|(r, (_, ps))| (r.clone(), ps.clone())).collect()
    }

    /// The snapshot kept for a room that is not open.
    pub fn kept(&self, room: &str) -> Option<&[u8]> {
        self.server.rooms.kept.get(room).map(Vec::as_slice)
    }

    pub fn health(&self) -> Health {
        Health {
            connections: self.sinks.values().filter(|s| !matches!(s, Sink::Standing(_))).count(),
            head: self.server.authority.log.head_seq(),
            horizon: self.server.authority.log.horizon(),
            rooms: self.server.rooms.open.iter().map(|(r, (_, ps))| (r.clone(), ps.len())).collect(),
            modules: self.server.authority.modules.keys().cloned().collect(),
            module: self.server.module.clone(),
            log_id: self.server.authority.log.id(),
            sessions: self
                .cursors
                .iter()
                .map(|((user, session), h)| SessionHealth {
                    user: user.clone(),
                    session: session.clone(),
                    cursor: h.cursor,
                    heard_ms: h.at_ms,
                    open: self
                        .sinks
                        .iter()
                        .filter(|(_, s)| !matches!(s, Sink::Standing(_)))
                        .filter(|(c, _)| self.server.identity(**c).is_some_and(|w| &w.user == user && &w.session == session))
                        .count(),
                })
                .collect(),
        }
    }

    // -- the machine ------------------------------------------------------------

    fn attach(&mut self, c: ConnId, sink: Sink) {
        self.sinks.insert(c, sink);
    }

    fn detach(&mut self, c: ConnId) {
        if matches!(self.sinks.get(&c), Some(Sink::Standing(_))) {
            let post = live::depart(&self.relay, &mut self.server.rooms, c);
            self.deliver(post);
        } else {
            // Heard until now; and a session leaving is worth writing down.
            self.touch(c);
            self.at.remove(&c);
            self.cursors_now = true;
            self.server.disconnect(c);
        }
        self.sinks.remove(&c);
        self.after();
    }

    fn recv(&mut self, c: ConnId, msg: ClientMsg) {
        if matches!(self.sinks.get(&c), Some(Sink::Standing(_))) {
            // A standing peer only speaks.
            if let ClientMsg::Say { frame } = msg {
                let post = live::speak(&self.relay, &mut self.server.rooms, c, &frame);
                self.deliver(post);
            }
        } else {
            // The cursor a `Hello` names, if it is a place in this log: one
            // naming another log is sent the snapshot at the head, and that
            // delivery is what records it (`retain` module docs).
            let hello = match &msg {
                ClientMsg::Hello { sub, .. } => {
                    let ours = self.server.authority.log.id();
                    let elsewhere = matches!((sub.log_id, ours), (Some(theirs), Some(ours)) if theirs != ours);
                    (!elsewhere).then_some(sub.since)
                }
                _ => None,
            };
            self.server.recv(c, msg);
            match hello {
                Some(since) => self.note(c, since),
                None => self.touch(c),
            }
        }
        self.after();
    }

    /// A connection is at `cursor`: its session is recorded at the lowest
    /// place any of its open connections is, heard now. The first note on a
    /// connection — a session arriving — is written at once.
    fn note(&mut self, c: ConnId, cursor: Seq) {
        let Some(who) = self.server.identity(c).cloned() else { return };
        if self.at.insert(c, cursor).is_none() {
            self.cursors_now = true;
        }
        let low = self
            .at
            .iter()
            .filter(|(k, _)| self.sinks.contains_key(k) && self.server.identity(**k) == Some(&who))
            .map(|(_, n)| *n)
            .min()
            .unwrap_or(cursor);
        self.cursors.insert(
            (who.user, who.session),
            Heard {
                cursor: low,
                at_ms: retain::now_ms(),
            },
        );
        self.cursors_due = true;
    }

    /// A connection was heard from: its session's time moves, its place
    /// does not.
    fn touch(&mut self, c: ConnId) {
        let Some(who) = self.server.identity(c) else { return };
        if let Some(h) = self.cursors.get_mut(&(who.user.clone(), who.session.clone())) {
            h.at_ms = retain::now_ms();
            self.cursors_due = true;
        }
    }

    /// Ask `ark::retention` whether to move the horizon, and move it: in
    /// memory here, on the disk by the write that follows (the module
    /// docs). A session with a connection open is heard now.
    fn retain_log(&mut self) {
        let now = retain::now_ms();
        self.at.retain(|c, _| self.sinks.contains_key(c));
        let open: BTreeSet<(String, String)> = self
            .at
            .keys()
            .filter_map(|c| self.server.identity(*c))
            .map(|w| (w.user.clone(), w.session.clone()))
            .collect();
        let heard = self.cursors.iter().map(|(k, h)| (h.cursor, if open.contains(k) { now } else { h.at_ms }));
        let log = &self.server.authority.log;
        let (head, horizon) = (log.head_seq(), log.horizon());
        if let Some(n) = retention::compact_to(head, horizon, self.retention, now, heard) {
            if self.server.authority.compact(n) {
                // The ids now below the horizon are kept by their keys
                // (`ark::log::Below`, `docs/plan-db.md` D6).
                self.server.authority.log.fold_ids();
                eprintln!("ark-server: the log's horizon moved from {horizon} to {n}; {} entries kept", head - n);
                self.cursors_now = true;
            }
        }
    }

    /// Write the cursors if they moved and it is time (`retain` module
    /// docs), or if `stopping` says so.
    fn write_cursors(&mut self, stopping: bool) {
        let Some(dir) = &self.data else { return };
        let due = self.cursors_due || self.cursors_now;
        if !due || !(stopping || self.cursors_now || self.cursors_written.elapsed() >= retain::WRITE_EVERY) {
            return;
        }
        match retain::save(dir, &self.cursors) {
            Ok(()) => {
                self.cursors_due = false;
                self.cursors_now = false;
                self.cursors_written = Instant::now();
            }
            Err(e) => eprintln!("ark-server: could not write the cursors: {e:#}"),
        }
    }

    fn stand(&mut self, c: ConnId, room: String, who: String, sink: Box<dyn Fn(Vec<u8>) + Send>) {
        self.sinks.insert(c, Sink::Standing(sink));
        let post = live::arrive(&self.relay, &mut self.server.rooms, Peer { conn: c, room, who });
        self.deliver(post);
        self.after();
    }

    fn deliver(&mut self, post: live::Post) {
        for (to, frame) in post.out {
            self.send(to, ServerMsg::Heard { frame });
        }
    }

    fn send(&self, to: ConnId, m: ServerMsg) {
        match self.sinks.get(&to) {
            // A writer that has gone is a disconnect on its way.
            Some(Sink::Socket(tx)) => {
                let _ = tx.send(m);
            }
            Some(Sink::Local(tx)) => {
                let _ = tx.send(m);
            }
            Some(Sink::Standing(f)) => {
                if let ServerMsg::Heard { frame } = m {
                    f(frame);
                }
            }
            None => {}
        }
    }

    // Write whatever moved, then deliver what the machine queued — in that
    // order, so no peer hears of an entry the disk does not hold (the module
    // docs). The whole queue waits on the write, not only the acks: a
    // connection's messages are an ordered stream, and one held back makes
    // everything after it wait with it.
    //
    // The horizon is moved first, so that a compaction is written by the
    // same write as the appends that prompted it.
    fn after(&mut self) {
        self.collect_committed();
        self.retain_log();
        let wrote = self.persist();
        self.held.extend(self.server.take_outgoing());
        match wrote {
            Ok(()) => {
                // Durable now: what the hooks wait for (`crate::hooks`).
                if let Some(hooks) = &self.hooks {
                    for (n, name, e, f) in std::mem::take(&mut self.due) {
                        hooks.committed(n, name, e, f);
                    }
                }
                if let Some(f) = self.failing.take() {
                    eprintln!("ark-server: the log is written again, after {:.1?}", f.since.elapsed());
                    *self.failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
                // Where each connection stood before what it is sent now:
                // the start of a page, or a snapshot's sequence (`retain`
                // module docs).
                let mut reached = vec![];
                for (c, m) in std::mem::take(&mut self.held) {
                    match &m {
                        ServerMsg::Batch { items, .. } => {
                            if let Some((n, _, _)) = items.first() {
                                reached.push((c, n - 1));
                            }
                        }
                        ServerMsg::SnapshotOf { seq, .. } => reached.push((c, *seq)),
                        _ => {}
                    }
                    self.send(c, m);
                }
                for (c, n) in reached {
                    self.note(c, n);
                }
                self.write_cursors(false);
            }
            Err(e) => {
                let why = format!("the log cannot be written: {e:#}");
                let give_up = self.give_up;
                let f = self.failing.get_or_insert_with(|| {
                    eprintln!("ark-server: {why}; holding what it would say for up to {give_up:?}");
                    Failing {
                        since: Instant::now(),
                        closed: false,
                    }
                });
                *self.failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(why);
                if f.since.elapsed() >= self.give_up {
                    self.close_for_the_disk();
                }
            }
        }
    }

    /// Whether the log is failing to be written: the hub's thread then tries
    /// again every [`RETRY_EVERY`] rather than only when a message comes.
    fn is_failing(&self) -> bool {
        self.failing.is_some()
    }

    /// Past [`GIVE_UP_AFTER`]: every connection that is a replica closed,
    /// and nothing said to any of them (the module docs).
    fn close_for_the_disk(&mut self) {
        let conns: Vec<ConnId> = self
            .sinks
            .iter()
            .filter(|(_, s)| !matches!(s, Sink::Standing(_)))
            .map(|(c, _)| *c)
            .collect();
        for c in &conns {
            // Dropping the sender is the close: the socket's task, or the
            // in-process transport, sees its channel end.
            self.sinks.remove(c);
            self.server.disconnect(*c);
        }
        self.held.clear();
        let _ = self.server.take_outgoing();
        if let Some(f) = &mut self.failing {
            if !f.closed && !conns.is_empty() {
                eprintln!("ark-server: closing {} connections until the log can be written", conns.len());
            }
            f.closed = true;
        }
    }

    /// A session was revoked: every replica connection identified as it is
    /// told `Denied` and forgotten by the machine (the module docs). The
    /// denial goes out through the held queue like anything else, after
    /// whatever was already queued for it and only once the log is written.
    fn revoked(&mut self, session: &str) {
        let gone: Vec<ConnId> = self
            .sinks
            .iter()
            .filter(|(_, s)| !matches!(s, Sink::Standing(_)))
            .map(|(c, _)| *c)
            .filter(|c| self.server.identity(*c).is_some_and(|who| who.session == session))
            .collect();
        if gone.is_empty() {
            return;
        }
        eprintln!("ark-server: a login was revoked; closing its {} connection(s)", gone.len());
        self.held.extend(self.server.take_outgoing());
        for c in gone {
            self.server.disconnect(c);
            self.held.push((c, ServerMsg::Denied { reason: REVOKED.into() }));
        }
        self.after();
    }

    /// Write the log and the rooms; `Err` where the log is not on the disk.
    fn persist(&mut self) -> Result<()> {
        let Some(dir) = self.data.clone() else {
            self.relay.keeps.lock().unwrap_or_else(|e| e.into_inner()).clear();
            return Ok(());
        };
        let mut wrote = Ok(());
        if let Some(file) = &mut self.log {
            wrote = file.write(&self.server.authority.log);
        }
        // The rooms: what the engine keeps for rooms that emptied, what a
        // hook asked to keep of an open one, and nothing for a room that is
        // neither any more.
        let mut kept = self.kept.clone();
        for (room, snap) in std::mem::take(&mut *self.relay.keeps.lock().unwrap_or_else(|e| e.into_inner())) {
            match snap {
                Some(b) => kept.insert(room, b),
                None => kept.remove(&room),
            };
        }
        for (room, b) in &self.server.rooms.kept {
            kept.insert(room.clone(), b.clone());
        }
        let live: BTreeSet<&String> = self.server.rooms.kept.keys().chain(self.server.rooms.open.keys()).collect();
        kept.retain(|r, _| live.contains(r));
        if kept != self.kept {
            let path = dir.join(KEPT);
            let tmp = dir.join(format!(".{KEPT}.tmp"));
            let wrote = std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(&tmp, encode_kept(&kept)))
                .and_then(|_| std::fs::rename(&tmp, &path));
            match wrote {
                Ok(()) => self.kept = kept,
                Err(e) => eprintln!("ark-server: could not write {}: {e}", path.display()),
            }
        }
        wrote
    }
}

/// The cursors that only moved are written when the hub stops, since
/// they are written lazily while it runs (`retain` module docs).
impl Drop for Hub {
    fn drop(&mut self) {
        self.write_cursors(true);
    }
}

type Reader = Box<dyn FnOnce(&Hub) + Send>;

enum Cmd {
    Attach(ConnId, Sink),
    Detach(ConnId),
    Recv(ConnId, ClientMsg),
    Stand(ConnId, String, String, Box<dyn Fn(Vec<u8>) + Send>),
    Read(Reader),
    Revoked(String),
}

/// The async (and sync) side's handle on the hub's thread. Cheap to clone;
/// the thread ends when the last one is dropped.
#[derive(Clone)]
pub struct HubHandle {
    tx: mpsc::UnboundedSender<Cmd>,
    next: Arc<AtomicI64>,
    failure: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for HubHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubHandle").finish_non_exhaustive()
    }
}

impl HubHandle {
    /// Build the hub on a thread of its own — it is born where it lives —
    /// and hand back the way to reach it, or what building it failed with.
    pub(crate) fn spawn(make: impl FnOnce() -> Result<Hub> + Send + 'static) -> Result<HubHandle> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
        let (built, ready) = std::sync::mpsc::channel::<Result<()>>();
        let failure: Arc<Mutex<Option<String>>> = Arc::default();
        let shared = failure.clone();
        std::thread::Builder::new()
            .name("ark-hub".into())
            .spawn(move || {
                let mut hub = match make() {
                    Ok(mut h) => {
                        h.failure = shared;
                        let _ = built.send(Ok(()));
                        h
                    }
                    Err(e) => {
                        let _ = built.send(Err(e));
                        return;
                    }
                };
                loop {
                    // While the log cannot be written, the hub does not wait
                    // for a message to try again (`GIVE_UP_AFTER`).
                    let cmd = if hub.is_failing() {
                        match rx.try_recv() {
                            Ok(cmd) => cmd,
                            Err(mpsc::error::TryRecvError::Empty) => {
                                std::thread::sleep(RETRY_EVERY);
                                hub.after();
                                continue;
                            }
                            Err(mpsc::error::TryRecvError::Disconnected) => break,
                        }
                    } else {
                        match rx.blocking_recv() {
                            Some(cmd) => cmd,
                            None => break,
                        }
                    };
                    match cmd {
                        Cmd::Attach(c, s) => hub.attach(c, s),
                        Cmd::Detach(c) => hub.detach(c),
                        Cmd::Recv(c, m) => hub.recv(c, m),
                        Cmd::Stand(c, room, who, sink) => hub.stand(c, room, who, sink),
                        Cmd::Read(f) => f(&hub),
                        Cmd::Revoked(session) => hub.revoked(&session),
                    }
                }
            })
            .context("spawning the hub thread")?;
        ready.recv().map_err(|_| anyhow!("the hub thread ended before it was built"))??;
        Ok(HubHandle {
            tx,
            next: Arc::new(AtomicI64::new(1)),
            failure,
        })
    }

    fn send(&self, cmd: Cmd) -> Result<()> {
        self.tx.send(cmd).map_err(|_| anyhow!("the hub has stopped"))
    }

    /// Why the hub is closing connections, while it is: its log cannot be
    /// written (the module docs). What a connection it let go is told.
    pub fn failure(&self) -> Option<String> {
        self.failure.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// A fresh connection id, never reused.
    pub fn fresh(&self) -> ConnId {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// A socket opened: its frames go down `tx`.
    pub fn attach_socket(&self, c: ConnId, tx: mpsc::UnboundedSender<ServerMsg>) -> Result<()> {
        self.send(Cmd::Attach(c, Sink::Socket(tx)))
    }

    /// A connection in this process: its frames go down a std channel.
    pub fn attach_local(&self, c: ConnId, tx: std::sync::mpsc::Sender<ServerMsg>) -> Result<()> {
        self.send(Cmd::Attach(c, Sink::Local(tx)))
    }

    /// A connection closed: the room hears it.
    pub fn detach(&self, c: ConnId) -> Result<()> {
        self.send(Cmd::Detach(c))
    }

    /// A session was revoked: close its connections (the module docs).
    pub fn revoke(&self, session: &str) -> Result<()> {
        self.send(Cmd::Revoked(session.into()))
    }

    /// [`HubHandle::revoke`] as what `ark_auth::server::Auth::on_revoke`
    /// takes. It holds the hub weakly: the authenticator the hub's machine
    /// holds keeps the `Auth` alive, and a strong handle in the `Auth`
    /// would keep the hub's thread alive for ever.
    pub fn revoker(&self) -> impl Fn(&str, &str) + Send + Sync + 'static {
        let hub = self.tx.downgrade();
        move |_user: &str, session: &str| {
            if let Some(tx) = hub.upgrade() {
                let _ = tx.send(Cmd::Revoked(session.into()));
            }
        }
    }

    /// A frame from a connection.
    pub fn recv(&self, c: ConnId, msg: ClientMsg) -> Result<()> {
        self.send(Cmd::Recv(c, msg))
    }

    /// Stand a device in `room` as `who`, with no socket and no replica:
    /// every live frame addressed to it is handed to `hears`, raw. Say
    /// things as it with [`HubHandle::say`]; take it out with
    /// [`HubHandle::detach`]. A standing peer keeps a room open like any
    /// other; an app that wants a room to close when its last *client*
    /// leaves takes its devices out in [`crate::Live::part`].
    pub fn stand(&self, room: &str, who: &str, hears: impl Fn(Vec<u8>) + Send + 'static) -> Result<ConnId> {
        let c = self.fresh();
        self.send(Cmd::Stand(c, room.into(), who.into(), Box::new(hears)))?;
        Ok(c)
    }

    /// Say a frame as a standing peer.
    pub fn say(&self, c: ConnId, frame: Vec<u8>) -> Result<()> {
        self.recv(c, ClientMsg::Say { frame })
    }

    /// Run `f` against the hub on its thread and hand back what it says.
    pub async fn read<T: Send + 'static>(&self, f: impl FnOnce(&Hub) -> T + Send + 'static) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        self.send(Cmd::Read(Box::new(move |h| {
            let _ = reply.send(f(h));
        })))?;
        answer.await.map_err(|_| anyhow!("the hub has stopped"))
    }

    /// [`HubHandle::read`] from a thread that is not async. Never from
    /// inside a runtime's worker: it blocks.
    pub fn read_blocking<T: Send + 'static>(&self, f: impl FnOnce(&Hub) -> T + Send + 'static) -> Result<T> {
        let (reply, answer) = std::sync::mpsc::channel();
        self.send(Cmd::Read(Box::new(move |h| {
            let _ = reply.send(f(h));
        })))?;
        answer.recv().map_err(|_| anyhow!("the hub has stopped"))
    }

    pub async fn health(&self) -> Result<Health> {
        self.read(Hub::health).await
    }

    pub async fn identity(&self, c: ConnId) -> Result<Option<Identity>> {
        self.read(move |h| h.identity(c)).await
    }

    pub async fn rows(&self, table: &str) -> Result<Vec<Row>> {
        let table = table.to_string();
        self.read(move |h| h.rows(&table)).await
    }

    /// A transport for an `ark_client::Peer` in this process: no socket, the
    /// same protocol. `peer.connect_with("local", hub.dial())`.
    pub fn dial(&self) -> ark_client::link::Dial {
        let hub = self.clone();
        Box::new(move |_url: &str| Box::new(Local::open(hub.clone())) as ark_client::link::BoxTransport)
    }
}

/// An in-process connection, as an `ark_client` transport.
struct Local {
    hub: HubHandle,
    conn: ConnId,
    rx: std::sync::mpsc::Receiver<ServerMsg>,
    opened: bool,
    closed: bool,
}

impl Local {
    fn open(hub: HubHandle) -> Local {
        let conn = hub.fresh();
        let (tx, rx) = std::sync::mpsc::channel();
        let closed = hub.attach_local(conn, tx).is_err();
        Local {
            hub,
            conn,
            rx,
            opened: false,
            closed,
        }
    }
}

impl ark_client::link::Transport for Local {
    fn send(&mut self, frame: Vec<u8>) {
        let msg = ark::canon::decode(&frame).ok().and_then(|v| ClientMsg::from_value(&v).ok());
        if let Some(m) = msg {
            if self.hub.recv(self.conn, m).is_err() {
                self.closed = true;
            }
        }
    }

    fn poll(&mut self) -> Vec<ark_client::link::Event> {
        use ark_client::link::Event;
        let mut out = vec![];
        if self.closed {
            if self.opened {
                self.opened = false;
                out.push(Event::Closed("the hub has stopped".into()));
            }
            return out;
        }
        if !self.opened {
            self.opened = true;
            out.push(Event::Opened);
        }
        loop {
            match self.rx.try_recv() {
                Ok(m) => out.push(Event::Frame(ark::canon::encode(&m.to_value()))),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // The hub let this connection go (its log cannot be
                // written): a close like a socket's, and the link dials again.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.closed = true;
                    self.opened = false;
                    let why = self.hub.failure().unwrap_or_else(|| "the hub closed this connection".into());
                    out.push(Event::Closed(why));
                    break;
                }
            }
        }
        out
    }

    fn close(&mut self) {
        if !self.closed {
            let _ = self.hub.detach(self.conn);
            self.closed = true;
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        ark_client::link::Transport::close(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark::protocol::{open_access, trusting};
    use ark_client::{args, demo, Options, Peer as Device, Value};

    /// §R3 The order the module docs state, held: an `Ack` is not sent for
    /// an entry the log could not write, and is sent — with everything
    /// queued behind it, in order — once a write succeeds. The write is
    /// made to fail by a directory standing where the journal goes.
    /// Falsified by writing after delivering, the order `after` had: the
    /// `Ack` arrives while nothing is on the disk.
    #[test]
    fn an_entry_is_acknowledged_only_once_the_disk_holds_it() {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let dir = tempfile::tempdir().unwrap();
        let (file, log) = LogFile::open(dir.path(), &schema).unwrap();
        assert!(log.is_none());
        let relay = Relay::new(Box::new(crate::Quiet));
        let mut a = Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        let server = Server::open(trusting(), open_access(), relay.clone(), a);
        let mut hub = Hub::new(server, relay, Some(dir.path().to_path_buf()), Some(file)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        hub.attach(1, Sink::Local(tx));

        let mut peer = Device::open_memory(d.clone(), Options::dev("alice")).unwrap();
        let exchange = |hub: &mut Hub, peer: &mut Device| {
            for m in peer.take_outgoing() {
                hub.recv(1, m);
            }
            let heard: Vec<ServerMsg> = rx.try_iter().collect();
            for m in &heard {
                peer.recv(m.clone());
            }
            heard
        };
        peer.connected();
        exchange(&mut hub, &mut peer);
        assert!(peer.linked(), "hello answered: nothing in the log moved");

        std::fs::create_dir(crate::persist::journal_path_of(dir.path())).unwrap();
        peer.mutate("create_playlist", args([("name", Value::text("Kept"))])).unwrap();
        let heard = exchange(&mut hub, &mut peer);
        assert!(heard.is_empty(), "nothing said of an entry the disk does not hold: {heard:?}");
        assert_eq!(hub.authority().log.head_seq(), 1, "sequenced, in memory");
        assert_eq!(peer.pending_len(), 1);

        std::fs::remove_dir(crate::persist::journal_path_of(dir.path())).unwrap();
        hub.after();
        let heard = exchange(&mut hub, &mut peer);
        assert!(matches!(heard.first(), Some(ServerMsg::Ack { seqs, .. }) if seqs == &[1]), "{heard:?}");
        let on_disk = crate::persist::load(dir.path(), &schema).unwrap().expect("a log");
        assert_eq!(on_disk, hub.authority().log);
        assert_eq!((peer.pending_len(), peer.cursor()), (0, 1));
    }

    /// `docs/plan-guards.md` D3 A server hook hears each entry of its
    /// function once the disk holds it, and not before: nothing while the
    /// journal cannot be written, the entry once it can; nothing of another
    /// function; nothing again of what a hub opened over the directory finds
    /// already in its log. A hook that panics leaves the entry as it was and
    /// the hooks after it running. Falsified by handing entries over before
    /// the write (`after` dispatching ahead of `persist`): the hook heard the
    /// first playlist while the disk held nothing.
    #[test]
    fn a_hook_hears_each_entry_once_it_is_durable() {
        use std::sync::atomic::AtomicUsize;
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let dir = tempfile::tempdir().unwrap();
        let open = |dir: &std::path::Path, heard: Arc<Mutex<Vec<String>>>, calls: Arc<AtomicUsize>| {
            let (file, log) = LogFile::open(dir, &schema).unwrap();
            let relay = Relay::new(Box::new(crate::Quiet));
            let mut a = Authority::new(schema.clone(), d.closures().clone());
            a.hold(d.native_list());
            if let Some(log) = log {
                a.store = log.state_at(log.head_seq()).unwrap();
                a.log = log;
            }
            let server = Server::open(trusting(), open_access(), relay.clone(), a);
            let mut hub = Hub::new(server, relay, Some(dir.to_path_buf()), Some(file)).unwrap();
            hub.hook(vec![
                (
                    "create_playlist".into(),
                    Box::new(move |e: &ark::log::Entry, f: &ark::log::Facts| {
                        heard.lock().unwrap().push(format!("{} {}", e.args["name"].as_text(), f.len()));
                    }) as crate::hooks::Hook,
                ),
                (
                    "create_playlist".into(),
                    Box::new(move |_: &ark::log::Entry, _: &ark::log::Facts| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        panic!("a hook that fails");
                    }),
                ),
            ]);
            hub
        };
        let heard = Arc::new(Mutex::new(vec![]));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut hub = open(dir.path(), heard.clone(), calls.clone());
        let mut alice = Wire::attach(&mut hub, 1, "alice");
        alice.peer.connected();
        alice.settle(&mut hub);
        let wait_for = |n: usize| {
            let t = Instant::now();
            while heard.lock().unwrap().len() < n && t.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(5));
            }
        };

        std::fs::create_dir(crate::persist::journal_path_of(dir.path())).unwrap();
        alice.peer.mutate("create_playlist", args([("name", Value::text("One"))])).unwrap();
        alice.settle(&mut hub);
        std::thread::sleep(Duration::from_millis(200));
        assert!(heard.lock().unwrap().is_empty(), "nothing heard of an entry the disk does not hold");
        std::fs::remove_dir(crate::persist::journal_path_of(dir.path())).unwrap();
        hub.after();
        wait_for(1);
        assert_eq!(*heard.lock().unwrap(), vec!["One 1".to_string()], "heard once durable, with its facts");

        let playlist = hub.authority().store.scan("playlist")[0].get("id").cloned().unwrap();
        alice
            .peer
            .mutate("add_to_playlist", args([("playlist_id", playlist), ("track_id", Value::text("t1"))]))
            .unwrap();
        alice.peer.mutate("create_playlist", args([("name", Value::text("Two"))])).unwrap();
        alice.settle(&mut hub);
        wait_for(2);
        assert_eq!(
            *heard.lock().unwrap(),
            vec!["One 1".to_string(), "Two 1".to_string()],
            "and of no other function"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2, "the failing hook was asked both times, and failed both");
        drop(alice);
        drop(hub);

        let again = Arc::new(Mutex::new(vec![]));
        let mut hub = open(dir.path(), again.clone(), Arc::new(AtomicUsize::new(0)));
        hub.after();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(hub.authority().log.head_seq(), 3);
        assert!(again.lock().unwrap().is_empty(), "a log found at start was committed by an earlier one");
    }

    /// §R3 A disk that stays unwritable does not hold the acks for ever:
    /// past the bound every connection is closed with the reason, the peer
    /// keeps its intent pending and dials again, and once the obstacle is
    /// gone its re-push is acknowledged — once: the log holds one entry.
    /// The bound is 100ms here, [`GIVE_UP_AFTER`] in a server. Falsified by
    /// never closing (`close_for_the_disk` doing nothing): the link is
    /// still open, reason-less, at the ten-second deadline.
    #[test]
    fn a_disk_that_stays_unwritable_closes_every_connection() {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let dir = tempfile::tempdir().unwrap();
        let journal = crate::persist::journal_path_of(dir.path());
        let (data, make_schema, make_domain) = (dir.path().to_path_buf(), schema.clone(), d.clone());
        let hub = HubHandle::spawn(move || {
            let (file, _) = LogFile::open(&data, &make_schema)?;
            let relay = Relay::new(Box::new(crate::Quiet));
            let mut a = Authority::new(make_schema.clone(), make_domain.closures().clone());
            a.hold(make_domain.native_list());
            let server = Server::open(trusting(), open_access(), relay.clone(), a);
            let mut h = Hub::new(server, relay, Some(data), Some(file))?;
            h.give_up = Duration::from_millis(100);
            Ok(h)
        })
        .unwrap();
        let quick = ark_client::Timing {
            first_backoff_ms: 5,
            max_backoff_ms: 50,
            ping_every_ms: 60_000,
            connect_timeout_ms: 2_000,
        };
        let mut peer = Device::open_memory(d.clone(), Options::dev("alice").with_timing(quick)).unwrap();
        peer.connect_with("local", hub.dial());
        let deadline = Instant::now() + Duration::from_secs(10);
        let pump_until = |peer: &mut Device, done: &dyn Fn(&Device) -> bool, what: &str| {
            while !done(peer) {
                peer.pump();
                assert!(Instant::now() < deadline, "{what}: {:?}", peer.status());
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        pump_until(&mut peer, &|p| p.linked(), "linked");

        // Where the journal goes, and where a snapshot is written before its
        // rename: neither an append nor the snapshot a failed append falls
        // back on can land.
        let tmp = dir.path().join(".log.ark-log.tmp");
        std::fs::create_dir(&journal).unwrap();
        std::fs::create_dir(&tmp).unwrap();
        let id = peer.mutate("create_playlist", args([("name", Value::text("Kept"))])).unwrap();
        pump_until(
            &mut peer,
            &|p| p.status().last_close.is_some_and(|w| w.starts_with("the log cannot be written")),
            "closed for the disk",
        );
        assert_eq!(peer.pending_len(), 1, "the intent is still pending");
        assert!(hub.failure().is_some());

        std::fs::remove_dir(&journal).unwrap();
        std::fs::remove_dir(&tmp).unwrap();
        pump_until(&mut peer, &|p| p.pending_len() == 0, "re-pushed and acknowledged");
        assert_eq!(peer.standing(&id), ark_client::Standing::Confirmed);
        assert_eq!(peer.cursor(), 1);
        let head = hub.read_blocking(|h| h.authority().log.head_seq()).unwrap();
        assert_eq!(head, 1, "acknowledged once: one entry");
        let on_disk = crate::persist::load(dir.path(), &schema).unwrap().expect("a log");
        assert_eq!((on_disk.head_seq(), on_disk.ids.len()), (1, 1));
        assert!(hub.failure().is_none(), "the stretch is over");
    }

    /// R6: a revoked session's connections are told `Denied` with
    /// [`REVOKED`] at once and taken off the machine, and nobody else's
    /// are: alice's two devices are two logins, and revoking one leaves the
    /// other linked and syncing. What the revoked one says after is not
    /// taken — its next intent stays pending and the log does not move.
    /// Falsified by `Hub::revoked` doing nothing: the first device is still
    /// linked, and undenied, at the ten-second deadline.
    #[test]
    fn a_revoked_login_is_closed_and_no_other() {
        let d = demo::domain();
        let make_domain = d.clone();
        // A token is `user:session`.
        let by_token: ark::protocol::Authenticate = Box::new(|t| {
            let (user, session) = t?.split_once(':')?;
            Some(Identity::new(user, session))
        });
        let hub = HubHandle::spawn(move || {
            let relay = Relay::new(Box::new(crate::Quiet));
            let schema = make_domain.module().schema.clone();
            let mut a = Authority::new(schema, make_domain.closures().clone());
            a.hold(make_domain.native_list());
            let server = Server::open(by_token, open_access(), relay.clone(), a);
            Hub::new(server, relay, None, None)
        })
        .unwrap();
        let quick = ark_client::Timing {
            first_backoff_ms: 5,
            max_backoff_ms: 50,
            ping_every_ms: 60_000,
            connect_timeout_ms: 2_000,
        };
        let device = |session: &str| {
            let o = Options::server("alice", session, Some(format!("alice:{session}"))).with_timing(quick.clone());
            let mut p = Device::open_memory(d.clone(), o).unwrap();
            p.connect_with("local", hub.dial());
            p
        };
        let (mut one, mut two) = (device("one"), device("two"));
        let deadline = Instant::now() + Duration::from_secs(10);
        let pump_until = |ps: &mut [&mut Device], done: &dyn Fn(&[&mut Device]) -> bool, what: &str| {
            while !done(ps) {
                for p in ps.iter_mut() {
                    p.pump();
                }
                assert!(
                    Instant::now() < deadline,
                    "{what}: {:?}",
                    ps.iter().map(|p| p.status()).collect::<Vec<_>>()
                );
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        pump_until(&mut [&mut one, &mut two], &|ps| ps.iter().all(|p| p.linked()), "both linked");

        hub.revoker()("alice", "one");
        pump_until(&mut [&mut one, &mut two], &|ps| ps[0].denied().is_some(), "the revoked one is told");
        assert_eq!(one.denied(), Some(REVOKED));
        assert!(!one.linked());
        one.mutate("create_playlist", args([("name", Value::text("After"))])).unwrap();
        two.mutate("create_playlist", args([("name", Value::text("Still"))])).unwrap();
        pump_until(&mut [&mut one, &mut two], &|ps| ps[1].pending_len() == 0, "the other syncs");
        for _ in 0..20 {
            one.pump();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!((two.linked(), two.denied()) == (true, None));
        assert_eq!(one.pending_len(), 1, "nothing the revoked one says is taken");
        let head = hub.read_blocking(|h| h.authority().log.head_seq()).unwrap();
        assert_eq!(head, 1);
    }

    // -- retention (`docs/plan-perf.md` R10) ------------------------------------

    /// A hub over `dir` keeping `retain`, trusting: every login is `(name, dev)`.
    fn retaining(dir: &std::path::Path, retain: Retention) -> Hub {
        let d = demo::domain();
        let schema = d.module().schema.clone();
        let (file, log) = LogFile::open(dir, &schema).unwrap();
        let relay = Relay::new(Box::new(crate::Quiet));
        let mut a = Authority::new(schema.clone(), d.closures().clone());
        a.hold(d.native_list());
        if let Some(log) = log {
            a.store = log.state_at(log.head_seq()).unwrap();
            a.log = log;
        }
        let server = Server::open(trusting(), open_access(), relay.clone(), a);
        let mut hub = Hub::new(server, relay, Some(dir.to_path_buf()), Some(file)).unwrap();
        hub.retention = retain;
        hub
    }

    /// A sans-io device on connection `c` of `hub`.
    struct Wire {
        c: ConnId,
        rx: std::sync::mpsc::Receiver<ServerMsg>,
        peer: Device,
        /// Every frame it was sent, oldest first.
        heard: Vec<ServerMsg>,
    }

    impl Wire {
        fn attach(hub: &mut Hub, c: ConnId, user: &str) -> Wire {
            let (tx, rx) = std::sync::mpsc::channel();
            hub.attach(c, Sink::Local(tx));
            let peer = Device::open_memory(demo::domain(), Options::dev(user)).unwrap();
            Wire { c, rx, peer, heard: vec![] }
        }

        /// Everything each side says, until neither says anything.
        fn settle(&mut self, hub: &mut Hub) {
            loop {
                let out = self.peer.take_outgoing();
                for m in &out {
                    hub.recv(self.c, m.clone());
                }
                let heard: Vec<ServerMsg> = self.rx.try_iter().collect();
                if out.is_empty() && heard.is_empty() {
                    return;
                }
                for m in &heard {
                    self.peer.recv(m.clone());
                }
                self.heard.extend(heard);
            }
        }

        /// `n` playlists authored offline, then one reconnect: one `Push`
        /// of all of them, and the pages back — a batch of appends as a
        /// peer that was away delivers it, without a sync per entry.
        fn burst(&mut self, hub: &mut Hub, from: u32, n: u32) {
            self.peer.disconnected();
            for i in from..from + n {
                self.peer
                    .mutate("create_playlist", args([("name", Value::text(format!("p{i:05}")))]))
                    .unwrap();
            }
            self.peer.connected();
            self.settle(hub);
            assert_eq!(self.peer.pending_len(), 0);
        }
    }

    /// R10, the guard: a hub that has sequenced 30,000 entries with every
    /// peer caught up holds [`RETAIN_ENTRIES`] of them in memory, give or
    /// take the half again it waits for before compacting —
    /// `log.entries.len()`, counted: 12,000 — and serves a fresh peer at cursor 0
    /// the snapshot, from which it reaches the head with the same state;
    /// and a hub started again over the directory holds the same horizon
    /// and the same cursors. Falsified by `retain_log` doing nothing: all
    /// 30,000 are held; by `Hub::new` not reading `cursors.cbor`: the
    /// reopened cursors are empty; and by the machine not following a
    /// snapshot with the page above it (`Server::fanout`): the fresh peer
    /// stops at 18,000; and by the hub not folding the ids after a
    /// compaction (`docs/plan-db.md` D6): every id is held whole, 30,000
    /// exact and none by its key.
    ///
    /// [`RETAIN_ENTRIES`]: ark::retention::RETAIN_ENTRIES
    #[test]
    fn a_caught_up_hub_holds_retain_entries_and_serves_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = retaining(dir.path(), Retention::default());
        let mut alice = Wire::attach(&mut hub, 1, "alice");
        alice.peer.connected();
        alice.settle(&mut hub);
        let t = Instant::now();
        for k in 0..30 {
            alice.burst(&mut hub, k * 1000, 1000);
        }
        let log = &hub.authority().log;
        let disk = |f: &str| std::fs::metadata(dir.path().join(f)).map_or(0, |m| m.len());
        eprintln!(
            "R10: 30,000 entries sequenced in {:.1?}; {} held in memory, horizon {}, {} ids; \
             on disk the snapshot is {} bytes and the journal {}",
            t.elapsed(),
            log.entries.len(),
            log.horizon(),
            log.id_count(),
            disk(crate::persist::FILE),
            disk(crate::persist::JOURNAL)
        );
        assert_eq!(log.head_seq(), 30_000);
        assert_eq!(alice.peer.cursor(), 30_000, "caught up");
        // Compacted at 16,000, 22,000 and 28,000 — each time the log held
        // more than half again as much as it keeps — to 10,000 below the
        // head; two bursts since the last.
        let keep = ark::retention::RETAIN_ENTRIES as usize;
        assert!((keep..=keep + keep / 2).contains(&log.entries.len()), "{}", log.entries.len());
        assert_eq!((log.horizon(), log.entries.len()), (18_000, 12_000));
        assert_eq!(log.id_count(), 30_000, "every id is kept below the horizon");
        // Exactly above it, by key below it (`ark::log::Below`, D6).
        assert_eq!((log.ids.len(), log.below.len()), (12_000, 18_000));
        let session = ("alice".to_string(), "dev".to_string());
        assert!(hub.cursors()[&session].cursor >= 29_000, "{:?}", hub.cursors());

        let mut fresh = Wire::attach(&mut hub, 2, "bob");
        fresh.peer.connected();
        fresh.settle(&mut hub);
        assert!(
            matches!(fresh.heard.first(), Some(ServerMsg::SnapshotOf { seq: 18_000, .. })),
            "a peer at 0 is served the snapshot: {:?}",
            fresh.heard.first().map(|m| format!("{m:?}").chars().take(80).collect::<String>())
        );
        assert_eq!(fresh.peer.cursor(), 30_000);
        assert_eq!(
            ark::hash::state_hash(&fresh.peer.replica().confirmed),
            ark::hash::state_hash(&hub.authority().store)
        );

        let (cursors, log) = (hub.cursors().clone(), hub.authority().log.clone());
        drop(hub);
        let again = retaining(dir.path(), Retention::default());
        assert_eq!(again.authority().log, log, "the horizon and the entries above it");
        assert_eq!(again.cursors(), &cursors, "the cursors");
    }

    /// R10: a session heard yesterday at cursor 100 keeps every entry
    /// above 100, however far the head runs past the floor; the same
    /// session heard thirty-one days ago does not, and the log is
    /// compacted to its floor. The session is written into `cursors.cbor`
    /// before the hub starts, as a restart finds it. Falsified by leaving
    /// the recorded sessions out of the rule: yesterday's case compacts
    /// to 950.
    #[test]
    fn a_session_heard_yesterday_keeps_the_log_above_its_cursor() {
        let small = Retention { entries: 50, days: 30 };
        for (ago, horizon) in [(1, 0), (31, 950)] {
            let dir = tempfile::tempdir().unwrap();
            let phone = ("carol".to_string(), "phone".to_string());
            let heard = Heard {
                cursor: 100,
                at_ms: retain::now_ms() - ago * ark::retention::DAY_MS,
            };
            retain::save(dir.path(), &[(phone.clone(), heard)].into()).unwrap();
            let mut hub = retaining(dir.path(), small);
            let mut alice = Wire::attach(&mut hub, 1, "alice");
            alice.peer.connected();
            alice.settle(&mut hub);
            alice.burst(&mut hub, 0, 1000);
            let log = &hub.authority().log;
            assert_eq!((log.head_seq(), log.horizon()), (1000, horizon), "heard {ago} days ago");
            if ago == 1 {
                assert!((101..=1000).all(|n| log.entries.contains_key(&n)), "everything above 100");
            }
            assert_eq!(hub.cursors()[&phone], heard, "a session nobody heard again stays as it was");
        }
    }
}
