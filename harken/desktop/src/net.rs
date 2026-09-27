//! The one place networking lives: a WebSocket client on a thread of its
//! own, speaking to the peer over two channels. The engine is sans-io, so
//! this is a loop and nothing more — connect, say so, move binary frames
//! both ways, say when the socket drops, and try again with a backoff from
//! half a second to thirty. Pings are answered by the library.

use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error, Message, WebSocket};

/// What the socket reports to the peer.
#[derive(Debug)]
pub enum Event {
    Connected,
    Frame(Vec<u8>),
    Disconnected(String),
}

/// The peer's handle on the thread.
pub struct Link {
    pub events: Receiver<Event>,
    outgoing: Option<Sender<Vec<u8>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Link {
    /// Queue a frame; dropped if the thread is gone.
    pub fn send(&self, frame: Vec<u8>) {
        if let Some(out) = &self.outgoing {
            let _ = out.send(frame);
        }
    }

    /// Close the socket properly: the thread notices the peer is gone at
    /// its next slice and sends a Close. Waits a little for it, never
    /// long — a connect attempt in flight is not worth holding the exit for.
    pub fn shutdown(mut self) {
        drop(self.outgoing.take());
        if let Some(t) = self.thread.take() {
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

const FIRST_BACKOFF: Duration = Duration::from_millis(500);
const LAST_BACKOFF: Duration = Duration::from_secs(30);
const READ_SLICE: Duration = Duration::from_millis(50);

/// Start dialling `url`, forever.
pub fn spawn(url: String) -> Link {
    let (ev_tx, ev_rx) = channel();
    let (out_tx, out_rx) = channel::<Vec<u8>>();
    let thread = thread::Builder::new()
        .name("harken-net".into())
        .spawn(move || run(&url, &ev_tx, &out_rx))
        .expect("spawn the network thread");
    Link {
        events: ev_rx,
        outgoing: Some(out_tx),
        thread: Some(thread),
    }
}

fn run(url: &str, events: &Sender<Event>, outgoing: &Receiver<Vec<u8>>) {
    let mut backoff = FIRST_BACKOFF;
    loop {
        match tungstenite::connect(url) {
            Ok((mut ws, _)) => {
                if let MaybeTlsStream::Plain(s) = ws.get_ref() {
                    let _ = s.set_read_timeout(Some(READ_SLICE));
                }
                // Anything queued while we were down was for a connection
                // that no longer exists; the peer says it all again.
                while outgoing.try_recv().is_ok() {}
                if events.send(Event::Connected).is_err() {
                    return;
                }
                backoff = FIRST_BACKOFF;
                let why = serve(&mut ws, events, outgoing);
                let _ = ws.close(None);
                if events.send(Event::Disconnected(why)).is_err() {
                    return;
                }
            }
            Err(e) => {
                if events.send(Event::Disconnected(e.to_string())).is_err() {
                    return;
                }
            }
        }
        if !sleep_unless_gone(backoff, outgoing) {
            return;
        }
        backoff = (backoff * 2).min(LAST_BACKOFF);
    }
}

// The backoff, in slices, cut short when the peer has gone; `false` then.
fn sleep_unless_gone(d: Duration, outgoing: &Receiver<Vec<u8>>) -> bool {
    let deadline = std::time::Instant::now() + d;
    while std::time::Instant::now() < deadline {
        if let Err(TryRecvError::Disconnected) = outgoing.try_recv() {
            return false;
        }
        thread::sleep(READ_SLICE);
    }
    true
}

// One connection, until it drops; the reason it did.
fn serve(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, events: &Sender<Event>, outgoing: &Receiver<Vec<u8>>) -> String {
    loop {
        match ws.read() {
            Ok(Message::Binary(b)) => {
                if events.send(Event::Frame(b.to_vec())).is_err() {
                    return "peer gone".into();
                }
            }
            Ok(Message::Close(_)) => return "closed by the server".into(),
            Ok(_) => {}
            Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return e.to_string(),
        }
        loop {
            match outgoing.try_recv() {
                Ok(frame) => {
                    if let Err(e) = ws.send(Message::binary(frame)) {
                        return e.to_string();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return "peer gone".into(),
            }
        }
        if let Err(e) = ws.flush() {
            if !matches!(&e, Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)) {
                return e.to_string();
            }
        }
    }
}
