//! The peer: the replica of an app's log, persisted; the engine's `Client`
//! around it; a local authority when there is no server; the link to the
//! server when there is one; and what a screen does — mutate, query, check,
//! hold a view.

use std::collections::{BTreeMap, BTreeSet};

use ark::canon;
use ark::eval::{self, Args, Checked, Ctx, EvalFault};
use ark::log::{snapshot_of, Log, Seq};
use ark::peer::{local_commit, Authority, Changes, Replica};
use ark::protocol::{Client, ClientMsg, Mode, ServerMsg};
use ark::schema::Schema;
use ark::store::{MemoryStore, Refusal};
use ark::value::{Id, Value};

use crate::autos::Autos;
use crate::link::{platform_dial, Dial, Link, State, Timing};
use crate::storage::{BoxStorage, Memory, ReplicaFile};
use crate::view::View;
use crate::{Domain, Error};

/// How a peer is opened.
#[derive(Clone, Debug)]
pub struct Options {
    /// Who authors: the entry's actor. Under `ark-auth` the login's
    /// `user.id`; under the engine's dev `trusting`, the token.
    pub user: String,
    /// The login it authors under: `Login::session`, or `"dev"` under
    /// `trusting`. The server holds every pushed entry to both.
    pub session: String,
    /// What the `Hello` proves the login with.
    pub token: Option<String>,
    /// No server: this peer is the authority for the log, and nothing
    /// stays pending (the demo, a peer working alone).
    pub alone: bool,
    pub autos: Autos,
    pub timing: Timing,
}

impl Options {
    /// A peer that syncs with a server, as `user` under `session`, proving
    /// it with `token`.
    pub fn server(user: impl Into<String>, session: impl Into<String>, token: Option<String>) -> Options {
        Options {
            user: user.into(),
            session: session.into(),
            token,
            alone: false,
            autos: Autos::system(),
            timing: Timing::default(),
        }
    }

    /// Dev auth (`ark::protocol::trusting`): the token is the name and the
    /// session is `"dev"`.
    pub fn dev(user: impl Into<String>) -> Options {
        let user = user.into();
        Options::server(user.clone(), "dev", Some(user))
    }

    /// A peer nobody has signed in on yet, for a server it will sync with
    /// once somebody does. What it authors is authored as nobody
    /// (`Ctx::nobody`), kept pending and durable — not committed, which is
    /// the difference from [`Options::alone`] — and nothing is dialled.
    /// [`Peer::sign_in`] makes all of it the signer's and connects.
    ///
    /// Opened over storage that has had a login, it authors as that login,
    /// still offline: see [`Peer::sign_out`].
    pub fn signed_out() -> Options {
        Options::server("", "", None)
    }

    /// A peer that is its own authority.
    pub fn alone(user: impl Into<String>) -> Options {
        Options {
            alone: true,
            ..Options::server(user, "local", None)
        }
    }

    pub fn with_autos(mut self, autos: Autos) -> Options {
        self.autos = autos;
        self
    }

    pub fn with_timing(mut self, timing: Timing) -> Options {
        self.timing = timing;
        self
    }
}

/// A verdict against one of this peer's own intents: it will never be in
/// the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub id: Id,
    pub reason: String,
}

/// Where one of this peer's own intents stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Applied here, not yet answered by the authority.
    Pending,
    /// In the log.
    Confirmed,
    /// Never going to be in the log, and the reason every replica reaches:
    /// what a screen shows beside the item that did not happen.
    Rejected(String),
    /// Not an intent this peer authored (in this run, or pending from the
    /// last).
    Unknown,
}

/// What one [`Peer::pump`] came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pumped {
    /// A frame arrived (the log, an ack, a live frame).
    pub moved: bool,
    /// The socket opened, and `Hello` went out.
    pub opened: bool,
    /// The socket that was open went, and why. The link dials again.
    pub dropped: Option<String>,
    /// The server turned this login away. The link stops; a new token and
    /// [`Peer::reconnect`] are the way back. Nothing pending is lost.
    pub denied: Option<String>,
    /// Verdicts that arrived.
    pub rejected: usize,
    /// Something went wrong that is worth a status line: a frame that did
    /// not decode, a replica that could not be written.
    pub note: Option<String>,
}

/// Everything a status line or a debug screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub user: String,
    pub session: String,
    pub alone: bool,
    /// Nobody is signed in: nothing is dialled. `user` is empty if nobody
    /// ever was, or the login this peer goes on authoring as.
    pub signed_out: bool,
    /// The engine is linked: the socket is open and `Hello` was said.
    pub linked: bool,
    /// The link in a word: `alone`, `signed out`, `offline`, `idle`,
    /// `connecting`, `open`, `waiting`.
    pub link: String,
    pub url: Option<String>,
    /// Connections opened.
    pub opens: u64,
    /// The connection count the engine keeps: what a live room compares.
    pub epoch: i64,
    /// The last confirmed sequence.
    pub cursor: Seq,
    pub pending: usize,
    pub diverged: usize,
    pub denied: Option<String>,
    pub last_close: Option<String>,
    /// Live frames received, decoded or not.
    pub heard_frames: u64,
    pub bad_frames: u64,
}

/// One peer of an app. See the crate docs for the shape.
pub struct Peer {
    domain: Domain,
    schema: Schema,
    client: Client,
    authority: Option<Authority>,
    storage: BoxStorage,
    ctx: Ctx,
    autos: Autos,
    timing: Timing,
    alone: bool,
    /// Nobody is signed in, so nothing is dialled.
    signed_out: bool,
    /// What the file holds — cursor, pending, login — or `None` when it is
    /// behind in a way those do not show (pending re-stamped by a sign-in).
    written: Option<Written>,
    link: Option<Link>,
    rejections: Vec<Rejection>,
    /// Every intent authored here this run or found pending at open.
    authored: BTreeSet<Id>,
    /// Every verdict, by intent, kept for [`Peer::standing`].
    rejected: BTreeMap<Id, String>,
    heard_frames: u64,
    bad_frames: u64,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("ctx", &self.ctx)
            .field("alone", &self.alone)
            .field("link", &self.link)
            .finish()
    }
}

type Written = (Seq, Vec<Id>, Ctx);

fn mode_word(alone: bool) -> &'static str {
    if alone {
        "alone"
    } else {
        "server"
    }
}

impl Peer {
    /// Open the replica from `storage` (or empty), replay what was pending
    /// on top, and — alone — sequence it. No socket yet: [`Peer::connect`]
    /// dials.
    pub fn open(domain: Domain, storage: BoxStorage, opts: Options) -> Result<Peer, Error> {
        let schema = domain.module().schema.clone();
        let natives = domain.native_list();
        let (confirmed, cursor, pending, was) = match storage.load(ReplicaFile::KEY)? {
            Some(bytes) => {
                let f = ReplicaFile::decode(&bytes, &schema)?;
                if f.mode != mode_word(opts.alone) {
                    return Err(Error::ModeMismatch {
                        was: f.mode,
                        now: mode_word(opts.alone).into(),
                    });
                }
                (f.confirmed, f.cursor, f.pending, Ctx::new(f.user, f.session))
            }
            None => (MemoryStore::empty(schema.clone()), 0, vec![], Ctx::nobody()),
        };
        let written = Some((cursor, pending.iter().map(|e| e.id).collect(), was.clone()));
        let authored = pending.iter().map(|e| e.id).collect();
        let authority = opts.alone.then(|| {
            // The authority a peer alone is: its log is the confirmed store
            // as a snapshot at the cursor, with nothing above it yet.
            let mut a = Authority::new(schema.clone(), domain.closures().clone());
            a.log = Log {
                base: snapshot_of(cursor, confirmed.clone()),
                entries: BTreeMap::new(),
                ids: BTreeMap::new(),
            };
            a.store = confirmed.clone();
            a.hold(natives.iter().cloned());
            a
        });
        let mut r = Replica::open(schema.clone(), domain.closures().clone(), confirmed, cursor, pending);
        r.hold(natives.iter().cloned());
        let mut client = Client::open(r, Mode::Whole, opts.token.clone());
        // Who authors. Opened signed out over storage somebody has used, it
        // is still them; opened with a login over work nobody authored, that
        // work is the login's — the same as `sign_in`, whichever order the
        // app did the two in.
        let mut ctx = Ctx::new(opts.user, opts.session);
        let signed_out = !opts.alone && ctx.is_nobody();
        let mut written = written;
        if signed_out {
            ctx = was;
        } else if !opts.alone && client.replica.pending.iter().any(|e| e.actor.is_empty() && e.session.is_empty()) {
            client.sign_in(&ctx, opts.token.clone());
            written = None;
        }
        let mut peer = Peer {
            domain,
            schema,
            client,
            authority,
            storage,
            ctx,
            autos: opts.autos,
            timing: opts.timing,
            alone: opts.alone,
            signed_out,
            written,
            link: None,
            rejections: vec![],
            authored,
            rejected: BTreeMap::new(),
            heard_frames: 0,
            bad_frames: 0,
        };
        // Alone, whatever a previous run left pending is sequenced now.
        peer.commit_alone();
        peer.collect_rejections();
        peer.persist()?;
        // Opening replayed pending on top of confirmed: a view hydrates from
        // here, so that `Rebuilt` is nobody's news.
        let _ = peer.take_changes();
        Ok(peer)
    }

    /// In memory: nothing survives the peer. Tests and the demo.
    pub fn open_memory(domain: Domain, opts: Options) -> Result<Peer, Error> {
        Peer::open(domain, Box::new(Memory::new()), opts)
    }

    /// A directory, holding the file `replica`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_path(domain: Domain, dir: impl Into<std::path::PathBuf>, opts: Options) -> Result<Peer, Error> {
        Peer::open(domain, Box::new(crate::storage::Dir(dir.into())), opts)
    }

    /// The browser's `localStorage`, under keys starting `ark:{name}:`.
    #[cfg(target_arch = "wasm32")]
    pub fn open_local(domain: Domain, name: &str, opts: Options) -> Result<Peer, Error> {
        let prefix = format!("ark:{name}:");
        Peer::open(domain, Box::new(crate::storage::Local { prefix }), opts)
    }

    pub fn domain(&self) -> &Domain {
        &self.domain
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Who this peer authors as.
    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }

    pub fn is_alone(&self) -> bool {
        self.alone
    }

    /// Whether nobody is signed in: nothing is dialled until
    /// [`Peer::sign_in`].
    pub fn is_signed_out(&self) -> bool {
        self.signed_out
    }

    /// Somebody signed in: `user` under `session`, proven with `token`.
    /// Every intent still pending that was authored as nobody becomes
    /// theirs, under this login, and the view is replayed so every row
    /// those intents wrote says so; the ids [`Peer::mutate`] returned stay
    /// the same, so [`Peer::standing`] then follows each to its fate. The
    /// rewritten intents are written down before anything is said, and the
    /// link — if [`Peer::connect`] made one — dials, so the next `Hello`
    /// pushes all of it.
    ///
    /// Intents authored as somebody (before a [`Peer::sign_out`]) keep their
    /// author: the server takes them if they are this person's under an
    /// older login (`Auth::owns`), and refuses them as `not yours` if not.
    pub fn sign_in(&mut self, user: impl Into<String>, session: impl Into<String>, token: Option<String>) -> Result<(), Error> {
        if self.alone {
            return Err(Error::Alone);
        }
        self.ctx = Ctx::new(user, session);
        self.client.sign_in(&self.ctx, token);
        self.signed_out = false;
        self.collect_rejections();
        self.written = None;
        self.persist()?;
        self.reconnect();
        Ok(())
    }

    /// Nobody is signed in any more: the token is forgotten and the link
    /// stops. The login is *not* forgotten — this peer goes on authoring as
    /// that person, offline, and what it authors stays theirs: pushed when
    /// they sign in again (under any login of theirs), refused if somebody
    /// else does. Only work done before anyone ever signed in on a peer is
    /// nobody's, because only that has never been anybody's.
    pub fn sign_out(&mut self) {
        if self.alone {
            return;
        }
        self.signed_out = true;
        self.client.token = None;
        if let Some(link) = &mut self.link {
            link.stop("signed out");
        }
        if self.client.linked {
            self.client.disconnected();
        }
    }

    /// Author under this login from now on (entries already pending keep
    /// the session they were authored under).
    pub fn set_session(&mut self, session: impl Into<String>) {
        self.ctx.session = session.into();
    }

    /// What the next `Hello` proves the login with. Say [`Peer::reconnect`]
    /// for it to be said now.
    pub fn set_token(&mut self, token: Option<String>) {
        self.client.token = token;
    }

    // -- a screen's side ------------------------------------------------------

    /// Author an intent by name: its autos drawn here, once, frozen in the
    /// entry; applied to the optimistic store natively (or through the
    /// interpreter); kept durable as pending; pushed if linked. The entry's
    /// id, or the refusal — a refusal changes nothing and records nothing.
    pub fn mutate(&mut self, name: &str, args: Args) -> Result<Id, Error> {
        let (fh, f) = self.domain.mutator(name)?;
        let (fh, f) = (fh.clone(), f.clone());
        let autos = self.autos.draw(&f);
        let id = self.autos.new_id();
        self.client.mutate(id, &self.ctx, &fh, &autos, &args).map_err(Error::Refused)?;
        self.authored.insert(id);
        self.commit_alone();
        self.collect_rejections();
        self.persist()?;
        Ok(id)
    }

    /// Run a query by name over the optimistic store, as this peer's user.
    pub fn query(&self, name: &str, args: &Args) -> Result<Value, Error> {
        let (fh, _) = self.domain.query(name)?;
        let store = self.store();
        let out = match self.domain.natives().get(fh) {
            Some(p) => p.query(&self.ctx, args, store),
            None => eval::query_closure(&self.schema, &self.domain.closures()[fh], &self.ctx, args, store),
        };
        out.map_err(|e| match e {
            EvalFault::Verdict(r) => Error::Refused(r),
            EvalFault::Bug(b) => Error::Bug(format!("{name}: {b:?}")),
        })
    }

    /// The form validator (`spec/AUTHORING.md` §1.3) over a partial input
    /// to a mutator or query: a message per failing field, and the values
    /// as the checks normalised them. Nothing is written.
    pub fn check(&self, name: &str, partial: &Args) -> Result<Checked, Error> {
        let (fh, _) = self.domain.function(name).ok_or_else(|| Error::UnknownFunction(name.into()))?;
        let store = self.store();
        eval::check(&self.schema, &self.domain.closures()[fh], &self.ctx, partial, store).map_err(|b| Error::Bug(format!("{name}: {b:?}")))
    }

    /// A query held as a list and kept up to date: see [`View`].
    pub fn view(&self, name: &str, args: Args) -> Result<View, Error> {
        View::open(self, name, args)
    }

    /// The optimistic store — `confirmed` with `pending` replayed — which
    /// every query reads.
    pub fn store(&self) -> &MemoryStore {
        &self.client.replica.view
    }

    /// The replica, to look at.
    pub fn replica(&self) -> &Replica {
        &self.client.replica
    }

    /// What moved since the last ask: `Applied(changes)` to the optimistic
    /// store, or `Rebuilt` — a rebase rolled it back and replayed pending on
    /// top, which no list of changes describes. Hand it to every
    /// [`View::update`].
    pub fn take_changes(&mut self) -> Changes {
        self.client.replica.take_changes()
    }

    /// Verdicts against this peer's intents since the last ask.
    pub fn take_rejections(&mut self) -> Vec<Rejection> {
        self.collect_rejections();
        std::mem::take(&mut self.rejections)
    }

    /// Where one of this peer's intents stands: what a screen shows beside
    /// the item it made.
    pub fn standing(&self, id: &Id) -> Standing {
        if let Some(why) = self.rejected.get(id) {
            return Standing::Rejected(why.clone());
        }
        let r = &self.client.replica;
        if let Some((_, why)) = r.rejections.iter().find(|(i, _)| i == id) {
            return Standing::Rejected(refusal_text(why));
        }
        if r.pending.iter().any(|e| e.id == *id) {
            Standing::Pending
        } else if self.authored.contains(id) {
            Standing::Confirmed
        } else {
            Standing::Unknown
        }
    }

    /// Intents authored here and not yet answered.
    pub fn pending_len(&self) -> usize {
        self.client.replica.pending.len()
    }

    /// The last confirmed sequence.
    pub fn cursor(&self) -> Seq {
        self.client.replica.cursor
    }

    /// Ask the authority whether it agrees with the confirmed state; the
    /// answer arrives in [`Peer::agreed`]. Alone, at once.
    pub fn verify(&mut self) {
        let Some(a) = &self.authority else {
            self.client.verify_all();
            return;
        };
        let (n, h) = self.client.replica.verify_at();
        let ok = a.log.state_at(n).map(|st| ark::hash::state_hash(&st)) == Some(h);
        self.client.agreed.push((n, ok));
    }

    /// Every `(seq, agreed)` the authority has answered.
    pub fn agreed(&self) -> &[(Seq, bool)] {
        &self.client.agreed
    }

    // -- the live channel -------------------------------------------------------

    /// Say a frame to this account's room. Dropped, not queued, while
    /// unlinked: "pause" is not worth saying on Friday.
    pub fn say(&mut self, frame: Vec<u8>) {
        self.client.say(frame);
    }

    /// Say an ArkDB value, as its canonical CBOR.
    pub fn say_value(&mut self, v: &Value) {
        self.say(canon::encode(v));
    }

    /// What the room said since the last ask. Never durable: a frame is
    /// about now, and a reconnect clears what was not taken.
    pub fn heard(&mut self) -> Vec<Vec<u8>> {
        self.client.take_heard()
    }

    /// [`Peer::heard`], each decoded as canonical CBOR; a frame that does
    /// not decode is dropped (a newer peer's sentence) and counted.
    pub fn heard_values(&mut self) -> Vec<Value> {
        let mut out = vec![];
        for f in self.heard() {
            match canon::decode(&f) {
                Ok(v) => out.push(v),
                Err(_) => self.bad_frames += 1,
            }
        }
        out
    }

    /// How many connections this peer has had: a live room that has never
    /// heard of this device is one whose epoch this peer has not introduced
    /// itself on.
    pub fn epoch(&self) -> i64 {
        self.client.epoch
    }

    /// Whether the engine is linked: the socket is open and `Hello` said.
    pub fn linked(&self) -> bool {
        self.client.linked
    }

    // -- the socket ---------------------------------------------------------------

    /// Dial the server's sync socket (`ws://host/sync`) and keep it up:
    /// every [`Peer::pump`] moves frames, and a drop dials again with a
    /// backoff. Replaces any link there was.
    pub fn connect(&mut self, url: &str) {
        let dial = platform_dial(&self.timing);
        self.connect_with(url, dial);
    }

    /// [`Peer::connect`] over a transport of the caller's.
    ///
    /// Signed out, the link is kept and not dialled: [`Peer::sign_in`] is
    /// what starts it.
    pub fn connect_with(&mut self, url: &str, dial: Dial) {
        self.disconnect();
        self.client.denied = None;
        let mut link = Link::new(url, dial, self.timing.clone());
        if self.signed_out {
            link.stop("signed out");
        }
        self.link = Some(link);
    }

    /// Go offline: close the socket and stop dialling. Everything authored
    /// meanwhile is pending, durable, and pushed on the next connection.
    pub fn disconnect(&mut self) {
        if let Some(link) = &mut self.link {
            link.stop("offline");
        }
        if self.client.linked {
            self.client.disconnected();
        }
    }

    /// Dial again after [`Peer::disconnect`] or a denial — with a new token,
    /// usually. Signed out, nothing: [`Peer::sign_in`] is the way back.
    pub fn reconnect(&mut self) {
        if self.signed_out {
            return;
        }
        self.client.denied = None;
        if let Some(link) = &mut self.link {
            if link.is_open() {
                link.stop("reconnecting");
                self.client.disconnected();
            }
            link.resume();
        }
    }

    /// The link, if [`Peer::connect`] made one.
    pub fn link(&self) -> Option<&Link> {
        self.link.as_ref()
    }

    /// One turn of the link: dial if due, hand the engine what arrived,
    /// send what it queued, and write whatever moved. Call it on a tick
    /// (fifty milliseconds is what the clients here use).
    pub fn pump(&mut self) -> Pumped {
        let mut p = Pumped::default();
        let Some(link) = &mut self.link else {
            return p;
        };
        let polled = link.poll();
        if polled.opened {
            self.client.connected();
            p.opened = true;
        }
        for f in polled.frames {
            p.moved = true;
            if let Err(e) = self.recv_frame(&f) {
                p.note = Some(e.to_string());
            }
        }
        if let Some(why) = polled.closed {
            self.client.disconnected();
            p.dropped = Some(why);
        }
        if let Some(reason) = self.client.denied.clone() {
            if let Some(link) = &mut self.link {
                if link.state() != &State::Idle {
                    link.stop(&format!("denied: {reason}"));
                    p.denied = Some(reason);
                }
            }
        }
        let frames = self.take_outgoing_frames();
        if let Some(link) = &mut self.link {
            for f in frames {
                link.send(f);
            }
        }
        let before = self.rejections.len();
        self.collect_rejections();
        p.rejected = self.rejections.len() - before;
        if let Err(e) = self.persist() {
            p.note = Some(e.to_string());
        }
        p
    }

    /// Why the server last turned this login away, until a reconnect.
    pub fn denied(&self) -> Option<&str> {
        self.client.denied.as_deref()
    }

    // -- sans-io: for a transport of the caller's own ---------------------------

    /// A connection opened: `Hello` at the cursor, then everything pending.
    pub fn connected(&mut self) {
        self.client.connected();
    }

    /// The connection is gone; what was queued is dropped.
    pub fn disconnected(&mut self) {
        self.client.disconnected();
    }

    /// A message from the server.
    pub fn recv(&mut self, msg: ServerMsg) {
        if matches!(msg, ServerMsg::Heard { .. }) {
            self.heard_frames += 1;
        }
        self.client.recv(msg);
    }

    /// A binary frame from the server: canonical CBOR of a `ServerMsg`.
    pub fn recv_frame(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let msg = canon::decode(bytes)
            .map_err(|e| e.to_string())
            .and_then(|v| ServerMsg::from_value(&v).map_err(|e| e.to_string()))
            .map_err(|e| {
                self.bad_frames += 1;
                Error::Corrupt(format!("a frame from the server: {e}"))
            })?;
        self.recv(msg);
        Ok(())
    }

    /// What the engine queued, oldest first. Empty while unlinked.
    pub fn take_outgoing(&mut self) -> Vec<ClientMsg> {
        self.client.take_outgoing()
    }

    /// [`Peer::take_outgoing`], each as its wire bytes.
    pub fn take_outgoing_frames(&mut self) -> Vec<Vec<u8>> {
        self.client.take_outgoing().iter().map(|m| canon::encode(&m.to_value())).collect()
    }

    /// Write the replica if its cursor or pending moved since it was last
    /// written. `mutate` and `pump` call it; a caller driving the sans-io
    /// half by hand calls it after `recv`.
    pub fn persist(&mut self) -> Result<(), Error> {
        let r = &self.client.replica;
        let now = (r.cursor, r.pending.iter().map(|e| e.id).collect::<Vec<Id>>(), self.ctx.clone());
        if self.written.as_ref() == Some(&now) {
            return Ok(());
        }
        let file = ReplicaFile {
            mode: mode_word(self.alone).into(),
            cursor: r.cursor,
            confirmed: r.confirmed.clone(),
            pending: r.pending.clone(),
            user: self.ctx.user.clone(),
            session: self.ctx.session.clone(),
        };
        self.storage.save(ReplicaFile::KEY, &file.encode())?;
        self.written = Some(now);
        Ok(())
    }

    pub fn status(&self) -> Status {
        let link = match (&self.link, self.alone) {
            (_, true) => "alone",
            (_, false) if self.signed_out => "signed out",
            (None, false) => "offline",
            (Some(l), false) => match l.state() {
                State::Idle => "idle",
                State::Connecting => "connecting",
                State::Open => "open",
                State::Waiting(_) => "waiting",
            },
        };
        Status {
            user: self.ctx.user.clone(),
            session: self.ctx.session.clone(),
            alone: self.alone,
            signed_out: self.signed_out,
            linked: self.client.linked,
            link: link.into(),
            url: self.link.as_ref().map(|l| l.url().to_string()),
            opens: self.link.as_ref().map_or(0, |l| l.opens),
            epoch: self.client.epoch,
            cursor: self.client.replica.cursor,
            pending: self.pending_len(),
            diverged: self.client.replica.diverged.len(),
            denied: self.client.denied.clone(),
            last_close: self.link.as_ref().and_then(|l| l.last_close.clone()),
            heard_frames: self.heard_frames,
            bad_frames: self.bad_frames,
        }
    }

    // -- inside -------------------------------------------------------------------

    fn commit_alone(&mut self) {
        if let Some(a) = &mut self.authority {
            local_commit(a, &mut self.client.replica);
        }
    }

    fn collect_rejections(&mut self) {
        // One intent can be refused twice: by this peer's own rebase, when a
        // confirmed entry leaves it nothing to apply to, and then by the
        // authority it had already been pushed to. It is one verdict.
        for (id, why) in std::mem::take(&mut self.client.replica.rejections) {
            if self.rejected.contains_key(&id) {
                continue;
            }
            let reason = refusal_text(&why);
            self.rejected.insert(id, reason.clone());
            self.rejections.push(Rejection { id, reason });
        }
    }
}

/// A verdict as a person reads it: a mutator's own refusal word for word,
/// a constraint named in a sentence (`ark::protocol::refusal_text`).
pub fn refusal_text(r: &Refusal) -> String {
    ark::protocol::refusal_text(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use crate::{args, demo};
    use ark::store::Store;

    /// An app that reopens with the login rather than calling `sign_in`
    /// gets the same answer: what nobody authored is the login's.
    #[test]
    fn opening_with_a_login_over_work_nobody_authored_makes_it_theirs() {
        let disk = Memory::new();
        let mut p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::signed_out()).unwrap();
        let id = p.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        drop(p);
        let p = Peer::open(demo::domain(), Box::new(disk.clone()), Options::server("bob", "s1", None)).unwrap();
        assert_eq!(p.replica().pending[0].actor, "bob");
        assert_eq!(p.replica().pending[0].session, "s1");
        assert_eq!(p.store().scan("playlist")[0]["user_id"], Value::text("bob"));
        assert_eq!(p.standing(&id), Standing::Pending);
        drop(p);
        // …and it was written down, not only replayed.
        let f = ReplicaFile::decode(&disk.load(ReplicaFile::KEY).unwrap().unwrap(), &demo::domain().module().schema).unwrap();
        assert_eq!((f.pending[0].actor.as_str(), f.user.as_str()), ("bob", "bob"));
    }

    #[test]
    fn a_peer_alone_has_nobody_to_sign_in_to() {
        let mut p = Peer::open_memory(demo::domain(), Options::alone("me")).unwrap();
        assert_eq!(p.sign_in("alice", "s", None), Err(Error::Alone));
        assert_eq!(p.ctx().user, "me");
    }
}
