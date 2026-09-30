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
//! Three kinds of connection, one numbering: a **socket** (frames go to its
//! writer task), a **local** peer (an `ark_client::Peer` in this process,
//! through [`HubHandle::dial`]: the scanner's shape), and a **standing**
//! peer — a device the server stands in for, a speaker, which is in a room
//! and not in the log and receives only live frames ([`HubHandle::stand`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use ark::live::{self, ConnId, Peer};
use ark::log::Seq;
use ark::peer::Authority;
use ark::protocol::{ClientMsg, Identity, Server, ServerMsg};
use ark::store::{Row, Store};
use tokio::sync::{mpsc, oneshot};

use crate::live::{decode_kept, encode_kept, Relay};
use crate::persist::LogFile;

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
    /// The rooms' kept snapshots as last written.
    kept: BTreeMap<String, Vec<u8>>,
}

/// What `/healthz` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub connections: usize,
    /// The head of the log.
    pub head: Seq,
    /// Open rooms and how many peers each has.
    pub rooms: Vec<(String, usize)>,
}

const KEPT: &str = "live.cbor";

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
        Ok(Hub {
            server,
            relay,
            sinks: BTreeMap::new(),
            data,
            log,
            held: vec![],
            kept,
        })
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
            rooms: self.server.rooms.open.iter().map(|(r, (_, ps))| (r.clone(), ps.len())).collect(),
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
            self.server.recv(c, msg);
        }
        self.after();
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
    fn after(&mut self) {
        let durable = self.persist();
        self.held.extend(self.server.take_outgoing());
        if durable {
            for (c, m) in std::mem::take(&mut self.held) {
                self.send(c, m);
            }
        }
    }

    /// Write the log and the rooms; whether the log is on the disk.
    fn persist(&mut self) -> bool {
        let Some(dir) = self.data.clone() else {
            self.relay.keeps.lock().unwrap_or_else(|e| e.into_inner()).clear();
            return true;
        };
        let mut durable = true;
        if let Some(file) = &mut self.log {
            if let Err(e) = file.write(&self.server.authority.log) {
                eprintln!("ark-server: could not write the log; holding what it would say: {e:#}");
                durable = false;
            }
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
        durable
    }
}

type Reader = Box<dyn FnOnce(&Hub) + Send>;

enum Cmd {
    Attach(ConnId, Sink),
    Detach(ConnId),
    Recv(ConnId, ClientMsg),
    Stand(ConnId, String, String, Box<dyn Fn(Vec<u8>) + Send>),
    Read(Reader),
}

/// The async (and sync) side's handle on the hub's thread. Cheap to clone;
/// the thread ends when the last one is dropped.
#[derive(Clone)]
pub struct HubHandle {
    tx: mpsc::UnboundedSender<Cmd>,
    next: Arc<AtomicI64>,
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
        std::thread::Builder::new()
            .name("ark-hub".into())
            .spawn(move || {
                let mut hub = match make() {
                    Ok(h) => {
                        let _ = built.send(Ok(()));
                        h
                    }
                    Err(e) => {
                        let _ = built.send(Err(e));
                        return;
                    }
                };
                while let Some(cmd) = rx.blocking_recv() {
                    match cmd {
                        Cmd::Attach(c, s) => hub.attach(c, s),
                        Cmd::Detach(c) => hub.detach(c),
                        Cmd::Recv(c, m) => hub.recv(c, m),
                        Cmd::Stand(c, room, who, sink) => hub.stand(c, room, who, sink),
                        Cmd::Read(f) => f(&hub),
                    }
                }
            })
            .context("spawning the hub thread")?;
        ready.recv().map_err(|_| anyhow!("the hub thread ended before it was built"))??;
        Ok(HubHandle {
            tx,
            next: Arc::new(AtomicI64::new(1)),
        })
    }

    fn send(&self, cmd: Cmd) -> Result<()> {
        self.tx.send(cmd).map_err(|_| anyhow!("the hub has stopped"))
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
        while let Ok(m) = self.rx.try_recv() {
            out.push(Event::Frame(ark::canon::encode(&m.to_value())));
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
}
