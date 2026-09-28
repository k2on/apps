//! Live rooms, with the app's hooks.
//!
//! The engine's half (`ark::live`) is which connections are in which room —
//! a room is one account, every connection the server verified as the same
//! user — and a pure machine told when one arrives, speaks or leaves. This
//! is that machine for an app written the way a server is written: one
//! [`Live`] holding every room, `&mut self`, told when a room opens (with
//! what it kept, if anything), when a peer joins, speaks and leaves, and
//! asked for a snapshot to keep when a room empties.
//!
//! **Nothing here is the log.** A frame is about now; nothing replays one,
//! and no view moves for one. What survives a restart is at most one
//! snapshot per room — kept when the room empties, or at once when a hook
//! calls [`Post::keep`] — in `live.cbor` beside the logs.
//!
//! **A repeated `Hello` on one connection is not a departure.** A client
//! pages a long log by saying `Hello` again on the same socket; the engine
//! leaves a connection already standing as exactly that peer where it is,
//! so [`Live::part`] is never called for it and the room hears nothing.
//!
//! Frames are opaque bytes both ways. An ArkDB app will usually make them
//! canonical CBOR of a `Value` (`ark::canon`), which `ark_client::Peer`
//! has `say_value` and `heard_values` for; the server never looks.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark::live::{ConnId, Machine};

pub use ark::live::Peer;

/// What a hook says, and to whom.
#[derive(Debug)]
pub struct Post<'a> {
    peers: &'a [Peer],
    out: Vec<(ConnId, Vec<u8>)>,
    keep: bool,
}

impl<'a> Post<'a> {
    fn new(peers: &'a [Peer]) -> Post<'a> {
        Post {
            peers,
            out: vec![],
            keep: false,
        }
    }

    /// Everybody in this room, right now.
    pub fn peers(&self) -> &[Peer] {
        self.peers
    }

    /// Whether a peer named `who` is here.
    pub fn here(&self, who: &str) -> bool {
        self.peers.iter().any(|p| p.who == who)
    }

    /// To one peer, by the name that survives its reconnects (a login's
    /// session, or what a standing device was called). A name nobody here
    /// answers to is dropped.
    pub fn tell(&mut self, who: &str, frame: Vec<u8>) {
        for p in self.peers.iter().filter(|p| p.who == who) {
            self.out.push((p.conn, frame.clone()));
        }
    }

    /// To one connection.
    pub fn tell_conn(&mut self, conn: ConnId, frame: Vec<u8>) {
        self.out.push((conn, frame));
    }

    /// To everybody in the room, including whoever prompted it.
    pub fn tell_room(&mut self, frame: Vec<u8>) {
        for p in self.peers {
            self.out.push((p.conn, frame.clone()));
        }
    }

    /// Write this room's snapshot now, not only when it empties: what just
    /// happened is worth a disk write.
    pub fn keep(&mut self) {
        self.keep = true;
    }
}

/// The app's live machine: every room's state, and the rules that move it.
/// Called on the hub's thread, one call at a time: keep it fast, and touch
/// nothing outside it but channels.
pub trait Live: Send + 'static {
    /// A room is opening: its first peer is about to join. `kept` is what
    /// [`Live::snapshot`] last wrote for it, if anything — possibly from
    /// before a restart. A snapshot this build cannot read is one to ignore.
    fn open(&mut self, room: &str, kept: Option<&[u8]>) {
        let _ = (room, kept);
    }

    /// A peer's connection joined. It is already in [`Post::peers`].
    fn join(&mut self, peer: &Peer, post: &mut Post<'_>) {
        let _ = (peer, post);
    }

    /// A peer said something.
    fn say(&mut self, peer: &Peer, frame: &[u8], post: &mut Post<'_>);

    /// A peer's connection closed; it is already out of [`Post::peers`]. A
    /// socket going is not a device going away.
    fn part(&mut self, peer: &Peer, post: &mut Post<'_>) {
        let _ = (peer, post);
    }

    /// What to keep of a room: asked after [`Post::keep`], and when the
    /// room empties. `None` keeps nothing (and forgets what was kept).
    fn snapshot(&mut self, room: &str) -> Option<Vec<u8>> {
        let _ = room;
        None
    }

    /// Nobody is in the room any more; the snapshot has been taken. Free
    /// whatever it held.
    fn close(&mut self, room: &str) {
        let _ = room;
    }
}

/// No live protocol: frames are dropped.
pub struct Quiet;

impl Live for Quiet {
    fn say(&mut self, _: &Peer, _: &[u8], _: &mut Post<'_>) {}
}

/// Every room echoes: a frame is told to everyone else in the room. What a
/// test or a toy wants.
pub struct Echo;

impl Live for Echo {
    fn say(&mut self, peer: &Peer, frame: &[u8], post: &mut Post<'_>) {
        let others: Vec<ConnId> = post.peers().iter().filter(|p| p.conn != peer.conn).map(|p| p.conn).collect();
        for c in others {
            post.tell_conn(c, frame.to_vec());
        }
    }
}

/// The engine's machine, over the app's [`Live`]. Cloned into the engine's
/// server and kept by the hub for standing peers; both share one `Live`.
#[derive(Clone)]
pub(crate) struct Relay {
    live: Arc<Mutex<Box<dyn Live>>>,
    /// Snapshots asked for with `keep`, not yet written.
    pub(crate) keeps: Keeps,
}

/// Rooms and what a hook asked to keep of them, in order.
pub(crate) type Keeps = Arc<Mutex<Vec<(String, Option<Vec<u8>>)>>>;

pub(crate) struct RoomState {
    room: Option<String>,
    kept: Option<Vec<u8>>,
}

impl Relay {
    pub(crate) fn new(live: Box<dyn Live>) -> Relay {
        Relay {
            live: Arc::new(Mutex::new(live)),
            keeps: Arc::default(),
        }
    }

    fn live(&self) -> std::sync::MutexGuard<'_, Box<dyn Live>> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finish(&self, room: &str, post: Post<'_>) -> ark::live::Post {
        if post.keep {
            let snap = self.live().snapshot(room);
            self.keeps.lock().unwrap_or_else(|e| e.into_inner()).push((room.to_string(), snap));
        }
        ark::live::Post {
            out: post.out,
            keep: post.keep,
        }
    }
}

impl Machine for Relay {
    type State = RoomState;

    fn wake(&self, kept: Option<&[u8]>) -> RoomState {
        RoomState {
            room: None,
            kept: kept.map(<[u8]>::to_vec),
        }
    }

    fn join(&self, peers: &[Peer], p: &Peer, mut s: RoomState) -> (RoomState, ark::live::Post) {
        if s.room.is_none() {
            self.live().open(&p.room, s.kept.as_deref());
            s.room = Some(p.room.clone());
            s.kept = None;
        }
        let mut post = Post::new(peers);
        self.live().join(p, &mut post);
        (s, self.finish(&p.room, post))
    }

    fn say(&self, peers: &[Peer], p: &Peer, frame: &[u8], s: RoomState) -> (RoomState, ark::live::Post) {
        let mut post = Post::new(peers);
        self.live().say(p, frame, &mut post);
        (s, self.finish(&p.room, post))
    }

    fn part(&self, peers: &[Peer], p: &Peer, s: RoomState) -> (RoomState, ark::live::Post) {
        let mut post = Post::new(peers);
        self.live().part(p, &mut post);
        (s, self.finish(&p.room, post))
    }

    fn snapshot(&self, s: &RoomState) -> Option<Vec<u8>> {
        let room = s.room.as_deref()?;
        let mut live = self.live();
        let snap = live.snapshot(room);
        live.close(room);
        snap
    }
}

/// The kept snapshots as a file: canonical CBOR of `{ room: bytes }`.
pub(crate) fn encode_kept(kept: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    ark::canon::encode(&ark::value::Value::Struct(
        kept.iter().map(|(r, b)| (r.clone(), ark::value::Value::Bytes(b.clone()))).collect(),
    ))
}

pub(crate) fn decode_kept(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    match ark::canon::decode(bytes).map_err(|e| e.to_string())? {
        ark::value::Value::Struct(m) => m
            .into_iter()
            .map(|(r, v)| match v {
                ark::value::Value::Bytes(b) => Ok((r, b)),
                other => Err(format!("room {r} kept {other:?}, not bytes")),
            })
            .collect(),
        other => Err(format!("not a map of rooms: {other:?}")),
    }
}
