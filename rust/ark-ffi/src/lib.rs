//! A device's peer for an app written in another language (`docs/plan-v5.md`
//! §1.16, N3): `ark-peer`'s app kit through UniFFI, every value as its JSON.
//!
//! The kit is domain-agnostic: an app's own Rust crate links its domain in
//! and opens a [`Peer`] over it ([`Peer::open`]; harken's is `harken/ffi`,
//! whose `harken_open` is the constructor its phones call). Through the
//! bridge a [`Peer`] is called by function name with JSON — `mutate`,
//! `query`, `view`, the live space's `call` — and says where it stands
//! (`status`), what became of each intent (`standing`, `take_rejections`),
//! which jobs of a kind are due for a handler written in the app (`due`),
//! who it is (`login`, `signed_in`, `logout`: `ark-auth`'s session rows)
//! and whether it is alone (`alone`, `join`).
//! The typed Kotlin and Swift over these names is generated from the domain
//! (`arkdb gen`), types only; a view whose row type has a twin is read
//! typed instead, its rows UniFFI's records ([`typed`]).
//!
//! **It drives itself.** A peer has a thread of its own that pumps its link
//! whenever the connection rings or a dial is due, and keeps its views up:
//! each [`ViewHandle`] gathers its patches ([`ViewUpdate`]) until taken, and
//! the app's [`Listener`] is told whenever anything moved — a frame came, a
//! view was patched — on whatever thread moved it; the app then takes its
//! views and asks `status` on its own. A view that could not be kept up is
//! read again by itself and says [`ViewUpdate::Reset`] with its rows.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use ark::log::EntryId;
use ark::peer::Who;
use ark_peer::link::{bell, Dial, Wake};
use ark_peer::{monotonic_ms, Config, Files, Update, Watched};

uniffi::setup_scaffolding!();

pub mod session;
pub mod typed;
pub use session::{Hold, Session};
pub use typed::{FileRefFfi, Op, RowId, Taken, Twin, Typed};

/// Why something was not done, as a sentence: what a form shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum Refusal {
    Refused { why: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Refusal::Refused { why } = self;
        f.write_str(why)
    }
}

impl std::error::Error for Refusal {}

impl<S: Into<String>> From<S> for Refusal {
    fn from(why: S) -> Refusal {
        Refusal::Refused { why: why.into() }
    }
}

/// A kind's jobs due now, each its row's JSON, and when the next is due.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Due {
    pub rows: Vec<String>,
    pub next_ms: Option<i64>,
}

/// A login as the server hands it over: the token the link proves itself
/// with, and who it is. The sign-in machine's (`ark_auth::onboarding`),
/// which hands it to the app.
pub use ark_auth::onboarding::Login;

/// Who a login is, as an entry carries it.
fn who(l: &Login) -> Who {
    Who {
        actor: l.user.clone(),
        session: l.session.clone(),
        roles: l.roles.clone(),
    }
}

/// One change to a view's rows, each row its JSON (§1.10).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Patch {
    /// `row` before the row at `at` (`at` the length appends).
    Insert {
        at: u64,
        row: String,
    },
    Remove {
        at: u64,
    },
    Update {
        at: u64,
        row: String,
    },
}

/// The server's answer to a call ([`Peer::request`]): what its function
/// gave back, as bytes, or why not.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Reply {
    Ok { output: Vec<u8> },
    Refused { why: String },
}

/// What a view did since it was last taken.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ViewUpdate {
    Unchanged,
    Patched {
        patches: Vec<Patch>,
    },
    /// It was read again: these are its rows.
    Reset {
        rows: Vec<String>,
    },
}

/// What has become of an intent this device authored.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Standing {
    Pending,
    Confirmed,
    Rejected { why: String },
    Unknown,
}

/// Where a device stands, as a status line says it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Status {
    /// `alone`, `signed out`, `online`, `offline`, `connecting` or `waiting
    /// to dial`.
    pub link: String,
    pub linked: bool,
    pub epoch: u64,
    pub cursor: i64,
    pub pending: u64,
    pub joining: bool,
    pub user: String,
    pub session: String,
    pub alone: bool,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
    /// Why the server last turned this login away (close 4401), until the
    /// next login: the link has stopped dialling.
    pub denied: Option<String>,
}

/// An intent the server refused: its id and why.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Rejection {
    pub id: String,
    pub why: String,
}

/// What the app is told when anything moved: it takes its views then.
#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    fn changed(&self);
}

/// A view as the bridge keeps it (`ark_peer::Watched`, its rows read from
/// the peer as they are asked for), and what it did since taken.
struct Kept {
    view: Watched<()>,
    update: Update,
}

struct State {
    peer: ark_peer::Peer,
    views: BTreeMap<u64, Kept>,
    /// Each app's store of its views ([`Session`]), by number.
    stores: BTreeMap<u64, Box<dyn session::Attached>>,
    next: u64,
    /// Until when the driver wakes often: a live call's relay is due.
    hurry: u64,
    /// Why the server last turned the login away.
    denied: Option<String>,
    /// The calls' answers waiting when the app was last told.
    replies: usize,
    /// The files that had arrived when the app was last told.
    files: u64,
}

impl State {
    /// Every view's updates taken from the peer into what the app has not
    /// taken; a view opened for another login, or refused, opened again.
    /// Whether any moved — or a call's answer came.
    fn settle(&mut self) -> bool {
        let mut moved = false;
        for k in self.views.values_mut() {
            match k.view.settle(&mut self.peer, true) {
                Update::Unchanged => continue,
                Update::Patched(ps) => k.update.patch(ps),
                Update::Reset => k.update = Update::Reset,
            }
            moved = true;
        }
        for a in self.stores.values_mut() {
            moved |= a.settle(&mut self.peer);
        }
        let replies = self.peer.replies_waiting();
        moved |= replies > self.replies;
        self.replies = replies;
        let files = self.peer.files_arrived();
        moved |= files != self.files;
        self.files = files;
        moved
    }

    /// A view's number and name while it answers, its rows about to be read as they stand:
    /// what it did until now is in them, so it is taken with them, here and
    /// in the peer, and the next `take` says only what it does after.
    /// Otherwise a patch the driver's thread settled between the view's
    /// opening and this read (the server's rows arriving as an app opens)
    /// came again in the next `take`, and the app's list, its rows with
    /// that patch spliced in, held a row twice: money's budget a category
    /// twice, a lazy list's key used twice, the app gone (app-screens
    /// 42718045335, money's dark phone).
    fn base(&mut self, id: u64) -> Option<(usize, String)> {
        let k = self.views.get_mut(&id)?;
        k.view.settle(&mut self.peer, true);
        k.update = Update::Unchanged;
        Some((k.view.view()?, k.view.name().to_string()))
    }
}

struct Shared {
    state: Mutex<State>,
    listener: Mutex<Option<Arc<dyn Listener>>>,
    wake: Wake,
    /// Pumped by whoever drives it ([`Peer::pump`]), never by its thread.
    by_hand: std::sync::atomic::AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `f` on the peer, its views settled after; the listener told when
    /// anything moved, and the driver rung to send what it said.
    fn act<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let (out, moved) = {
            let mut s = self.lock();
            let out = f(&mut s);
            (out, s.settle())
        };
        if moved {
            self.tell();
        }
        (self.wake)();
        out
    }

    fn tell(&self) {
        let l = self.listener.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(l) = l {
            l.changed();
        }
        self.tell_sessions();
    }

    /// How long the driver may sleep: until the next dial, a relay's
    /// interval while one is due, a second at most.
    fn patience(&self) -> Duration {
        let s = self.lock();
        let now = monotonic_ms();
        let mut ms = 1000;
        if let Some(d) = s.peer.deadline() {
            ms = ms.min(d.saturating_sub(now));
        }
        if s.hurry > now {
            let every = s.peer.config.live.as_ref().map_or(50, |l| l.every_ms.max(1) as u64);
            ms = ms.min(every);
        }
        Duration::from_millis(ms)
    }

    fn pump(&self) -> bool {
        let moved = {
            let mut s = self.lock();
            let p = s.peer.pump();
            if p.denied.is_some() {
                s.denied = p.denied;
            }
            s.settle() | p.moved
        };
        if moved {
            self.tell();
        }
        moved
    }
}

/// The driver: a pump whenever the link rings or the peer's patience runs
/// out, until the peer is gone.
fn drive(shared: Weak<Shared>, rx: Receiver<()>) {
    loop {
        let wait = match shared.upgrade() {
            Some(s) => s.patience(),
            None => return,
        };
        if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(wait) {
            return;
        }
        while rx.try_recv().is_ok() {}
        match shared.upgrade() {
            Some(s) if !s.by_hand.load(std::sync::atomic::Ordering::Relaxed) => drop(s.pump()),
            Some(_) => {}
            None => return,
        }
    }
}

/// A value's JSON, or what went wrong reading it (which a test sees).
fn text(json: Result<String, String>) -> String {
    json.unwrap_or_else(|why| format!("{{\"error\":{}}}", serde_json::Value::String(why)))
}

/// A view's patch, each row its JSON as `elem` writes it.
fn patch(p: ark_peer::Patch, elem: &dyn Fn(&[u8]) -> String) -> Patch {
    match p {
        ark::plan::Patch::Insert { at, node } => Patch::Insert {
            at: at as u64,
            row: elem(&node),
        },
        ark::plan::Patch::Remove { at } => Patch::Remove { at: at as u64 },
        ark::plan::Patch::Update { at, node } => Patch::Update {
            at: at as u64,
            row: elem(&node),
        },
    }
}

/// A device's peer: the app kit of `ark-peer` over a domain, on a disk.
#[derive(uniffi::Object)]
pub struct Peer {
    shared: Arc<Shared>,
}

impl Peer {
    /// The device `dir` holds, under `config`'s domain, as whoever last
    /// signed in on it — or, given a login, as that login — with its driver
    /// started. What an app's own crate calls with its domain.
    pub fn open(config: Config, dir: &str, login: Option<Login>) -> Result<Arc<Peer>, Refusal> {
        let disk = Files::new(dir).map_err(|e| format!("{dir}: {e}"))?;
        let login = login.map(|l| (who(&l), l.token));
        Peer::of(ark_peer::Peer::open_as(config, Box::new(disk), login))
    }

    /// A peer opened by its host its own way — as a login it keeps itself,
    /// its link tuned — driven from now on as [`Peer::open`]'s is.
    pub fn of(mut peer: ark_peer::Peer) -> Result<Arc<Peer>, Refusal> {
        // What this device does goes to its server (docs/plan-observe.md).
        ark_peer::observe::install(ark_peer::observe::Sink::Outbox);
        let (wake, rx) = bell();
        peer.set_waker(wake.clone());
        let state = State {
            peer,
            views: BTreeMap::new(),
            stores: BTreeMap::new(),
            next: 0,
            hurry: 0,
            denied: None,
            replies: 0,
            files: 0,
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            listener: Mutex::new(None),
            wake,
            by_hand: Default::default(),
        });
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("ark-peer".into())
            .spawn(move || drive(weak, rx))
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(Peer { shared }))
    }

    /// `f` on the peer itself, its views settled after: what an app's crate
    /// reads beyond the kit with (its live state as plain Rust).
    pub fn with<T>(&self, f: impl FnOnce(&mut ark_peer::Peer) -> T) -> T {
        self.shared.act(|s| f(&mut s.peer))
    }

    /// [`Peer::connect`] through `dial`: a test's in-process server.
    pub fn connect_with(&self, url: &str, dial: impl Dial + 'static) {
        self.shared.act(|s| s.peer.connect_with(url, dial));
    }

    /// Pumped by hand from now on — [`Peer::pump`], and never by its own
    /// thread — so that a test says when what arrived is taken in.
    pub fn by_hand(&self) {
        self.shared.by_hand.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// What the connection said taken in now, and what the peer has to say
    /// sent: whether anything moved.
    pub fn pump(&self) -> bool {
        self.shared.pump()
    }

    /// A maintained view of `name` on its input's bytes.
    pub fn view_bytes(&self, name: String, args: Vec<u8>) -> Result<Arc<ViewHandle>, Refusal> {
        let id = self.shared.act(|s| -> Result<u64, String> {
            let view = Watched::try_open(&mut s.peer, &name, args, |_| ())?;
            let id = s.next;
            s.views.insert(
                id,
                Kept {
                    view,
                    update: Update::Unchanged,
                },
            );
            s.next += 1;
            Ok(id)
        })?;
        Ok(Arc::new(ViewHandle {
            shared: self.shared.clone(),
            id,
        }))
    }
}

#[uniffi::export]
impl Peer {
    /// Tell `listener` whenever anything moved; none tells nobody.
    pub fn set_listener(&self, listener: Option<Arc<dyn Listener>>) {
        *self.shared.listener.lock().unwrap_or_else(|e| e.into_inner()) = listener;
    }

    /// Dial `url` (`ws://…/sync`, [`socket_url`]) whenever signed in.
    pub fn connect(&self, url: String) {
        self.shared.act(|s| s.peer.connect(&url));
    }

    /// Offline on purpose, or back online.
    pub fn set_online(&self, online: bool) {
        self.shared.act(|s| if online { s.peer.reconnect() } else { s.peer.disconnect() });
    }

    /// Dial the server now, whatever the link's backoff says: the app came
    /// to the front, so a server that is there is reached before the screens
    /// say it is not. Nothing while connected, offline on purpose or signed out.
    pub fn dial_now(&self) {
        self.shared.act(|s| s.peer.dial_now());
    }

    /// An intent of the function `name` on its input's JSON: its id, or why
    /// its preview — or, alone, its sequencing — refused it.
    pub fn mutate(&self, name: String, input: String) -> Result<String, Refusal> {
        let _call = tracing::debug_span!(target: "ark", "ffi", call = "mutate", name = %name).entered();
        let id = self.shared.act(|s| {
            let input = s.peer.input_json(&name, &input)?;
            // An app's mutate is somebody's doing — a tap — and the root of
            // what it causes, unless the app opened a span around it; a
            // job's own steps answer what made the job due instead.
            let job = [".claim", ".finish", ".cancel"].iter().any(|v| name.ends_with(v));
            if job || s.peer.open_span().is_some() {
                return s.peer.mutate_bytes(&name, input);
            }
            s.peer.begin(&name);
            let out = s.peer.mutate_bytes(&name, input);
            s.peer.end();
            out
        })?;
        Ok(ark::hash::hex(&id.0))
    }

    /// An intent of the function `name` on its input's bytes (the value
    /// format), as [`Peer::mutate`] makes one of JSON: what an app's typed
    /// change (money's `MoneyChange`) is sent as, no JSON on the way.
    pub fn mutate_encoded(&self, name: &str, input: Vec<u8>) -> Result<String, Refusal> {
        let _call = tracing::debug_span!(target: "ark", "ffi", call = "mutate", name = %name).entered();
        let id = self.shared.act(|s| {
            if s.peer.open_span().is_some() {
                return s.peer.mutate_bytes(name, input);
            }
            s.peer.begin(name);
            let out = s.peer.mutate_bytes(name, input);
            s.peer.end();
            out
        })?;
        Ok(ark::hash::hex(&id.0))
    }

    /// A call of one of the server's own functions (`ark_peer::Config::calls`)
    /// on its input's bytes, sent now: its id, for [`Peer::take_reply`]; never
    /// logged, and refused while not connected. The listener rings when its
    /// answer comes.
    pub fn request(&self, name: String, input: Vec<u8>) -> Result<u64, Refusal> {
        Ok(self.shared.act(|s| s.peer.request(&name, input))?)
    }

    /// What the server answered the call `id`, once it has; told once.
    pub fn take_reply(&self, id: u64) -> Option<Reply> {
        self.shared.act(|s| s.peer.take_reply(id)).map(|r| match r {
            Ok(output) => Reply::Ok { output },
            Err(why) => Reply::Refused { why },
        })
    }

    /// A file made on this device (docs/plan-files.md): kept in its
    /// `files/`, sent to the server on this or the next connection; its
    /// name (the hex sha256 of `bytes`), for the `FileRef` a mutation
    /// names it by.
    pub fn put_file(&self, bytes: Vec<u8>, media: String) -> Result<String, Refusal> {
        Ok(self.shared.act(|s| s.peer.put_file(&bytes, &media))?.hash)
    }

    /// A file's bytes when this device holds them; when not, fetched from
    /// the server, and the listener rings when they arrive.
    pub fn file(&self, hash: String, size: i64, media: String) -> Option<Vec<u8>> {
        let f = ark_peer::FileRef { hash, size, media };
        self.shared.act(|s| s.peer.open_file(&f))
    }

    /// Why the server would not give a file, when it said.
    pub fn file_absent(&self, hash: String) -> Option<String> {
        let f = ark_peer::FileRef { hash, ..Default::default() };
        self.shared.lock().peer.file_absent(&f).map(String::from)
    }

    /// The first of an input's refinements that fails, as a form says it.
    pub fn check(&self, name: String, input: String) -> Option<String> {
        let s = self.shared.lock();
        let f = s.peer.binding(&name).and_then(|b| s.peer.domain().get(&b).cloned());
        match f {
            Some(f) => match s.peer.input_json(&name, &input) {
                Ok(b) => (f.check)(&b).err().and_then(|mut e| e.drain(..).next()).map(|(_, w)| w),
                Err(why) => Some(why),
            },
            None => Some(format!("no function {name}")),
        }
    }

    /// A query's or a view's answer on its input's JSON, as JSON, over the
    /// optimistic state.
    pub fn query(&self, name: String, input: String) -> Result<String, Refusal> {
        let s = self.shared.lock();
        let answer = s.peer.ask(&name, &s.peer.input_json(&name, &input)?)?;
        Ok(s.peer.answer_json(&name, &answer)?)
    }

    /// The jobs of `kind` (a table of `ark::effect` rows) a claim would take
    /// now, each its row's JSON, and when the next falls due: what a
    /// handler written in the app asks whenever the listener rings or its
    /// timer for `next_ms` fires, then answers through `mutate` —
    /// `<kind>.claim`, and once that is confirmed (`standing`),
    /// `<kind>.finish`. A view's rows stream through `view` already.
    pub fn due(&self, kind: String) -> Result<Due, Refusal> {
        let answer = self.query(format!("{kind}.due"), "null".into())?;
        let a: serde_json::Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        let rows = a["rows"].as_array().into_iter().flatten();
        Ok(Due {
            rows: rows.map(|r| r.to_string()).collect(),
            next_ms: a["next_ms"].as_i64(),
        })
    }

    /// A maintained view of the function `name` on its input's JSON.
    pub fn view(&self, name: String, input: String) -> Result<Arc<ViewHandle>, Refusal> {
        let args = self.shared.lock().peer.input_json(&name, &input)?;
        self.view_bytes(name, args)
    }

    /// A call on the live space (§1.9) on its input's JSON: an ephemeral
    /// one through the relay, any other at once.
    pub fn call(&self, name: String, input: String) -> Result<(), Refusal> {
        let _call = tracing::debug_span!(target: "ark", "ffi", call = "call", name = %name).entered();
        Ok(self.shared.act(|s| {
            s.hurry = monotonic_ms() + 1000;
            let input = s.peer.input_json(&name, &input)?;
            s.peer.call(&name, input)
        })?)
    }

    /// A trace begun here, named `name` — what a tap, a gesture, a scan in
    /// the app opens — and everything this peer authors under it until
    /// [`Peer::end`]: what it caused, everywhere, is one trace. Its span, as
    /// `trace/span` in hex.
    pub fn begin(&self, name: String) -> String {
        let s = self.shared.act(|s| s.peer.begin(&name));
        format!("{}/{}", ark::hash::hex(&s.trace.0), ark::hash::hex(&s.id))
    }

    /// The span [`Peer::begin`] opened, closed.
    pub fn end(&self) {
        self.shared.act(|s| s.peer.end());
    }

    /// Something the app saw, said among this peer's events under the span
    /// it authors under now: `name`, and `detail` as the app writes it.
    pub fn event(&self, name: String, detail: String) {
        let s = self.shared.lock();
        let span = s
            .peer
            .cause(ark::schema::Space::Durable, ark_peer::now_ms())
            .map(|s| ark::hash::hex(&s.id))
            .unwrap_or_default();
        tracing::info!(target: "ark", peer = %s.peer.label(), span = %span, name = %name, detail = %detail, "app");
    }

    /// The rows of the live space's `table` this device holds, as JSON.
    pub fn live_rows(&self, table: String) -> Vec<String> {
        let s = self.shared.lock();
        let Some(live) = &s.peer.live else { return vec![] };
        live.store.rows(&table).map(|r| text(s.peer.row_json(&table, r))).collect()
    }

    /// Whether nothing this device has asked for is still on its way: alone,
    /// or linked with no intent of its own unconfirmed and no call's answer
    /// waiting to be taken. A screenshot is taken only once it is (and the
    /// app's own runtime is idle: ui/docs/shots.md), so it never shows a
    /// replica half caught up.
    pub fn settled(&self) -> bool {
        let s = self.shared.lock();
        let st = s.peer.status();
        (st.alone || st.linked) && st.pending == 0 && s.peer.replies_waiting() == 0
    }

    pub fn status(&self) -> Status {
        let s = self.shared.lock();
        let st = s.peer.status();
        Status {
            link: st.link,
            linked: st.linked,
            epoch: st.epoch,
            cursor: st.cursor,
            pending: st.pending as u64,
            joining: false,
            user: st.user,
            session: s.peer.me().session.clone(),
            alone: st.alone,
            failure: s.peer.failure().map(Into::into),
            denied: s.denied.clone(),
        }
    }

    /// Where the intent `id` (as [`Peer::mutate`] gave it) stands.
    pub fn standing(&self, id: String) -> Standing {
        let Some(id) = ark::hash::unhex(&id).and_then(|b| b.try_into().ok()).map(EntryId) else {
            return Standing::Unknown;
        };
        match self.shared.lock().peer.standing(&id) {
            ark_peer::Standing::Pending => Standing::Pending,
            ark_peer::Standing::Confirmed => Standing::Confirmed,
            ark_peer::Standing::Rejected(why) => Standing::Rejected { why },
            ark_peer::Standing::Unknown => Standing::Unknown,
        }
    }

    /// What the server refused of this device's intents since the last ask.
    pub fn take_rejections(&self) -> Vec<Rejection> {
        let mut s = self.shared.lock();
        let r = s.peer.take_rejections();
        r.into_iter()
            .map(|(id, why)| Rejection {
                id: ark::hash::hex(&id.0),
                why,
            })
            .collect()
    }

    /// What the server refused of this device's live calls since the last
    /// ask.
    pub fn take_live_refusals(&self) -> Vec<String> {
        self.shared.lock().peer.take_live_refusals()
    }

    /// Somebody signed in: every pending intent theirs, dialled as them.
    pub fn login(&self, login: Login) {
        self.shared.act(|s| {
            s.denied = None;
            s.peer.login(who(&login), login.token)
        });
    }

    /// Who this device is signed in as, as its replica holds its session
    /// row (`ark_auth::rows::current`): its person, roles granted merged,
    /// and no token. Nobody signed out, or before the session has synced.
    pub fn signed_in(&self) -> Option<Login> {
        let s = self.shared.lock().peer.current()?;
        Some(Login {
            token: String::new(),
            session: s.id,
            user: s.user,
            name: s.name,
            email: s.email,
            roles: s.roles,
            expires_ms: s.expires_ms,
            picture: s.picture,
        })
    }

    /// Signed out: the session revoked through `session.revoke` (pushed
    /// while linked, pending for the next sign-in otherwise), nothing
    /// dialled after it, what is pending kept.
    pub fn logout(&self) {
        self.shared.act(|s| drop(s.peer.sign_out()));
    }

    /// Alone: its own authority over a local history (§1.7).
    pub fn alone(&self) {
        self.shared.act(|s| s.peer.alone());
    }

    /// Joined to the server at `url`: the local history handed over.
    pub fn join(&self, url: String) {
        self.shared.act(|s| {
            s.peer.join();
            s.peer.connect(&url);
        });
    }
}

/// A view the app holds: its rows, and what it did since last taken. Held
/// no more when dropped.
#[derive(uniffi::Object)]
pub struct ViewHandle {
    shared: Arc<Shared>,
    id: u64,
}

#[uniffi::export]
impl ViewHandle {
    /// Its rows as they stand, each its JSON; what it did until now is in
    /// them, and the next `take` says only what it does after.
    pub fn rows(&self) -> Vec<String> {
        let mut s = self.shared.lock();
        let Some((at, name)) = s.base(self.id) else {
            return vec![];
        };
        s.peer.rows(at).iter().map(|r| text(s.peer.element_json(&name, r))).collect()
    }

    /// What it did since last taken.
    pub fn take(&self) -> ViewUpdate {
        let mut s = self.shared.lock();
        s.settle();
        let Some(k) = s.views.get_mut(&self.id) else {
            return ViewUpdate::Unchanged;
        };
        let (update, name) = (std::mem::replace(&mut k.update, Update::Unchanged), k.view.name().to_string());
        let elem = |b: &[u8]| text(s.peer.element_json(&name, b));
        match update {
            Update::Unchanged => ViewUpdate::Unchanged,
            Update::Patched(ps) => ViewUpdate::Patched {
                patches: ps.into_iter().map(|p| patch(p, &elem)).collect(),
            },
            Update::Reset => {
                drop(s);
                ViewUpdate::Reset { rows: self.rows() }
            }
        }
    }
}

impl ViewHandle {
    /// Its rows as they stand, each its bytes; what it did until now is in
    /// them, and the next `take` says only what it does after.
    pub fn rows_bytes(&self) -> Vec<Vec<u8>> {
        let mut s = self.shared.lock();
        s.base(self.id).map_or(vec![], |(at, _)| s.peer.rows(at))
    }

    /// What it did since last taken, and the rows that came with it, each
    /// its bytes ([`Taken`]).
    pub fn take_bytes(&self) -> (Taken, Vec<Vec<u8>>) {
        let mut s = self.shared.lock();
        s.settle();
        let Some(k) = s.views.get_mut(&self.id) else {
            return (Taken::Unchanged, vec![]);
        };
        match std::mem::replace(&mut k.update, Update::Unchanged) {
            Update::Unchanged => (Taken::Unchanged, vec![]),
            Update::Patched(ps) => {
                let (mut ops, mut rows) = (vec![], vec![]);
                for p in ps {
                    ops.push(match p {
                        ark::plan::Patch::Insert { at, node } => {
                            rows.push(node);
                            Op::Insert { at: at as u64 }
                        }
                        ark::plan::Patch::Remove { at } => Op::Remove { at: at as u64 },
                        ark::plan::Patch::Update { at, node } => {
                            rows.push(node);
                            Op::Update { at: at as u64 }
                        }
                    });
                }
                (Taken::Patched { ops }, rows)
            }
            Update::Reset => {
                drop(s);
                (Taken::Reset, self.rows_bytes())
            }
        }
    }
}

impl Drop for ViewHandle {
    fn drop(&mut self) {
        let mut s = self.shared.lock();
        if let Some(k) = s.views.remove(&self.id) {
            k.view.close(&mut s.peer);
        }
    }
}

/// The sync socket of the server at `server` (`http://host:port`).
#[uniffi::export]
pub fn socket_url(server: String) -> String {
    ark_auth::socket_url(&server)
}

/// A join link's device code redeemed at `server`
/// (`ark_auth::onboarding::Effect::Redeem`): a login of this device's own.
/// A refusal is the server's sentence.
#[uniffi::export]
pub fn redeem(server: String, code: String) -> Result<Login, Refusal> {
    Ok(ark_auth::client::redeem(&server, &code)?.into())
}

/// "Add a Device": the join link another device scans or opens — a
/// one-time code made at `server` as `token`'s account, on the app's
/// `scheme` — and its QR code. Blocking: the server is asked.
#[uniffi::export]
pub fn device_link(scheme: String, server: String, token: String) -> Result<DeviceLink, Refusal> {
    let (code, _) = ark_auth::client::device_code(&server, &token)?;
    let link = ark_auth::onboarding::join_link(scheme, server, Some(code));
    let qr = ark_auth::onboarding::qr_code(link.clone()).ok_or("the link is too long for a QR code")?;
    Ok(DeviceLink { link, qr })
}

/// What "Add a Device" shows: the link, and the link as a QR code.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeviceLink {
    pub link: String,
    pub qr: ark_auth::onboarding::Qr,
}

/// The login a sign-in's code stands for, from the server that gave it.
#[uniffi::export]
pub fn exchange(server: String, code: String) -> Result<Login, Refusal> {
    Ok(ark_auth::client::exchange(&server, &code)?.into())
}

/// The sign-in machine and the first run's pages, for an app's bridge
/// (`first_run_app!`'s), which names them by these paths.
pub use ark_app;
pub use ark_auth;

/// An app's first run over UniFFI: the shared pages (`ark_app::welcome`)
/// over the sign-in machine (`ark_auth::first_run`), as one object the
/// phone's holder reads and sends to. Written in the app's bridge, after
/// its twins of `ark_app::welcome`'s records (`WelcomePageFfi`,
/// `WelcomeMsgFfi`, `AddDevicePageFfi`), which UniFFI exports in the app's
/// own namespace: `ark_ffi::first_run_app!(money_app::welcome::copy)`.
/// `$copy` is the app's words, `fn(scan, demo) -> WelcomeCopy`.
#[macro_export]
macro_rules! first_run_app {
    ($copy:path) => {
        /// What a press on the first run asks of the phone: the machine's
        /// effects, and whether to scan another device's code.
        #[derive(uniffi::Record)]
        pub struct FirstRunAsked {
            pub effects: Vec<$crate::ark_auth::onboarding::Effect>,
            pub scan: bool,
        }

        /// The app's first run: its pages, over the sign-in machine.
        #[derive(uniffi::Object)]
        pub struct FirstRunApp(std::sync::Mutex<$crate::ark_auth::first_run::FirstRun>);

        impl FirstRunApp {
            fn run(&self) -> std::sync::MutexGuard<'_, $crate::ark_auth::first_run::FirstRun> {
                self.0.lock().unwrap_or_else(|e| e.into_inner())
            }
        }

        #[uniffi::export]
        impl FirstRunApp {
            /// The first run of `app`; `scan` where a camera reads another
            /// device's code, `demo` where the demo household runs.
            #[uniffi::constructor]
            pub fn new(app: $crate::ark_auth::onboarding::AppInfo, scan: bool, demo: bool) -> std::sync::Arc<FirstRunApp> {
                std::sync::Arc::new(FirstRunApp(std::sync::Mutex::new($crate::ark_auth::first_run::FirstRun::new(
                    app,
                    $copy(scan, demo),
                ))))
            }

            /// The page that shows; none of them while the app does.
            pub fn page(&self) -> WelcomePageFfi {
                use $crate::Twin;
                self.run().page().ffi()
            }

            /// A press on a page.
            pub fn send(&self, msg: WelcomeMsgFfi) -> FirstRunAsked {
                use $crate::Twin;
                let a = self.run().send($crate::ark_app::welcome::WelcomeMsg::from_ffi(msg));
                FirstRunAsked {
                    effects: a.effects,
                    scan: a.scan,
                }
            }

            /// One of the machine's events: what an effect came to, a link
            /// opened, the app back in front.
            pub fn event(&self, event: $crate::ark_auth::onboarding::Event) -> Vec<$crate::ark_auth::onboarding::Effect> {
                self.run().event(event)
            }

            /// What the camera read for "Scan a Code".
            pub fn scanned(&self, text: Option<String>) -> Vec<$crate::ark_auth::onboarding::Effect> {
                self.run().scanned(text)
            }

            /// The app's pages passed over: a launch straight to a route,
            /// a link or a screenshot's, as a returning person's would.
            pub fn skip_intro(&self) {
                self.run().skip_intro()
            }

            /// The app's pages from the first, over wherever it stands:
            /// their route (`/welcome/`).
            pub fn show_intro(&self) {
                self.run().show_intro()
            }

            pub fn step(&self) -> $crate::ark_auth::onboarding::Step {
                self.run().machine().step()
            }

            pub fn said(&self) -> $crate::ark_auth::onboarding::Said {
                self.run().machine().said()
            }

            pub fn remembered(&self) -> $crate::ark_auth::onboarding::Remembered {
                self.run().machine().remembered()
            }
        }

        /// "Add a Device" as it stands: none while the code is made, the
        /// link made, or why not.
        #[uniffi::export]
        pub fn add_device_page(link: Option<$crate::DeviceLink>, failed: Option<String>) -> AddDevicePageFfi {
            use $crate::Twin;
            let made = match (link, failed) {
                (Some(l), _) => Some(Ok((l.link, l.qr))),
                (None, Some(why)) => Some(Err(why)),
                (None, None) => None,
            };
            $crate::ark_auth::first_run::add_device_page(made).ffi()
        }
    };
}
