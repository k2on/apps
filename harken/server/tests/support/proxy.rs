//! A TCP proxy the test holds the network of one peer with
//! (`docs/plan-fleet.md` §2): every way a laptop lid and a bad hotel wifi
//! take a connection away, done to the bytes between a real `harken-peer`
//! and a real `harken-server`.
//!
//! It is protocol-blind about ArkDB and knows exactly one thing about the
//! WebSocket: where a client's frame ends. That is what lets it forward
//! client frames whole — so a frame it injects ([`Proxy::replay_last`])
//! never lands in the middle of another — and remember the last binary
//! one. Everything the server sends is forwarded as it comes.
//!
//! - [`Proxy::pass`]: forward everything, now. Connections that were
//!   black-holed are cut on the way: they lost bytes, so they are streams
//!   nothing can resume, and a network that comes back is a network whose
//!   old connections are gone.
//! - [`Proxy::blackhole`]: accept and read, send nothing either way, and
//!   never close — the lid-closed case the keepalive exists for. A
//!   connection made meanwhile is accepted and treated the same.
//! - [`Proxy::cut`]: close both sides of every connection, now.
//! - [`Proxy::pause`]: hold every byte for a while, then deliver them all.
//! - [`Proxy::cut_after`]: let this many more bytes from the server
//!   through, then cut the connection mid-stream — the mid-page case.
//! - [`Proxy::delay`]: every chunk late by this much.
//! - [`Proxy::replay_last`]: send the client's last binary frame again.
//!
//! One thread accepts, two more per connection forward; each reads with a
//! short timeout so it notices being told to stop.
#![allow(dead_code)]

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long a forwarding thread blocks in a read before it looks around.
const SLICE: Duration = Duration::from_millis(20);

/// Where the proxies of one fleet forward to: the server's current
/// address, which a restart moves.
pub type Upstream = Arc<Mutex<SocketAddr>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Pass,
    Blackhole,
    /// Hold every byte until then.
    Pause(Instant),
}

struct State {
    mode: Mode,
    delay: Duration,
    /// Bytes from the server still allowed before a cut, when armed.
    cut_after: Option<usize>,
}

struct Shared {
    upstream: Upstream,
    state: Mutex<State>,
    changed: Condvar,
    conns: Mutex<Vec<Arc<Conn>>>,
    stop: AtomicBool,
    next: AtomicU64,
    /// How many `cut_after`s have fired.
    fired: AtomicUsize,
    /// Bytes the server sent that reached the client, over every connection.
    down_bytes: AtomicUsize,
    accepted: AtomicUsize,
}

/// One client connection and, unless it arrived black-holed, its server.
struct Conn {
    id: u64,
    client: TcpStream,
    server: Option<TcpStream>,
    /// It lost bytes, or never had a server: it can never carry the
    /// protocol again, and is cut when the network is given back.
    tainted: AtomicBool,
    closed: AtomicBool,
    /// Held across a whole client frame, so a replay cannot interleave.
    to_server: Mutex<()>,
    /// The last whole binary frame the client sent, as it went on the wire
    /// (masked), and its payload unmasked.
    last: Mutex<Option<(Vec<u8>, Vec<u8>)>>,
}

impl Conn {
    fn cut(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _ = self.client.shutdown(Shutdown::Both);
        if let Some(s) = &self.server {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    fn live(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }
}

/// The network of one peer.
pub struct Proxy {
    shared: Arc<Shared>,
    pub port: u16,
    accept: Option<JoinHandle<()>>,
}

impl Proxy {
    /// Listen on a free loopback port, forwarding to wherever `upstream`
    /// says when a connection arrives.
    pub fn start(upstream: Upstream) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port for a proxy");
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Shared {
            upstream,
            state: Mutex::new(State {
                mode: Mode::Pass,
                delay: Duration::ZERO,
                cut_after: None,
            }),
            changed: Condvar::new(),
            conns: Mutex::new(vec![]),
            stop: AtomicBool::new(false),
            next: AtomicU64::new(1),
            fired: AtomicUsize::new(0),
            down_bytes: AtomicUsize::new(0),
            accepted: AtomicUsize::new(0),
        });
        let accept = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name(format!("proxy-{port}"))
                .spawn(move || {
                    for stream in listener.incoming() {
                        if shared.stop.load(Ordering::SeqCst) {
                            break;
                        }
                        if let Ok(client) = stream {
                            open(&shared, client);
                        }
                    }
                })
                .unwrap()
        };
        Proxy {
            shared,
            port,
            accept: Some(accept),
        }
    }

    /// `http://127.0.0.1:PORT`: what the peer is told the server is.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn set(&self, f: impl FnOnce(&mut State)) {
        let mut st = self.shared.state.lock().unwrap();
        f(&mut st);
        self.shared.changed.notify_all();
    }

    fn conns(&self) -> Vec<Arc<Conn>> {
        let mut conns = self.shared.conns.lock().unwrap();
        conns.retain(|c| c.live());
        conns.clone()
    }

    /// Forward everything; a connection that lost bytes is cut.
    pub fn pass(&self) {
        self.set(|st| {
            st.mode = Mode::Pass;
            st.delay = Duration::ZERO;
        });
        for c in self.conns() {
            if c.tainted.load(Ordering::SeqCst) {
                c.cut();
            }
        }
    }

    /// Read everything, send nothing, close nothing.
    pub fn blackhole(&self) {
        self.set(|st| st.mode = Mode::Blackhole);
        for c in self.conns() {
            c.tainted.store(true, Ordering::SeqCst);
        }
    }

    /// Close every connection, both sides, now. New ones are let through.
    pub fn cut(&self) {
        for c in self.conns() {
            c.cut();
        }
    }

    /// Hold every byte for `d`, then deliver.
    pub fn pause(&self, d: Duration) {
        self.set(|st| st.mode = Mode::Pause(Instant::now() + d));
    }

    /// Every chunk forwarded `d` late.
    pub fn delay(&self, d: Duration) {
        self.set(|st| st.delay = d);
    }

    /// Let `n` more bytes from the server through, then cut the connection
    /// they were on, mid-stream. Once.
    pub fn cut_after(&self, n: usize) {
        self.set(|st| st.cut_after = Some(n));
    }

    /// How many `cut_after`s have fired.
    pub fn cuts(&self) -> usize {
        self.shared.fired.load(Ordering::SeqCst)
    }

    /// Bytes from the server that reached the client, so far.
    pub fn down_bytes(&self) -> usize {
        self.shared.down_bytes.load(Ordering::SeqCst)
    }

    /// Connections accepted, so far.
    pub fn accepted(&self) -> usize {
        self.shared.accepted.load(Ordering::SeqCst)
    }

    /// Connections open now.
    pub fn open(&self) -> usize {
        self.conns().len()
    }

    /// The payload of the last binary frame the client sent on the newest
    /// connection, unmasked: what [`Proxy::replay_last`] would send again.
    pub fn last_payload(&self) -> Option<Vec<u8>> {
        let c = self
            .conns()
            .into_iter()
            .rev()
            .find(|c| !c.tainted.load(Ordering::SeqCst))?;
        let last = c.last.lock().unwrap();
        last.as_ref().map(|(_, p)| p.clone())
    }

    /// Send the client's last binary frame on the newest connection to the
    /// server again, whole. Whether there was one.
    pub fn replay_last(&self) -> bool {
        let Some(c) = self
            .conns()
            .into_iter()
            .rev()
            .find(|c| !c.tainted.load(Ordering::SeqCst))
        else {
            return false;
        };
        let Some(server) = &c.server else {
            return false;
        };
        let _whole = c.to_server.lock().unwrap();
        let frame = c.last.lock().unwrap().as_ref().map(|(f, _)| f.clone());
        match frame {
            Some(f) => (&*server).write_all(&f).is_ok(),
            None => false,
        }
    }
}

impl Proxy {
    /// Send `payload` to the server as a binary frame of the client's own,
    /// whole, on the newest connection — a frame the client never sent.
    /// What a falsification uses to show a check would see one.
    pub fn inject(&self, payload: &[u8]) -> bool {
        let Some(c) = self
            .conns()
            .into_iter()
            .rev()
            .find(|c| !c.tainted.load(Ordering::SeqCst))
        else {
            return false;
        };
        let Some(server) = &c.server else {
            return false;
        };
        // A client frame must be masked (RFC 6455 §5.3); a zero key is a
        // mask that changes nothing.
        let mut frame = vec![0x82];
        match payload.len() {
            n if n < 126 => frame.push(0x80 | n as u8),
            n if n < 65536 => {
                frame.push(0x80 | 126);
                frame.extend((n as u16).to_be_bytes());
            }
            n => {
                frame.push(0x80 | 127);
                frame.extend((n as u64).to_be_bytes());
            }
        }
        frame.extend([0, 0, 0, 0]);
        frame.extend_from_slice(payload);
        let _whole = c.to_server.lock().unwrap();
        (&*server).write_all(&frame).is_ok()
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
        for c in self.conns() {
            c.cut();
        }
    }
}

/// A client arrived: connect it through, or — black-holed — not.
fn open(shared: &Arc<Shared>, client: TcpStream) {
    shared.accepted.fetch_add(1, Ordering::SeqCst);
    let _ = client.set_nodelay(true);
    let _ = client.set_read_timeout(Some(SLICE));
    let blackholed = shared.state.lock().unwrap().mode == Mode::Blackhole;
    let server = if blackholed {
        None
    } else {
        let addr = *shared.upstream.lock().unwrap();
        match TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                let _ = s.set_read_timeout(Some(SLICE));
                Some(s)
            }
            // Nothing there: the client learns it the way it would have
            // from the server's own port, by the connection ending.
            Err(_) => {
                let _ = client.shutdown(Shutdown::Both);
                return;
            }
        }
    };
    let conn = Arc::new(Conn {
        id: shared.next.fetch_add(1, Ordering::SeqCst),
        client,
        server,
        tainted: AtomicBool::new(blackholed),
        closed: AtomicBool::new(false),
        to_server: Mutex::new(()),
        last: Mutex::new(None),
    });
    shared.conns.lock().unwrap().push(conn.clone());
    for up in [true, false] {
        let (shared, conn) = (shared.clone(), conn.clone());
        let _ = std::thread::Builder::new()
            .name(format!(
                "proxy-conn-{}-{}",
                conn.id,
                if up { "up" } else { "down" }
            ))
            .spawn(move || forward(&shared, &conn, up));
    }
}

/// What happens to a chunk now.
enum Gate {
    Send(Duration),
    Discard,
    Stop,
}

/// Wait out a pause; say whether to send (and how late), drop, or stop.
fn gate(shared: &Shared, conn: &Conn) -> Gate {
    let mut st = shared.state.lock().unwrap();
    loop {
        if !conn.live() || shared.stop.load(Ordering::SeqCst) {
            return Gate::Stop;
        }
        if conn.tainted.load(Ordering::SeqCst) {
            return Gate::Discard;
        }
        match st.mode {
            Mode::Pass => return Gate::Send(st.delay),
            Mode::Blackhole => {
                conn.tainted.store(true, Ordering::SeqCst);
                return Gate::Discard;
            }
            Mode::Pause(until) => {
                let now = Instant::now();
                if now >= until {
                    st.mode = Mode::Pass;
                    continue;
                }
                st = shared
                    .changed
                    .wait_timeout(st, (until - now).min(SLICE))
                    .unwrap()
                    .0;
            }
        }
    }
}

/// One direction of one connection, until it ends.
fn forward(shared: &Shared, conn: &Conn, up: bool) {
    let (mut src, dst) = match (up, &conn.server) {
        (true, s) => (&conn.client, s.as_ref()),
        (false, Some(s)) => (s, Some(&conn.client)),
        // No server, nothing comes down.
        (false, None) => return,
    };
    let mut frames = Frames::default();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if !conn.live() || shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let n = match src.read(&mut buf) {
            Ok(0) => {
                // An end. Passed on only on a connection that still carries
                // the protocol: a black hole never closes anything.
                if !conn.tainted.load(Ordering::SeqCst) {
                    if let Some(d) = dst {
                        let _ = d.shutdown(Shutdown::Write);
                    }
                }
                return;
            }
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => {
                if !conn.tainted.load(Ordering::SeqCst) {
                    conn.cut();
                }
                return;
            }
        };
        let Some(mut dst) = dst else { continue };
        let chunk = &buf[..n];
        if up {
            if conn.tainted.load(Ordering::SeqCst) {
                continue;
            }
            frames.push(chunk);
            while let Some(unit) = frames.next_unit() {
                match gate(shared, conn) {
                    Gate::Stop => return,
                    Gate::Discard => {
                        frames = Frames::default();
                        break;
                    }
                    Gate::Send(late) => {
                        if !late.is_zero() {
                            std::thread::sleep(late);
                        }
                        let _whole = conn.to_server.lock().unwrap();
                        if dst.write_all(&unit.bytes).is_err() {
                            conn.cut();
                            return;
                        }
                        if let Some(payload) = unit.binary {
                            *conn.last.lock().unwrap() = Some((unit.bytes.clone(), payload));
                        }
                    }
                }
            }
        } else {
            match gate(shared, conn) {
                Gate::Stop => return,
                Gate::Discard => continue,
                Gate::Send(late) => {
                    if !late.is_zero() {
                        std::thread::sleep(late);
                    }
                    // The mid-page cut: exactly the bytes allowed, then
                    // nothing, on both sides.
                    let allowed = {
                        let mut st = shared.state.lock().unwrap();
                        match st.cut_after {
                            Some(left) if left <= chunk.len() => {
                                st.cut_after = None;
                                Some(left)
                            }
                            Some(left) => {
                                st.cut_after = Some(left - chunk.len());
                                None
                            }
                            None => None,
                        }
                    };
                    let send = allowed.unwrap_or(chunk.len());
                    if dst.write_all(&chunk[..send]).is_err() {
                        conn.cut();
                        return;
                    }
                    shared.down_bytes.fetch_add(send, Ordering::SeqCst);
                    if allowed.is_some() {
                        shared.fired.fetch_add(1, Ordering::SeqCst);
                        conn.cut();
                        return;
                    }
                }
            }
        }
    }
}

/// What the client sends, split where it can be split: an HTTP request's
/// head whole; then, when it asked for a WebSocket, frames whole (RFC 6455
/// §5.2), and when it did not — a sign-in, `/healthz` — the rest as it
/// comes, since a body is not frames.
#[derive(Default)]
struct Frames {
    buf: Vec<u8>,
    phase: Phase,
}

#[derive(Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Head,
    Raw,
    Ws,
}

struct Unit {
    bytes: Vec<u8>,
    /// A complete binary frame's payload, unmasked.
    binary: Option<Vec<u8>>,
}

impl Frames {
    fn push(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    fn next_unit(&mut self) -> Option<Unit> {
        match self.phase {
            Phase::Head => {
                let end = self.buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
                let head = String::from_utf8_lossy(&self.buf[..end]).to_ascii_lowercase();
                self.phase = if head.contains("upgrade: websocket") {
                    Phase::Ws
                } else {
                    Phase::Raw
                };
                return Some(Unit {
                    bytes: self.buf.drain(..end).collect(),
                    binary: None,
                });
            }
            Phase::Raw if self.buf.is_empty() => return None,
            Phase::Raw => {
                return Some(Unit {
                    bytes: std::mem::take(&mut self.buf),
                    binary: None,
                })
            }
            Phase::Ws => {}
        }
        let b = &self.buf;
        if b.len() < 2 {
            return None;
        }
        let (fin, opcode) = (b[0] & 0x80 != 0, b[0] & 0x0f);
        let masked = b[1] & 0x80 != 0;
        let (len, mut at) = match b[1] & 0x7f {
            126 => {
                if b.len() < 4 {
                    return None;
                }
                (u16::from_be_bytes([b[2], b[3]]) as usize, 4)
            }
            127 => {
                if b.len() < 10 {
                    return None;
                }
                (
                    u64::from_be_bytes(b[2..10].try_into().unwrap()) as usize,
                    10,
                )
            }
            n => (n as usize, 2),
        };
        let mask = if masked {
            if b.len() < at + 4 {
                return None;
            }
            let m = [b[at], b[at + 1], b[at + 2], b[at + 3]];
            at += 4;
            Some(m)
        } else {
            None
        };
        if b.len() < at + len {
            return None;
        }
        let payload: Vec<u8> = b[at..at + len]
            .iter()
            .enumerate()
            .map(|(i, x)| mask.map_or(*x, |m| x ^ m[i % 4]))
            .collect();
        let bytes: Vec<u8> = self.buf.drain(..at + len).collect();
        Some(Unit {
            bytes,
            binary: (fin && opcode == 2).then_some(payload),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client's frames come out whole however the bytes were chunked, and
    /// the binary ones are unmasked. Falsified by reading a 16-bit length
    /// little-endian: the second frame never completes.
    #[test]
    fn frames_are_split_where_they_end() {
        let mut f = Frames::default();
        let mask = [1u8, 2, 3, 4];
        let big: Vec<u8> = (0..300u32).map(|i| i as u8).collect();
        let mut wire = b"GET /sync HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\n\r\n".to_vec();
        for (op, payload) in [(2u8, b"hi".to_vec()), (2u8, big.clone()), (9u8, vec![])] {
            wire.push(0x80 | op);
            if payload.len() < 126 {
                wire.push(0x80 | payload.len() as u8);
            } else {
                wire.push(0x80 | 126);
                wire.extend((payload.len() as u16).to_be_bytes());
            }
            wire.extend(mask);
            wire.extend(payload.iter().enumerate().map(|(i, x)| x ^ mask[i % 4]));
        }
        let mut units = vec![];
        for chunk in wire.chunks(7) {
            f.push(chunk);
            while let Some(u) = f.next_unit() {
                units.push(u);
            }
        }
        assert_eq!(units.len(), 4);
        assert!(units[0].bytes.ends_with(b"\r\n\r\n") && units[0].binary.is_none());
        assert_eq!(units[1].binary.as_deref(), Some(&b"hi"[..]));
        assert_eq!(units[2].binary.as_deref(), Some(&big[..]));
        assert!(units[3].binary.is_none(), "a ping is not a binary frame");
        assert_eq!(
            units.iter().map(|u| u.bytes.len()).sum::<usize>(),
            wire.len()
        );

        // A request that is not an upgrade has a body, and it is not frames.
        let mut f = Frames::default();
        f.push(b"POST /auth/exchange HTTP/1.1\r\nContent-Length: 4\r\n\r\n{\"c\"");
        assert!(f.next_unit().is_some());
        assert_eq!(
            f.next_unit().map(|u| u.bytes).as_deref(),
            Some(&b"{\"c\""[..])
        );
        assert!(f.next_unit().is_none());
    }
}
