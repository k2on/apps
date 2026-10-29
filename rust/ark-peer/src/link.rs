//! The link (§1.11's transport): a device's connection to its server,
//! dialled again whenever it closes: at once when the connection had been
//! up — a server restarting, a network that came back — and with backoff
//! from a dial that fails, so "waiting to dial" is only ever said of a
//! server that was tried and not reached. Whoever drives the peer may also
//! have it dialled now ([`Peer::dial_now`]): the app came to the front.
//!
//! A connection's first message is the login's token, which the server
//! resolves to who the connection is; every message after is one frame.
//! A [`Wire`] says [`Event::Open`] when its connection is up — which the
//! peer turns into `Connected`, a `Hello` and its pending intents pushed
//! again — then every frame that comes, then [`Event::Closed`]: with
//! `denied: …` when the server turned the token away (close code 4401),
//! which stops the dialling until another login. A frame the peer has
//! while no connection is up is dropped: the machine pushes what is
//! pending again on the next one.
//!
//! A wire is made by a [`Dial`]: [`socket`] is a WebSocket (`ws://…/sync`)
//! on a thread of its own natively and the page's own in a browser; the
//! server's in-process transport (`serve::Handle::dialer`) is another.
//! Each rings the peer's [`Wake`] whenever it has something to say, and
//! [`Peer::pump`] takes it in.

use crate::Peer;

/// What a connection says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Open,
    Frame(Vec<u8>),
    Closed(String),
}

/// `Send` natively, where a peer moves between threads; nothing in a
/// browser, where a socket's callbacks are not.
#[cfg(not(target_arch = "wasm32"))]
pub trait Sendable: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> Sendable for T {}
#[cfg(target_arch = "wasm32")]
pub trait Sendable {}
#[cfg(target_arch = "wasm32")]
impl<T> Sendable for T {}

/// What wakes whoever drives the peer.
#[cfg(not(target_arch = "wasm32"))]
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;
#[cfg(target_arch = "wasm32")]
pub type Wake = std::rc::Rc<dyn Fn()>;

/// One connection, open or opening: frames out, and what it said.
pub trait Wire: Sendable {
    fn send(&mut self, frame: Vec<u8>);
    fn events(&mut self) -> Vec<Event>;
}

/// How a connection to `url` is made, its first message `token`, ringing
/// `wake` whenever it has something to say.
pub trait Dial: Sendable {
    fn dial(&mut self, url: &str, token: &str, wake: Option<Wake>) -> Box<dyn Wire>;
}

impl<F: FnMut(&str, &str, Option<Wake>) -> Box<dyn Wire> + Sendable> Dial for F {
    fn dial(&mut self, url: &str, token: &str, wake: Option<Wake>) -> Box<dyn Wire> {
        self(url, token, wake)
    }
}

/// A connection that is closed already, saying why.
pub struct Closed(pub Option<String>);

impl Wire for Closed {
    fn send(&mut self, _: Vec<u8>) {}
    fn events(&mut self) -> Vec<Event> {
        self.0.take().map(Event::Closed).into_iter().collect()
    }
}

/// The link's state: where and as whom it dials, the connection, and when
/// it is dialled again — at once after a connection that had opened closes;
/// `backoff.0` milliseconds after a dial that fails, doubling to
/// `backoff.1`, back to the first once one opens; and how many of what the
/// connection said one pump takes in (`chunk`), the rest kept for the next,
/// so a driver answers between them.
pub struct Link {
    pub url: Option<String>,
    pub token: Option<String>,
    pub backoff: (u64, u64),
    pub chunk: usize,
    backlog: std::collections::VecDeque<Event>,
    pub(crate) dial: Option<Box<dyn Dial>>,
    pub(crate) wire: Option<Box<dyn Wire>>,
    due: Option<u64>,
    pub(crate) wait: u64,
    /// Whether the connection last dialled opened: its close is a drop, dialled again at once,
    /// where a dial that never opened failed, and waits.
    pub(crate) opened: bool,
    pub(crate) offline: bool,
    waker: Option<Wake>,
    /// How many connections have opened.
    pub epoch: u64,
}

impl Default for Link {
    fn default() -> Link {
        Link {
            url: None,
            token: None,
            backoff: (250, 30_000),
            chunk: usize::MAX,
            backlog: Default::default(),
            dial: None,
            wire: None,
            due: None,
            wait: 250,
            opened: false,
            offline: false,
            waker: None,
            epoch: 0,
        }
    }
}

/// What one pump did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pumped {
    /// A connection opened.
    pub opened: bool,
    /// Anything arrived.
    pub moved: bool,
    /// Frames that arrived, by space: the log's, the live space's.
    pub frames: [usize; 2],
    /// Why connections closed.
    pub closed: Vec<String>,
    /// The server turned this login away: why.
    pub denied: Option<String>,
}

impl Peer {
    /// Dial `url` (a `ws://…/sync`) whenever there is a token, now and
    /// after every close.
    pub fn connect(&mut self, url: &str) {
        self.link.url = Some(url.to_string());
        if self.link.token.is_some() && !self.link.offline {
            self.link.due = Some(crate::monotonic_ms());
        }
    }

    /// [`Peer::connect`], through `dial`.
    pub fn connect_with(&mut self, url: &str, dial: impl Dial + 'static) {
        self.link.dial = Some(Box::new(dial));
        self.connect(url);
    }

    /// The login's token the link dials with; none stops it dialling.
    pub fn set_token(&mut self, token: Option<String>) {
        self.link.token = token;
        if self.link.token.is_none() {
            self.drop_wire();
        } else if let Some(url) = self.link.url.clone() {
            self.connect(&url);
        }
    }

    fn drop_wire(&mut self) {
        self.link.wire = None;
        self.link.backlog.clear();
        self.link.due = None;
        self.disconnected();
    }

    /// Offline on purpose: the connection closed, nothing dialled.
    pub fn disconnect(&mut self) {
        self.link.offline = true;
        self.drop_wire();
    }

    /// Back online.
    pub fn reconnect(&mut self) {
        self.link.offline = false;
        if let Some(url) = self.link.url.clone() {
            self.connect(&url);
        }
    }

    pub fn set_waker(&mut self, w: Wake) {
        self.link.waker = Some(w);
    }

    /// The one timer the peer needs: the next dial, a handler's next job,
    /// and the relay's next send while it holds a call.
    pub fn deadline(&self) -> Option<u64> {
        let every = self.config.live.as_ref().map_or(0, |l| l.every_ms);
        let relay = (!self.relay.is_empty()).then(|| {
            let wait = self.relayed.map_or(0, |t| t + every - crate::now_ms()).max(0);
            crate::monotonic_ms() + wait as u64
        });
        self.link.due.into_iter().chain(self.due()).chain(relay).min()
    }

    /// Whether a connection is being made.
    pub fn dialling(&self) -> bool {
        self.link.wire.is_some()
    }

    /// Dial now, whatever the backoff says: the person is looking — the app
    /// came to the front — so a server that is there is reached before the
    /// screens say it is not, and the backoff starts over for them. Nothing
    /// while a connection is up or being made, offline on purpose, or signed
    /// out.
    pub fn dial_now(&mut self) {
        if self.link.wire.is_some() || self.link.offline || self.link.token.is_none() || self.link.url.is_none() {
            return;
        }
        self.link.wait = self.link.backoff.0;
        self.link.due = Some(crate::monotonic_ms());
    }

    // The dial that is due, made.
    fn dial_if_due(&mut self, now: u64) {
        if !self.link.due.is_some_and(|d| now >= d) {
            return;
        }
        self.link.due = None;
        let label = self.label();
        if let (Some(url), Some(token), false) = (&self.link.url, &self.link.token, self.link.offline) {
            let dial = self.link.dial.get_or_insert_with(|| Box::new(socket));
            crate::observe::link(&label, "dial", Some(url));
            self.link.wire = Some(dial.dial(url, token, self.link.waker.clone()));
        }
    }

    /// How much of what the connection said waits for the next pump.
    pub fn backlog(&self) -> usize {
        self.link.backlog.len()
    }

    /// The connection's news taken in, the clock given to the relay, what
    /// the machines have to say sent; and a dial when one is due.
    pub fn pump(&mut self) -> Pumped {
        let now = crate::monotonic_ms();
        let mut p = Pumped::default();
        self.dial_if_due(now);
        let l = &mut self.link;
        l.backlog.extend(l.wire.as_mut().map(|w| w.events()).unwrap_or_default());
        let events: Vec<Event> = l.backlog.drain(..l.chunk.min(l.backlog.len())).collect();
        for e in events {
            p.moved = true;
            match e {
                Event::Open => {
                    p.opened = true;
                    crate::observe::link(&self.label(), "up", None);
                    self.connected();
                }
                Event::Frame(b) => {
                    let space = ark::frames::Frame::dec(&b).map_or(0, |f| f.space() as usize);
                    p.frames[space] += 1;
                    self.receive(b);
                }
                Event::Closed(why) => {
                    crate::observe::link(&self.label(), "down", Some(&why));
                    self.disconnected();
                    self.link.wire = None;
                    match why.strip_prefix("denied: ") {
                        Some(reason) => {
                            p.denied = Some(reason.to_string());
                            self.link.token = None;
                        }
                        // A connection that had opened is dialled again at once; a dial that
                        // failed waits, and the next waits twice as long.
                        None if self.link.token.is_some() && !self.link.offline => {
                            if self.link.opened {
                                self.link.due = Some(now);
                            } else {
                                self.link.due = Some(now + self.link.wait);
                                self.link.wait = (self.link.wait * 2).min(self.link.backoff.1);
                            }
                            self.link.opened = false;
                        }
                        None => {}
                    }
                    p.closed.push(why);
                }
            }
        }
        // A dial due at once is made before this pump ends, so nobody is shown a link waiting
        // that is about to be dialled.
        self.dial_if_due(crate::monotonic_ms());
        self.tick(crate::now_ms());
        self.jobs(crate::now_ms());
        self.flush();
        p
    }

    /// What the machines have to say, sent — when there is a connection to
    /// send it on; a driver carrying frames by hand takes them itself.
    pub fn flush(&mut self) {
        if self.linked && self.link.wire.is_some() {
            self.ship();
        }
        if let Some(w) = &mut self.link.wire {
            for f in std::mem::take(&mut self.frames) {
                w.send(f);
            }
        }
    }
}

/// A peer's wake and what it rings: a driver with no window waits on it
/// ([`wait`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn bell() -> (Wake, std::sync::mpsc::Receiver<()>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (std::sync::Arc::new(move || _ = tx.send(())), rx)
}

/// Waits until `rx` rings, the link's next dial is due or `most` has
/// passed, then takes every ring.
#[cfg(not(target_arch = "wasm32"))]
pub fn wait(peer: &Peer, rx: &std::sync::mpsc::Receiver<()>, most: std::time::Duration) {
    let due = peer
        .deadline()
        .map(|d| std::time::Duration::from_millis(d.saturating_sub(crate::monotonic_ms())));
    let _ = rx.recv_timeout(due.map_or(most, |d| d.min(most)));
    while rx.try_recv().is_ok() {}
}

/// Looks at `done` every `every` until it holds or `within` has passed:
/// whether it held. What a driver with no window — a test, a headless
/// peer — steps and settles by: `done` pumps what it drives and says
/// whether what it waits for has happened.
#[cfg(not(target_arch = "wasm32"))]
pub fn until(within: std::time::Duration, every: std::time::Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + within;
    loop {
        if done() {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        std::thread::sleep(every.min(deadline - now));
    }
}

/// A WebSocket on a thread of its own: the token first, then frames; a
/// close with code 4401 is the server turning the login away.
#[cfg(not(target_arch = "wasm32"))]
pub fn socket(url: &str, token: &str, wake: Option<Wake>) -> Box<dyn Wire> {
    use std::sync::mpsc::{channel, TryRecvError};
    use tungstenite::{stream::MaybeTlsStream, Error, Message};
    struct Thread(std::sync::mpsc::Sender<Vec<u8>>, std::sync::mpsc::Receiver<Event>);
    impl Wire for Thread {
        fn send(&mut self, frame: Vec<u8>) {
            let _ = self.0.send(frame);
        }
        fn events(&mut self) -> Vec<Event> {
            self.1.try_iter().collect()
        }
    }
    let (out, from) = channel::<Vec<u8>>();
    let (to, events) = channel();
    let (url, token) = (url.to_string(), token.to_string());
    std::thread::spawn(move || {
        let said = |e: Event| {
            let ok = to.send(e).is_ok();
            wake.iter().for_each(|w| w());
            ok
        };
        let closed = |why: String| _ = said(Event::Closed(why));
        // A snapshot is one frame, which a library outgrows tungstenite's
        // 16 MiB for frames long before its 64 MiB for messages: the message
        // is what is bounded, then, and a frame is any part of it.
        let config = tungstenite::protocol::WebSocketConfig {
            max_frame_size: None,
            ..Default::default()
        };
        let mut ws = match tungstenite::client::connect_with_config(&url, Some(config), 3) {
            Ok((ws, _)) => ws,
            Err(e) => return closed(e.to_string()),
        };
        // A read waits at most this long, so that what the peer has to
        // send goes out between reads, over TLS as over a plain socket.
        let every = Some(std::time::Duration::from_millis(5));
        match ws.get_mut() {
            MaybeTlsStream::Plain(s) => _ = s.set_read_timeout(every),
            MaybeTlsStream::Rustls(s) => _ = s.get_ref().set_read_timeout(every),
            _ => {}
        }
        if ws.send(Message::Binary(token.into_bytes())).is_err() || !said(Event::Open) {
            return;
        }
        loop {
            loop {
                match from.try_recv() {
                    Ok(f) => {
                        if ws.send(Message::Binary(f)).is_err() {
                            return closed("a send failed".into());
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return drop(ws.close(None)),
                }
            }
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    if !said(Event::Frame(b)) {
                        return;
                    }
                }
                Ok(Message::Close(c)) => {
                    return closed(match c {
                        Some(f) if u16::from(f.code) == 4401 => format!("denied: {}", f.reason),
                        Some(f) => f.reason.to_string(),
                        None => "the connection closed".into(),
                    })
                }
                Ok(_) => {}
                Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    let _ = ws.flush();
                }
                Err(e) => return closed(e.to_string()),
            }
        }
    });
    Box::new(Thread(out, events))
}

/// The page's own `WebSocket`: the token first, then frames, each event
/// ringing the bell.
#[cfg(target_arch = "wasm32")]
pub fn socket(url: &str, token: &str, wake: Option<Wake>) -> Box<dyn Wire> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::{closure::Closure, JsCast};
    use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

    type Kept = Closure<dyn FnMut(web_sys::Event)>;
    struct Socket(WebSocket, Rc<RefCell<Vec<Event>>>, #[allow(dead_code)] Vec<Kept>);
    impl Wire for Socket {
        fn send(&mut self, frame: Vec<u8>) {
            let _ = self.0.send_with_u8_array(&frame);
        }
        fn events(&mut self) -> Vec<Event> {
            std::mem::take(&mut *self.1.borrow_mut())
        }
    }
    impl Drop for Socket {
        fn drop(&mut self) {
            (self.0.set_onopen(None), self.0.set_onmessage(None), self.0.set_onclose(None));
            let _ = self.0.close();
        }
    }
    let ws = match WebSocket::new(url) {
        Ok(ws) => ws,
        Err(e) => return Box::new(Closed(Some(format!("{e:?}")))),
    };
    ws.set_binary_type(BinaryType::Arraybuffer);
    let events: Rc<RefCell<Vec<Event>>> = Rc::default();
    let on = |f: Box<dyn Fn(web_sys::Event) -> Option<Event>>| {
        let (q, w) = (events.clone(), wake.clone());
        Kept::new(move |e| {
            if let Some(e) = f(e) {
                q.borrow_mut().push(e);
                w.iter().for_each(|w| w());
            }
        })
    };
    let (w, t) = (ws.clone(), token.to_string());
    let open = on(Box::new(move |_| {
        let _ = w.send_with_u8_array(t.as_bytes());
        Some(Event::Open)
    }));
    let message = on(Box::new(|e| {
        let buf = e.dyn_ref::<MessageEvent>()?.data().dyn_into::<js_sys::ArrayBuffer>().ok()?;
        Some(Event::Frame(js_sys::Uint8Array::new(&buf).to_vec()))
    }));
    let close = on(Box::new(|e| {
        Some(Event::Closed(match e.dyn_ref::<CloseEvent>() {
            Some(c) if c.code() == 4401 => format!("denied: {}", c.reason()),
            Some(c) => c.reason(),
            None => "the connection closed".into(),
        }))
    }));
    ws.set_onopen(Some(open.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
    ws.set_onclose(Some(close.as_ref().unchecked_ref()));
    Box::new(Socket(ws, events, vec![open, message, close]))
}
