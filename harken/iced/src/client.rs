//! This device's replica (`docs/plan-v5.md` §1.7, §1.9, §1.11): an
//! `ark_client::Client` — the log's machine and the listening session's,
//! over one connection — and what a window asks of it, by name.
//!
//! The client is sans-io; a [`Replica`] drives it. It authors and asks by a
//! function's name (`create_playlist`, or its whole binding,
//! `playlists.create_playlist`) with the domain's own input types; it holds
//! the maintained views a window's lists are, and hands each one's patches
//! back as an [`Update`]; it keeps every intent's standing and the
//! refusals the server sent; and it keeps the connection — dialled when
//! there is a server and somebody signed in, dialled again on a deadline
//! after it closes ([`Replica::deadline`]), ringing the window's bell
//! whenever it opens, closes or brings a frame ([`Replica::set_waker`]).
//!
//! **Nobody has to be signed in.** A replica opened with no login authors
//! as whoever last signed in on it — nobody, on a device where nobody ever
//! did — and dials nothing; signing in makes its pending intents the
//! signer's, as `ark_client::Client::sign_in` re-authors them, and dials.
//! A replica that joined a server while alone hands its history over as
//! intents pending under the login that signs in there (§1.7's join hands a
//! chain, which the server adopts only from the session that made it — a
//! session nobody's alone history can have).
//!
//! A connection's transport is a [`Wire`] a [`Dialer`] makes: a WebSocket
//! on a thread of its own natively, the page's `WebSocket` in a browser,
//! queues in a test.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::domain::Device;
use ark::hash::StateHash;
use ark::ir::Node;
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use ark::schema::Space;
use ark::view::Patch;
use ark::Domain;
pub use ark_client::link::Event;
use ark_client::{Client, Config, Disk, Live, Memory, Module};

/// The milliseconds between two ephemeral calls the relay sends — a
/// scrubber dragged — twenty a second; the last always goes.
pub const THROTTLE_MS: u64 = 50;

/// How long after a connection closes it is dialled again, at first and at
/// most: doubling from the one to the other, back to the first once one
/// opens.
const FIRST_MS: u64 = 250;
const MOST_MS: u64 = 30_000;

/// harken's module, assembled once.
pub fn domain() -> &'static Domain {
    static D: OnceLock<Domain> = OnceLock::new();
    D.get_or_init(crate::domain::host::load)
}

/// The binding a name means: itself, or the one whose last part it is.
pub fn binding(name: &str) -> Option<String> {
    let m = &domain().module;
    m.bindings
        .iter()
        .map(|(n, _)| n)
        .find(|n| *n == name || n.rsplit('.').next() == Some(name))
        .cloned()
}

/// The wall clock, in milliseconds: what an intent's `now` is.
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }
}

/// A clock that only goes forward, in milliseconds from some start: what a
/// deadline is (`Instant` panics on wasm32-unknown-unknown).
pub fn monotonic_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }
}

/// What this device says it is in its volatile `Hello` (§1.9): the server
/// makes it `on_join`'s input with the login's user and session.
pub fn device(can_play: bool, name: &str) -> Vec<u8> {
    Device {
        can_play,
        id: String::new(),
        kind: crate::domain::host::Kind::Computer.name().into(),
        name: name.into(),
        user: String::new(),
    }
    .to_bytes()
}

/// The client's two modules, and the relay: `scrub` is the one ephemeral
/// transition.
pub fn config(device: Vec<u8>) -> Config {
    let d = domain();
    let m = Module {
        schema: d.schema.clone(),
        cas: d.cas(),
        lib: d.lib.clone(),
    };
    Config {
        log: m.clone(),
        live: Some(Live {
            module: m,
            device: Some(device),
            ephemeral: [d.get("listening.scrub")].into(),
            every_ms: THROTTLE_MS as i64,
        }),
    }
}

/// What has become of an intent this replica authored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    Pending,
    Confirmed,
    Rejected(String),
    Unknown,
}

/// What one view did since it was last asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    Patched(Vec<Patch>),
    /// It could not be kept up: read it again.
    Reset,
}

/// What wakes a window.
#[cfg(not(target_arch = "wasm32"))]
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;
#[cfg(target_arch = "wasm32")]
pub type Wake = std::rc::Rc<dyn Fn()>;

/// One connection, open or opening: frames out, and what it said.
pub trait Wire {
    fn send(&mut self, frame: Vec<u8>);
    fn events(&mut self) -> Vec<Event>;
}

/// How a connection to `url` is made, its first message the token, ringing
/// the wake whenever it has something to say.
pub type Dialer = Box<dyn FnMut(&str, &str, Option<Wake>) -> Box<dyn Wire>>;

/// What one pump did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pumped {
    /// A connection opened.
    pub opened: bool,
    /// Anything arrived.
    pub moved: bool,
    /// The server turned this login away: why.
    pub denied: Option<String>,
}

/// Where the replica stands, as a status line says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub link: String,
    pub linked: bool,
    /// How many connections have opened.
    pub epoch: u64,
    pub cursor: i64,
    pub pending: usize,
    /// A chain joined and not answered yet.
    pub joining: bool,
    pub user: String,
    pub alone: bool,
}

/// The listening session's machine, in numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveStatus {
    pub seq: i64,
    pub pending: usize,
    /// This device stands in the session: its `on_join` has landed.
    pub synced: bool,
}

pub struct Replica {
    pub client: Client,
    token: Option<String>,
    url: Option<String>,
    dialer: Option<Dialer>,
    wire: Option<Box<dyn Wire>>,
    due: Option<u64>,
    backoff: u64,
    waker: Option<Wake>,
    offline: bool,
    epoch: u64,
    /// Bumped whenever who this replica is changes: a view made for one
    /// login is not another's.
    pub who_epoch: u64,
    standing: BTreeMap<EntryId, Standing>,
    rejections: Vec<(EntryId, String)>,
    live_refused: Vec<(EntryId, String)>,
    updates: BTreeMap<usize, Update>,
}

/// Nobody, on a session of this device's own: who a replica nobody ever
/// signed in on authors as.
pub fn nobody() -> Who {
    let here = 0u8;
    let seed = format!("{}:{}:{:p}", now_ms(), monotonic_ms(), &here);
    let id = ark::hash::hex(&ark::hash::sha256(&[seed.as_bytes()])[..8]);
    Who {
        actor: String::new(),
        session: format!("device-{id}"),
        roles: vec![],
    }
}

impl Replica {
    /// The replica `disk` holds, as whoever last signed in on it — nobody
    /// ([`nobody`]) on a disk nobody ever signed in on — and then, given a
    /// login and its token, as that login, its pending intents made the
    /// login's when they were somebody else's. `device` is what its
    /// volatile `Hello` says.
    pub fn open(mut disk: Box<dyn Disk>, login: Option<(Who, String)>, device: Vec<u8>) -> Replica {
        let kept = disk.get("who").ok().flatten().and_then(|b| <(String, String)>::from_bytes(&b).ok());
        let me = match kept {
            Some((actor, session)) => Who {
                actor,
                session,
                roles: vec![],
            },
            None => {
                let me = nobody();
                let _ = disk.put("who", &(me.actor.clone(), me.session.clone()).to_bytes());
                me
            }
        };
        let mut r = Replica {
            client: Client::open(config(device), me, disk, false),
            token: None,
            url: None,
            dialer: None,
            wire: None,
            due: None,
            backoff: FIRST_MS,
            waker: None,
            offline: false,
            epoch: 0,
            who_epoch: 0,
            standing: BTreeMap::new(),
            rejections: vec![],
            live_refused: vec![],
            updates: BTreeMap::new(),
        };
        r.absorb();
        match login {
            Some((who, token)) if (&who.actor, &who.session) != (&r.me().actor, &r.me().session) => r.sign_in(who, token),
            Some((who, token)) => {
                r.set_roles(who.roles);
                r.token = Some(token);
            }
            None => {}
        }
        r
    }

    /// A replica in memory, as `me`, its token `token` when there is one.
    pub fn memory(me: Who, token: Option<&str>) -> Replica {
        Replica::memory_as(me, token, crate::listening::device())
    }

    /// [`Replica::memory`], saying `device` in its volatile `Hello`.
    pub fn memory_as(me: Who, token: Option<&str>, device: Vec<u8>) -> Replica {
        let mut disk = Memory::default();
        let _ = disk.put("who", &(me.actor.clone(), me.session.clone()).to_bytes());
        let mut r = Replica::open(Box::new(disk), None, device);
        r.set_roles(me.roles);
        r.token = token.map(Into::into);
        r
    }

    /// This replica its own authority (`docs/plan-alone.md` §4): every
    /// intent sequenced as it is made.
    pub fn alone(&mut self) {
        self.client.alone();
        self.absorb();
    }

    pub fn me(&self) -> &Who {
        &self.client.log.me
    }

    /// Who a query reads as, and a view is kept for.
    pub fn ctx(&self) -> Ctx {
        let me = self.me();
        Ctx {
            id: EntryId::of(&me.session, 0),
            actor: me.actor.clone(),
            session: me.session.clone(),
            roles: me.roles.clone(),
            now: now_ms(),
        }
    }

    /// The roles this login holds, which every intent it authors carries:
    /// a library write is refused here as the server would refuse it.
    pub fn set_roles(&mut self, roles: impl IntoIterator<Item = impl Into<String>>) {
        let roles: Vec<String> = roles.into_iter().map(Into::into).collect();
        self.client.log.me.roles.clone_from(&roles);
        if let Some(l) = &mut self.client.live {
            l.me.roles = roles;
        }
        self.who_epoch += 1;
    }

    fn func(name: &str) -> Result<ark::hash::FnHash, String> {
        let b = binding(name).ok_or_else(|| format!("no function {name}"))?;
        Ok(domain().get(&b))
    }

    /// An intent: its id, or the refusal its preview — or, alone, its
    /// sequencing — gave it here and now.
    pub fn mutate(&mut self, name: &str, input: &impl Node) -> Result<EntryId, String> {
        self.mutate_bytes(name, input.to_bytes())
    }

    pub fn mutate_bytes(&mut self, name: &str, input: Vec<u8>) -> Result<EntryId, String> {
        let f = Replica::func(name)?;
        let id = self.client.log.next_id();
        self.client.mutate(f, input, now_ms());
        self.absorb();
        if let Some(i) = self.rejections.iter().rposition(|(r, _)| *r == id) {
            return Err(self.rejections.remove(i).1);
        }
        self.flush();
        Ok(id)
    }

    /// A transition of the listening session (§1.9): an ephemeral one
    /// through the relay, any other at once; the preview's refusal said.
    pub fn call(&mut self, name: &str, input: Vec<u8>) -> Result<(), String> {
        let f = Replica::func(name)?;
        let id = match &self.client.live {
            Some(l) => l.next_id(),
            None => return Err("no listening session here".into()),
        };
        self.client.call(f, input, now_ms());
        self.absorb();
        if let Some(i) = self.live_refused.iter().rposition(|(r, _)| *r == id) {
            return Err(self.live_refused.remove(i).1);
        }
        self.flush();
        Ok(())
    }

    /// A query's answer over this replica, as this login reads it.
    pub fn query<T: Node>(&self, name: &str, input: &impl Node) -> Result<T, String> {
        let b = binding(name).ok_or_else(|| format!("no function {name}"))?;
        let bytes = domain()
            .ask(&self.client.log.store, &self.ctx(), &b, &input.to_bytes())
            .map_err(|r| r.0)?;
        T::from_bytes(&bytes).map_err(|r| format!("{name}: {r}"))
    }

    /// A function's answer over the optimistic state, as this login reads
    /// it: its bytes, as the bindings' `want` reads them.
    fn answer(&self, name: &str, input: &[u8], want: &ark::ty::Ty) -> Result<Vec<u8>, String> {
        let b = binding(name).ok_or_else(|| format!("no function {name}"))?;
        let bytes = domain().ask(&self.client.log.store, &self.ctx(), &b, input).map_err(|r| r.0)?;
        ark::gen::answer(domain(), &b, &bytes, want).map_err(|r| r.0)
    }

    /// A maintained view of a query (§1.10) over the optimistic state, as
    /// this login reads it: its number, or the query's refusal.
    pub fn watch(&mut self, name: &str, input: &[u8]) -> Result<usize, String> {
        let f = Replica::func(name)?;
        let ctx = self.ctx();
        self.client.watch(Space::Durable, &f, input, ctx).map_err(|r| r.0)
    }

    /// A view's rows as they stand, each its bytes.
    pub fn rows(&self, view: usize) -> Vec<Vec<u8>> {
        self.client.rows(view).unwrap_or_default().iter().map(|v| v.to_bytes()).collect()
    }

    pub fn unwatch(&mut self, view: usize) {
        self.client.unwatch(view);
        self.updates.remove(&view);
    }

    /// What a view did since it was last asked.
    pub fn update(&mut self, view: usize) -> Update {
        self.updates.remove(&view).unwrap_or(Update::Unchanged)
    }

    /// What the client said, kept: verdicts by id, patches by view.
    fn absorb(&mut self) {
        for e in self.client.take_events() {
            match e {
                ark_client::Event::Refused {
                    space: Space::Durable,
                    id,
                    why,
                } => {
                    self.standing.insert(id, Standing::Rejected(why.clone()));
                    self.rejections.push((id, why));
                }
                ark_client::Event::Refused { id, why, .. } => self.live_refused.push((id, why)),
                ark_client::Event::Confirmed { space: Space::Durable, id } => {
                    self.standing.insert(id, Standing::Confirmed);
                }
                ark_client::Event::Confirmed { .. } => {}
                ark_client::Event::Patches { patches, .. } if patches.is_empty() => {}
                ark_client::Event::Patches { view, patches } => match self.updates.entry(view).or_insert(Update::Unchanged) {
                    u @ Update::Unchanged => *u = Update::Patched(patches),
                    Update::Patched(ps) => ps.extend(patches),
                    Update::Reset => {}
                },
                ark_client::Event::Dropped { view, .. } => {
                    self.updates.insert(view, Update::Reset);
                }
            }
        }
    }

    /// What the machines have to say, sent — when there is a connection
    /// to send it on; a test carrying frames by hand takes them itself.
    fn flush(&mut self) {
        if let Some(w) = &mut self.wire {
            for f in self.client.take_frames() {
                w.send(f);
            }
        }
    }

    /// A connection opened: a new sight of the session.
    pub fn opened(&mut self) {
        self.epoch += 1;
        self.backoff = FIRST_MS;
        self.client.connected();
        self.absorb();
    }

    /// A frame from the server, by hand (a test's carrier).
    #[cfg(test)]
    pub fn receive(&mut self, bytes: Vec<u8>) {
        self.client.receive(bytes);
        self.absorb();
    }

    /// The connection's news taken in, the clock given to the relay, what
    /// the machines have to say sent; and a redial when it is due.
    pub fn pump(&mut self) -> Pumped {
        let now = monotonic_ms();
        let mut p = Pumped::default();
        if self.due.is_some_and(|d| now >= d) {
            self.dial();
        }
        let events = self.wire.as_mut().map(|w| w.events()).unwrap_or_default();
        for e in events {
            match e {
                Event::Open => {
                    self.opened();
                    (p.opened, p.moved) = (true, true);
                }
                Event::Frame(b) => {
                    self.client.receive(b);
                    p.moved = true;
                }
                Event::Closed(why) => {
                    self.client.disconnected();
                    self.wire = None;
                    match why.strip_prefix("denied: ") {
                        Some(reason) => {
                            p.denied = Some(reason.to_string());
                            self.token = None;
                        }
                        None if self.token.is_some() && !self.offline => {
                            self.due = Some(now + self.backoff);
                            self.backoff = (self.backoff * 2).min(MOST_MS);
                        }
                        None => {}
                    }
                    p.moved = true;
                }
            }
        }
        self.client.tick(now_ms());
        self.absorb();
        self.flush();
        p
    }

    fn dial(&mut self) {
        self.due = None;
        let (Some(url), Some(token)) = (self.url.clone(), self.token.clone()) else {
            return;
        };
        if self.offline {
            return;
        }
        let dialer = self.dialer.get_or_insert_with(default_dialer);
        self.wire = Some(dialer(&url, &token, self.waker.clone()));
    }

    /// Dial `url` (a `ws://…/sync`) whenever somebody is signed in, now and
    /// after every close.
    pub fn connect(&mut self, url: &str) {
        self.url = Some(url.to_string());
        if self.token.is_some() && !self.offline {
            self.due = Some(monotonic_ms());
        }
    }

    /// [`Replica::connect`], through `dialer`.
    pub fn connect_with(&mut self, url: &str, dialer: Dialer) {
        self.dialer = Some(dialer);
        self.connect(url);
    }

    /// Offline on purpose: the connection closed, nothing dialled.
    pub fn disconnect(&mut self) {
        self.offline = true;
        self.wire = None;
        self.due = None;
        self.client.disconnected();
    }

    /// Back online.
    pub fn reconnect(&mut self) {
        self.offline = false;
        if let Some(url) = self.url.clone() {
            self.connect(&url);
        }
    }

    /// What this replica did alone handed to the server at `url`
    /// (`docs/plan-alone.md` §4), in place: its history pending again
    /// above where it forked, every list told what moved — nothing.
    pub fn join(&mut self, url: &str) {
        self.client.join();
        self.absorb();
        self.connect(url);
    }

    /// Somebody signed in: every pending intent theirs, and the connection
    /// dialled with their token.
    pub fn sign_in(&mut self, who: Who, token: String) {
        // A joined history goes as intents under the login, not as a chain
        // only nobody's session could hand over.
        self.client.log.joining = None;
        let _ = self.client.disk.put("who", &(who.actor.clone(), who.session.clone()).to_bytes());
        self.wire = None;
        self.client.disconnected();
        self.client.sign_in(who);
        self.token = Some(token);
        self.who_epoch += 1;
        self.absorb();
        if let Some(url) = self.url.clone() {
            self.connect(&url);
        }
    }

    /// Signed out: the replica stays and goes on authoring as them, with
    /// nothing dialled.
    pub fn sign_out(&mut self) {
        self.token = None;
        self.wire = None;
        self.due = None;
        self.client.disconnected();
    }

    pub fn is_signed_out(&self) -> bool {
        self.token.is_none()
    }

    pub fn is_alone(&self) -> bool {
        self.client.log.is_authority()
    }

    pub fn linked(&self) -> bool {
        self.client.linked
    }

    pub fn pending_len(&self) -> usize {
        self.client.log.pending.len()
    }

    /// The head of the confirmed chain.
    pub fn cursor(&self) -> i64 {
        self.client.log.log().head().0
    }

    /// The confirmed and pending state's hash: what moves when the store
    /// does.
    pub fn state(&self) -> StateHash {
        self.client.log.store.state_hash()
    }

    pub fn status(&self) -> Status {
        let link = match (self.is_alone(), self.token.is_some(), self.client.linked, self.offline) {
            (true, ..) => "alone",
            (_, false, ..) => "signed out",
            (_, _, true, _) => "online",
            (_, _, false, true) => "offline",
            _ if self.wire.is_some() => "connecting",
            _ => "waiting to dial",
        };
        Status {
            link: link.into(),
            linked: self.client.linked,
            epoch: self.epoch,
            cursor: self.cursor(),
            pending: self.pending_len(),
            joining: self.client.log.joining.is_some(),
            user: self.me().actor.clone(),
            alone: self.is_alone(),
        }
    }

    /// The listening session's machine, in numbers.
    pub fn live_status(&self) -> Option<LiveStatus> {
        let l = self.client.live.as_ref()?;
        Some(LiveStatus {
            seq: l.log().head().0,
            pending: l.pending.len(),
            synced: self.present(),
        })
    }

    /// Whether this device stands in its account's listening session: its
    /// row is in the volatile store, which only `on_join` writes.
    pub fn present(&self) -> bool {
        self.listening().device(&self.me().session).is_some()
    }

    /// The account's listening session as this device holds it: the
    /// server's, with its own calls previewed on top.
    pub fn listening(&self) -> crate::domain::host::Listening {
        match &self.client.live {
            Some(l) => crate::domain::host::Listening::of(&l.store, &self.me().actor),
            None => Default::default(),
        }
    }

    /// How many connections have opened: a new one is a new sight of the
    /// session.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The server's clock, as far as this device knows it: its own (v5's
    /// entries carry the clock of whoever authored them).
    pub fn server_now(&self) -> i64 {
        now_ms()
    }

    /// The one timer this replica needs: the next dial.
    pub fn deadline(&self) -> Option<u64> {
        self.due
    }

    pub fn set_waker(&mut self, w: Wake) {
        self.waker = Some(w);
    }

    pub fn standing(&self, id: &EntryId) -> Standing {
        if self.client.log.pending.iter().any(|p| p.entry.id == *id) {
            return Standing::Pending;
        }
        self.standing.get(id).cloned().unwrap_or(Standing::Unknown)
    }

    /// What the server refused of this replica's intents since the last ask.
    pub fn take_rejections(&mut self) -> Vec<(EntryId, String)> {
        std::mem::take(&mut self.rejections)
    }

    /// What the server refused of this device's calls since the last ask.
    pub fn take_live_refusals(&mut self) -> Vec<String> {
        std::mem::take(&mut self.live_refused).into_iter().map(|(_, w)| w).collect()
    }
}

/// The transport this build dials with.
fn default_dialer() -> Dialer {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(native::dial)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Box::new(web::dial)
    }
}

/// A WebSocket on a thread of its own: the token first, then frames; a
/// close with code 4401 is the server turning the login away.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use tungstenite::stream::MaybeTlsStream;
    use tungstenite::{Error, Message};

    use super::{Event, Wake, Wire};

    struct Thread {
        out: Sender<Vec<u8>>,
        events: Receiver<Event>,
    }

    impl Wire for Thread {
        fn send(&mut self, frame: Vec<u8>) {
            let _ = self.out.send(frame);
        }
        fn events(&mut self) -> Vec<Event> {
            self.events.try_iter().collect()
        }
    }

    pub fn dial(url: &str, token: &str, wake: Option<Wake>) -> Box<dyn Wire> {
        let (out, from) = channel::<Vec<u8>>();
        let (to, events) = channel();
        let (url, token) = (url.to_string(), token.to_string());
        std::thread::spawn(move || {
            let said = |e: Event| {
                let ok = to.send(e).is_ok();
                if let Some(w) = &wake {
                    w();
                }
                ok
            };
            let closed = |why: String| {
                said(Event::Closed(why));
            };
            let mut ws = match tungstenite::connect(&url) {
                Ok((ws, _)) => ws,
                Err(e) => return closed(e.to_string()),
            };
            if let MaybeTlsStream::Plain(s) = ws.get_mut() {
                let _ = s.set_read_timeout(Some(Duration::from_millis(5)));
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
                        let why = match c {
                            Some(f) if u16::from(f.code) == 4401 => format!("denied: {}", f.reason),
                            Some(f) => f.reason.to_string(),
                            None => "the connection closed".into(),
                        };
                        return closed(why);
                    }
                    Ok(_) => {}
                    Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        let _ = ws.flush();
                    }
                    Err(e) => return closed(e.to_string()),
                }
            }
        });
        Box::new(Thread { out, events })
    }
}

/// The page's own `WebSocket`: the token first, then frames, each event
/// ringing the bell. Not compiled here (no wasm32 target in this tree's
/// toolchain); written against `web-sys`'s documented API.
#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

    use super::{Event, Wake, Wire};

    struct Socket {
        ws: Option<WebSocket>,
        events: Rc<RefCell<Vec<Event>>>,
        _kept: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    }

    impl Wire for Socket {
        fn send(&mut self, frame: Vec<u8>) {
            if let Some(ws) = &self.ws {
                let _ = ws.send_with_u8_array(&frame);
            }
        }
        fn events(&mut self) -> Vec<Event> {
            std::mem::take(&mut *self.events.borrow_mut())
        }
    }

    impl Drop for Socket {
        fn drop(&mut self) {
            if let Some(ws) = &self.ws {
                ws.set_onopen(None);
                ws.set_onmessage(None);
                ws.set_onclose(None);
                let _ = ws.close();
            }
        }
    }

    pub fn dial(url: &str, token: &str, wake: Option<Wake>) -> Box<dyn Wire> {
        let events: Rc<RefCell<Vec<Event>>> = Rc::default();
        let ring = move || {
            if let Some(w) = &wake {
                w();
            }
        };
        let ws = match WebSocket::new(url) {
            Ok(ws) => ws,
            Err(e) => {
                events.borrow_mut().push(Event::Closed(format!("{e:?}")));
                ring();
                return Box::new(Socket {
                    ws: None,
                    events,
                    _kept: vec![],
                });
            }
        };
        ws.set_binary_type(BinaryType::Arraybuffer);
        let (q, r, w, t) = (events.clone(), ring.clone(), ws.clone(), token.to_string());
        let open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = w.send_with_u8_array(t.as_bytes());
            q.borrow_mut().push(Event::Open);
            r();
        });
        let (q, r) = (events.clone(), ring.clone());
        let message = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(e) = e.dyn_ref::<MessageEvent>() else { return };
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                q.borrow_mut().push(Event::Frame(js_sys::Uint8Array::new(&buf).to_vec()));
                r();
            }
        });
        let (q, r) = (events.clone(), ring);
        let close = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let why = match e.dyn_ref::<CloseEvent>() {
                Some(c) if c.code() == 4401 => format!("denied: {}", c.reason()),
                Some(c) => c.reason(),
                None => "the connection closed".into(),
            };
            q.borrow_mut().push(Event::Closed(why));
            r();
        });
        ws.set_onopen(Some(open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(close.as_ref().unchecked_ref()));
        Box::new(Socket {
            ws: Some(ws),
            events,
            _kept: vec![open, message, close],
        })
    }
}

/// The records of a replica in the page's `localStorage`, under one key's
/// prefix: an `ark_client::Memory` read once and written through, each
/// part as hex. Not compiled here (no wasm32 target).
#[cfg(target_arch = "wasm32")]
pub struct Stored {
    key: String,
    mem: Memory,
}

#[cfg(target_arch = "wasm32")]
impl Stored {
    pub fn open(key: &str) -> Result<Stored, String> {
        let s = ark_auth::web::storage().ok_or("this page has no localStorage")?;
        let get = |k: &str| s.get_item(&format!("{key}:{k}")).ok().flatten().and_then(|h| ark::hash::unhex(&h));
        let mut mem = Memory::default();
        mem.snapshot = get("snapshot");
        if let Some(b) = get("chain") {
            mem.chain = <Vec<Vec<u8>>>::from_bytes(&b).map_err(|e| e.0)?;
        }
        if let Some(b) = get("blobs") {
            mem.blobs = <Vec<(String, Vec<u8>)>>::from_bytes(&b).map_err(|e| e.0)?.into_iter().collect();
        }
        Ok(Stored { key: key.into(), mem })
    }

    fn save(&self) -> std::io::Result<()> {
        let s = ark_auth::web::storage().ok_or_else(|| std::io::Error::other("no localStorage"))?;
        let put = |k: &str, b: &[u8]| {
            s.set_item(&format!("{}:{k}", self.key), &ark::hash::hex(b))
                .map_err(|e| std::io::Error::other(format!("{e:?}")))
        };
        if let Some(b) = &self.mem.snapshot {
            put("snapshot", b)?;
        }
        put("chain", &self.mem.chain.to_bytes())?;
        let blobs: Vec<(String, Vec<u8>)> = self.mem.blobs.clone().into_iter().collect();
        put("blobs", &blobs.to_bytes())
    }
}

#[cfg(target_arch = "wasm32")]
impl Disk for Stored {
    fn load(&mut self) -> std::io::Result<ark_client::storage::Stored> {
        self.mem.load()
    }
    fn record(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.mem.record(bytes)?;
        self.save()
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.mem.put(name, bytes)?;
        self.save()
    }
    fn append(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.mem.append(name, bytes)?;
        self.save()
    }
    fn get(&mut self, name: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.mem.get(name)
    }
}

/// The bindings' stubs (`crate::domain`) call the domain through a
/// replica: a mutator authored as an intent, a query asked of the
/// optimistic state.
impl crate::domain::Call for Replica {
    type Authored = Result<EntryId, String>;

    fn author(&mut self, name: &str, input: Vec<u8>) -> Result<EntryId, String> {
        self.mutate_bytes(name, input)
    }

    fn ask(&self, name: &str, input: Vec<u8>, want: &ark::ty::Ty) -> Result<Vec<u8>, String> {
        self.answer(name, &input, want)
    }
}
