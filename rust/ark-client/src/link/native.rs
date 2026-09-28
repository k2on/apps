//! A WebSocket on a thread of its own: blocking `tungstenite`, a short
//! read timeout so the thread gets a turn to write, and two channels to
//! whoever polls it.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WsError, Message, WebSocket};

use super::{BoxTransport, Event, Timing, Transport};

/// How long a read blocks before the thread looks at what is to be sent.
const SLICE: Duration = Duration::from_millis(20);

struct Native {
    out: Option<Sender<Vec<u8>>>,
    events: Receiver<Event>,
    done: bool,
}

impl Transport for Native {
    fn send(&mut self, frame: Vec<u8>) {
        if let Some(out) = &self.out {
            let _ = out.send(frame);
        }
    }

    fn poll(&mut self) -> Vec<Event> {
        let mut got = Vec::new();
        if self.done {
            return got;
        }
        loop {
            match self.events.try_recv() {
                Ok(e) => {
                    let end = matches!(e, Event::Closed(_));
                    got.push(e);
                    if end {
                        self.done = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    got.push(Event::Closed("the network thread ended".into()));
                    self.done = true;
                    break;
                }
            }
        }
        got
    }

    /// Dropping the sender is the thread's signal: it closes the socket
    /// properly at its next slice and ends.
    fn close(&mut self) {
        self.out = None;
        self.done = true;
    }
}

/// Start dialling `url` (`ws://` or `wss://`) on a thread; it reports
/// `Opened`, then frames, then exactly one `Closed`.
pub fn dial(url: &str, timing: &Timing) -> BoxTransport {
    let (ev_tx, ev_rx) = channel();
    let (out_tx, out_rx) = channel::<Vec<u8>>();
    let url = url.to_string();
    let timing = timing.clone();
    let spawned = std::thread::Builder::new().name("ark-link".into()).spawn(move || {
        let why = run(&url, &timing, &ev_tx, &out_rx);
        let _ = ev_tx.send(Event::Closed(why));
    });
    if let Err(e) = spawned {
        let (tx, rx) = channel();
        let _ = tx.send(Event::Closed(format!("could not start the network thread: {e}")));
        return Box::new(Native {
            out: None,
            events: rx,
            done: false,
        });
    }
    Box::new(Native {
        out: Some(out_tx),
        events: ev_rx,
        done: false,
    })
}

/// `host:port` of a WebSocket URL, with the scheme's default port.
fn authority(url: &str) -> Result<String, String> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("wss://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("ws://") {
        (false, r)
    } else {
        return Err(format!("{url} is not a ws:// or wss:// URL"));
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return Err(format!("{url} names no host"));
    }
    let has_port = match host.rfind(':') {
        Some(i) => !host[i..].contains(']'),
        None => false,
    };
    Ok(if has_port {
        host.to_string()
    } else {
        format!("{host}:{}", if tls { 443 } else { 80 })
    })
}

fn connect(url: &str, timing: &Timing) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let authority = authority(url)?;
    let addrs: Vec<_> = authority.to_socket_addrs().map_err(|e| format!("{authority}: {e}"))?.collect();
    let mut last = format!("{authority} resolves to nothing");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(timing.connect_timeout_ms)) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return handshake(url, stream);
            }
            Err(e) => last = format!("{addr}: {e}"),
        }
    }
    Err(last)
}

#[cfg(feature = "tls")]
fn handshake(url: &str, stream: TcpStream) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    tungstenite::client_tls(url, stream).map(|(ws, _)| ws).map_err(|e| format!("{url}: {e}"))
}

#[cfg(not(feature = "tls"))]
fn handshake(url: &str, stream: TcpStream) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    if url.starts_with("wss://") {
        return Err(format!("{url}: this build has no TLS (ark-client's `tls` feature)"));
    }
    tungstenite::client(url, MaybeTlsStream::Plain(stream))
        .map(|(ws, _)| ws)
        .map_err(|e| format!("{url}: {e}"))
}

fn set_slice(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let tcp: Option<&TcpStream> = match ws.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        #[cfg(feature = "tls")]
        MaybeTlsStream::Rustls(s) => Some(s.get_ref()),
        _ => None,
    };
    if let Some(s) = tcp {
        let _ = s.set_read_timeout(Some(SLICE));
    }
}

fn quiet(e: &WsError) -> bool {
    matches!(e, WsError::Io(io) if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
}

// One connection, until it ends; why it did.
fn run(url: &str, timing: &Timing, events: &Sender<Event>, out: &Receiver<Vec<u8>>) -> String {
    let mut ws = match connect(url, timing) {
        Ok(ws) => ws,
        Err(e) => return e,
    };
    set_slice(&mut ws);
    if events.send(Event::Opened).is_err() {
        let _ = ws.close(None);
        return "the peer went away".into();
    }
    let ping_every = Duration::from_millis(timing.ping_every_ms.max(1));
    let mut heard = Instant::now();
    let mut pinged = Instant::now();
    let why = loop {
        match ws.read() {
            Ok(Message::Binary(b)) => {
                heard = Instant::now();
                if events.send(Event::Frame(b.to_vec())).is_err() {
                    break "the peer went away".to_string();
                }
            }
            Ok(Message::Close(_)) => break "closed by the server".into(),
            // A ping (answered by the library), a pong, text: the server is
            // there, which is all a keepalive asks.
            Ok(_) => heard = Instant::now(),
            Err(e) if quiet(&e) => {}
            Err(e) => break e.to_string(),
        }
        let mut gone = false;
        loop {
            match out.try_recv() {
                Ok(frame) => {
                    if let Err(e) = ws.send(Message::binary(frame)) {
                        return e.to_string();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        if gone {
            let _ = ws.close(None);
            let _ = ws.flush();
            break "closed".into();
        }
        if heard.elapsed() >= ping_every * 3 {
            break format!("nothing from the server for {}s", (ping_every * 3).as_secs());
        }
        if pinged.elapsed() >= ping_every {
            pinged = Instant::now();
            if let Err(e) = ws.send(Message::Ping(Vec::new().into())) {
                break e.to_string();
            }
        }
        if let Err(e) = ws.flush() {
            if !quiet(&e) {
                break e.to_string();
            }
        }
    };
    why
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authority_follows_the_scheme() {
        assert_eq!(authority("ws://127.0.0.1:8787/sync").unwrap(), "127.0.0.1:8787");
        assert_eq!(authority("wss://h.example/sync").unwrap(), "h.example:443");
        assert_eq!(authority("ws://h.example").unwrap(), "h.example:80");
        assert_eq!(authority("ws://[::1]:9/sync").unwrap(), "[::1]:9");
        assert_eq!(authority("ws://[::1]/sync").unwrap(), "[::1]:80");
        assert!(authority("http://h/sync").is_err());
    }

    #[test]
    fn a_refused_connection_is_reported_closed() {
        // Bind and drop, so nothing listens on the port.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut t = dial(&format!("ws://127.0.0.1:{port}/sync"), &Timing::default());
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = vec![];
        while got.is_empty() && Instant::now() < deadline {
            got = t.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(got.as_slice(), [Event::Closed(_)]), "{got:?}");
    }
}
