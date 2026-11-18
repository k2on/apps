//! A test upstream for the Pebble host, playing the server for the demo
//! watchapp: the kernel's [`Authority`] over a WebSocket, which the
//! watch's relay script connects to, and beside it a player, a [`Node`]
//! of the same user standing in for harken's phone, which owns playback.
//!
//! The player seeds the queue, plays its first track, and when a track
//! ends pops the next and plays it, adding tracks as the queue runs low.
//! Its playback is a session row set whole, an anchor the watch
//! extrapolates; the watch's toggles and skips reach it the same way.
//!
//! ```text
//! now-playing-upstream [--port 9010]
//! ```
//!
//! Lines on standard input steer it: `toggle`, `skip`, `add TITLE`,
//! `offline` (close every connection and refuse new ones, as a phone that
//! lost the server), `online`, and `state`. It logs every frame either way.

use std::collections::HashMap;
use std::io::BufRead;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ark_kernel::codec::{from_slice, put_frame};
use ark_kernel::serve::Out;
use ark_kernel::{Actor, Anchor, Authority, Conn, Down, Entry, Frames, Memory, Node, Origin, Read, Space, Spaces, Up, Verdict};
use now_playing::{playing, Msg, Playing, Track, HANDLERS, QUERIES, QUEUE, VERSION};
use tungstenite::handshake::server::{Request, Response};
use tungstenite::{Message, WebSocket};

/// The player's connection, in the process.
const PLAYER: Conn = Conn(0);
/// The one user: the watch's relay signs it in as this actor.
const USER: Actor = Actor(1);
/// Keep the queue at least this long.
const LOW: usize = 6;

const CATALOG: &[(&str, &str, u32)] = &[
    ("Lantern Weather", "The Quiet Fleet", 38_000),
    ("Paper Harbour", "Juniper Lane", 44_000),
    ("Static Bloom", "Northbound Owls", 31_000),
    ("Slow Orbit", "Mara Quill", 47_000),
    ("Copper Tide", "The Field Recordings", 35_000),
    ("Night Ferry", "Ada Moraine", 41_000),
    ("Glass Meridian", "Low Pines", 29_000),
    ("Second Summer", "The Quiet Fleet", 52_000),
    ("Under Lamplight", "Juniper Lane", 36_000),
    ("Kite String", "Northbound Owls", 33_000),
    ("Salt Road", "Mara Quill", 45_000),
    ("Window Seat", "Ada Moraine", 39_000),
];

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn stamp() -> String {
    let ms = now() % 86_400_000;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

macro_rules! say {
    ($($t:tt)*) => { eprintln!("[{}] {}", stamp(), format!($($t)*)) };
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn body(m: &Msg) -> String {
    match m {
        Msg::Add { at, track } => format!("Add {at} {:?}", text(&track.title)),
        Msg::Pop { at } => format!("Pop {at}"),
        Msg::Play { playing: p, .. } => format!(
            "Play {:?} at {} ms {}",
            text(&p.track.title),
            p.anchor.at,
            if p.anchor.playing { "playing" } else { "paused" }
        ),
    }
}

fn entry(e: &Entry<Msg>) -> String {
    format!("{:x}/{} {}", e.id.origin.0, e.id.n, body(&e.body))
}

fn up(u: &Up<Msg>) -> String {
    match u {
        Up::Hello { cursor, time } => format!("Hello cursor {} at its {time}", cursor.0),
        Up::Push(e) => format!("Push {}", entry(e)),
        Up::Live(e) => format!("Live {}", entry(e)),
        Up::Rate { table, every } => format!("Rate table {} every {every}", table.0),
        Up::Subscribe { query, .. } => format!("Subscribe query {}", query.0),
        Up::Unsubscribe { query, .. } => format!("Unsubscribe query {}", query.0),
    }
}

fn down(d: &Down<Msg>) -> String {
    match d {
        Down::Logged(l) => format!(
            "Logged seq {} {} ({} facts)",
            l.entry.seq.map_or(0, |s| s.0),
            entry(&l.entry),
            l.facts.0.len()
        ),
        Down::Verdict { id, verdict } => match verdict {
            Verdict::At { seq, last } => format!("Verdict {:x}/{} at seq {} last {last}", id.origin.0, id.n, seq.0),
            Verdict::Refused(r) => format!("Verdict {:x}/{} refused {}", id.origin.0, id.n, r.0),
        },
        Down::Live(e) => format!("Live {}", entry(e)),
        Down::Fill { seq, tables, parts } => format!("Fill to seq {} emptying {} tables, {parts} parts", seq.0, tables.len()),
        Down::Rows(w) => format!("Rows {}", w.0.len()),
        Down::Moved(w) => format!("Moved {} rows", w.0.len()),
        Down::Got { query, .. } => format!("Got query {}", query.0),
        Down::Welcome { sent, time } => format!("Welcome: its {sent}, our {time} ({} ms ahead)", *time as i64 - *sent as i64),
    }
}

enum Event {
    Open(Conn, Option<Origin>, mpsc::Sender<ToConn>),
    Bytes(Conn, Vec<u8>),
    Closed(Conn),
    Line(String),
}

enum ToConn {
    Bytes(Vec<u8>),
    Close,
}

struct Client {
    tx: mpsc::Sender<ToConn>,
    frames: Frames,
    /// What is to go down at the end of this turn: one WebSocket message.
    out: Vec<u8>,
}

struct Server {
    authority: Authority<Msg, Memory>,
    player: Node<Msg, Memory>,
    clients: HashMap<u32, Client>,
    next_at: u32,
    added: usize,
    offline: Arc<AtomicBool>,
}

fn spaces() -> Spaces<Memory> {
    Spaces {
        local: Memory::new(),
        device: Memory::new(),
        session: Memory::new(),
        shared: Memory::new(),
    }
}

impl Server {
    fn new(offline: Arc<AtomicBool>) -> Server {
        let mut authority = Authority::new(&HANDLERS, Memory::new());
        authority.set_queries(&QUERIES);
        // A fill's part carries at most this many rows, so each frame stays
        // well within the watch's inbox.
        authority.set_part(8);
        // A watch's record is kept ten minutes once it goes, so that one
        // that connects again is sent only what changed.
        authority.set_ttl(60_000, 600_000);
        let mut player = Node::new(Origin(0x9a7e_0000_0000_0001), 0, VERSION, &HANDLERS, spaces());
        player.set_actor(USER);
        let mut s = Server {
            authority,
            player,
            clients: HashMap::new(),
            next_at: 1,
            added: 0,
            offline,
        };
        s.authority.open(PLAYER, USER, Some(s.player.origin()));
        let mut up = Vec::new();
        s.player.connect(&mut up, now());
        s.relay_player(up);
        while s.queue().len() < LOW + 2 {
            s.add(None);
        }
        s.advance();
        s
    }

    /// The player's queue: each place and track.
    fn queue(&self) -> Vec<(u32, Track)> {
        let mut out = Vec::new();
        self.player.view(Space::Shared).scan(QUEUE, ark_kernel::store::Range::all(), &mut |k, v| {
            if let (Some(at), Ok(t)) = (now_playing::place(k), from_slice::<Track>(v)) {
                out.push((at, t));
            }
            true
        });
        out
    }

    fn playing(&self) -> Option<Playing> {
        playing(&self.player.view(Space::Session)).map(|(_, p)| p)
    }

    /// The tree's time: the server's clock, as the player measured it
    /// (here, in the same process, its own).
    fn time(&self) -> u64 {
        self.player.time(now())
    }

    /// Sets the player's playback whole.
    fn play(&mut self, playing: Playing) {
        let player = self.player.origin();
        self.make(Space::Session, Msg::Play { player, playing });
    }

    fn make(&mut self, space: Space, m: Msg) {
        let e = self.player.make(space, m, now());
        say!("player makes {}", entry(&e));
        let mut up = Vec::new();
        self.player.route(e, &mut up);
        self.relay_player(up);
    }

    fn add(&mut self, title: Option<String>) {
        let (t, a, ms) = CATALOG[self.added % CATALOG.len()];
        self.added += 1;
        let track = Track {
            title: title.map_or_else(|| t.as_bytes().to_vec(), String::into_bytes),
            artist: a.as_bytes().to_vec(),
            ms,
        };
        let at = self.next_at;
        self.next_at += 1;
        self.make(Space::Shared, Msg::Add { at, track });
    }

    /// Plays the next track from its start, taking it off the queue.
    fn advance(&mut self) {
        let Some((at, track)) = self.queue().into_iter().next() else { return };
        self.make(Space::Shared, Msg::Pop { at });
        let anchor = Anchor {
            at: 0,
            wall: self.time(),
            rate: 1000,
            playing: true,
        };
        self.play(Playing { track, anchor });
    }

    fn toggle(&mut self) {
        let Some(p) = self.playing() else { return };
        let t = self.time();
        let anchor = Anchor {
            at: p.anchor.position(t).min(u64::from(p.track.ms)),
            wall: t,
            rate: p.anchor.rate,
            playing: !p.anchor.playing,
        };
        self.play(Playing { track: p.track, anchor });
    }

    /// Passes what the player said to the authority, and its answers on,
    /// until nothing is left to say.
    fn relay_player(&mut self, mut up: Vec<Up<Msg>>) {
        while !up.is_empty() {
            for u in std::mem::take(&mut up) {
                let outs = self.authority.receive(PLAYER, u, now());
                self.deliver(outs, &mut up);
            }
        }
    }

    fn deliver(&mut self, outs: Vec<Out<Msg>>, up: &mut Vec<Up<Msg>>) {
        for (conn, d) in outs {
            if conn == PLAYER {
                self.player.receive(d, up, now());
            } else if let Some(c) = self.clients.get_mut(&conn.0) {
                say!("  conn {} <- {}", conn.0, down(&d));
                put_frame(&mut c.out, &d);
            }
        }
    }

    /// Sends each client what this turn has for it.
    fn send(&mut self) {
        for c in self.clients.values_mut() {
            if !c.out.is_empty() {
                let _ = c.tx.send(ToConn::Bytes(std::mem::take(&mut c.out)));
            }
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Open(conn, origin, tx) => {
                if self.offline.load(Ordering::SeqCst) {
                    let _ = tx.send(ToConn::Close);
                    return;
                }
                say!("conn {} opened, client {:x}", conn.0, origin.map_or(0, |o| o.0));
                self.authority.open(conn, USER, origin);
                self.clients.insert(
                    conn.0,
                    Client {
                        tx,
                        frames: Frames::new(1 << 16),
                        out: Vec::new(),
                    },
                );
            }
            Event::Bytes(conn, bytes) => {
                let Some(c) = self.clients.get_mut(&conn.0) else { return };
                c.frames.push(&bytes);
                let mut ups = Vec::new();
                while let Ok(Some(u)) = c.frames.read::<Up<Msg>>() {
                    ups.push(u);
                }
                for u in ups {
                    say!("conn {} -> {}", conn.0, up(&u));
                    let outs = self.authority.receive(conn, u, now());
                    let mut more = Vec::new();
                    self.deliver(outs, &mut more);
                    self.relay_player(more);
                }
            }
            Event::Closed(conn) => {
                if self.clients.remove(&conn.0).is_some() {
                    say!("conn {} closed", conn.0);
                    self.authority.close(conn, now());
                }
            }
            Event::Line(line) => self.command(line.trim()),
        }
    }

    fn command(&mut self, line: &str) {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        match cmd {
            "toggle" | "t" => self.toggle(),
            "skip" | "n" => self.advance(),
            "add" | "a" => self.add((!rest.is_empty()).then(|| rest.to_string())),
            "offline" => {
                self.offline.store(true, Ordering::SeqCst);
                for (id, c) in self.clients.drain() {
                    let _ = c.tx.send(ToConn::Close);
                    self.authority.close(Conn(id), now());
                }
                say!("offline: every connection closed, new ones refused");
            }
            "online" => {
                self.offline.store(false, Ordering::SeqCst);
                say!("online");
            }
            "state" | "s" => self.state(),
            "" => {}
            _ => say!("commands: toggle, skip, add TITLE, offline, online, state"),
        }
    }

    fn state(&self) {
        let t = self.time();
        match self.playing() {
            Some(p) => say!(
                "playing {:?} at {} of {} ms ({})",
                text(&p.track.title),
                p.anchor.position(t),
                p.track.ms,
                if p.anchor.playing { "playing" } else { "paused" }
            ),
            None => say!("nothing playing"),
        }
        let q: Vec<String> = self.queue().iter().map(|(at, t)| format!("{at} {}", text(&t.title))).collect();
        say!("queue: {}", q.join(", "));
        say!(
            "log head {}, {} refusals, {} clients",
            self.authority.head().0,
            self.authority.refused().len(),
            self.clients.len()
        );
    }

    fn tick(&mut self) {
        let t = now();
        let outs = self.authority.tick(t);
        let mut up = Vec::new();
        self.deliver(outs, &mut up);
        self.relay_player(up);
        if let Some(p) = self.playing() {
            if p.anchor.playing && p.anchor.position(self.time()) >= u64::from(p.track.ms) {
                say!("player: {:?} ended", text(&p.track.title));
                self.advance();
            }
        }
        while self.queue().len() < LOW {
            self.add(None);
        }
    }
}

/// A connection's thread: the WebSocket read with a short timeout, so
/// what the server has for it goes out between reads.
#[allow(clippy::result_large_err)]
fn serve(stream: TcpStream, conn: Conn, events: mpsc::Sender<Event>) {
    let mut origin = None;
    let ws = tungstenite::accept_hdr(stream, |req: &Request, res: Response| {
        let query = req.uri().query().unwrap_or("");
        origin = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("origin="))
            .and_then(|h| u64::from_str_radix(h, 16).ok())
            .map(Origin);
        Ok(res)
    });
    let Ok(mut ws) = ws else { return };
    let _ = ws.get_mut().set_read_timeout(Some(Duration::from_millis(20)));
    let (tx, rx) = mpsc::channel();
    if events.send(Event::Open(conn, origin, tx)).is_err() {
        return;
    }
    run(&mut ws, conn, &events, &rx);
    let _ = ws.close(None);
    let _ = ws.flush();
    let _ = events.send(Event::Closed(conn));
}

fn run(ws: &mut WebSocket<TcpStream>, conn: Conn, events: &mpsc::Sender<Event>, rx: &mpsc::Receiver<ToConn>) {
    loop {
        match ws.read() {
            Ok(Message::Binary(b)) => {
                let _ = events.send(Event::Bytes(conn, b));
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
        loop {
            match rx.try_recv() {
                Ok(ToConn::Bytes(b)) => {
                    if ws.send(Message::Binary(b)).is_err() {
                        return;
                    }
                }
                Ok(ToConn::Close) | Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
    }
}

fn main() {
    let mut port = 9010u16;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--port" {
            port = args.next().and_then(|p| p.parse().ok()).unwrap_or(port);
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the port is free");
    say!("listening on ws://127.0.0.1:{port}/");
    let offline = Arc::new(AtomicBool::new(false));
    let (events, rx) = mpsc::channel();
    {
        let events = events.clone();
        thread::spawn(move || {
            let next = AtomicU32::new(1);
            for stream in listener.incoming().flatten() {
                let conn = Conn(next.fetch_add(1, Ordering::SeqCst));
                let events = events.clone();
                thread::spawn(move || serve(stream, conn, events));
            }
        });
    }
    {
        let events = events.clone();
        thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if events.send(Event::Line(line)).is_err() {
                    return;
                }
            }
        });
    }
    let mut server = Server::new(offline);
    server.state();
    loop {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
            server.event(ev);
            while let Ok(ev) = rx.try_recv() {
                server.event(ev);
            }
        }
        server.tick();
        server.send();
    }
}
