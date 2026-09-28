//! The one `ark::protocol::Server`, and the thread it lives on.
//!
//! The machine is sans-io and single-owner, and a mutex held across a disk
//! write would stall every socket waiting on it; so one thread owns it and
//! everything — a WebSocket handler, an in-process peer, a standing device,
//! `/healthz` — talks to it through a [`HubHandle`]. That thread is also
//! where each log and the rooms' kept snapshots are written, after the
//! message that moved them.
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
    /// The head of the log as last written.
    head: Seq,
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
    pub(crate) fn new(mut server: Server<Relay>, relay: Relay, data: Option<PathBuf>) -> Result<Hub> {
        let head = server.authority.log.head_seq();
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
            head,
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

    // Deliver what the machine queued, then write whatever moved.
    fn after(&mut self) {
        for (c, m) in self.server.take_outgoing() {
            self.send(c, m);
        }
        self.persist();
    }

    fn persist(&mut self) {
        let Some(dir) = self.data.clone() else {
            self.relay.keeps.lock().unwrap_or_else(|e| e.into_inner()).clear();
            return;
        };
        let head = self.server.authority.log.head_seq();
        if head != self.head {
            match crate::persist::save(&dir, &self.server.authority.log) {
                Ok(()) => self.head = head,
                Err(e) => eprintln!("ark-server: could not write the log: {e:#}"),
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
