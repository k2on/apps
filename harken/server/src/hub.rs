//! The one `ark::protocol::Server` machine, and the thread it lives on.
//!
//! The machine is sans-io and single-owner. It could sit behind a mutex —
//! its callbacks are `Send + Sync` — but a mutex held across a disk write
//! stalls every handler waiting on it; instead one thread owns it and every
//! transport — a WebSocket handler, the scanner's in-process peer,
//! `/healthz` — talks to it through a [`HubHandle`]. That thread is also
//! where the log is written after a batch of appends, so a disk
//! write never sits on the reactor.
//!
//! A connection is a [`ConnId`] and a sender: every frame the machine
//! queues for that connection goes down the sender to whatever writes the
//! socket. A *local* connection has no sender; its frames wait in an inbox
//! until the next [`HubHandle::exchange`], which is how the scanner is a
//! peer without a socket.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread;

use anyhow::{anyhow, Context, Result};
use ark::live::{ConnId, Silent};
use ark::log::Seq;
use ark::protocol::{ClientMsg, Identity, Server, ServerMsg};
use ark::store::Row;
use tokio::sync::{mpsc, oneshot};

/// What the machine owns, on its thread.
pub struct Hub {
    server: Server<Silent>,
    senders: BTreeMap<ConnId, mpsc::UnboundedSender<ServerMsg>>,
    local: BTreeMap<ConnId, Vec<ServerMsg>>,
    next_conn: ConnId,
    data: Option<PathBuf>,
    /// The head as last written, so the log is rewritten only when it moved.
    head: Seq,
}

/// What `/healthz` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub connections: usize,
    /// The head of the log.
    pub head: Seq,
}

impl Hub {
    /// Own a server with its authority. `data` is where the log is
    /// written; `None` keeps nothing.
    pub fn new(server: Server<Silent>, data: Option<PathBuf>) -> Hub {
        let head = server.authority.log.head_seq();
        Hub {
            server,
            senders: BTreeMap::new(),
            local: BTreeMap::new(),
            next_conn: 1,
            data,
            head,
        }
    }

    fn fresh(&mut self) -> ConnId {
        let c = self.next_conn;
        self.next_conn += 1;
        c
    }

    /// A socket opened: its frames go down `tx`.
    pub fn connect(&mut self, tx: mpsc::UnboundedSender<ServerMsg>) -> ConnId {
        let c = self.fresh();
        self.senders.insert(c, tx);
        c
    }

    /// An in-process peer: its frames wait for `exchange`.
    pub fn connect_local(&mut self) -> ConnId {
        let c = self.fresh();
        self.local.insert(c, vec![]);
        c
    }

    /// A connection closed: the machine hears it, and nothing more is
    /// queued for it.
    pub fn disconnect(&mut self, c: ConnId) {
        self.server.disconnect(c);
        self.senders.remove(&c);
        self.local.remove(&c);
        self.route();
    }

    /// A frame from a socket: run it, deliver everything the machine
    /// queued, and write whatever log moved.
    pub fn recv(&mut self, c: ConnId, msg: ClientMsg) {
        self.server.recv(c, msg);
        self.route();
        self.persist();
    }

    /// The in-process shape: these frames in, and this connection's frames
    /// out, with every other connection's delivered along the way.
    pub fn exchange(&mut self, c: ConnId, msgs: Vec<ClientMsg>) -> Vec<ServerMsg> {
        for m in msgs {
            self.server.recv(c, m);
        }
        self.route();
        self.persist();
        self.local
            .get_mut(&c)
            .map(std::mem::take)
            .unwrap_or_default()
    }

    pub fn health(&self) -> Health {
        Health {
            connections: self.senders.len() + self.local.len(),
            head: self.server.authority.log.head_seq(),
        }
    }

    pub fn identity(&self, c: ConnId) -> Option<Identity> {
        self.server.identity(c).cloned()
    }

    /// The rows of a table at the head of the log, as the authority holds
    /// them.
    pub fn rows(&self, table: &str) -> Vec<Row> {
        use ark::store::Store;
        self.server.authority.store.scan(table)
    }

    fn route(&mut self) {
        for (c, m) in self.server.take_outgoing() {
            if let Some(tx) = self.senders.get(&c) {
                // A writer that has gone is a disconnect on its way.
                let _ = tx.send(m);
            } else if let Some(inbox) = self.local.get_mut(&c) {
                inbox.push(m);
            }
        }
    }

    fn persist(&mut self) {
        let Some(dir) = &self.data else { return };
        let a = &self.server.authority;
        let head = a.log.head_seq();
        if self.head == head {
            return;
        }
        match crate::persist::save(dir, crate::LOG, &a.log) {
            Ok(()) => self.head = head,
            Err(e) => eprintln!("harken-server: could not write the log: {e:#}"),
        }
    }
}

enum Cmd {
    Connect(mpsc::UnboundedSender<ServerMsg>, oneshot::Sender<ConnId>),
    ConnectLocal(oneshot::Sender<ConnId>),
    Disconnect(ConnId),
    Recv(ConnId, ClientMsg),
    Exchange(ConnId, Vec<ClientMsg>, oneshot::Sender<Vec<ServerMsg>>),
    Health(oneshot::Sender<Health>),
    Identity(ConnId, oneshot::Sender<Option<Identity>>),
    Rows(String, oneshot::Sender<Vec<Row>>),
}

/// The async side's handle on the hub's thread. Cheap to clone; the thread
/// ends when the last one is dropped.
#[derive(Clone)]
pub struct HubHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl HubHandle {
    /// Build the hub on a thread of its own — the machine is neither
    /// `Send` nor `Sync`, so it is born where it lives — and hand back the
    /// way to reach it, or whatever building it failed with.
    pub fn spawn(make: impl FnOnce() -> Result<Hub> + Send + 'static) -> Result<HubHandle> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
        let (built, ready) = std::sync::mpsc::channel::<Result<()>>();
        thread::Builder::new()
            .name("harken-hub".into())
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
                        Cmd::Connect(sender, reply) => {
                            let _ = reply.send(hub.connect(sender));
                        }
                        Cmd::ConnectLocal(reply) => {
                            let _ = reply.send(hub.connect_local());
                        }
                        Cmd::Disconnect(c) => hub.disconnect(c),
                        Cmd::Recv(c, m) => hub.recv(c, m),
                        Cmd::Exchange(c, ms, reply) => {
                            let _ = reply.send(hub.exchange(c, ms));
                        }
                        Cmd::Health(reply) => {
                            let _ = reply.send(hub.health());
                        }
                        Cmd::Identity(c, reply) => {
                            let _ = reply.send(hub.identity(c));
                        }
                        Cmd::Rows(table, reply) => {
                            let _ = reply.send(hub.rows(&table));
                        }
                    }
                }
            })
            .context("spawning the hub thread")?;
        ready
            .recv()
            .map_err(|_| anyhow!("the hub thread ended before it was built"))??;
        Ok(HubHandle { tx })
    }

    fn send(&self, cmd: Cmd) -> Result<()> {
        self.tx
            .send(cmd)
            .map_err(|_| anyhow!("the hub has stopped"))
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        self.send(make(reply))?;
        answer.await.map_err(|_| anyhow!("the hub has stopped"))
    }

    pub async fn connect(&self, tx: mpsc::UnboundedSender<ServerMsg>) -> Result<ConnId> {
        self.ask(|r| Cmd::Connect(tx, r)).await
    }

    pub async fn connect_local(&self) -> Result<ConnId> {
        self.ask(Cmd::ConnectLocal).await
    }

    pub fn disconnect(&self, c: ConnId) -> Result<()> {
        self.send(Cmd::Disconnect(c))
    }

    pub fn recv(&self, c: ConnId, msg: ClientMsg) -> Result<()> {
        self.send(Cmd::Recv(c, msg))
    }

    pub async fn exchange(&self, c: ConnId, msgs: Vec<ClientMsg>) -> Result<Vec<ServerMsg>> {
        self.ask(|r| Cmd::Exchange(c, msgs, r)).await
    }

    pub async fn health(&self) -> Result<Health> {
        self.ask(Cmd::Health).await
    }

    pub async fn identity(&self, c: ConnId) -> Result<Option<Identity>> {
        self.ask(|r| Cmd::Identity(c, r)).await
    }

    pub async fn rows(&self, table: &str) -> Result<Vec<Row>> {
        self.ask(|r| Cmd::Rows(table.into(), r)).await
    }
}
