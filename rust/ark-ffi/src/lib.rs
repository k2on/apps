//! A device's peer for an app written in another language (`docs/plan-v5.md`
//! §1.16, N3): `ark-peer`'s app kit through UniFFI, every value as its JSON.
//!
//! The kit is domain-agnostic: an app's own Rust crate links its domain in
//! and opens a [`Peer`] over it ([`Peer::open`]; harken's is `harken/ffi`,
//! whose `harken_open` is the constructor its phones call). Through the
//! bridge a [`Peer`] is called by function name with JSON — `mutate` (on
//! the space the function writes), `query`, `view` — and says where it stands
//! (`status`), what became of each intent (`outcome`, `take_refusals`),
//! and who it is (`login`, `signed_in`, `logout`: `ark-auth`'s session
//! rows). With no server (`connect` never called) it is a replica whose
//! intents stay pending, kept on its disk until it has one.
//! The typed Kotlin and Swift over these names is generated from the domain
//! (`arkdb gen`), types only; a view whose row type has a twin is read
//! typed instead, its rows UniFFI's records ([`typed`]).
//!
//! **It drives itself.** A peer has a thread of its own that pumps its link
//! whenever the connection rings or a dial is due, and keeps its views up:
//! each [`ViewHandle`] gathers its patches ([`ViewTaken`]) until taken, and
//! the app's [`Listener`] is told whenever anything moved — a frame came, a
//! view was patched — on whatever thread moved it; the app then takes its
//! views and asks `status` on its own. A view that could not be kept up is
//! made again by itself, and says so as its patches: its rows removed and
//! inserted.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use ark::log::EntryId;
use ark::peer::Ctx;
use ark_peer::handle::{Handle, Locked};
use ark_peer::link::Dial;
use ark_peer::{Config, Files, Patch, Watched};

uniffi::setup_scaffolding!();

pub mod typed;
pub use typed::{Op, RowId, Twin, Typed};

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

/// A login as the server hands it over: the token the link proves itself
/// with, and who it is.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Login {
    pub token: String,
    pub session: String,
    pub user: String,
    pub name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub expires_ms: i64,
    /// The provider's picture of them (`ark_auth::Account::picture`), or
    /// empty. Last and defaulted, so a login made before it still builds.
    #[uniffi(default = "")]
    pub picture: String,
}

impl From<ark_auth::Login> for Login {
    fn from(l: ark_auth::Login) -> Login {
        Login {
            token: l.token,
            session: l.session,
            user: l.user.id,
            name: l.user.name,
            email: l.user.email,
            roles: l.user.roles,
            expires_ms: l.expires_ms,
            picture: l.user.picture,
        }
    }
}

impl Login {
    fn who(&self) -> Ctx {
        Ctx::who(self.user.clone(), self.session.clone(), self.roles.clone())
    }
}

/// What a view did since it was last taken — its changes ([`Op`]), none
/// when it did nothing — and the rows that came with them, each its JSON
/// (§1.10): a typed view's `<T>Taken`, by name.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ViewTaken {
    pub ops: Vec<Op>,
    pub rows: Vec<String>,
}

/// What has become of the intent `id` (as [`Peer::mutate`] gave it):
/// pending, taken by the server (neither `pending` nor `refused`), or
/// refused and why.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Outcome {
    pub id: String,
    pub pending: bool,
    pub refused: Option<String>,
}

impl Outcome {
    fn of(id: EntryId, o: ark_peer::Outcome) -> Outcome {
        let (pending, refused) = match o {
            ark_peer::Outcome::Pending => (true, None),
            ark_peer::Outcome::Ok => (false, None),
            ark_peer::Outcome::Refused(why) => (false, Some(why)),
        };
        Outcome {
            id: ark::hash::hex(&id.0),
            pending,
            refused,
        }
    }
}

/// Where a device stands, as a status line says it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Status {
    /// `update needed` (the server cannot serve this build: the app is to be
    /// updated), `alone` (no server), `signed out`, `online`, `offline`,
    /// `connecting` or `waiting to dial`.
    pub link: String,
    pub linked: bool,
    pub epoch: u64,
    pub cursor: i64,
    pub pending: u64,
    pub user: String,
    pub session: String,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
    /// Why the server last turned this login away (close 4401), until the
    /// next login: the link has stopped dialling.
    pub denied: Option<String>,
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
    patches: Vec<Patch>,
}

/// What the bridge keeps beside the peer: its views, and what the app was
/// last told of.
#[derive(Default)]
struct State {
    views: BTreeMap<u64, Kept>,
    next: u64,
    /// Why the server last turned the login away.
    denied: Option<String>,
}

/// The peer held, and the bridge's state beside it.
struct Held<'a> {
    peer: Locked<'a>,
    state: MutexGuard<'a, State>,
}

impl Held<'_> {
    /// Every view's updates taken from the peer into what the app has not
    /// taken; a view opened for another login, or refused, opened again.
    /// Whether any moved.
    fn settle(&mut self) -> bool {
        let (peer, s) = (&mut *self.peer, &mut *self.state);
        let mut moved = false;
        for k in s.views.values_mut() {
            let ps = k.view.settle(peer, true);
            moved |= !ps.is_empty();
            k.patches.extend(ps);
        }
        moved
    }

    /// A view's number and name, while it answers.
    fn kept(&self, id: u64) -> Option<(usize, String)> {
        let k = self.state.views.get(&id)?;
        Some((k.view.view()?, k.view.name().to_string()))
    }
}

/// The peer, on a thread of its own ([`Handle`]), the bridge's
/// state, and who is told when anything moved.
struct Shared {
    peer: Handle,
    state: Mutex<State>,
    listener: Mutex<Option<Arc<dyn Listener>>>,
}

impl Shared {
    fn lock(&self) -> Held<'_> {
        let peer = self.peer.lock();
        Held {
            peer,
            state: self.state.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// `f` on the peer, its views settled after; the listener told when
    /// anything moved. Let go, the driver sends what the peer said.
    fn act<T>(&self, f: impl FnOnce(&mut Held) -> T) -> T {
        let (out, moved) = {
            let mut s = self.lock();
            let out = f(&mut s);
            (out, s.settle())
        };
        if moved {
            self.tell();
        }
        out
    }

    fn tell(&self) {
        let l = self.listener.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(l) = l {
            l.changed();
        }
    }

    /// What the driver's pumps took in — or, pumped by hand, a pump now —
    /// settled into the views: whether anything moved, the app told so.
    fn pump(&self) -> bool {
        let p = self.peer.pump();
        let moved = {
            let mut s = self.lock();
            if p.denied.is_some() {
                s.state.denied = p.denied;
            }
            s.settle() | p.moved
        };
        if moved {
            self.tell();
        }
        moved
    }
}

/// A value's JSON, or what went wrong reading it (which a test sees).
fn text(json: Result<String, String>) -> String {
    json.unwrap_or_else(|why| format!("{{\"error\":{}}}", serde_json::Value::String(why)))
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
        let login = login.map(|l| (l.who(), l.token));
        Peer::of(ark_peer::Peer::open_as(config, Box::new(disk), login))
    }

    /// A peer opened by its host its own way — as a login it keeps itself,
    /// its link tuned — driven from now on as [`Peer::open`]'s is.
    pub fn of(peer: ark_peer::Peer) -> Result<Arc<Peer>, Refusal> {
        // What this device does goes to its server (docs/plan-observe.md).
        let _ = ark_peer::observe::install(ark_peer::observe::Sink::Outbox);
        let shared = Arc::new(Shared {
            peer: Handle::spawn(peer),
            state: Mutex::default(),
            listener: Mutex::new(None),
        });
        let weak = Arc::downgrade(&shared);
        shared.peer.set_waker(Arc::new(move || {
            if let Some(s) = weak.upgrade() {
                s.pump();
            }
        }));
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
        self.shared.peer.by_hand();
    }

    /// What the connection said taken in now, and what the peer has to say
    /// sent: whether anything moved.
    pub fn pump(&self) -> bool {
        self.shared.pump()
    }

    /// A maintained view of `name` on its input's bytes.
    pub(crate) fn view_bytes(&self, name: String, args: Vec<u8>) -> Result<Arc<ViewHandle>, Refusal> {
        let id = self.shared.act(|s| -> Result<u64, String> {
            let view = Watched::try_open(&mut s.peer, &name, args, |_| ())?;
            let id = s.state.next;
            s.state.views.insert(id, Kept { view, patches: vec![] });
            s.state.next += 1;
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

    /// Dial the server at `server` (`http://host:port`; its sync socket,
    /// `ark_auth::socket_url`) whenever signed in.
    pub fn connect(&self, server: String) {
        self.shared.act(|s| s.peer.connect(&ark_auth::socket_url(&server)));
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

    /// An intent of the function `name` on its input's JSON, on the space
    /// the function writes (a live one through the relay when it is
    /// ephemeral): its id, or why its preview — or, alone, its sequencing —
    /// refused it.
    pub fn mutate(&self, name: String, input: String) -> Result<String, Refusal> {
        let _call = tracing::debug_span!(target: "ark", "ffi", call = "mutate", name = %name).entered();
        let id = self.shared.act(|s| {
            let input = s.peer.input_json(&name, &input)?;
            // An app's mutate of the log is somebody's doing — a tap — and
            // the root of what it causes; a job's own steps answer what made
            // the job due instead, and a live call what woke it.
            let job = [".claim", ".finish", ".cancel"].iter().any(|v| name.ends_with(v));
            if job || s.peer.function(&name)?.space == ark::schema::Space::Volatile {
                return s.peer.mutate(&name, input);
            }
            s.peer.begin(&name);
            let out = s.peer.mutate(&name, input);
            s.peer.end();
            out
        })?;
        Ok(ark::hash::hex(&id.0))
    }

    /// A file made on this device (docs/plan-files.md): kept in its
    /// `files/`, sent to the server on this or the next connection; its
    /// name (the hex sha256 of `bytes`), for the `FileRef` a mutation
    /// names it by.
    pub fn put_file(&self, bytes: Vec<u8>, media: String) -> Result<String, Refusal> {
        Ok(self.shared.act(|s| s.peer.put_file(&bytes[..], &media))?.hash)
    }

    /// `put_file` of the file at `path`, streamed from it: a large one is
    /// never held whole.
    pub fn put_file_at(&self, path: String, media: String) -> Result<String, Refusal> {
        let from = std::fs::File::open(&path).map_err(|e| format!("{path}: {e}"))?;
        Ok(self.shared.act(|s| s.peer.put_file(from, &media))?.hash)
    }

    /// At most `n` of a file's bytes from `at` when this device holds it
    /// whole; when not, fetched from the server, and the listener rings
    /// when they arrive.
    pub fn file(&self, hash: String, size: i64, media: String, at: i64, n: i64) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let f = ark_peer::FileRef { hash, size, media };
        let mut b = self.shared.act(|s| s.peer.file(&f)).ok().flatten()?;
        let mut bytes = vec![];
        b.seek(SeekFrom::Start(at.max(0) as u64)).ok()?;
        b.take(n.max(0) as u64).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    /// The first of an input's refinements that fails, as a form says it.
    pub fn check(&self, name: String, input: String) -> Option<String> {
        let s = self.shared.lock();
        let checked = s.peer.function(&name).and_then(|f| Ok((f.check)(&s.peer.input_json(&name, &input)?)));
        match checked {
            Ok(c) => c.err().and_then(|mut e| e.drain(..).next()).map(|(_, w)| w),
            Err(why) => Some(why),
        }
    }

    /// A view's answer on its input's JSON, as JSON, over the optimistic
    /// state: a kind's jobs due (`<kind>.due`) among them, what a handler
    /// written in the app asks whenever the listener rings, then answers
    /// through `mutate` — `<kind>.claim`, and once that is taken
    /// (`outcome`), `<kind>.finish`.
    pub fn query(&self, name: String, input: String) -> Result<String, Refusal> {
        let s = self.shared.lock();
        let answer = s.peer.ask(&name, &s.peer.input_json(&name, &input)?)?;
        Ok(s.peer.answer_json(&name, &answer)?)
    }

    /// A maintained view of the function `name` on its input's JSON.
    pub fn view(&self, name: String, input: String) -> Result<Arc<ViewHandle>, Refusal> {
        let args = self.shared.lock().peer.input_json(&name, &input)?;
        self.view_bytes(name, args)
    }

    /// The rows of the live space's `table` this device holds, as JSON.
    pub fn live_rows(&self, table: String) -> Vec<String> {
        let s = self.shared.lock();
        let Some(live) = &s.peer.live else { return vec![] };
        live.store.rows(&table).map(|r| text(s.peer.row_json(&table, r))).collect()
    }

    pub fn status(&self) -> Status {
        let s = self.shared.lock();
        let p = &s.peer;
        Status {
            link: p.link_state().into(),
            linked: p.linked,
            epoch: p.link.epoch,
            cursor: p.cursor(),
            pending: p.pending_len() as u64,
            user: p.me().actor.clone(),
            session: p.me().session.clone(),
            failure: s.peer.failure().map(Into::into),
            denied: s.state.denied.clone(),
        }
    }

    /// What became of the intent `id` on the log (as [`Peer::mutate`] gave
    /// it): none for one this run did not author.
    pub fn outcome(&self, id: String) -> Option<Outcome> {
        let id = ark::hash::unhex(&id).and_then(|b| b.try_into().ok()).map(EntryId)?;
        let o = self.shared.lock().peer.outcome(&id)?;
        Some(Outcome::of(id, o))
    }

    /// What the server refused of this device's intents on the log since
    /// the last ask.
    pub fn take_refusals(&self) -> Vec<Outcome> {
        let r = self.shared.lock().peer.take_refusals(ark::schema::Space::Durable);
        r.into_iter().map(|(id, why)| Outcome::of(id, ark_peer::Outcome::Refused(why))).collect()
    }

    /// Somebody signed in: every pending intent theirs, dialled as them.
    pub fn login(&self, login: Login) {
        self.shared.act(|s| {
            s.state.denied = None;
            s.peer.login(login.who(), login.token)
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
    /// Its rows as they stand, each its JSON.
    pub fn rows(&self) -> Vec<String> {
        self.json(self.rows_bytes())
    }

    /// What it did since last taken, and the rows that came with it.
    pub fn take(&self) -> ViewTaken {
        let (ops, rows) = self.take_bytes();
        ViewTaken { ops, rows: self.json(rows) }
    }
}

impl ViewHandle {
    /// Rows of this view, each its JSON.
    fn json(&self, rows: Vec<Vec<u8>>) -> Vec<String> {
        let s = self.shared.lock();
        let Some((_, name)) = s.kept(self.id) else { return vec![] };
        rows.iter().map(|r| text(s.peer.element_json(&name, r))).collect()
    }

    /// Its rows as they stand, each its bytes.
    pub(crate) fn rows_bytes(&self) -> Vec<Vec<u8>> {
        let s = self.shared.lock();
        s.kept(self.id).and_then(|(at, _)| s.peer.rows(at)).unwrap_or_default()
    }

    /// What it did since last taken, and the rows that came with it, each
    /// its bytes: each insert's and update's, in order.
    pub(crate) fn take_bytes(&self) -> (Vec<Op>, Vec<Vec<u8>>) {
        let mut s = self.shared.lock();
        s.settle();
        let Some(k) = s.state.views.get_mut(&self.id) else {
            return (vec![], vec![]);
        };
        let (mut ops, mut rows) = (vec![], vec![]);
        for p in std::mem::take(&mut k.patches) {
            ops.push(match p {
                Patch::Insert { at, node } => {
                    rows.push(node);
                    Op::Insert { at: at as u64 }
                }
                Patch::Remove { at } => Op::Remove { at: at as u64 },
                Patch::Update { at, node } => {
                    rows.push(node);
                    Op::Update { at: at as u64 }
                }
            });
        }
        (ops, rows)
    }
}

impl Drop for ViewHandle {
    fn drop(&mut self) {
        let mut s = self.shared.lock();
        if let Some(k) = s.state.views.remove(&self.id) {
            k.view.close(&mut s.peer);
        }
    }
}

/// Where a sign-in starts: the server's own page, which sends its code on
/// to `redirect` (`harken://auth`).
#[uniffi::export]
pub fn login_url(server: String, redirect: String) -> String {
    ark_auth::login_url(&server, &redirect, None)
}

/// The login a sign-in's code stands for, from the server that gave it.
#[uniffi::export]
pub fn exchange(server: String, code: String) -> Result<Login, Refusal> {
    Ok(ark_auth::client::exchange(&server, &code)?.into())
}
