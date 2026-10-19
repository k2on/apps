//! The link (§1.11's transport): a connection to the hub, dialled again
//! with backoff whenever it closes, on a thread of its own.
//!
//! A connection's first message is the login's token (empty for none),
//! which the hub resolves to who the connection is; every message after is
//! one frame. The link says [`Event::Open`] when a connection is up —
//! which the client turns into `Connected`, a `Hello` and its pending
//! intents pushed again — then every frame that comes, then
//! [`Event::Closed`]. A frame the client sends while no connection is up
//! is dropped: the machine pushes what is pending again on the next one,
//! and frames left from a connection that closed are never sent on the
//! next.
//!
//! A transport is a [`Dial`]: [`Socket`] is a WebSocket (`ws://…/sync`),
//! [`pipe`] two ends in this process, which the hub's in-process transport
//! and the tests use.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// What one wait on a connection gave.
pub enum Recv {
    Frame(Vec<u8>),
    Idle,
    Closed,
}

/// One open connection.
pub trait Conn: Send {
    /// Whether it was sent.
    fn send(&mut self, frame: Vec<u8>) -> bool;
    fn recv(&mut self, wait: Duration) -> Recv;
}

/// How a connection is opened, carrying the token as its first message.
pub trait Dial: Send {
    fn dial(&mut self, token: &str) -> Result<Box<dyn Conn>, String>;
}

/// One end of an in-process connection.
pub struct End {
    pub tx: Sender<Vec<u8>>,
    pub rx: Receiver<Vec<u8>>,
}

/// Two ends: what one sends, the other receives; dropping either closes it.
pub fn pipe() -> (End, End) {
    let (a, b) = (channel(), channel());
    (End { tx: a.0, rx: b.1 }, End { tx: b.0, rx: a.1 })
}

impl Conn for End {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        self.tx.send(frame).is_ok()
    }

    fn recv(&mut self, wait: Duration) -> Recv {
        match self.rx.recv_timeout(wait) {
            Ok(f) => Recv::Frame(f),
            Err(RecvTimeoutError::Timeout) => Recv::Idle,
            Err(RecvTimeoutError::Disconnected) => Recv::Closed,
        }
    }
}

/// A WebSocket to `url` (`ark_auth::socket_url` of the server).
pub struct Socket(pub String);

struct Ws(tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>);

impl Dial for Socket {
    fn dial(&mut self, token: &str) -> Result<Box<dyn Conn>, String> {
        let (mut ws, _) = tungstenite::connect(&self.0).map_err(|e| e.to_string())?;
        if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_mut() {
            s.set_read_timeout(Some(Duration::from_millis(20))).map_err(|e| e.to_string())?;
        }
        ws.send(tungstenite::Message::Binary(token.as_bytes().to_vec()))
            .map_err(|e| e.to_string())?;
        Ok(Box::new(Ws(ws)))
    }
}

impl Conn for Ws {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        self.0.send(tungstenite::Message::Binary(frame)).is_ok()
    }

    fn recv(&mut self, _: Duration) -> Recv {
        use tungstenite::{Error, Message};
        match self.0.read() {
            Ok(Message::Binary(b)) => Recv::Frame(b),
            Ok(Message::Close(_)) => Recv::Closed,
            Ok(_) => Recv::Idle,
            Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                let _ = self.0.flush();
                Recv::Idle
            }
            Err(_) => Recv::Closed,
        }
    }
}

/// What the link says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Open,
    Frame(Vec<u8>),
    Closed(String),
}

/// How long the link waits before dialling again: from `first`, doubling
/// to `most`; back to `first` once a connection opens.
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    pub first: Duration,
    pub most: Duration,
}

impl Default for Backoff {
    fn default() -> Backoff {
        Backoff {
            first: Duration::from_millis(250),
            most: Duration::from_secs(30),
        }
    }
}

/// A connection kept up on a thread of its own.
pub struct Link {
    out: Sender<Vec<u8>>,
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
}

impl Link {
    pub fn start(mut dial: Box<dyn Dial>, token: String, backoff: Backoff) -> Link {
        let (out, from_client) = channel::<Vec<u8>>();
        let (to_client, events) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        thread::spawn(move || {
            let mut wait = backoff.first;
            while !halt.load(Ordering::Relaxed) {
                let why = match dial.dial(&token) {
                    Err(e) => e,
                    Ok(mut conn) => {
                        wait = backoff.first;
                        while from_client.try_recv().is_ok() {}
                        if to_client.send(Event::Open).is_err() {
                            return;
                        }
                        loop {
                            if halt.load(Ordering::Relaxed) {
                                return;
                            }
                            let mut closed = false;
                            loop {
                                match from_client.try_recv() {
                                    Ok(f) => closed |= !conn.send(f),
                                    Err(TryRecvError::Empty) => break,
                                    Err(TryRecvError::Disconnected) => return,
                                }
                            }
                            match conn.recv(Duration::from_millis(5)) {
                                Recv::Frame(f) => {
                                    if to_client.send(Event::Frame(f)).is_err() {
                                        return;
                                    }
                                }
                                Recv::Closed => break "the connection closed".into(),
                                _ if closed => break "a send failed".into(),
                                _ => {}
                            }
                        }
                    }
                };
                if to_client.send(Event::Closed(why)).is_err() {
                    return;
                }
                thread::sleep(wait);
                wait = (wait * 2).min(backoff.most);
            }
        });
        Link { out, events, stop }
    }

    pub fn send(&self, frame: Vec<u8>) {
        let _ = self.out.send(frame);
    }

    /// What has happened since the last call.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Waits up to `wait` for something to happen, then takes all of it.
    pub fn wait(&self, wait: Duration) -> Vec<Event> {
        let mut out: Vec<Event> = self.events.recv_timeout(wait).into_iter().collect();
        out.extend(self.events.try_iter());
        out
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
